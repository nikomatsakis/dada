use crate::{input_file::InputFile, token_tree::TokenTree};

/// "Code" represents a block of code attached to a method.
/// After parsing, it just contains a token tree, but you can...
///
/// * use the `ast` method from the `dada_parse` prelude to
///   parse it into an `Ast`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct UnparsedCode<'db> {
    /// Tokens for the body (parsed when we generate the syntax tree).
    pub body_tokens: TokenTree<'db>,
}

impl<'db> UnparsedCode<'db> {
    pub fn new(body_tokens: TokenTree<'db>) -> Self {
        Self { body_tokens }
    }

    pub fn input_file(self, db: &dyn crate::Db) -> InputFile {
        self.body_tokens.input_file(db)
    }
}

impl<'db, Db: ?Sized + crate::Db> salsa::DebugWithDb<Db> for UnparsedCode<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &Db) -> std::fmt::Result {
        f.debug_struct("Code")
            .field("body_tokens", &self.body_tokens.debug(db))
            .finish()
    }
}

pub mod bir;
pub mod syntax;
pub mod validated;
