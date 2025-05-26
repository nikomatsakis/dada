use dada_ir_ast::diagnostic::Errors;
use dada_util::boxed_async_fn;

use crate::{
    check::{
        env::Env,
        places::PlaceTy,
        predicates::{
            Predicate,
            var_infer::{require_infer_is, require_var_is},
        },
        red::RedTy,
        report::{Because, OrElse},
        to_red::ToRedTy,
    },
    ir::{
        classes::SymAggregateStyle,
        types::{SymGenericTerm, SymPerm, SymPermKind, SymPlace, SymTy},
    },
};

use super::is_provably_shared::{perm_is_provably_shared, term_is_provably_shared};

pub(crate) async fn require_term_is_shared<'db>(
    env: &mut Env<'db>,
    term: SymGenericTerm<'db>,
    or_else: &dyn OrElse<'db>,
) -> Errors<()> {
    match term {
        SymGenericTerm::Type(sym_ty) => require_ty_is_shared(env, sym_ty, or_else).await,
        SymGenericTerm::Perm(sym_perm) => require_perm_is_shared(env, sym_perm, or_else).await,
        SymGenericTerm::Place(place) => panic!("unexpected place term: {place:?}"),
        SymGenericTerm::Error(reported) => Err(reported),
    }
}

/// Requires that `(lhs rhs)` satisfies the given predicate.
/// The semantics of `(lhs rhs)` is: `rhs` if `rhs is copy` or `lhs union rhs` otherwise.
async fn require_either_is_shared<'db>(
    env: &mut Env<'db>,
    lhs: SymGenericTerm<'db>,
    rhs: SymGenericTerm<'db>,
    or_else: &dyn OrElse<'db>,
) -> Errors<()> {
    // Simultaneously test for whether LHS/RHS is `predicate`.
    // If either is, we are done.
    // If either is *not*, the other must be.
    env.require_both(
        async |env| {
            if !term_is_provably_shared(env, rhs).await? {
                require_term_is_shared(env, lhs, or_else).await?;
            }
            Ok(())
        },
        async |env| {
            if !term_is_provably_shared(env, lhs).await? {
                require_term_is_shared(env, rhs, or_else).await?;
            }
            Ok(())
        },
    )
    .await
}

#[boxed_async_fn]
async fn require_ty_is_shared<'db>(
    env: &mut Env<'db>,
    term: SymTy<'db>,
    or_else: &dyn OrElse<'db>,
) -> Errors<()> {
    env.indent("require_ty_is_shared", &[&term], async |env| {
        let db = env.db();
        let (red_ty, perm) = term.to_red_ty(env);
        match red_ty {
            // Error cases first
            RedTy::Error(reported) => Err(reported),

            // Never
            RedTy::Never => Err(or_else.report(env, Because::NeverIsNotCopy)),

            // Inference variables
            RedTy::Infer(infer) => {
                require_infer_is(env, perm, infer, Predicate::Shared, or_else).await
            }

            // Universal variables
            RedTy::Var(var) => {
                env.require(
                    async |env| {
                        env.either(
                            async |env| perm_is_provably_shared(env, perm).await,
                            async |env| Ok(env.var_is_declared_to_be(var, Predicate::Shared)),
                        )
                        .await
                    },
                    |_env| Because::JustSo,
                    or_else,
                )
                .await
            }

            // Named types
            RedTy::Named(sym_ty_name, ref generics) => match sym_ty_name.style(db) {
                SymAggregateStyle::Struct => {
                    require_generics_are_shared(env, perm, generics, or_else).await
                }
                SymAggregateStyle::Class => {
                    env.require_both(
                        async |env| require_perm_is_shared(env, perm, or_else).await,
                        async |env| {
                            require_generics_are_shared(env, SymPerm::my(db), generics, or_else)
                                .await
                        },
                    )
                    .await
                }
            },

            RedTy::Perm => require_perm_is_shared(env, perm, or_else).await,
        }
    })
    .await
}

async fn require_generics_are_shared<'db>(
    env: &mut Env<'db>,
    perm: SymPerm<'db>,
    generics: &[SymGenericTerm<'db>],
    or_else: &dyn OrElse<'db>,
) -> Errors<()> {
    let db = env.db();
    env.require_for_all(generics, async |env, &generic| {
        require_term_is_shared(env, perm.apply_to(db, generic), or_else).await
    })
    .await
}

#[boxed_async_fn]
pub(super) async fn require_perm_is_shared<'db>(
    env: &mut Env<'db>,
    perm: SymPerm<'db>,
    or_else: &dyn OrElse<'db>,
) -> Errors<()> {
    env.indent("require_perm_is_shared", &[&perm], async |env| {
        let db = env.db();
        match *perm.kind(db) {
            SymPermKind::Error(reported) => Err(reported),

            SymPermKind::My => Err(or_else.report(env, Because::JustSo)),

            SymPermKind::Our => Ok(()),

            SymPermKind::Referenced(_) => Ok(()),

            SymPermKind::Mutable(ref places) => {
                // For a mutable[p] to be copy, all the places in `p` must have copy permission.
                env.require_for_all(places, async |env, &place| {
                    require_place_is_shared(env, place, or_else).await
                })
                .await
            }

            // Apply
            SymPermKind::Apply(lhs, rhs) => {
                require_either_is_shared(env, lhs.into(), rhs.into(), or_else).await
            }

            // Variable and inference
            SymPermKind::Var(var) => require_var_is(env, var, Predicate::Shared, or_else),

            SymPermKind::Infer(infer) => {
                require_infer_is(env, SymPerm::my(db), infer, Predicate::Shared, or_else).await
            }

            SymPermKind::Or(_, _) => todo!(),
        }
    })
    .await
}

pub(crate) async fn require_place_is_shared<'db>(
    env: &mut Env<'db>,
    place: SymPlace<'db>,
    or_else: &dyn OrElse<'db>,
) -> Errors<()> {
    env.indent("require_place_is_shared", &[&place], async |env| {
        let ty = place.place_ty(env).await;
        require_ty_is_shared(env, ty, or_else).await
    })
    .await
}
