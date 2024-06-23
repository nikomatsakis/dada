//! The "validated" IR is the IR that we use for type checks
//! and so forth. It is still in tree form and is mildly
//! desugared and easy to work with.

use crate::{
    class::Class, code::validated::op::Op, function::Function, in_ir_db::InIrDb,
    intrinsic::Intrinsic, prelude::InIrDbExt, storage::Atomic, word::Word,
};
use dada_id::{id, prelude::*, tables};
use salsa::DebugWithDb;

use super::syntax;

/// The "validated" form of a particular [syntax tree](`crate::code::syntax::Tree`).
#[salsa::tracked]
#[customize(DebugWithDb)]
pub struct Tree<'db> {
    /// The function that this tree is associated with.
    pub function: Function,

    #[return_ref]
    pub data: TreeData,

    #[return_ref]
    pub origins: Origins,
}

impl DebugWithDb<dyn crate::Db + '_> for Tree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &dyn crate::Db) -> std::fmt::Result {
        let in_db = &self.in_ir_db(db);
        DebugWithDb::fmt(self.data(db), f, in_db)
    }
}

/// Stores the ast for a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeData {
    /// Interning tables for expressions and the like.
    pub tables: Tables,

    /// Number of parameters; these will be local variables 0..N
    pub num_parameters: usize,

    /// The root
    pub root_expr: Expr,
}

impl DebugWithDb<InIrDb<'_, Tree>> for TreeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        DebugWithDb::fmt(&self.root_expr, f, db)
    }
}

impl InIrDb<'_, Tree> {
    fn tables(&self) -> &Tables {
        &self.data(self.db()).tables
    }

    fn origins(&self) -> &Origins {
        let tree: Tree = **self;
        tree.origins(self.db())
    }
}

impl TreeData {
    pub fn new(tables: Tables, num_parameters: usize, root_expr: Expr) -> Self {
        Self {
            tables,
            root_expr,
            num_parameters,
        }
    }

    pub fn parameters(&self) -> impl Iterator<Item = LocalVariable> {
        LocalVariable::range(0, self.num_parameters)
    }

    pub fn max_local_variable(&self) -> LocalVariable {
        LocalVariable::max_key(&self.tables)
    }
}

tables! {
    /// Tables that store the data for expr in the AST.
    /// You can use `tables[expr]` (etc) to access the data.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Tables {
        local_variables: alloc LocalVariable => LocalVariableData,
        exprs: alloc Expr => ExprData,
        named_exprs: alloc NamedExpr => NamedExprData,
        places: alloc Place => PlaceData,
        target_places: alloc TargetPlace => TargetPlaceData,
        names: alloc Name => NameData,
    }
}

origin_table! {
    /// Side table that contains the spans for everything in a syntax tree.
    /// This isn't normally needed except for diagnostics, so it's
    /// kept separate to avoid reducing incremental reuse.
    /// You can request it by invoking the `spans`
    /// method in the `dada_parse` prelude.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Origins {
        expr_spans: Expr => ExprOrigin,
        place_spans: Place => ExprOrigin,
        target_place_spans: TargetPlace => ExprOrigin,
        named_exprs: NamedExpr => syntax::NamedExpr,
        local_variables: LocalVariable => LocalVariableOrigin,
        names: Name => syntax::Name,
    }
}

/// The "validated" trees sometimes contain synthetic nodes caused by
/// lowering the syntax expressions. We track the expression they came
/// from, but also the fact that they are synthetic. This is needed to
/// help place cursors and so forth.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct ExprOrigin {
    pub syntax_expr: syntax::Expr,
    pub synthesized: bool,
}

impl ExprOrigin {
    pub fn real(expr: syntax::Expr) -> Self {
        Self {
            syntax_expr: expr,
            synthesized: false,
        }
    }
    pub fn synthesized(expr: syntax::Expr) -> Self {
        Self {
            syntax_expr: expr,
            synthesized: true,
        }
    }
}

impl std::fmt::Debug for ExprOrigin {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ExprOrigin {
            synthesized,
            syntax_expr,
        } = *self;

        if synthesized {
            write!(fmt, "synthesized from {syntax_expr:?}")
        } else {
            write!(fmt, "from {syntax_expr:?}")
        }
    }
}

impl From<syntax::Expr> for ExprOrigin {
    fn from(e: syntax::Expr) -> Self {
        Self::real(e)
    }
}

impl From<ExprOrigin> for syntax::Expr {
    fn from(e: ExprOrigin) -> Self {
        e.syntax_expr
    }
}

id!(pub struct LocalVariable);

