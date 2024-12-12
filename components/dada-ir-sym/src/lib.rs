//! "Symbolic IR": High-level, type checked representaton. Derived from the AST.

#![feature(trait_upcasting)]
#![feature(async_closure)]

use dada_ir_ast::{
    ast::Identifier,
    inputs::{CompilationRoot, Krate, SourceFile},
};

/// Core functionality needed to symbolize.
#[salsa::db]
pub trait Db: dada_ir_ast::Db {
    /// Access the [`CompilationRoot`], from which all crates and sources can be reached.
    fn root(&self) -> CompilationRoot;

    /// Load a source-file from the given directory.
    /// The modules is a list of parent modules that translates to a file path.
    fn source_file<'db>(&'db self, krate: Krate, modules: &[Identifier<'db>]) -> SourceFile;
}

mod check;
pub mod ir;
pub mod well_known;

pub mod prelude {
    use crate::ir::binder::Binder;
    use crate::ir::classes::SymField;
    use crate::ir::exprs::SymExpr;
    use crate::ir::functions::{SymFunction, SymFunctionSignature};
    use crate::ir::types::SymTy;
    use dada_ir_ast::diagnostic::Errors;

    /// Return the symbol corresponding to the AST node.
    /// Implementations are memoized so that this can be called many times and will always yield the same symbol.
    pub trait Symbol<'db>: Copy {
        type Output;

        fn symbol(self, db: &'db dyn crate::Db) -> Self::Output;
    }

    pub trait CheckedBody<'db> {
        fn checked_body(self, db: &'db dyn crate::Db) -> Option<SymExpr<'db>>;
    }

    #[salsa::tracked]
    impl<'db> CheckedBody<'db> for SymFunction<'db> {
        #[salsa::tracked]
        fn checked_body(self, db: &'db dyn crate::Db) -> Option<SymExpr<'db>> {
            crate::check::blocks::check_function_body(db, self)
        }
    }

    pub trait CheckedFieldTy<'db> {
        fn checked_field_ty(self, db: &'db dyn crate::Db) -> Binder<'db, Binder<'db, SymTy<'db>>>;
    }

    #[salsa::tracked]
    impl<'db> CheckedFieldTy<'db> for SymField<'db> {
        #[salsa::tracked]
        fn checked_field_ty(self, db: &'db dyn crate::Db) -> Binder<'db, Binder<'db, SymTy<'db>>> {
            match crate::check::fields::check_field(db, self) {
                Ok(v) => v,
                Err(reported) => crate::check::fields::field_err_ty(db, self, reported),
            }
        }
    }
    pub trait CheckedSignature<'db> {
        fn checked_signature(self, db: &'db dyn crate::Db) -> Errors<SymFunctionSignature<'db>>;
    }

    #[salsa::tracked]
    impl<'db> CheckedSignature<'db> for SymFunction<'db> {
        #[salsa::tracked]
        fn checked_signature(self, db: &'db dyn crate::Db) -> Errors<SymFunctionSignature<'db>> {
            crate::check::signature::check_function_signature(db, self)
        }
    }
}
