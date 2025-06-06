use dada_ir_ast::ast::{AstModule, AstWhereClause};
use dada_util::FromImpls;
use salsa::Update;
use serde::Serialize;

use crate::{
    check::scope::Scope, ir::classes::SymAggregate, ir::functions::SymFunction,
    ir::module::SymModule, ir::variables::SymVariable,
};

/// A `ScopeItem` defines a name resolution scope.
#[derive(
    Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Update, FromImpls, Serialize,
)]
pub enum ScopeItem<'db> {
    /// A module; for phasing reasons, we sometimes add this to the scope tree as the ast node.
    AstModule(AstModule<'db>),

    /// A module
    SymModule(SymModule<'db>),

    /// A class or other aggregate
    Class(SymAggregate<'db>),

    /// A function or method
    SymFunction(SymFunction<'db>),
}

pub trait ScopeTreeNode<'db>: Sized + Into<ScopeItem<'db>> {
    /// Convert this scope item into a scope for the items declared within it.
    fn into_scope(self, db: &'db dyn crate::Db) -> Scope<'db, 'db>;

    fn direct_super_scope(self, db: &'db dyn crate::Db) -> Option<ScopeItem<'db>>;

    /// Iterator that starts from self and traverses up to all super scope items.
    fn iter_super_scopes(self, db: &'db dyn crate::Db) -> impl Iterator<Item = ScopeItem<'db>> {
        let mut cursor: Option<ScopeItem<'db>> = Some(self.into());
        std::iter::from_fn(move || {
            let p = cursor?;
            cursor = p.direct_super_scope(db);
            Some(p)
        })
    }

    fn direct_generic_parameters(self, db: &'db dyn crate::Db) -> &'db Vec<SymVariable<'db>>;

    /// Compute the set of transitive generic parameters.
    /// The returned vector begins with the parameters from the outermost vector.
    fn transitive_generic_parameters(self, db: &'db dyn crate::Db) -> Vec<SymVariable<'db>> {
        let mut generic_parameters = self
            .iter_super_scopes(db)
            .flat_map(|s| s.direct_generic_parameters(db).iter().rev())
            .copied()
            .collect::<Vec<_>>();
        generic_parameters.reverse();
        generic_parameters
    }

    /// Compute the set of transitive generic parameters.
    /// The returned vector begins with the parameters from the outermost vector.
    fn expected_generic_parameters(self, db: &'db dyn crate::Db) -> usize {
        self.iter_super_scopes(db)
            .flat_map(|s| s.direct_generic_parameters(db).iter().rev())
            .copied()
            .count()
    }

    fn push_direct_ast_where_clauses(
        self,
        db: &'db dyn crate::Db,
        out: &mut Vec<AstWhereClause<'db>>,
    );

    fn push_transitive_where_clauses(
        self,
        db: &'db dyn crate::Db,
        out: &mut Vec<AstWhereClause<'db>>,
    ) {
        self.iter_super_scopes(db)
            .for_each(|s| s.push_direct_ast_where_clauses(db, out));
    }
}

impl<'db> ScopeTreeNode<'db> for ScopeItem<'db> {
    fn direct_super_scope(self, db: &'db dyn crate::Db) -> Option<ScopeItem<'db>> {
        match self {
            ScopeItem::AstModule(sym) => sym.direct_super_scope(db),
            ScopeItem::SymModule(sym) => sym.direct_super_scope(db),
            ScopeItem::Class(sym) => sym.direct_super_scope(db),
            ScopeItem::SymFunction(sym) => sym.direct_super_scope(db),
        }
    }

    fn direct_generic_parameters(self, db: &'db dyn crate::Db) -> &'db Vec<SymVariable<'db>> {
        match self {
            ScopeItem::AstModule(sym) => sym.direct_generic_parameters(db),
            ScopeItem::SymModule(sym) => sym.direct_generic_parameters(db),
            ScopeItem::Class(sym) => sym.direct_generic_parameters(db),
            ScopeItem::SymFunction(sym) => sym.direct_generic_parameters(db),
        }
    }

    fn into_scope(self, db: &'db dyn crate::Db) -> Scope<'db, 'db> {
        match self {
            ScopeItem::AstModule(sym) => sym.into_scope(db),
            ScopeItem::SymModule(sym) => sym.into_scope(db),
            ScopeItem::Class(sym) => sym.into_scope(db),
            ScopeItem::SymFunction(sym) => sym.into_scope(db),
        }
    }

    fn push_direct_ast_where_clauses(
        self,
        db: &'db dyn crate::Db,
        out: &mut Vec<AstWhereClause<'db>>,
    ) {
        match self {
            ScopeItem::AstModule(_) => {}
            ScopeItem::SymModule(_) => {}
            ScopeItem::Class(sym) => {
                sym.push_direct_ast_where_clauses(db, out);
            }
            ScopeItem::SymFunction(sym) => {
                sym.push_direct_ast_where_clauses(db, out);
            }
        }
    }
}