impl DebugWithDb<InIrDb<'_, Tree>> for LocalVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let id = u32::from(*self);
        let data = self.data(db.tables());
        let name = data
            .name
            .map(|n| n.data(db.tables()).word.as_str(db.db()))
            .unwrap_or("temp");
        write!(f, "{name}{{{id}}}")
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub struct LocalVariableData {
    /// Name given to this variable by the user.
    /// If it is None, then this is a temporary
    /// introduced by the compiler.
    ///
    /// Temporaries in validation are introduced
    /// specifically for operations like `foo().share`
    /// that operate on a *place* semantically but
    /// which can accept an arbitrary expression
    /// syntactically.
    ///
    /// It's important that we not introduce arbitrary
    /// temporaries because validation temporaries are
    /// considered roots for the GC in the official
    /// semantics.
    pub name: Option<Name>,

    pub atomic: Atomic,
}

#[derive(PartialEq, Eq, Copy, Clone, Hash, Debug)]
pub enum LocalVariableOrigin {
    /// Temporary introduces to hold the value of the given expression.
    Temporary(syntax::Expr),

    /// A local variable declared in the function.
    LocalVariable(syntax::LocalVariableDecl),

    /// A local variable declared in the function signature.
    ///
    /// Note that this uses a distinct set of syntax tables/spans!
    Parameter(syntax::LocalVariableDecl),
}

id!(pub struct Expr);

impl DebugWithDb<InIrDb<'_, Tree>> for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let name = format!("{self:?}");
        f.debug_tuple(&name)
            .field(&self.data(db.tables()).debug(db))
            .field(&db.origins()[*self])
            .finish()
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub enum ExprData {
    /// true, false
    BooleanLiteral(bool),

    /// `22i`, `22_222i`, etc
    SignedIntegerLiteral(i64),

    /// `22u`, `22_222u`, etc
    UnsignedIntegerLiteral(u64),

    /// `22`, `22_222`, etc
    IntegerLiteral(u64),

    /// `2.2`
    FloatLiteral(eq_float::F64),

    /// `"foo"` with no format strings
    StringLiteral(Word),

    /// Concatenates a bunch of strings from a string literal like `"foo{bar}baz"`
    Concatenate(Vec<Expr>),

    /// `expr.await`
    Await(Expr),

    /// `expr(id: expr, ...)`
    Call(Expr, Vec<NamedExpr>),

    /// `<value>.share`
    IntoShared(Expr),

    /// `<place>.share`
    Share(Place),

    /// `expr.lease`
    Lease(Place),

    /// `expr.give`
    Give(Place),

    /// `()` or `(a, b, ...)` (i.e., expr seq cannot have length 1)
    Tuple(Vec<Expr>),

    /// `if condition { block } [else { block }]`
    If(Expr, Expr, Expr),

    /// `atomic { block }`
    Atomic(Expr),

    /// `loop { block }`
    Loop(Expr),

    /// `break [from expr] [with value]`
    ///
    /// * `from_expr`: Identifies the loop from which we are breaking
    /// * `with_value`: The value produced by the loop
    Break { from_expr: Expr, with_value: Expr },

    /// `continue`
    ///
    /// * `0`: identifies the loop with which we are continuing.
    Continue(Expr),

    /// `break [from expr] [with value]`
    Return(Expr),

    /// `expr[0]; expr[1]; ...`
    Seq(Vec<Expr>),

    /// `a + b`
    Op(Expr, Op, Expr),

    /// `<op> x`
    Unary(Op, Expr),

    /// `a = b` or `a := b`
    Assign(TargetPlace, Expr),

    /// Bring the variables in scope during the expression
    Declare(Vec<LocalVariable>, Expr),

    /// parse or other error
    Error,
}

impl DebugWithDb<InIrDb<'_, Tree>> for ExprData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        self.pretty_print(None, f, db)
    }
}

