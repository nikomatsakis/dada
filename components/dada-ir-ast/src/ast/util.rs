use std::ops::Deref;

use salsa::Update;

use crate::span::Span;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Update, Debug)]
pub struct AstVec<'db, T: Update> {
    //                    ------ FIXME: Bug in the derive?
    pub span: Span<'db>,
    pub values: Vec<T>,
}

impl<'db, T: Update> Deref for AstVec<'db, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<'db, T> IntoIterator for &'db AstVec<'db, T>
where
    T: Update,
{
    type Item = &'db T;

    type IntoIter = std::slice::Iter<'db, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}
