use crate::{
    code::syntax,
    input_file::InputFile,
    span::{Anchored, Span},
    word::Word,
};

#[salsa::tracked]
#[customize(DebugWithDb)]
pub struct Class<'db> {
    #[id]
    pub name: Word<'db>,

    pub input_file: InputFile,

    #[return_ref]
    pub signature_syntax: syntax::Signature,

    /// Overall span of the class (including any body)
    pub span: Span,
}

impl<'db> Class<'db> {
    pub fn name_span(self, db: &dyn crate::Db) -> Span {
        let signature = self.signature_syntax(db);
        signature.spans[signature.name]
    }
}

impl<'db> Anchored for Class<'db> {
    fn input_file(&self, db: &dyn crate::Db) -> InputFile {
        Class::input_file(*self, db)
    }
}

impl<'db, Db: ?Sized + crate::Db> salsa::DebugWithDb<Db> for Class<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>, _db: &Db) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
