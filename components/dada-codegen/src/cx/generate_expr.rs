use std::sync::Arc;

use dada_ir_ast::{ast::PermissionOp, diagnostic::Reported};
use dada_ir_sym::{primitive::SymPrimitiveKind, subst::Subst, symbol::SymVariable, ty::SymTyName};
use dada_object_check::object_ir::{
    MatchArm, ObjectBinaryOp, ObjectExpr, ObjectExprKind, ObjectGenericTerm, ObjectTy,
    ObjectTyKind, PrimitiveLiteral,
};
use dada_util::Map;
use wasm_encoder::{Instruction, ValType};
use wasm_place_repr::{WasmLocal, WasmPlaceRepr};

use super::{wasm_repr::WasmRepr, Cx};

pub(crate) mod wasm_place_repr;

pub(crate) struct ExprCodegen<'cx, 'db> {
    cx: &'cx mut Cx<'db>,

    generics: Map<SymVariable<'db>, ObjectGenericTerm<'db>>,

    /// Accumulates wasm locals. We make no effort to reduce the number of local variables created.
    wasm_locals: Vec<wasm_encoder::ValType>,

    /// Local variable that stores starting address in our stack frame
    wasm_stack_pointer: WasmLocal,

    /// Values we are putting onto the stack frame (actually located in the WASM heap)
    wasm_stack_frame_size: u32,

    /// Maps each Dada variable to a range of wasm locals. Note that a single value can be inlined into multiple wasm locals.
    variables: Map<SymVariable<'db>, Arc<WasmPlaceRepr>>,

    /// Accumulates wasm instructions.
    instructions: Vec<Instruction<'static>>,
}

impl<'cx, 'db> ExprCodegen<'cx, 'db> {
    pub fn new(
        cx: &'cx mut Cx<'db>,
        generics: Map<SymVariable<'db>, ObjectGenericTerm<'db>>,
    ) -> Self {
        // Initially there is one local variable, the stack pointer.
        Self {
            cx,
            generics,
            wasm_locals: vec![ValType::I32],
            variables: Default::default(),
            instructions: Default::default(),
            wasm_stack_frame_size: 0,
            wasm_stack_pointer: WasmLocal { index: 0 },
        }
    }

    pub fn into_function(self) -> wasm_encoder::Function {
        let mut f = wasm_encoder::Function::new_with_locals_types(self.wasm_locals);
        for instruction in self.instructions {
            f.instruction(&instruction);
        }
        f
    }

    pub fn pop_arguments(&mut self, inputs: &[SymVariable<'db>], input_tys: &[ObjectTy<'db>]) {
        assert_eq!(inputs.len(), input_tys.len());
        for (&input, &input_ty) in inputs.iter().zip(input_tys).rev() {
            self.insert_variable(input, input_ty);
            self.pop_and_store(&self.place_for_local(input));
        }
        self.instructions
            .push(Instruction::LocalSet(self.wasm_stack_pointer.index));
    }

