use dada_ir_ast::diagnostic::Errors;
use dada_util::boxed_async_fn;

use crate::{
    check::{
        combinator::{both, exists},
        env::Env,
        places::PlaceTy,
        predicates::{
            Predicate,
            var_infer::{test_infer_is_known_to_be, test_var_is_known_to_be},
        },
    },
    ir::{
        classes::SymAggregateStyle,
        types::{SymGenericTerm, SymPerm, SymPermKind, SymPlace, SymTy, SymTyKind, SymTyName},
    },
};

pub(crate) async fn term_is_ktb_move<'db>(
    env: &Env<'db>,
    term: SymGenericTerm<'db>,
) -> Errors<bool> {
    match term {
        SymGenericTerm::Type(sym_ty) => ty_is_ktb_move(env, sym_ty).await,
        SymGenericTerm::Perm(sym_perm) => perm_is_ktb_move(env, sym_perm).await,
        SymGenericTerm::Place(sym_place) => panic!("term_is invoked on place: {sym_place:?}"),
        SymGenericTerm::Error(reported) => Err(reported),
    }
}

#[boxed_async_fn]
async fn ty_is_ktb_move<'db>(env: &Env<'db>, ty: SymTy<'db>) -> Errors<bool> {
    let db = env.db();
    match *ty.kind(db) {
        SymTyKind::Perm(sym_perm, sym_ty) => {
            Ok(application_is_ktb_move(env, sym_perm.into(), sym_ty.into()).await?)
        }
        SymTyKind::Infer(infer) => Ok(test_infer_is_known_to_be(env, infer, Predicate::Move).await),
        SymTyKind::Var(var) => Ok(test_var_is_known_to_be(env, var, Predicate::Move)),
        SymTyKind::Never => Ok(true),
        SymTyKind::Error(reported) => Err(reported),
        SymTyKind::Named(sym_ty_name, ref generics) => match sym_ty_name {
            SymTyName::Primitive(_) => Ok(false),
            SymTyName::Aggregate(sym_aggregate) => match sym_aggregate.style(db) {
                SymAggregateStyle::Struct => {
                    exists(generics, async |&generic| {
                        term_is_ktb_move(env, generic).await
                    })
                    .await
                }
                SymAggregateStyle::Class => Ok(true),
            },
            SymTyName::Future => Ok(false),
            SymTyName::Tuple { arity: _ } => {
                exists(generics, async |&generic| {
                    term_is_ktb_move(env, generic).await
                })
                .await
            }
        },
    }
}

async fn application_is_ktb_move<'db>(
    env: &Env<'db>,
    lhs: SymGenericTerm<'db>,
    rhs: SymGenericTerm<'db>,
) -> Errors<bool> {
    both(term_is_ktb_move(env, lhs), term_is_ktb_move(env, rhs)).await
}

#[boxed_async_fn]
pub(crate) async fn perm_is_ktb_move<'db>(env: &Env<'db>, perm: SymPerm<'db>) -> Errors<bool> {
    let db = env.db();
    match *perm.kind(db) {
        SymPermKind::Error(reported) => Err(reported),
        SymPermKind::My => Ok(true),
        SymPermKind::Our | SymPermKind::Shared(_) => Ok(false),
        SymPermKind::Leased(ref places) => {
            exists(places, async |&place| place_is_ktb_move(env, place).await).await
        }

        SymPermKind::Apply(lhs, rhs) => {
            Ok(application_is_ktb_move(env, lhs.into(), rhs.into()).await?)
        }

        SymPermKind::Var(var) => Ok(test_var_is_known_to_be(env, var, Predicate::Move)),

        SymPermKind::Infer(infer) => {
            Ok(test_infer_is_known_to_be(env, infer, Predicate::Move).await)
        }
    }
}

pub(crate) async fn place_is_ktb_move<'db>(env: &Env<'db>, place: SymPlace<'db>) -> Errors<bool> {
    let ty = place.place_ty(env).await;
    ty_is_ktb_move(env, ty).await
}
