use crate::{code::syntax::op::Op, in_ir_db::InIrDb, in_ir_db::InIrDbExt, span::Span, word::Word};
use dada_id::{id, prelude::*, tables};
use derive_new::new;
use salsa::DebugWithDb;

/// The "syntax signature" is the parsed form of a function signature,
/// including e.g. its parameter types.
#[derive(new, Clone, Debug, PartialEq, Eq)]
#[allow(clippy::too_many_arguments)]
pub struct Signature {
    /// The name of the function.
    pub name: Name,

    /// The keyword declaring the function.
    pub fn_decl: FnDecl,

    /// The "effect" of the fn (i.e., is it declared as async, atomic?), if any.
    pub effect: Option<EffectKeyword>,

    /// The generic parameters to the function, if any.
    pub generic_parameters: Vec<GenericParameter>,

    /// The parameters to the function.
    pub parameters: Vec<LocalVariableDecl>,

    /// Return type declaration.
    pub return_type: Option<ReturnTy>,

    /// Interning tables for expressions and the like.
    pub tables: Tables,

    /// The span information for each node in the tree.
    pub spans: Spans,
}

/// Generic parameter declared on a class or function.
#[derive(new, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GenericParameter {
    /// E.g., `fn foo[T]`
    Type(Name),

    /// E.g., `fn foo[perm P]`
    Permission(Perm, Name),
}

/// The "syntax tree" is the parsed form of a function body.
/// It maps more-or-less directly to what the user typed.
#[salsa::tracked]
#[customize(DebugWithDb)]
pub struct Tree<'db> {
    /// Identifies the root expression in the function body.
    #[return_ref]
    pub data: TreeData,

    /// Interning tables for expressions and the like.
    #[return_ref]
    pub tables: Tables,

    /// The span information for each node in the tree.
    #[return_ref]
    pub spans: Spans,
}

impl DebugWithDb<dyn crate::Db> for Tree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &dyn crate::Db) -> std::fmt::Result {
        f.debug_struct("syntax::Tree")
            .field("info", &self.data(db).debug(&self.in_ir_db(db)))
            .finish()
    }
}

impl InIrDb<'_, Tree> {
    fn tables(&self) -> &Tables {
        Tree::tables(**self, self.db())
    }
}

/// Stores the ast for a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeData {
    /// The root
    pub root_expr: Expr,
}

impl DebugWithDb<InIrDb<'_, Tree>> for TreeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        f.debug_struct("syntax::Tree")
            .field("root_expr", &self.root_expr.debug(db)) // FIXME
            .finish()
    }
}

tables! {
    /// Tables that store the data for expr in the AST.
    /// You can use `tables[expr]` (etc) to access the data.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Tables {
        exprs: alloc Expr => ExprData,
        named_exprs: alloc NamedExpr => NamedExprData,
        local_variable_decls: alloc LocalVariableDecl => LocalVariableDeclData,
        atomic_keyword: alloc AtomicKeyword => AtomicKeywordData,
        async_keyword: alloc AsyncKeyword => AsyncKeywordData,
        fn_decl: alloc FnDecl => FnDeclData,
        name: alloc Name => NameData,
        ty: alloc Ty => TyData,
        perm: alloc Perm => PermData,
        path: alloc Path => PathData,
        perm_paths: alloc PermPaths => PermPathsData,
        return_ty: alloc ReturnTy => ReturnTyData,
    }
}

origin_table! {
    /// Side table that contains the spans for everything in a syntax tree.
    /// This isn't normally needed except for diagnostics, so it's
    /// kept separate to avoid reducing incremental reuse.
    /// You can request it by invoking the `spans`
    /// method in the `dada_parse` prelude.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Spans {
        expr_spans: Expr => Span,
        named_expr_spans: NamedExpr => Span,
        local_variable_decl_spans: LocalVariableDecl => Span,
        atomic_keyword: AtomicKeyword => Span,
        async_keyword: AsyncKeyword => Span,
        fn_decl: FnDecl => Span,
        name: Name => Span,
        ty: Ty => Span,
        perm: Perm => Span,
        path: Path => Span,
        perm_paths: PermPaths => Span,
        return_ty: ReturnTy => Span,
    }
}

id!(pub struct Expr);