    /// Generate code to execute the expression, leaving the result on the top of the wasm stack.
    pub fn push_expr(&mut self, expr: ObjectExpr<'db>) {
        let db = self.cx.db;
        match *expr.kind(db) {
            ObjectExprKind::Semi(object_expr, object_expr1) => {
                self.push_expr(object_expr);
                self.pop_and_drop(object_expr.ty(db));
                self.push_expr(object_expr1);
            }
            ObjectExprKind::Tuple(ref elements) => {
                // the representation of a tuple is inlined onto the stack (like any other struct type)
                for &element in elements {
                    self.push_expr(element);
                }
            }
            ObjectExprKind::Primitive(literal) => self.push_literal(expr.ty(db), literal),
            ObjectExprKind::LetIn {
                lv,
                sym_ty: _,
                ty,
                initializer,
                body,
            } => {
                self.insert_variable(lv, ty);

                if let Some(initializer) = initializer {
                    self.push_expr(initializer);
                    self.pop_and_store(&self.variables[&lv].clone());
                } else {
                    // FIXME: should zero out the values
                }

                self.push_expr(body);
            }
            ObjectExprKind::Await {
                future,
                await_keyword: _,
            } => {
                self.push_expr(future);
                // FIXME: for now we just ignore futures and execute everything synchronously
            }
            ObjectExprKind::Assign { place, value } => {
                let wasm_place = self.place(place);
                self.push_expr(value);

                // FIXME: have to drop the old value

                self.pop_and_store(&wasm_place);
            }
            ObjectExprKind::PermissionOp(permission_op, object_place_expr) => {
                let wasm_place_repr = self.place(object_place_expr);
                match permission_op {
                    PermissionOp::Lease => {
                        self.push_leased_from(&wasm_place_repr);
                    }

                    PermissionOp::Share => {
                        self.push_shared_from(&wasm_place_repr);
                    }

                    PermissionOp::Give => {
                        self.push_from(&wasm_place_repr);
                    }
                }
            }
            ObjectExprKind::Call {
                function,
                ref substitution,
                sym_substitution: _,
                ref arg_temps,
            } => {
                let fn_args = substitution.subst_vars(db, &self.generics);
                let fn_index = self.cx.declare_fn(function, fn_args);

                // First push the stack pointer for the new function;
                self.push_pointer(self.next_stack_frame());

                // Now push each of the arguments in turn.
                for arg_temp in arg_temps {
                    let place = self.variables[arg_temp].clone();
                    self.push_from(&place);
                }

                self.instructions.push(Instruction::Call(fn_index.0));
            }
            ObjectExprKind::Return(object_expr) => {
                self.push_expr(object_expr);
                self.instructions.push(Instruction::Return);
            }
            ObjectExprKind::Not {
                operand,
                op_span: _,
            } => {
                self.push_expr(operand);
                self.instructions.push(Instruction::I32Const(1));
                self.instructions.push(Instruction::I32Xor);
            }
            ObjectExprKind::BinaryOp(binary_op, object_expr, object_expr1) => {
                self.push_expr(object_expr);
                self.push_expr(object_expr1);
                self.execute_binary_op(binary_op, object_expr.ty(db), object_expr.ty(db));
            }
            ObjectExprKind::Aggregate { ty, ref fields } => {
                let wasm_repr = self.cx.wasm_repr_of_type(ty, &self.generics);
                match wasm_repr {
                    WasmRepr::Struct(field_reprs) => {
                        assert_eq!(fields.len(), field_reprs.len());
                        for &field in fields {
                            self.push_expr(field);
                        }
                    }
                    WasmRepr::Class(field_reprs) => {
                        assert_eq!(fields.len(), field_reprs.len());

                        // push flag word
                        self.instructions.push(Instruction::I32Const(1));

                        for &field in fields {
                            self.push_expr(field);
                        }
                    }
                    WasmRepr::Val(_) | WasmRepr::Nothing => {
                        panic!("not an aggregate: {ty:?}")
                    }
                }
            }
            ObjectExprKind::Match { ref arms } => {
                self.push_match_expr(expr.ty(db), arms);
            }
            ObjectExprKind::Error(reported) => self.push_error(reported),
        }
    }

    fn pop_and_drop(&mut self, _of_type: ObjectTy<'db>) {
        // currently everything is stack allocated, no dropping required
    }

    pub(super) fn pop_and_return(&mut self, _of_type: ObjectTy<'db>) {
        self.instructions.push(Instruction::Return);
    }

    /// Push the correct instructions to execute `binary_op` on operands of type `lhs_ty` and `rhs_ty`
    fn execute_binary_op(
        &mut self,
        binary_op: ObjectBinaryOp,
        lhs_ty: ObjectTy<'db>,
        rhs_ty: ObjectTy<'db>,
    ) {
        match self.primitive_kind(lhs_ty) {
            Ok(prim_kind) => {
                assert_eq!(self.primitive_kind(rhs_ty), Ok(prim_kind));
                self.execute_binary_op_on_primitives(binary_op, prim_kind)
            }
            Err(e) => match e {
                NotPrimitive::DeadCode => (),
                NotPrimitive::OtherType => panic!(
                    "don't know how to execute a binary op on ({:?}, {:?})",
                    lhs_ty, rhs_ty
                ),
            },
        }
    }

