//! "Chains" are a canonicalized form of types/permissions.
//! They can only be produced after inference is complete as they require enumerating the bounds of inference variables.
//! They are used in borrow checking and for producing the final version of each inference variable.

use dada_ir_ast::diagnostic::{Err, Errors};
use dada_util::{boxed_async_fn, vecset::VecSet};

use crate::ir::{
    indices::FromInfer,
    types::{SymGenericTerm, SymPerm, SymPermKind, SymPlace, SymTy, SymTyKind},
};

use super::{
    Env,
    live_places::LivePlaces,
    places::PlaceTy,
    predicates::{
        Predicate, is_provably_copy::place_is_provably_copy, test_perm_infer_is_known_to_be,
        test_var_is_provably,
    },
    red::{Lien, RedPerm, RedTy},
    runtime::Runtime,
};

trait ChainExt<'db>: Sized {
    /// Concatenate two lien chains; if `other` is copy, just returns `other`.
    async fn concat(&self, env: &mut Env<'db>, other: &Self) -> Errors<Self>;

    /// Check if the chain is copy. Will block if this chain contains an inference variable.
    async fn is_copy(&self, env: &mut Env<'db>) -> Errors<bool>;
}

impl<'db> ChainExt<'db> for RedPerm<'db> {
    /// See [`ChainExt::concat`][].
    async fn concat(&self, env: &mut Env<'db>, other: &Self) -> Errors<Self> {
        if other.is_copy(env).await? {
            Ok(other.clone())
        } else {
            let mut links = self.liens.clone();
            links.extend(other.liens.iter());
            Ok(Self::new(env.db(), links))
        }
    }

    /// See [`ChainExt::is_copy`][].
    async fn is_copy(&self, env: &mut Env<'db>) -> Errors<bool> {
        for lien in &self.liens {
            if lien.is_copy(env).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

trait LienExt<'db>: Sized {
    /// Check if the lien is copy, blocking if inference info is needed.
    async fn is_copy(&self, env: &mut Env<'db>) -> Errors<bool>;
}

impl<'db> LienExt<'db> for Lien<'db> {
    /// See [`LienExt::is_copy`][].
    async fn is_copy(&self, env: &mut Env<'db>) -> Errors<bool> {
        match *self {
            Lien::Our | Lien::Shared(_) => Ok(true),
            Lien::Leased(_) => Ok(false),
            Lien::Var(v) => Ok(test_var_is_provably(env, v, Predicate::Copy)),
            Lien::Infer(v) => test_perm_infer_is_known_to_be(env, v, Predicate::Copy).await,
            Lien::Error(reported) => Err(reported),
        }
    }
}

pub trait RedTyExt<'db>: Sized {
    fn display<'a>(&'a self, env: &'a Env<'db>) -> impl std::fmt::Display;
}

impl<'db> RedTyExt<'db> for RedTy<'db> {
    fn display<'a>(&'a self, env: &'a Env<'db>) -> impl std::fmt::Display {
        struct Wrapper<'a, 'db> {
            ty: &'a RedTy<'db>,
            #[expect(dead_code)] // FIXME?
            env: &'a Env<'db>,
        }

        impl std::fmt::Display for Wrapper<'_, '_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match &self.ty {
                    RedTy::Error(_reported) => write!(f, "<error>"),
                    RedTy::Named(sym_ty_name, sym_generic_terms) => {
                        write!(f, "{}[{:?}]", sym_ty_name, sym_generic_terms)
                    }
                    RedTy::Never => write!(f, "!"),

                    // FIXME: do better by querying the env state
                    RedTy::Infer(v) => write!(f, "?{}", v.as_usize()),

                    RedTy::Var(sym_variable) => write!(f, "{}", sym_variable),
                    RedTy::Perm => write!(f, "<perm>"),
                }
            }
        }

        Wrapper { ty: self, env }
    }
}

/// Convert something to a [`RedTy`] and an (optional) permission that is applied to that [`RedTy`][].
pub trait ToRedTy<'db> {
    fn to_red_ty(&self, env: &mut Env<'db>) -> (RedTy<'db>, Option<SymPerm<'db>>);
}

impl<'db> ToRedTy<'db> for SymGenericTerm<'db> {
    fn to_red_ty(&self, env: &mut Env<'db>) -> (RedTy<'db>, Option<SymPerm<'db>>) {
        match *self {
            SymGenericTerm::Type(ty) => ty.to_red_ty(env),
            SymGenericTerm::Perm(perm) => perm.to_red_ty(env),
            SymGenericTerm::Place(_) => panic!("cannot create a red term from a place"),
            SymGenericTerm::Error(reported) => (RedTy::err(env.db(), reported), None),
        }
    }
}

impl<'db> ToRedTy<'db> for SymTy<'db> {
    fn to_red_ty(&self, env: &mut Env<'db>) -> (RedTy<'db>, Option<SymPerm<'db>>) {
        to_red_ty_with_runtime(*self, env.runtime())
    }
}

