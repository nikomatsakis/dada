use crate::{
    input_file::InputFile,
    span::{Anchored, FileSpan, Span},
    token::Token,
};

#[salsa::tracked]
#[customize(DebugWithDb)]
pub struct TokenTree<'db> {
    pub input_file: InputFile,
    pub span: Span,
    #[return_ref]
    pub tokens: Vec<Token>,
}

impl Anchored for TokenTree {
    fn input_file(&self, db: &dyn crate::Db) -> InputFile {
        TokenTree::input_file(*self, db)
    }
}

impl<Db: ?Sized + crate::Db> salsa::DebugWithDb<Db> for TokenTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, db: &Db) -> std::fmt::Result {
        let db = db.as_dyn_ir_db();
        let file_span: FileSpan = self.span(db).anchor_to(db, self);
        write!(f, "Tokens({:?})", file_span.into_debug(db))
    }
}

impl TokenTree {
    #[allow(clippy::len_without_is_empty)]
    pub fn len(self, db: &dyn crate::Db) -> u32 {
        self.span(db).len()
    }

    pub fn spanned_tokens(self, db: &dyn crate::Db) -> impl Iterator<Item = (Span, Token)> + '_ {
        let mut start = self.span(db).start;
        self.tokens(db).iter().map(move |token| {
            let len = token.span_len(db);
            let span = Span::from(start, start + len);
            start = start + len;
            (span, *token)
        })
    }
}