    /// Push the correct instructions to execute `binary_op` on operands of type `prim_kind`
    fn execute_binary_op_on_primitives(
        &mut self,
        binary_op: ObjectBinaryOp,
        prim_kind: SymPrimitiveKind,
    ) {
        let instruction = match (prim_kind, binary_op) {
            (SymPrimitiveKind::Char, ObjectBinaryOp::Add)
            | (SymPrimitiveKind::Char, ObjectBinaryOp::Sub)
            | (SymPrimitiveKind::Char, ObjectBinaryOp::Mul)
            | (SymPrimitiveKind::Char, ObjectBinaryOp::Div)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::Add)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::Sub)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::Mul)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::Div) => {
                panic!("invalid primitive binary op: {binary_op:?}, {prim_kind:?}")
            }

            (SymPrimitiveKind::Char, ObjectBinaryOp::GreaterThan)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::GreaterThan) => Instruction::I32GtU,

            (SymPrimitiveKind::Char, ObjectBinaryOp::LessThan)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::LessThan) => Instruction::I32LtU,

            (SymPrimitiveKind::Char, ObjectBinaryOp::GreaterEqual)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::GreaterEqual) => Instruction::I32GeU,

            (SymPrimitiveKind::Char, ObjectBinaryOp::LessEqual)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::LessEqual) => Instruction::I32GeU,

            (SymPrimitiveKind::Char, ObjectBinaryOp::EqualEqual)
            | (SymPrimitiveKind::Bool, ObjectBinaryOp::EqualEqual) => Instruction::I32Eq,

            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Add) if bits <= 32 => {
                Instruction::I32Add
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Sub) if bits <= 32 => {
                Instruction::I32Sub
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Mul) if bits <= 32 => {
                Instruction::I32Mul
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Div) if bits <= 32 => {
                Instruction::I32DivS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::GreaterThan) if bits <= 32 => {
                Instruction::I32GtS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::LessThan) if bits <= 32 => {
                Instruction::I32LtS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::GreaterEqual) if bits <= 32 => {
                Instruction::I32GeS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::LessEqual) if bits <= 32 => {
                Instruction::I32LeS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::EqualEqual) if bits <= 32 => {
                Instruction::I32Eq
            }

            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Add) if bits <= 64 => {
                Instruction::I64Add
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Sub) if bits <= 64 => {
                Instruction::I64Sub
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Mul) if bits <= 64 => {
                Instruction::I64Mul
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::Div) if bits <= 64 => {
                Instruction::I64DivS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::GreaterThan) if bits <= 64 => {
                Instruction::I64GtS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::LessThan) if bits <= 64 => {
                Instruction::I64LtS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::GreaterEqual) if bits <= 64 => {
                Instruction::I64GeS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::LessEqual) if bits <= 64 => {
                Instruction::I64LeS
            }
            (SymPrimitiveKind::Int { bits }, ObjectBinaryOp::EqualEqual) if bits <= 64 => {
                Instruction::I64Eq
            }

            (SymPrimitiveKind::Isize, ObjectBinaryOp::Add) => Instruction::I32Add,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::Sub) => Instruction::I32Sub,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::Mul) => Instruction::I32Mul,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::Div) => Instruction::I32DivS,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::GreaterThan) => Instruction::I32GtS,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::LessThan) => Instruction::I32LtS,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::GreaterEqual) => Instruction::I32GeS,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::LessEqual) => Instruction::I32LeS,
            (SymPrimitiveKind::Isize, ObjectBinaryOp::EqualEqual) => Instruction::I32Eq,

            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Add) if bits <= 32 => {
                Instruction::I32Add
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Sub) if bits <= 32 => {
                Instruction::I32Sub
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Mul) if bits <= 32 => {
                Instruction::I32Mul
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Div) if bits <= 32 => {
                Instruction::I32DivU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::GreaterThan) if bits <= 32 => {
                Instruction::I32GtU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::LessThan) if bits <= 32 => {
                Instruction::I32LtU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::GreaterEqual) if bits <= 32 => {
                Instruction::I32GeU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::LessEqual) if bits <= 32 => {
                Instruction::I32LeU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::EqualEqual) if bits <= 32 => {
                Instruction::I32Eq
            }

            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Add) if bits <= 64 => {
                Instruction::I64Add
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Sub) if bits <= 64 => {
                Instruction::I64Sub
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Mul) if bits <= 64 => {
                Instruction::I64Mul
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::Div) if bits <= 64 => {
                Instruction::I64DivU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::GreaterThan) if bits <= 64 => {
                Instruction::I64GtU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::LessThan) if bits <= 64 => {
                Instruction::I64LtU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::GreaterEqual) if bits <= 64 => {
                Instruction::I64GeU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::LessEqual) if bits <= 64 => {
                Instruction::I64LeU
            }
            (SymPrimitiveKind::Uint { bits }, ObjectBinaryOp::EqualEqual) if bits <= 64 => {
                Instruction::I64Eq
            }

            (SymPrimitiveKind::Usize, ObjectBinaryOp::Add) => Instruction::I32Add,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::Sub) => Instruction::I32Sub,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::Mul) => Instruction::I32Mul,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::Div) => Instruction::I32DivU,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::GreaterThan) => Instruction::I32GtU,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::LessThan) => Instruction::I32LtU,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::GreaterEqual) => Instruction::I32GeU,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::LessEqual) => Instruction::I32LeU,
            (SymPrimitiveKind::Usize, ObjectBinaryOp::EqualEqual) => Instruction::I32Eq,

            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Add) if bits <= 32 => {
                Instruction::F32Add
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Sub) if bits <= 32 => {
                Instruction::F32Sub
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Mul) if bits <= 32 => {
                Instruction::F32Mul
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Div) if bits <= 32 => {
                Instruction::F32Div
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::GreaterThan) if bits <= 32 => {
                Instruction::F32Gt
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::LessThan) if bits <= 32 => {
                Instruction::F32Lt
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::GreaterEqual) if bits <= 32 => {
                Instruction::F32Ge
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::LessEqual) if bits <= 32 => {
                Instruction::F32Le
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::EqualEqual) if bits <= 32 => {
                Instruction::F32Eq
            }

            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Add) if bits <= 64 => {
                Instruction::F64Add
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Sub) if bits <= 64 => {
                Instruction::F64Sub
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Mul) if bits <= 64 => {
                Instruction::F64Mul
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::Div) if bits <= 64 => {
                Instruction::F64Div
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::GreaterThan) if bits <= 64 => {
                Instruction::F64Gt
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::LessThan) if bits <= 64 => {
                Instruction::F64Lt
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::GreaterEqual) if bits <= 64 => {
                Instruction::F64Ge
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::LessEqual) if bits <= 64 => {
                Instruction::F64Le
            }
            (SymPrimitiveKind::Float { bits }, ObjectBinaryOp::EqualEqual) if bits <= 64 => {
                Instruction::F64Eq
            }

            (SymPrimitiveKind::Int { bits: _ }, _)
            | (SymPrimitiveKind::Uint { bits: _ } | SymPrimitiveKind::Float { bits: _ }, _) => {
                panic!("invalid number of bits for scalar: {prim_kind:?}")
            }
        };

        self.instructions.push(instruction);
    }

    /// Return the primitive kind that represents `ty` or `Err` if `ty` is not a primitive.
    fn primitive_kind(&self, ty: ObjectTy<'db>) -> Result<SymPrimitiveKind, NotPrimitive> {
        let db = self.cx.db;
        match ty.kind(db) {
            ObjectTyKind::Named(ty_name, _ty_args) => match ty_name {
                SymTyName::Primitive(sym_primitive) => Ok(sym_primitive.kind(db)),
                SymTyName::Aggregate(_) | SymTyName::Future | SymTyName::Tuple { arity: _ } => {
                    Err(NotPrimitive::OtherType)
                }
            },
            ObjectTyKind::Var(sym_variable) => {
                self.primitive_kind(self.generics[sym_variable].assert_type(db))
            }
            ObjectTyKind::Never | ObjectTyKind::Error(_) => Err(NotPrimitive::DeadCode),
            ObjectTyKind::Infer(_) => panic!("unexpected inference variable"),
        }
    }

    fn push_match_expr(&mut self, match_ty: ObjectTy<'db>, arms: &[MatchArm<'db>]) {
        let Some((if_arm, else_arms)) = arms.split_first() else {
            return;
        };

        if let Some(condition) = if_arm.condition {
            // Evaluate the condition.
            self.push_expr(condition);

            // The `If` block will execute the next set of instructions
            // if the condition was true. Otherwise it will skip to the `Else` or `End.`
            let block_type = self.block_type(match_ty);
            self.instructions.push(Instruction::If(block_type));

            // Code to execute if true.
            self.push_expr(if_arm.body);

            // If false push an `Else` and evaluate it recursively.
            self.instructions.push(Instruction::Else);
            self.push_match_expr(match_ty, else_arms);

            // End the if.
            self.instructions.push(Instruction::End);
        } else {
            // Execute body unconditionally.
            self.push_expr(if_arm.body);

            // Any remaining arms are ignored.
            let _ = else_arms;
        }
    }

    /// [Block control-flow instructions][cfi] like `if` and friends
    /// come equipped with an associated "block type". This is a function
    /// type indicating the *inputs* they consume from the stack (in our case,
    /// always none) and the *outputs* they produce. As a shorthand, if they produce
    /// nothing or a single value, there is a shorthand form. This function converts
    /// an object-type into this form.
    ///
    /// [cfi]: https://webassembly.github.io/spec/core/syntax/instructions.html#control-instructions
    fn block_type(&mut self, match_ty: ObjectTy<'db>) -> wasm_encoder::BlockType {
        let val_types = self
            .cx
            .wasm_repr_of_type(match_ty, &self.generics)
            .flatten();
        match val_types.len() {
            0 => wasm_encoder::BlockType::Empty,
            1 => wasm_encoder::BlockType::Result(val_types[0]),
            _ => wasm_encoder::BlockType::FunctionType(u32::from(
                self.cx.declare_fn_type(vec![], val_types),
            )),
        }
    }

    fn push_literal(&mut self, ty: ObjectTy<'db>, literal: PrimitiveLiteral) {
        let db = self.cx.db;
        let kind = match ty.kind(db) {
            ObjectTyKind::Named(sym_ty_name, _) => match sym_ty_name {
                SymTyName::Primitive(sym_primitive) => sym_primitive.kind(db),
                SymTyName::Aggregate(_) | SymTyName::Future | SymTyName::Tuple { arity: _ } => {
                    panic!("unexpected type for literal {literal:?}: {ty:?}")
                }
            },
            ObjectTyKind::Var(sym_variable) => {
                return self.push_literal(self.generics[sym_variable].assert_type(db), literal);
            }
            ObjectTyKind::Infer(_) | ObjectTyKind::Never => {
                panic!("unexpected type for literal {literal:?}: {ty:?}")
            }
            ObjectTyKind::Error(reported) => {
                return self.push_error(*reported);
            }
        };
        match kind {
            SymPrimitiveKind::Bool
            | SymPrimitiveKind::Isize
            | SymPrimitiveKind::Usize
            | SymPrimitiveKind::Char => {
                let PrimitiveLiteral::Integral { bits } = literal else {
                    panic!("expected integral {literal:?}");
                };
                self.instructions.push(Instruction::I32Const(bits as i32));
            }
            SymPrimitiveKind::Int { bits } | SymPrimitiveKind::Uint { bits } if bits <= 32 => {
                let PrimitiveLiteral::Integral { bits } = literal else {
                    panic!("expected integral {literal:?}");
                };
                self.instructions.push(Instruction::I32Const(bits as i32));
            }
            SymPrimitiveKind::Int { bits } | SymPrimitiveKind::Uint { bits } if bits <= 64 => {
                let PrimitiveLiteral::Integral { bits } = literal else {
                    panic!("expected integral {literal:?}");
                };
                self.instructions.push(Instruction::I64Const(bits as i64));
            }
            SymPrimitiveKind::Float { bits } if bits <= 32 => {
                let PrimitiveLiteral::Float { bits } = literal else {
                    panic!("expected float {literal:?}");
                };
                self.instructions.push(Instruction::F32Const(bits.0 as f32));
            }
            SymPrimitiveKind::Float { bits } if bits <= 32 => {
                let PrimitiveLiteral::Float { bits } = literal else {
                    panic!("expected float {literal:?}");
                };
                self.instructions.push(Instruction::F64Const(bits.0));
            }
            SymPrimitiveKind::Int { .. }
            | SymPrimitiveKind::Uint { .. }
            | SymPrimitiveKind::Float { .. } => {
                panic!("unexpected kind: {kind:?}");
            }
        }
    }

    fn push_error(&mut self, _reported: Reported) {
        self.instructions.push(Instruction::Unreachable);
    }
}

/// Error `enum` for [`ExprCodegen::primitive_kind`].
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum NotPrimitive {
    DeadCode,
    OtherType,
}