/// Convert `ty` to a red-ty given a runtime.
///
/// See [`ToRedTy`][].
pub fn to_red_ty_with_runtime<'db>(
    ty: SymTy<'db>,
    runtime: &Runtime<'db>,
) -> (RedTy<'db>, Option<SymPerm<'db>>) {
    let db = runtime.db;
    match *ty.kind(db) {
        SymTyKind::Perm(perm0, sym_ty) => match to_red_ty_with_runtime(sym_ty, runtime) {
            (red_ty, None) => (red_ty, Some(perm0)),
            (red_ty, Some(perm1)) => (red_ty, Some(SymPerm::apply(db, perm0, perm1))),
        },
        SymTyKind::Named(n, ref g) => (RedTy::Named(n, g.clone()), None),
        SymTyKind::Infer(infer) => {
            // every type inference variable has an associated permission inference variable,
            // so split that off
            let perm_infer = runtime.perm_infer(infer);
            (RedTy::Infer(infer), Some(SymPerm::infer(db, perm_infer)))
        }
        SymTyKind::Var(v) => (RedTy::Var(v), None),
        SymTyKind::Never => (RedTy::Never, None),
        SymTyKind::Error(reported) => (RedTy::err(db, reported), None),
    }
}

impl<'db> ToRedTy<'db> for SymPerm<'db> {
    fn to_red_ty(&self, env: &mut Env<'db>) -> (RedTy<'db>, Option<SymPerm<'db>>) {
        let db = env.db();
        match *self.kind(db) {
            SymPermKind::Error(reported) => (RedTy::err(db, reported), None),
            _ => (RedTy::Perm, Some(*self)),
        }
    }
}

pub trait ToRedPerms<'db> {
    async fn to_red_perms(
        &self,
        env: &mut Env<'db>,
        live_after: LivePlaces,
    ) -> Errors<VecSet<RedPerm<'db>>>;
}

impl<'db> ToRedPerms<'db> for SymPerm<'db> {
    #[boxed_async_fn]
    async fn to_red_perms(
        &self,
        env: &mut Env<'db>,
        live_after: LivePlaces,
    ) -> Errors<VecSet<RedPerm<'db>>> {
        let mut output = VecSet::new();
        let db = env.db();
        match *self.kind(db) {
            SymPermKind::My => {
                output.insert(RedPerm::my(db));
            }
            SymPermKind::Our => {
                output.insert(RedPerm::our(db));
            }
            SymPermKind::Shared(ref places) => {
                for &place in places {
                    if place_is_provably_copy(env, place).await.is_ok() {
                        output.extend(place.to_red_perms(env, live_after).await?);
                    } else {
                        output.insert(RedPerm::shared(env.db(), place));
                    }
                }
            }
            SymPermKind::Leased(ref places) => {
                for &place in places {
                    if place_is_provably_copy(env, place).await.is_ok() {
                        output.extend(place.to_red_perms(env, live_after).await?);
                    } else {
                        output.insert(RedPerm::leased(db, place));
                    }
                }
            }
            SymPermKind::Apply(lhs, rhs) => {
                let lhs_chains = lhs.to_red_perms(env, live_after).await?;
                let rhs_chains = rhs.to_red_perms(env, live_after).await?;
                for lhs_chain in &lhs_chains {
                    for rhs_chain in &rhs_chains {
                        output.insert(lhs_chain.concat(env, rhs_chain).await?);
                    }
                }
            }
            SymPermKind::Infer(v) => {
                output.insert(RedPerm::infer(db, v));
            }
            SymPermKind::Var(v) => {
                output.insert(RedPerm::var(db, v));
            }
            SymPermKind::Error(reported) => return Err(reported),
        }
        Ok(output)
    }
}

impl<'db> ToRedPerms<'db> for SymPlace<'db> {
    async fn to_red_perms(
        &self,
        env: &mut Env<'db>,
        live_after: LivePlaces,
    ) -> Errors<VecSet<RedPerm<'db>>> {
        let ty = self.place_ty(env).await;
        ty.to_red_perms(env, live_after).await
    }
}

impl<'db> ToRedPerms<'db> for SymTy<'db> {
    #[boxed_async_fn]
    async fn to_red_perms(
        &self,
        env: &mut Env<'db>,
        live_after: LivePlaces,
    ) -> Errors<VecSet<RedPerm<'db>>> {
        let mut output = VecSet::new();
        let db = env.db();
        match *self.kind(db) {
            SymTyKind::Perm(lhs, rhs) => {
                let lhs_chains = lhs.to_red_perms(env, live_after).await?;
                let rhs_chains = rhs.to_red_perms(env, live_after).await?;
                for lhs_chain in &lhs_chains {
                    for rhs_chain in &rhs_chains {
                        output.insert(lhs_chain.concat(env, rhs_chain).await?);
                    }
                }
            }
            SymTyKind::Infer(infer) => {
                output.insert(RedPerm::infer(db, env.perm_infer(infer)));
            }
            SymTyKind::Never | SymTyKind::Named(..) | SymTyKind::Var(_) => {
                output.insert(RedPerm::my(db));
            }
            SymTyKind::Error(reported) => return Err(reported),
        }
        Ok(output)
    }
}
