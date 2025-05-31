use dada_util::{FromImpls, SalsaSerialize};
use salsa::Update;
use serde::Serialize;

use super::{AstGenericDecl, AstPerm, AstStatement, AstTy, SpanVec, SpannedIdentifier};
use crate::{
    ast::{AstVisibility, AstWhereClauses, DeferredParse},
    span::{Span, Spanned},
};

/// `fn foo() { }`
#[derive(SalsaSerialize)]
#[salsa::tracked(debug)]
pub struct AstFunction<'db> {
    /// Overall span of the function declaration
    pub span: Span<'db>,

    /// Declared effects (e.g., `async`)
    pub effects: AstFunctionEffects<'db>,

    /// Span of the `fn` keyword
    pub fn_span: Span<'db>,

    /// Visibility of the function
    pub visibility: Option<AstVisibility<'db>>,

    /// Name of the function
    pub name: SpannedIdentifier<'db>,

    /// Any explicit generics e.g., `[type T]`
    #[return_ref]
    pub generics: Option<SpanVec<'db, AstGenericDecl<'db>>>,

    /// Arguments to the function
    #[return_ref]
    pub inputs: SpanVec<'db, AstFunctionInput<'db>>,

    /// Return type of the function (if provided)
    pub output_ty: Option<AstTy<'db>>,

    /// Where clauses (if any)
    #[return_ref]
    pub where_clauses: Option<AstWhereClauses<'db>>,

    /// Body (if provided)
    #[return_ref]
    pub body: Option<DeferredParse<'db>>,
}

/// `print("Hello world")` appearing at the top of a module.
/// This creates an implicit `fn main() { ... }` later on.
#[derive(SalsaSerialize)]
#[salsa::tracked(debug)]
pub struct AstMainFunction<'db> {
    #[return_ref]
    pub statements: SpanVec<'db, AstStatement<'db>>,
}

impl<'db> Spanned<'db> for AstMainFunction<'db> {
    fn span(&self, db: &'db dyn crate::Db) -> Span<'db> {
        self.statements(db).span
    }
}

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Update, Debug, Serialize)]
pub struct AstFunctionEffects<'db> {
    pub async_effect: Option<Span<'db>>,
    pub unsafe_effect: Option<Span<'db>>,
}

impl<'db> Spanned<'db> for AstFunction<'db> {
    fn span(&self, db: &'db dyn crate::Db) -> Span<'db> {
        AstFunction::span(*self, db)
    }
}

#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Update, Debug, FromImpls, Serialize,
)]
pub enum AstFunctionInput<'db> {
    SelfArg(AstSelfArg<'db>),
    Variable(VariableDecl<'db>),
}

impl<'db> Spanned<'db> for AstFunctionInput<'db> {
    fn span(&self, db: &'db dyn crate::Db) -> Span<'db> {
        match self {
            AstFunctionInput::SelfArg(arg) => arg.span(db),
            AstFunctionInput::Variable(var) => var.span(db),
        }
    }
}

#[derive(SalsaSerialize)]
#[salsa::tracked(debug)]
pub struct AstSelfArg<'db> {
    /// Permission written by the user.
    /// If `None`, we will supply a suitable default.
    pub perm: Option<AstPerm<'db>>,
    pub self_span: Span<'db>,
}

impl<'db> Spanned<'db> for AstSelfArg<'db> {
    fn span(&self, db: &'db dyn crate::Db) -> Span<'db> {
        if let Some(perm) = self.perm(db) {
            self.self_span(db).start_from(perm.span(db))
        } else {
            self.self_span(db)
        }
    }
}

/// `[mut] x: T`
#[derive(SalsaSerialize)]
#[salsa::tracked(debug)]
pub struct VariableDecl<'db> {
    /// Span of the `mut` keyword, if present.
    pub mutable: Option<Span<'db>>,

    /// Variable name.
    pub name: SpannedIdentifier<'db>,

    /// Permission written by the user.
    /// If `None`, we will supply a suitable default.
    pub perm: Option<AstPerm<'db>>,

    /// Variable type, excluding any permission,
    /// which can be found in `perm`.
    pub base_ty: AstTy<'db>,
}

impl<'db> Spanned<'db> for VariableDecl<'db> {
    fn span(&self, db: &'db dyn crate::Db) -> Span<'db> {
        self.name(db).span.to(db, self.base_ty(db).span(db))
    }
}
