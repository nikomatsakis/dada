use crate::ast::Path;

use super::{Expected, Parse, ParseFail, Parser};

impl<'db> Parse<'db> for Path<'db> {
    type Output = Self;

    fn opt_parse(
        _db: &'db dyn crate::Db,
        parser: &mut Parser<'_, 'db>,
    ) -> Result<Option<Self>, ParseFail<'db>> {
        let Ok(id) = parser.eat_id() else {
            return Ok(None);
        };
        let mut ids = vec![id];

        while parser.eat_op(".").is_ok() {
            let id = parser.eat_id()?;
            ids.push(id);
        }

        Ok(Some(Path { ids }))
    }

    fn expected() -> Expected {
        Expected::Path
    }
}

pub trait OrOptParse<'db, Enum> {
    fn or_opt_parse<Variant2>(
        self,
        db: &'db dyn crate::Db,
        parser: &mut Parser<'_, 'db>,
    ) -> Result<Option<Enum>, ParseFail<'db>>
    where
        Variant2: Parse<'db, Output: Into<Enum>>;
}

impl<'db, Enum, Variant1> OrOptParse<'db, Enum> for Result<Option<Variant1>, ParseFail<'db>>
where
    Variant1: Into<Enum>,
{
    fn or_opt_parse<Variant2>(
        self,
        db: &'db dyn crate::Db,
        parser: &mut Parser<'_, 'db>,
    ) -> Result<Option<Enum>, ParseFail<'db>>
    where
        Variant2: Parse<'db, Output: Into<Enum>>,
    {
        match self {
            Ok(Some(v1)) => Ok(Some(v1.into())),
            Ok(None) => match Variant2::opt_parse(db, parser) {
                Ok(Some(v2)) => Ok(Some(v2.into())),
                Ok(None) => Ok(None),
                Err(err) => Err(err),
            },
            Err(err) => Err(err),
        }
    }
}
