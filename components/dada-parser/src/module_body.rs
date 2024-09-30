use std::path::PathBuf;

use dada_ir_ast::{
    ast::{
        AstClassItem, AstFunction, AstItem, AstModule, AstPath, AstUseItem, Identifier, SpanVec,
    },
    diagnostic::Diagnostic,
};

use super::{miscellaneous::OrOptParse, tokenizer::Keyword, Expected, Parse, ParseFail, Parser};

impl<'db> Parse<'db> for AstModule<'db> {
    type Output = Self;

    fn opt_parse(
        db: &'db dyn crate::Db,
        parser: &mut Parser<'_, 'db>,
    ) -> Result<Option<Self>, ParseFail<'db>> {
        let mut items: Vec<AstItem<'db>> = vec![];

        // Derive the name of the module from the source file in the span.
        // Is this...ok?
        let path = PathBuf::from(parser.last_span().source_file(db).path(db));
        let name = match path.file_stem() {
            None => Identifier::new(db, "<input>".to_string()),
            Some(s) => Identifier::new(db, s.to_string_lossy().to_string()),
        };

        // Parse items, skipping unrecognized tokens.
        let start_span = parser.peek_span();
        while let Some(token) = parser.peek() {
            let span = token.span;
            match AstItem::opt_parse(db, parser) {
                Ok(Some(v)) => items.push(v),
                Err(e) => parser.push_diagnostic(e.into_diagnostic(db)),
                Ok(None) => {
                    parser.eat_next_token().unwrap();
                    parser.push_diagnostic(Diagnostic::error(
                        db,
                        span,
                        "expected a module-level item",
                    ));
                }
            }
        }

        Ok(Some(AstModule::new(
            db,
            name,
            SpanVec {
                span: start_span.to(parser.last_span()),
                values: items,
            },
        )))
    }

    fn expected() -> Expected {
        panic!("infallible")
    }
}

impl<'db> Parse<'db> for AstItem<'db> {
    type Output = Self;

    fn opt_parse(
        db: &'db dyn crate::Db,
        parser: &mut Parser<'_, 'db>,
    ) -> Result<Option<Self>, ParseFail<'db>> {
        AstClassItem::opt_parse(db, parser)
            .or_opt_parse::<Self, AstUseItem<'db>>(db, parser)
            .or_opt_parse::<Self, AstFunction<'db>>(db, parser)
    }

    fn expected() -> Expected {
        panic!("module-level item (class, function, use)")
    }
}

/// use path [as name];
impl<'db> Parse<'db> for AstUseItem<'db> {
    type Output = Self;

    fn opt_parse(
        db: &'db dyn crate::Db,
        parser: &mut Parser<'_, 'db>,
    ) -> Result<Option<Self>, ParseFail<'db>> {
        let Ok(start) = parser.eat_keyword(Keyword::Use) else {
            return Ok(None);
        };

        let crate_name = parser.eat_id()?;
        let _dot = parser.eat_op(".")?;
        let path = AstPath::eat(db, parser)?;

        let as_id = if parser.eat_keyword(Keyword::As).is_ok() {
            Some(parser.eat_id()?)
        } else {
            None
        };

        parser.eat_op(";")?;

        Ok(Some(AstUseItem::new(
            db,
            start.to(parser.last_span()),
            crate_name,
            path,
            as_id,
        )))
    }

    fn expected() -> Expected {
        Expected::Keyword(Keyword::Use)
    }
}