impl DebugWithDb<InIrDb<'_, Tree>> for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        f.debug_tuple(&format!("{self:?}"))
            .field(&self.data(db.tables()).debug(db))
            .finish()
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub enum ExprData {
    Id(Word),

    /// true, false
    BooleanLiteral(bool),

    /// (`22`, suffix: `u`), (`22_222`, suffix: `i`), etc
    IntegerLiteral(Word, Option<Word>),

    /// `integer-part.fractional-part`
    FloatLiteral(Word, Word),

    /// `"foo"` with no format strings
    ///
    /// FIXME: We should replace the FormatString token with a Concatenate
    /// that has parsed expressions.
    StringLiteral(Word),

    /// Generated by a format string like `"foo{x}bar"`, which would
    /// yield `Concatenate(StringLiteral("foo"), x, StringLiteral("bar"))`
    Concatenate(Vec<Expr>),

    /// `expr.ident`
    Dot(Expr, Word),

    /// `expr.await`
    Await(Expr),

    /// `expr(id: expr, ...)`
    Call(Expr, Vec<NamedExpr>),

    /// `expr.share`
    Share(Expr),

    /// `expr.lease`
    Lease(Expr),

    /// `expr.give`
    Give(Expr),

    /// `[shared|var|atomic] x = expr`
    Var(LocalVariableDecl, Expr),

    /// `(expr)`
    Parenthesized(Expr),

    /// `()` or `(a, b, ...)` (i.e., expr seq cannot have length 1)
    Tuple(Vec<Expr>),

    /// `if condition { block } [else { block }]`
    If(Expr, Expr, Option<Expr>),

    /// `atomic { block }`
    Atomic(AtomicKeyword, Expr),

    /// `loop { block }`
    Loop(Expr),

    /// `while condition { block }`
    While(Expr, Expr),

    // `{ ... }`, but only as part of a control-flow construct
    Seq(Vec<Expr>),

    /// `a + b`
    Op(Expr, Op, Expr),

    /// `a += b`
    OpEq(Expr, Op, Expr),

    Unary(Op, Expr),

    /// `a := b`
    Assign(Expr, Expr),

    /// continue
    Continue,

    /// break
    Break(Option<Expr>),

    /// return
    Return(Option<Expr>),

    /// parse or other error
    Error,
}

impl DebugWithDb<InIrDb<'_, Tree>> for ExprData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        match self {
            ExprData::Id(w) => f.debug_tuple("Id").field(&w.debug(db.db())).finish(),
            ExprData::BooleanLiteral(v) => f.debug_tuple("Boolean").field(&v).finish(),
            ExprData::IntegerLiteral(v, _) => {
                f.debug_tuple("Integer").field(&v.debug(db.db())).finish()
            }
            ExprData::FloatLiteral(v, d) => f
                .debug_tuple("Float")
                .field(&v.debug(db.db()))
                .field(&d.debug(db.db()))
                .finish(),
            ExprData::StringLiteral(v) => f.debug_tuple("String").field(&v.debug(db.db())).finish(),
            ExprData::Concatenate(exprs) => f
                .debug_tuple("Concatenate")
                .field(&exprs.debug(db))
                .finish(),
            ExprData::Dot(lhs, rhs) => f
                .debug_tuple("Dot")
                .field(&lhs.debug(db))
                .field(&rhs.debug(db.db()))
                .finish(),
            ExprData::Await(e) => f.debug_tuple("Await").field(&e.debug(db)).finish(),
            ExprData::Call(func, args) => f
                .debug_tuple("Call")
                .field(&func.debug(db))
                .field(&args.debug(db))
                .finish(),
            ExprData::Share(e) => f.debug_tuple("Share").field(&e.debug(db)).finish(),
            ExprData::Lease(e) => f.debug_tuple("Lease").field(&e.debug(db)).finish(),
            ExprData::Give(e) => f.debug_tuple("Give").field(&e.debug(db)).finish(),
            ExprData::Var(v, e) => f
                .debug_tuple("Var")
                .field(&v.debug(db))
                .field(&e.debug(db))
                .finish(),
            ExprData::Parenthesized(e) => {
                f.debug_tuple("Parenthesized").field(&e.debug(db)).finish()
            }
            ExprData::Tuple(e) => f.debug_tuple("Tuple").field(&e.debug(db)).finish(),
            ExprData::If(c, t, e) => f
                .debug_tuple("If")
                .field(&c.debug(db))
                .field(&t.debug(db))
                .field(&e.debug(db))
                .finish(),
            ExprData::Atomic(_, e) => f.debug_tuple("Atomic").field(&e.debug(db)).finish(),
            ExprData::Loop(e) => f.debug_tuple("Loop").field(&e.debug(db)).finish(),
            ExprData::While(c, e) => f
                .debug_tuple("While")
                .field(&c.debug(db))
                .field(&e.debug(db))
                .finish(),
            ExprData::Seq(e) => f.debug_tuple("Seq").field(&e.debug(db)).finish(),
            ExprData::Op(l, o, r) => f
                .debug_tuple("Op")
                .field(&l.debug(db))
                .field(&o)
                .field(&r.debug(db))
                .finish(),
            ExprData::OpEq(l, o, r) => f
                .debug_tuple("OpEq")
                .field(&l.debug(db))
                .field(&o)
                .field(&r.debug(db))
                .finish(),
            ExprData::Assign(l, r) => f
                .debug_tuple("Assign")
                .field(&l.debug(db))
                .field(&r.debug(db))
                .finish(),
            ExprData::Error => f.debug_tuple("Error").finish(),
            ExprData::Continue => f.debug_tuple("Continue").finish(),
            ExprData::Break(e) => f.debug_tuple("Break").field(&e.debug(db)).finish(),
            ExprData::Return(e) => f.debug_tuple("Return").field(&e.debug(db)).finish(),
            ExprData::Unary(o, e) => f
                .debug_tuple("Unary")
                .field(&o)
                .field(&e.debug(db))
                .finish(),
        }
    }
}

