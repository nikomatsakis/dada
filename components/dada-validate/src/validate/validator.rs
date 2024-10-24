use dada_id::prelude::*;
use dada_ir::code::syntax;
use dada_ir::code::validated;
use dada_ir::code::Code;
use dada_ir::diagnostic::ErrorReported;
use dada_ir::origin_table::HasOriginIn;
use dada_ir::origin_table::PushOriginIn;
use dada_ir::span::FileSpan;
use dada_ir::span::Span;
use dada_ir::storage_mode::StorageMode;
use dada_parse::prelude::*;
use std::str::FromStr;

use super::name_lookup::Definition;
use super::name_lookup::Scope;

pub(crate) struct Validator<'me> {
    db: &'me dyn crate::Db,
    code: Code,
    syntax_tree: &'me syntax::TreeData,
    tables: &'me mut validated::Tables,
    origins: &'me mut validated::Origins,
    loop_stack: Vec<validated::Expr>,
    scope: Scope<'me>,
}

impl<'me> Validator<'me> {
    pub(crate) fn new(
        db: &'me dyn crate::Db,
        code: Code,
        syntax_tree: syntax::Tree,
        tables: &'me mut validated::Tables,
        origins: &'me mut validated::Origins,
        scope: Scope<'me>,
    ) -> Self {
        let syntax_tree = syntax_tree.data(db);
        Self {
            db,
            code,
            syntax_tree,
            tables,
            origins,
            loop_stack: vec![],
            scope,
        }
    }