impl ExprData {
    fn pretty_print(
        &self,
        id: Option<Expr>,
        f: &mut std::fmt::Formatter<'_>,
        db: &InIrDb<'_, Tree>,
    ) -> std::fmt::Result {
        let id = id.map(u32::from);
        match self {
            ExprData::BooleanLiteral(v) => std::fmt::Debug::fmt(v, f),
            ExprData::IntegerLiteral(v) => write!(f, "{v}"),
            ExprData::UnsignedIntegerLiteral(v) => write!(f, "{v}"),
            ExprData::SignedIntegerLiteral(v) => write!(f, "{v}"),
            ExprData::FloatLiteral(v) => write!(f, "{v}"),
            ExprData::StringLiteral(v) => std::fmt::Debug::fmt(&v.as_str(db.db()), f),
            ExprData::Await(expr) => f.debug_tuple("Await").field(&expr.debug(db)).finish(),
            ExprData::Call(expr, args) => f
                .debug_tuple("Call")
                .field(&expr.debug(db))
                .field(&args.debug(db))
                .finish(),
            ExprData::IntoShared(p) => f.debug_tuple("IntoShared").field(&p.debug(db)).finish(),
            ExprData::Lease(p) => f.debug_tuple("Lease").field(&p.debug(db)).finish(),
            ExprData::Share(p) => f.debug_tuple("Share").field(&p.debug(db)).finish(),
            ExprData::Give(p) => f.debug_tuple("Give").field(&p.debug(db)).finish(),
            ExprData::Tuple(exprs) => {
                let mut f = f.debug_tuple("Tuple");
                for expr in exprs {
                    f.field(&expr.debug(db));
                }
                f.finish()
            }
            ExprData::Concatenate(exprs) => {
                let mut f = f.debug_tuple("Concatenate");
                for expr in exprs {
                    f.field(&expr.debug(db));
                }
                f.finish()
            }
            ExprData::If(condition, if_true, if_false) => f
                .debug_tuple("If")
                .field(&condition.debug(db))
                .field(&if_true.debug(db))
                .field(&if_false.debug(db))
                .finish(),
            ExprData::Atomic(e) => f.debug_tuple("Atomic").field(&e.debug(db)).finish(),
            ExprData::Loop(e) => f
                .debug_tuple("Loop")
                .field(&id)
                .field(&e.debug(db))
                .finish(),
            ExprData::Break {
                from_expr,
                with_value,
            } => f
                .debug_tuple("Break")
                .field(&u32::from(*from_expr))
                .field(&with_value.debug(db))
                .finish(),
            ExprData::Continue(loop_expr) => f
                .debug_tuple("Continue")
                .field(&u32::from(*loop_expr))
                .finish(),
            ExprData::Return(value) => f.debug_tuple("Return").field(&value.debug(db)).finish(),
            ExprData::Seq(exprs) => f.debug_tuple("Seq").field(&exprs.debug(db)).finish(),
            ExprData::Op(lhs, op, rhs) => f
                .debug_tuple("Op")
                .field(&lhs.debug(db))
                .field(op)
                .field(&rhs.debug(db))
                .finish(),
            ExprData::Assign(place, expr) => f
                .debug_tuple("Assign")
                .field(&place.debug(db))
                .field(&expr.debug(db))
                .finish(),
            ExprData::Declare(vars, expr) => f
                .debug_tuple("Declare")
                .field(&vars.debug(db))
                .field(&expr.debug(db))
                .finish(),
            ExprData::Error => f.debug_tuple("Error").finish(),
            ExprData::Unary(op, rhs) => f
                .debug_tuple("Unary")
                .field(op)
                .field(&rhs.debug(db))
                .finish(),
        }
    }
}

id!(pub struct Place);

impl DebugWithDb<InIrDb<'_, Tree>> for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let name = format!("{self:?}");
        f.debug_tuple(&name)
            .field(&self.data(db.tables()).debug(db))
            .field(&db.origins()[*self])
            .finish()
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub enum PlaceData {
    LocalVariable(LocalVariable),
    Function(Function),
    Intrinsic(Intrinsic),
    Class(Class),
    Dot(Place, Word),
}

impl DebugWithDb<InIrDb<'_, Tree>> for PlaceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        match self {
            PlaceData::LocalVariable(lv) => DebugWithDb::fmt(lv, f, db),
            PlaceData::Function(function) => DebugWithDb::fmt(function, f, db.db()),
            PlaceData::Intrinsic(intrinsic) => std::fmt::Debug::fmt(intrinsic, f),
            PlaceData::Class(class) => DebugWithDb::fmt(class, f, db.db()),
            PlaceData::Dot(place, field) => f
                .debug_tuple("Dot")
                .field(&place.debug(db))
                .field(&field.debug(db.db()))
                .finish(),
        }
    }
}

id!(pub struct TargetPlace);

impl DebugWithDb<InIrDb<'_, Tree>> for TargetPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let name = format!("{self:?}");
        f.debug_tuple(&name)
            .field(&self.data(db.tables()).debug(db))
            .field(&db.origins()[*self])
            .finish()
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub enum TargetPlaceData {
    LocalVariable(LocalVariable),
    Dot(Place, Word),
}

impl DebugWithDb<InIrDb<'_, Tree>> for TargetPlaceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        match self {
            TargetPlaceData::LocalVariable(lv) => DebugWithDb::fmt(lv, f, db),
            TargetPlaceData::Dot(place, field) => f
                .debug_tuple("Dot")
                .field(&place.debug(db))
                .field(&field.debug(db.db()))
                .finish(),
        }
    }
}

id!(pub struct NamedExpr);

impl DebugWithDb<InIrDb<'_, Tree>> for NamedExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        DebugWithDb::fmt(&self.data(db.tables()), f, db)
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub struct NamedExprData {
    pub name: Option<Name>,
    pub expr: Expr,
}

impl DebugWithDb<InIrDb<'_, Tree>> for NamedExprData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        f.debug_tuple("NamedExpr")
            .field(&self.name.debug(db))
            .field(&self.expr.debug(db))
            .finish()
    }
}

pub mod op;

id!(pub struct Name);

impl DebugWithDb<InIrDb<'_, Tree>> for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        self.data(db.tables()).word.fmt(f, db.db())
    }
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub struct NameData {
    pub word: Word,
}