id!(pub struct LocalVariableDecl);

impl DebugWithDb<InIrDb<'_, Tree>> for LocalVariableDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        DebugWithDb::fmt(self.data(db.tables()), f, db)
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct LocalVariableDeclData {
    pub atomic: Option<AtomicKeyword>,
    pub name: Name,
    pub ty: Option<Ty>,
}

impl DebugWithDb<InIrDb<'_, Tree>> for LocalVariableDeclData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        f.debug_struct("LocalVariableDeclData")
            .field("atomic", &self.atomic)
            .field("name", &self.name.debug(db))
            .field("ty", &self.ty.debug(db))
            .finish()
    }
}

id!(pub struct NamedExpr);

impl DebugWithDb<InIrDb<'_, Tree>> for NamedExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        DebugWithDb::fmt(self.data(db.tables()), f, db)
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct NamedExprData {
    pub name: Option<Name>,
    pub expr: Expr,
}

impl DebugWithDb<InIrDb<'_, Tree>> for NamedExprData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        f.debug_tuple(&format!("{:?}", self.name.debug(db)))
            .field(&self.expr.debug(db))
            .finish()
    }
}

pub mod op;

// Represents the `fn` or `class` keyword that defined the function. Used primarily to carry the span.
id!(pub struct FnDecl);

impl DebugWithDb<InIrDb<'_, Tree>> for FnDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        match self.data(db.tables()) {
            FnDeclData::Fn => write!(f, "fn"),
            FnDeclData::Class => write!(f, "class"),
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash, Debug)]
pub enum FnDeclData {
    /// The `fn` in a `fn foo()` declaration
    Fn,

    /// The `class` in a `class Foo()` declaration
    Class,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash, Debug)]
pub enum EffectKeyword {
    Async(AsyncKeyword),
    Atomic(AtomicKeyword),
}

// Represents an `async` keyword. Used to carry the span.
id!(pub struct AsyncKeyword);

impl DebugWithDb<InIrDb<'_, Tree>> for AsyncKeyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, _db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        write!(f, "atomic")
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct AsyncKeywordData;

// Represents an atomic keyword. Used to carry the span.
id!(pub struct AtomicKeyword);

impl DebugWithDb<InIrDb<'_, Tree>> for AtomicKeyword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, _db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        write!(f, "atomic")
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct AtomicKeywordData;

// Represents the name of something (an identifier).
id!(pub struct Name);

impl DebugWithDb<InIrDb<'_, Tree>> for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        self.data(db.tables()).word.fmt(f, db.db())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct NameData {
    pub word: Word,
}

id!(pub struct Ty);

/// A Dada type looks like `Perm Path`.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct TyData {
    pub perm: Option<Perm>,
    pub path: Path,
}

impl DebugWithDb<InIrDb<'_, Tree>> for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let TyData { perm, path } = self.data(db.tables());
        f.debug_struct("Ty")
            .field("perm", &perm.debug(db))
            .field("path", &path.debug(db))
            .finish()
    }
}

id!(pub struct Perm);

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub enum PermData {
    My,
    Our,
    Shared(Option<PermPaths>),
    Leased(Option<PermPaths>),
    Given(Option<PermPaths>),
}

impl DebugWithDb<InIrDb<'_, Tree>> for Perm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        match self.data(db.tables()) {
            PermData::My => write!(f, "my"),
            PermData::Our => write!(f, "our"),
            PermData::Shared(paths) => write!(f, "shared({:?})", paths.debug(db)),
            PermData::Leased(paths) => write!(f, "leased({:?})", paths.debug(db)),
            PermData::Given(paths) => write!(f, "given({:?})", paths.debug(db)),
        }
    }
}

// A (possibly empty) list of paths like `{a, b.c}`
id!(pub struct PermPaths);

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct PermPathsData {
    pub paths: Vec<Path>,
}

impl DebugWithDb<InIrDb<'_, Tree>> for PermPaths {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let PermPathsData { paths } = self.data(db.tables());
        f.debug_tuple("PermPaths").field(&paths.debug(db)).finish()
    }
}

id!(pub struct Path);

/// A path like `foo` or `foo.bar`
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct PathData {
    pub start_name: Name,
    pub dot_names: Vec<Name>,
}

impl DebugWithDb<InIrDb<'_, Tree>> for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let PathData {
            start_name,
            dot_names,
        } = self.data(db.tables());
        write!(f, "{:?}.{:?}", start_name.debug(db), dot_names.debug(db))
    }
}

// Indicates a `-> type` annotation (where the type is optional).
id!(pub struct ReturnTy);

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct ReturnTyData {
    pub ty: Option<Ty>,
}

impl DebugWithDb<InIrDb<'_, Tree>> for ReturnTy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &InIrDb<'_, Tree>) -> std::fmt::Result {
        let ReturnTyData { ty } = self.data(db.tables());
        f.debug_tuple("ReturnTy").field(&ty.debug(db)).finish()
    }
}