    fn subscope(&mut self) -> Validator<'_> {
        Validator {
            db: self.db,
            code: self.code,
            syntax_tree: self.syntax_tree,
            tables: self.tables,
            origins: self.origins,
            loop_stack: self.loop_stack.clone(),
            scope: self.scope.subscope(),
        }
    }

    pub(crate) fn syntax_tables(&self) -> &'me syntax::Tables {
        &self.syntax_tree.tables
    }

    fn add<V, O>(&mut self, data: V, origin: O) -> V::Key
    where
        V: dada_id::InternValue<Table = validated::Tables>,
        V::Key: PushOriginIn<validated::Origins, Origin = O>,
    {
        let key = self.tables.add(data);
        self.origins.push(key, origin);
        key
    }

    fn or_error(
        &mut self,
        data: Result<validated::Expr, ErrorReported>,
        origin: syntax::Expr,
    ) -> validated::Expr {
        data.unwrap_or_else(|ErrorReported| self.add(validated::ExprData::Error, origin))
    }

    fn span(&self, e: impl HasOriginIn<syntax::Spans, Origin = Span>) -> FileSpan {
        self.code.syntax_tree(self.db).spans(self.db)[e].in_file(self.code.filename(self.db))
    }

    fn empty_tuple(&mut self, origin: syntax::Expr) -> validated::Expr {
        self.add(validated::ExprData::Tuple(vec![]), origin)
    }

    pub(crate) fn validate_expr(&mut self, expr: syntax::Expr) -> validated::Expr {
        match expr.data(self.syntax_tables()) {
            syntax::ExprData::Dot(..) | syntax::ExprData::Id(_) => {
                let place = self.validate_expr_as_place(expr);
                self.place_to_expr(place, expr)
            }

            syntax::ExprData::BooleanLiteral(b) => {
                self.add(validated::ExprData::BooleanLiteral(*b), expr)
            }

            syntax::ExprData::IntegerLiteral(w) => {
                let raw_str = w.as_str(self.db);
                let without_underscore: String = raw_str.chars().filter(|&c| c != '_').collect();
                match u64::from_str(&without_underscore) {
                    Ok(v) => self.add(validated::ExprData::IntegerLiteral(v), expr),
                    Err(e) => {
                        dada_ir::error!(
                            self.span(expr),
                            "`{}` is not a valid integer: {}",
                            w.as_str(self.db),
                            e,
                        )
                        .emit(self.db);
                        self.add(validated::ExprData::Error, expr)
                    }
                }
            }

            syntax::ExprData::StringLiteral(w) => {
                self.add(validated::ExprData::StringLiteral(*w), expr)
            }

            syntax::ExprData::Await(future_expr) => {
                let validated_future_expr = self.validate_expr(*future_expr);
                self.add(validated::ExprData::Await(validated_future_expr), expr)
            }

            syntax::ExprData::Call(func_expr, named_exprs) => {
                let validated_func_expr = self.validate_expr(*func_expr);
                let validated_named_exprs = self.validate_named_exprs(named_exprs);
                self.add(
                    validated::ExprData::Call(validated_func_expr, validated_named_exprs),
                    expr,
                )
            }

            syntax::ExprData::Share(target_expr) => {
                self.validate_permission_expr(expr, *target_expr, validated::ExprData::Share)
            }

            syntax::ExprData::Lease(target_expr) => {
                self.validate_permission_expr(expr, *target_expr, validated::ExprData::Lease)
            }

            syntax::ExprData::Give(target_expr) => {
                self.validate_permission_expr(expr, *target_expr, validated::ExprData::Give)
            }

            syntax::ExprData::Var(decl, initializer_expr) => {
                let decl_data = decl.data(self.syntax_tables());
                let local_variable = self.add(
                    validated::LocalVariableData {
                        name: Some(decl_data.name),
                        storage_mode: decl_data.mode.unwrap_or(StorageMode::Shared),
                    },
                    expr,
                );
                let place = self.add(validated::PlaceData::LocalVariable(local_variable), expr);
                let validated_initializer_expr = self.validate_expr(*initializer_expr);
                self.scope.insert(decl_data.name, local_variable);
                self.add(
                    validated::ExprData::Assign(place, validated_initializer_expr),
                    expr,
                )
            }

            syntax::ExprData::Parenthesized(parenthesized_expr) => {
                self.validate_expr(*parenthesized_expr)
            }

            syntax::ExprData::Tuple(element_exprs) => {
                let validated_exprs = element_exprs
                    .iter()
                    .map(|expr| self.validate_expr(*expr))
                    .collect();
                self.add(validated::ExprData::Tuple(validated_exprs), expr)
            }

            syntax::ExprData::If(condition_expr, then_expr, else_expr) => {
                let validated_condition_expr = self.validate_expr(*condition_expr);
                let validated_then_expr = self.validate_expr(*then_expr);
                let validated_else_expr = match else_expr {
                    None => self.empty_tuple(expr),
                    Some(else_expr) => self.validate_expr(*else_expr),
                };
                self.add(
                    validated::ExprData::If(
                        validated_condition_expr,
                        validated_then_expr,
                        validated_else_expr,
                    ),
                    expr,
                )
            }

            syntax::ExprData::Atomic(atomic_expr) => {
                let validated_atomic_expr = self.validate_expr(*atomic_expr);
                self.add(validated::ExprData::Atomic(validated_atomic_expr), expr)
            }

            syntax::ExprData::Loop(body_expr) => {
                // Create the `validated::Expr` up front with "Error" to start; we are going to replace this later
                // with the actual loop.
                let loop_expr = self.add(validated::ExprData::Error, expr);

                let mut subscope = self.subscope();
                subscope.loop_stack.push(loop_expr);
                let validated_body_expr = subscope.validate_expr(*body_expr);

                self.tables[loop_expr] = validated::ExprData::Loop(validated_body_expr);

                loop_expr
            }

            syntax::ExprData::While(condition_expr, body_expr) => {
                // while C { E }
                //
                // lowers to
                //
                // loop { E; if C {} else {break} }

                let loop_expr = self.add(validated::ExprData::Error, expr);

                // lower the condition C
                let validated_condition_expr = self.validate_expr(*condition_expr);

                // lower the body E, in a subscope so that `break` breaks out from `loop_expr`
                let mut subscope = self.subscope();
                subscope.loop_stack.push(loop_expr);
                let validated_body_expr = subscope.validate_expr(*body_expr);

                let if_break_expr = {
                    // break
                    let empty_tuple = self.empty_tuple(expr);
                    let break_expr = self.add(
                        validated::ExprData::Break {
                            from_expr: loop_expr,
                            with_value: empty_tuple,
                        },
                        expr,
                    );

                    //
                    self.add(
                        validated::ExprData::If(validated_condition_expr, empty_tuple, break_expr),
                        expr,
                    )
                };

                // replace `loop_expr` contents with the loop body `{E; if C {} else break}`
                let loop_body = self.add(
                    validated::ExprData::Seq(vec![validated_body_expr, if_break_expr]),
                    expr,
                );
                self.tables[loop_expr] = validated::ExprData::Loop(loop_body);

                loop_expr
            }

            syntax::ExprData::Op(lhs_expr, op, rhs_expr) => {
                let validated_lhs_expr = self.validate_expr(*lhs_expr);
                let validated_rhs_expr = self.validate_expr(*rhs_expr);
                self.add(
                    validated::ExprData::Op(validated_lhs_expr, *op, validated_rhs_expr),
                    expr,
                )
            }

            syntax::ExprData::OpEq(lhs_expr, op, rhs_expr) => {
                let result = try {
                    let (validated_opt_temp_expr, validated_lhs_place) =
                        self.validate_expr_as_place(*lhs_expr)?;
                    let validated_lhs_expr =
                        self.add(validated::ExprData::Place(validated_lhs_place), expr);
                    let validated_rhs_expr = self.validate_expr(*rhs_expr);
                    let validated_op_expr = self.add(
                        validated::ExprData::Op(validated_lhs_expr, *op, validated_rhs_expr),
                        expr,
                    );
                    let assign_expr = self.add(
                        validated::ExprData::Assign(validated_lhs_place, validated_op_expr),
                        expr,
                    );
                    self.maybe_seq(validated_opt_temp_expr, assign_expr, expr)
                };
                self.or_error(result, expr)
            }

            syntax::ExprData::Assign(lhs_expr, rhs_expr) => {
                let place = try {
                    let (validated_opt_temp_expr, validated_lhs_place) =
                        self.validate_expr_as_place(*lhs_expr)?;
                    let validated_rhs_expr = self.validate_expr(*rhs_expr);
                    let assign_expr = self.add(
                        validated::ExprData::Assign(validated_lhs_place, validated_rhs_expr),
                        expr,
                    );
                    self.maybe_seq(validated_opt_temp_expr, assign_expr, expr)
                };
                self.or_error(place, expr)
            }

            syntax::ExprData::Error => self.add(validated::ExprData::Error, expr),
            syntax::ExprData::Seq(exprs) => {
                let validated_exprs: Vec<_> =
                    exprs.iter().map(|expr| self.validate_expr(*expr)).collect();
                self.add(validated::ExprData::Seq(validated_exprs), expr)
            }
        }
    }

    fn maybe_seq(
        &mut self,
        expr1: Option<validated::Expr>,
        expr2: validated::Expr,
        origin: syntax::Expr,
    ) -> validated::Expr {
        if let Some(expr1) = expr1 {
            self.add(validated::ExprData::Seq(vec![expr1, expr2]), origin)
        } else {
            expr2
        }
    }

    fn place_to_expr(
        &mut self,
        data: Result<(Option<validated::Expr>, validated::Place), ErrorReported>,
        origin: syntax::Expr,
    ) -> validated::Expr {
        match data {
            Ok((opt_assign_expr, place)) => {
                let place_expr = self.add(validated::ExprData::Place(place), origin);
                self.maybe_seq(opt_assign_expr, place_expr, origin)
            }
            Err(ErrorReported) => self.add(validated::ExprData::Error, origin),
        }
    }

    fn validate_permission_expr(
        &mut self,
        perm_expr: syntax::Expr,
        target_expr: syntax::Expr,
        perm_variant: impl Fn(validated::Place) -> validated::ExprData,
    ) -> validated::Expr {
        let validated_data = try {
            let (opt_temporary_expr, place) = self.validate_expr_as_place(target_expr)?;
            let permission_expr = self.add(perm_variant(place), perm_expr);
            self.maybe_seq(opt_temporary_expr, permission_expr, perm_expr)
        };
        self.or_error(validated_data, perm_expr)
    }

    fn validate_expr_as_place(
        &mut self,
        expr: syntax::Expr,
    ) -> Result<(Option<validated::Expr>, validated::Place), ErrorReported> {
        match expr.data(self.syntax_tables()) {
            syntax::ExprData::Id(name) => Ok((
                None,
                match self.scope.lookup(*name) {
                    Some(Definition::Class(c)) => self.add(validated::PlaceData::Class(c), expr),
                    Some(Definition::Function(f)) => {
                        self.add(validated::PlaceData::Function(f), expr)
                    }
                    Some(Definition::LocalVariable(lv)) => {
                        self.add(validated::PlaceData::LocalVariable(lv), expr)
                    }
                    Some(Definition::Intrinsic(i)) => {
                        self.add(validated::PlaceData::Intrinsic(i), expr)
                    }
                    None => {
                        return Err(dada_ir::error!(
                            self.span(expr),
                            "can't find anything named `{}`",
                            name.as_str(self.db)
                        )
                        .emit(self.db))
                    }
                },
            )),
            syntax::ExprData::Dot(owner_expr, field) => {
                let (opt_temporary_expr, validated_owner_place) =
                    self.validate_expr_as_place(*owner_expr)?;
                Ok((
                    opt_temporary_expr,
                    self.add(
                        validated::PlaceData::Dot(validated_owner_place, *field),
                        expr,
                    ),
                ))
            }
            syntax::ExprData::Parenthesized(parenthesized_expr) => {
                self.validate_expr_as_place(*parenthesized_expr)
            }
            syntax::ExprData::Error => Err(ErrorReported),
            _ => {
                let (assign_expr, temporary_place) = self.validate_expr_in_temporary(expr);
                Ok((Some(assign_expr), temporary_place))
            }
        }
    }

    /// Given an expression E, create a new temporary variable V and return a `V = E` expression.
    fn validate_expr_in_temporary(
        &mut self,
        expr: syntax::Expr,
    ) -> (validated::Expr, validated::Place) {
        let local_variable = self.add(
            validated::LocalVariableData {
                name: None,
                storage_mode: StorageMode::Var,
            },
            expr,
        );

        let validated_place = self.add(validated::PlaceData::LocalVariable(local_variable), expr);
        let validated_expr = self.validate_expr(expr);

        let assign_expr = self.add(
            validated::ExprData::Assign(validated_place, validated_expr),
            expr,
        );
        (assign_expr, validated_place)
    }

    fn validate_named_exprs(
        &mut self,
        named_exprs: &[syntax::NamedExpr],
    ) -> Vec<validated::NamedExpr> {
        named_exprs
            .iter()
            .map(|named_expr| self.validate_named_expr(*named_expr))
            .collect()
    }

    fn validate_named_expr(&mut self, named_expr: syntax::NamedExpr) -> validated::NamedExpr {
        let syntax::NamedExprData { name, expr } = named_expr.data(self.syntax_tables());
        let validated_expr = self.validate_expr(*expr);
        self.add(
            validated::NamedExprData {
                name: *name,
                expr: validated_expr,
            },
            named_expr,
        )
    }
}
