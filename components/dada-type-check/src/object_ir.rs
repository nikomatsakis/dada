//! The "object IR" is an intermediate IR that we create
//! as a first pass for type checking. The name "object"
//! derives from the fact that it doesn't track precise
//! types, but rather just the type of the underlying
//! object without any permissions (i.e., what class/struct/enum/etc is it?).
//! This can then be used to bootstrap full type checking.
//!
//! We need to create this IR first because full type checking will
//! require knowing which variables are live. Knowing that
//! requires that we have fully parsed the AST. But fully parsing
//! the AST requires being able to disambiguate things like `x.foo[..]()`,
//! which could be either indexing a field `foo` and then calling the
//! result or invoking a method `foo` with generic arguments.
//! The object IR gives us enough information to make those determinations.

use dada_ir_ast::{ast::Literal, diagnostic::Reported, span::Span};
use dada_ir_sym::{
    class::SymField,
    function::SymFunction,
    symbol::{HasKind, SymGenericKind, SymVariable},
    ty::{SymGenericTerm, SymTy, SymTyName, Var},
};
use dada_util::FromImpls;
use salsa::Update;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct ObjectExpr<'chk, 'db> {
    pub span: Span<'db>,
    pub ty: ObjectTy<'db>,
    pub kind: &'chk ObjectExprKind<'chk, 'db>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum ObjectExprKind<'chk, 'db> {
    /// `$expr1; $expr2`
    Semi(ObjectExpr<'chk, 'db>, ObjectExpr<'chk, 'db>),

    /// `(...)`
    Tuple(Vec<ObjectExpr<'chk, 'db>>),

    /// `22`
    Literal(Literal<'db>),

    /// `let $lv: $ty [= $initializer] in $body`
    LetIn {
        lv: SymVariable<'db>,

        // If this is a true local variable (as opposed to a temporary),
        // then this will be its "sym ty". For temporaries, it's just None
        // because no sym ty has been created yet.
        sym_ty: Option<SymTy<'db>>,

        ty: ObjectTy<'db>,
        initializer: Option<ObjectExpr<'chk, 'db>>,
        body: ObjectExpr<'chk, 'db>,
    },

    /// `$place = $expr`
    Assign {
        place: ObjectPlaceExpr<'chk, 'db>,
        expr: ObjectExpr<'chk, 'db>,
    },

    /// `$0.give`
    Give(ObjectPlaceExpr<'chk, 'db>),

    /// `$0.lease`
    Lease(ObjectPlaceExpr<'chk, 'db>),

    /// `$0.share` or just `$place`
    Share(ObjectPlaceExpr<'chk, 'db>),

    /// `$0[$1..]($2..)`
    ///
    /// During construction we ensure that the arities match and terms are well-kinded
    /// (or generate errors).
    Call {
        function: SymFunction<'db>,
        class_substitution: Vec<SymGenericTerm<'db>>,
        method_substitution: Vec<SymGenericTerm<'db>>,
        arg_temps: Vec<SymVariable<'db>>,
    },

    /// Error occurred somewhere.
    Error(Reported),
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct ObjectPlaceExpr<'chk, 'db> {
    pub span: Span<'db>,
    pub ty: ObjectTy<'db>,
    pub kind: &'chk ObjectPlaceExprKind<'chk, 'db>,
}

impl<'chk, 'db> ObjectPlaceExpr<'chk, 'db> {
    pub fn to_object_place(&self) -> ObjectGenericTerm<'db> {
        ObjectGenericTerm::Place
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) enum ObjectPlaceExprKind<'chk, 'db> {
    Var(SymVariable<'db>),
    Field(ObjectPlaceExpr<'chk, 'db>, SymField<'db>),
    Error(Reported),
}

#[salsa::interned]
pub(crate) struct ObjectTy<'db> {
    #[return_ref]
    pub kind: ObjectTyKind<'db>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Update, Debug)]
pub(crate) enum ObjectTyKind<'db> {
    /// `path[arg1, arg2]`, e.g., `Vec[String]`
    ///
    /// Important: the generic arguments must be well-kinded and of the correct number.
    Named(SymTyName<'db>, Vec<ObjectGenericTerm<'db>>),

    /// Reference to a generic or inference variable, e.g., `T` or `?X`
    Var(Var<'db>),

    /// Indicates the user wrote `?` and we should use gradual typing.
    Unknown,

    /// Indicates some kind of error occurred and has been reported to the user.
    Error(Reported),
}

impl<'db> ObjectTy<'db> {
    pub fn unit(db: &'db dyn crate::Db) -> ObjectTy<'db> {
        SymTy::unit(db).into_object_ir(db)
    }

    pub fn error(db: &'db dyn crate::Db, reported: Reported) -> ObjectTy<'db> {
        ObjectTy::new(db, ObjectTyKind::Error(reported))
    }

    pub fn shared(self, db: &'db dyn crate::Db) -> ObjectTy<'db> {
        self
    }
}

/// Value of a generic parameter
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Update, Debug, FromImpls)]
pub(crate) enum ObjectGenericTerm<'db> {
    Type(ObjectTy<'db>),
    #[no_from_impl]
    Perm,
    #[no_from_impl]
    Place,
    Error(Reported),
}

impl<'db> HasKind<'db> for ObjectGenericTerm<'db> {
    fn has_kind(&self, _db: &'db dyn crate::Db, kind: SymGenericKind) -> bool {
        match self {
            ObjectGenericTerm::Type(_) => kind == SymGenericKind::Type,
            ObjectGenericTerm::Perm => kind == SymGenericKind::Perm,
            ObjectGenericTerm::Place => kind == SymGenericKind::Place,
            ObjectGenericTerm::Error(Reported) => true,
        }
    }
}

impl<'db> ObjectGenericTerm<'db> {
    pub fn assert_type(self, db: &'db dyn crate::Db) -> ObjectTy<'db> {
        match self {
            ObjectGenericTerm::Type(ty) => ty,
            ObjectGenericTerm::Error(r) => ObjectTy::new(db, ObjectTyKind::Error(r)),
            _ => panic!("`{self:?}` is not a type"),
        }
    }
}

pub trait IntoObjectIr<'db>: Update {
    type Object: Update;

    fn into_object_ir(self, db: &'db dyn crate::Db) -> Self::Object;
}

mod subst_impls;
mod to_object_impls;
