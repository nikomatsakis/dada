use crate::span::{AbsoluteOffset, AbsoluteSpan, Anchor, Offset, Span, Spanned};

#[salsa::input]
pub struct SourceFile {
    #[return_ref]
    pub path: String,

    #[return_ref]
    pub contents: String,
}

impl<'db> Spanned<'db> for SourceFile {
    fn span(&self, db: &'db dyn crate::Db) -> crate::span::Span<'db> {
        Span {
            anchor: Anchor::SourceFile(*self),
            start: Offset::ZERO,
            end: Offset::from(self.contents(db).len()),
        }
    }
}

impl SourceFile {
    /// Returns an absolute span representing the entire source file.
    pub fn absolute_span(self, db: &dyn crate::Db) -> AbsoluteSpan {
        AbsoluteSpan {
            source_file: self,
            start: AbsoluteOffset::ZERO,
            end: AbsoluteOffset::from(self.contents(db).len()),
        }
    }
}
