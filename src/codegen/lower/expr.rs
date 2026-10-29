//! Expressions: values, places (things with an address), conversions,
//! operators, casts and calls.

use num_bigint::{BigInt, BigUint};

use super::{EnumLayout, Lowerer, array_initializer, int_bits, wrap_int};
use crate::checker::{IntType, Resolution, Type};
use crate::codegen::ir::{
    BinaryOp, CastOp, FloatPredicate, Instruction, IntPredicate, IrType, Typed, Value,
};
use crate::parser::{BinaryOperator, Expression, ExpressionKind, UnaryOperator};

/// `ty`, with an untyped literal type replaced by the type it takes: `hint`
/// if that's a compatible number type, otherwise `s32` or `f64`.
pub(super) fn resolve(ty: &Type, hint: Option<&Type>) -> Type {
    match ty {
        Type::UntypedInt(_) => match hint {
            Some(hint @ (Type::Int(_) | Type::Float(_))) => hint.clone(),
            _ => Type::Int(IntType::S32),
        },
        Type::UntypedFloat => match hint {
            Some(hint @ Type::Float(_)) => hint.clone(),
            _ => Type::Float(64),
        },
        other => other.clone(),
    }
}

impl<'a> Lowerer<'a> {
    /// The type of `e`, with untyped literals resolved using `hint`.
    pub(super) fn natural(&self, e: &Expression, hint: Option<&Type>) -> Type {
        let ty = self.program.types.get(&e.id).unwrap_or(&Type::Unknown);
        resolve(ty, hint)
    }

    /// The type of something assigned to. Identifiers being assigned aren't
    /// read, so their type comes from the variable.
    pub(super) fn target_type(&self, target: &Expression) -> Type {
        match self.program.resolutions.get(&target.id) {
            Some(Resolution::Variable(variable))
                if matches!(target.kind, ExpressionKind::IdentifierExpression { .. }) =>
            {
                self.program.variables[*variable].ty.clone()
            }
            _ => self.natural(target, None),
        }
    }

    /// Where a variable is stored. Locals of top-level initializers count as
    /// globals to the checker, but live in the function setting them.
    pub(super) fn variable_ptr(&self, variable: usize) -> Value {
        match self.function.variables.get(&variable) {
            Some(slot) => slot.clone(),
            None if self.program.variables[variable].global => {
                Value::Global(self.global_name(variable))
            }
            None => Value::Poison,
        }
    }

    /// Lowers `e` and converts it to `want`.
    pub(super) fn expr_as(&mut self, e: &'a Expression, want: &Type) -> Typed {
        let from = self.natural(e, Some(want));

        // An array passed where its size is unknown is a pointer to it
        if let (Type::Array { size: None, .. }, Type::Array { size: Some(_), .. }) = (want, &from) {
            return Typed::ptr(self.place(e));
        }
        if want.is_void_pointer() {
            if let Type::Array { size: Some(_), .. } = from {
                return Typed::ptr(self.place(e));
            }
            if self.natural(e, None).is_integer() {
                return Typed::ptr(Value::Null);
            }
        }

        match self.expr(e, Some(want)) {
            Some(value) => self.convert(value, &from, want),
            None => Typed::new(self.ir_type(want), Value::Poison),
        }
    }

    /// The implicit conversions the checker allows.
    pub(super) fn convert(&mut self, value: Typed, from: &Type, to: &Type) -> Typed {
        match (from, to) {
            _ if from == to => value,
            (Type::Int(from), Type::Int(to)) => self.resize_int(value, *from, int_bits(*to)),
            (Type::Float(32), Type::Float(64)) => self.cast(CastOp::FPExt, value, IrType::Double),
            (Type::Array { size: Some(_), .. }, Type::Array { size: None, .. }) => {
                let slot = self.alloca(value.ty.clone());
                self.store(value, &slot);
                Typed::ptr(slot)
            }
            _ => Typed::new(self.ir_type(to), value.value),
        }
    }

    pub(super) fn cast(&mut self, op: CastOp, value: Typed, to: IrType) -> Typed {
        let result = self.temp();
        self.push(Instruction::Cast {
            result: result.clone(),
            op,
            value,
            to: to.clone(),
        });
        Typed::new(to, Value::Local(result))
    }

    /// Truncates or extends an integer of type `from` to `bits` wide.
    pub(super) fn resize_int(&mut self, value: Typed, from: IntType, bits: u8) -> Typed {
        let from_bits = int_bits(from);
        let to = IrType::Int(u32::from(bits));
        match from_bits.cmp(&bits) {
            std::cmp::Ordering::Equal => Typed::new(to, value.value),
            std::cmp::Ordering::Greater => self.cast(CastOp::Trunc, value, to),
            std::cmp::Ordering::Less if from.signed => self.cast(CastOp::SExt, value, to),
            std::cmp::Ordering::Less => self.cast(CastOp::ZExt, value, to),
        }
    }

    /// Lowers `e`, returning its value, or `None` if it has none (it's a
    /// statement, or never finishes).
    pub(super) fn expr(&mut self, e: &'a Expression, hint: Option<&Type>) -> Option<Typed> {
        match &e.kind {
            ExpressionKind::ImportExpression { .. }
            | ExpressionKind::StructDeclarationExpression { .. }
            | ExpressionKind::EnumDeclarationExpression { .. }
            | ExpressionKind::NamespaceExpression { .. }
            | ExpressionKind::ExternFunctionDeclarationExpression { .. }
            | ExpressionKind::TypeExpression { .. } => None,
            ExpressionKind::FunctionDeclarationExpression { .. } => {
                self.pending.push(e);
                None
            }
            ExpressionKind::StringLiteralExpression { value } => {
                let global = self.string_global(value);
                let ty = self.natural(e, None);
                let ir_type = self.ir_type(&ty);
                Some(self.load(ir_type, &global))
            }
            ExpressionKind::IntegerLiteralExpression { value, .. } => {
                let ty = self.natural(e, hint);
                Some(self.int_constant(value, &ty))
            }
            ExpressionKind::FloatLiteralExpression { value, .. } => {
                let ty = self.natural(e, hint);
                Some(Typed::new(self.ir_type(&ty), Value::Float(*value)))
            }
            ExpressionKind::BooleanLiteralExpression { value } => Some(Typed::bool(*value)),
            ExpressionKind::IdentifierExpression { .. } => {
                match self.program.resolutions.get(&e.id) {
                    Some(&Resolution::Variant { enum_id, index }) => {
                        Some(self.variant_value(enum_id, index, None))
                    }
                    _ => {
                        let ptr = self.place(e);
                        let ty = self.natural(e, None);
                        let ir_type = self.ir_type(&ty);
                        Some(self.load(ir_type, &ptr))
                    }
                }
            }
            ExpressionKind::AtExpression
            | ExpressionKind::IndexExpression { .. }
            | ExpressionKind::MemberExpression { .. } => {
                let ptr = self.place(e);
                let ty = self.natural(e, None);
                let ir_type = self.ir_type(&ty);
                Some(self.load(ir_type, &ptr))
            }
            ExpressionKind::LetExpression {
                typeinfo, value, ..
            } => {
                self.lower_let(e, array_initializer(typeinfo.as_ref()), value.as_deref());
                None
            }
            ExpressionKind::StructLiteralExpression { fields, .. } => {
                let given: Vec<_> = fields
                    .iter()
                    .map(|field| (field.name.as_str(), &field.value))
                    .collect();
                self.struct_literal(e, &given)
            }
            ExpressionKind::BlockExpression { block } => {
                let ty = self.natural(e, hint);
                self.lower_block(block, Some(&ty))
            }
            ExpressionKind::AssignmentExpression {
                operator,
                target,
                value,
            } => {
                self.assign(e, *operator, target, value);
                None
            }
            ExpressionKind::IncrementExpression { target, decrement } => {
                self.increment(target, *decrement);
                None
            }
            ExpressionKind::CastExpression { value, .. } => {
                let to = self.natural(e, None);
                let from = self.natural(value, Some(&to));
                let lowered = self.expr(value, Some(&to))?;
                Some(self.cast_value(lowered, &from, &to))
            }
            ExpressionKind::BinaryExpression {
                operator,
                left,
                right,
            } => Some(self.binary(e, *operator, left, right, hint)),
            ExpressionKind::UnaryExpression { operator, operand } => {
                self.unary(e, *operator, operand, hint)
            }
            ExpressionKind::CallExpression { callee, args } => self.call_expression(callee, args),
            ExpressionKind::IfExpression { .. }
            | ExpressionKind::SwitchExpression { .. }
            | ExpressionKind::WhileExpression { .. }
            | ExpressionKind::LoopExpression { .. }
            | ExpressionKind::DoExpression { .. }
            | ExpressionKind::ForExpression { .. }
            | ExpressionKind::ForClauseExpression { .. }
            | ExpressionKind::ReturnExpression { .. }
            | ExpressionKind::YieldExpression { .. }
            | ExpressionKind::BreakExpression
            | ExpressionKind::ContinueExpression => self.control(e, hint),
        }
    }

    pub(super) fn int_constant(&self, value: &BigUint, ty: &Type) -> Typed {
        match ty {
            Type::Float(_) => {
                let float = value.to_string().parse().unwrap_or(0.0);
                Typed::new(self.ir_type(ty), Value::Float(float))
            }
            _ => {
                let ir_type = self.ir_type(ty);
                let bits = match ir_type {
                    IrType::Int(bits) => bits,
                    _ => 32,
                };
                Typed::int(ir_type, wrap_int(&BigInt::from(value.clone()), bits))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Places
    // -------------------------------------------------------------------------

    /// The address of `e`. Expressions that aren't stored anywhere are copied
    /// into a temporary.
    pub(super) fn place(&mut self, e: &'a Expression) -> Value {
        match &e.kind {
            ExpressionKind::IdentifierExpression { .. } => {
                if let Some(&Resolution::Variable(variable)) = self.program.resolutions.get(&e.id) {
                    return self.variable_ptr(variable);
                }
            }
            ExpressionKind::AtExpression => {
                if let Some(&variable) = self.program.at_targets.get(&e.id) {
                    return self.variable_ptr(variable);
                }
            }
            ExpressionKind::StringLiteralExpression { value } => {
                return self.string_global(value);
            }
            ExpressionKind::UnaryExpression {
                operator: UnaryOperator::Deref,
                operand,
            } => {
                if let Some(pointer) = self.expr(operand, None) {
                    return pointer.value;
                }
            }
            ExpressionKind::IndexExpression { target, index } => {
                let (base, base_type) = self.deref_place(target);
                let index = self.index_value(index);
                match base_type {
                    Type::Array {
                        element,
                        size: Some(size),
                    } => {
                        let array = IrType::Array(size, Box::new(self.ir_type(&element)));
                        return self.gep(array, &base, vec![Typed::int(IrType::I64, 0), index]);
                    }
                    Type::Array {
                        element,
                        size: None,
                    } => {
                        let pointer = self.load(IrType::Ptr, &base).value;
                        let element = self.ir_type(&element);
                        return self.gep(element, &pointer, vec![index]);
                    }
                    _ => {}
                }
            }
            ExpressionKind::MemberExpression { target, member } => {
                let (base, base_type) = self.deref_place(target);
                if let Type::Struct(id) = base_type {
                    let index = self.program.structs[id]
                        .fields
                        .iter()
                        .position(|(name, _)| name == member)
                        .unwrap_or(0);
                    let ty = IrType::Named(self.struct_types[id].clone());
                    return self.field_ptr(ty, &base, index as u32);
                }
            }
            _ => {}
        }

        // A value with no address of its own
        let ty = self.natural(e, None);
        let ir_type = self.ir_type(&ty);
        let slot = self.alloca(ir_type);
        if let Some(value) = self.expr(e, None) {
            self.store(value, &slot);
        }
        slot
    }

    /// The address of the object `e` refers to, looking through `&T` and `*T`
    /// as `.field` and indexing do, and that object's type.
    fn deref_place(&mut self, e: &'a Expression) -> (Value, Type) {
        let mut ty = self.natural(e, None);
        let mut ptr = self.place(e);
        while let Type::Reference(inner) | Type::Pointer(inner) = ty {
            ptr = self.load(IrType::Ptr, &ptr).value;
            ty = *inner;
        }
        (ptr, ty)
    }

    /// An index as an `i64`, extended according to its signedness.
    fn index_value(&mut self, index: &'a Expression) -> Typed {
        let ty = self.natural(index, Some(&Type::Int(IntType::USIZE)));
        let value = self.expr_as(index, &ty);
        match ty {
            Type::Int(int) => self.resize_int(value, int, 64),
            _ => value,
        }
    }

    // -------------------------------------------------------------------------
    // Aggregates
    // -------------------------------------------------------------------------

    /// A struct (or variant payload) value built from its fields.
    pub(super) fn aggregate(&mut self, ty: IrType, fields: Vec<Typed>) -> Typed {
        if fields.is_empty() {
            return Typed::new(ty, Value::ZeroInitializer);
        }
        let mut value = Typed::new(ty.clone(), Value::Poison);
        for (index, field) in fields.into_iter().enumerate() {
            let result = self.temp();
            self.push(Instruction::InsertValue {
                result: result.clone(),
                aggregate: value,
                value: field,
                index: index as u32,
            });
            value = Typed::new(ty.clone(), Value::Local(result));
        }
        value
    }

    /// A value of enum `enum_id` holding variant `index`, with its fields'
    /// payload struct if it has any.
    pub(super) fn variant_value(
        &mut self,
        enum_id: usize,
        index: usize,
        payload: Option<Typed>,
    ) -> Typed {
        let discriminant = self.discriminants[enum_id][index];
        match self.enum_layouts[enum_id].clone() {
            EnumLayout::Plain(tag) => Typed::int(tag, discriminant),
            EnumLayout::Tagged { name, tag, .. } => {
                let ty = IrType::Named(name);
                let slot = self.alloca(ty.clone());
                let tag_ptr = self.field_ptr(ty.clone(), &slot, 0);
                self.store(Typed::int(tag, discriminant), &tag_ptr);
                if let Some(payload) = payload {
                    let payload_ptr = self.field_ptr(ty.clone(), &slot, 1);
                    self.store(payload, &payload_ptr);
                }
                self.load(ty, &slot)
            }
        }
    }

    /// A struct literal, or a variant with named fields. Fields are evaluated
    /// in the order written, then assembled in declaration order.
    fn struct_literal(
        &mut self,
        e: &'a Expression,
        given: &[(&str, &'a Expression)],
    ) -> Option<Typed> {
        let (fields, struct_type, variant) = match self.program.resolutions.get(&e.id)? {
            Resolution::Struct(id) => (
                self.program.structs[*id].fields.clone(),
                IrType::Named(self.struct_types[*id].clone()),
                None,
            ),
            &Resolution::Variant { enum_id, index } => {
                let crate::checker::VariantShape::Struct(fields) =
                    self.program.enums[enum_id].variants[index].1.clone()
                else {
                    return None;
                };
                let EnumLayout::Tagged { payloads, .. } = &self.enum_layouts[enum_id] else {
                    return None;
                };
                let payload = payloads[index].clone()?;
                (fields, payload, Some((enum_id, index)))
            }
            _ => return None,
        };

        let mut values = Vec::new();
        for (name, value) in given {
            let ty = fields
                .iter()
                .find(|(field, _)| field == name)
                .map_or(Type::Unknown, |(_, ty)| ty.clone());
            values.push((*name, self.expr_as(value, &ty)));
        }

        let ordered = fields
            .iter()
            .map(|(name, ty)| {
                values.iter().find(|(given, _)| given == name).map_or_else(
                    || Typed::new(self.ir_type(ty), Value::Poison),
                    |(_, value)| value.clone(),
                )
            })
            .collect();
        let value = self.aggregate(struct_type, ordered);

        Some(match variant {
            Some((enum_id, index)) => self.variant_value(enum_id, index, Some(value)),
            None => value,
        })
    }

    // -------------------------------------------------------------------------
    // Assignment
    // -------------------------------------------------------------------------

    fn assign(
        &mut self,
        e: &'a Expression,
        operator: Option<BinaryOperator>,
        target: &'a Expression,
        value: &'a Expression,
    ) {
        let target_type = self.target_type(target);
        let ptr = self.place(target);

        let result = match operator {
            None => self.expr_as(value, &target_type),
            Some(operator) => {
                let ir_type = self.ir_type(&target_type);
                let old = self.load(ir_type, &ptr);
                let operands = resolve(
                    self.program
                        .operand_types
                        .get(&e.id)
                        .unwrap_or(&target_type),
                    Some(&target_type),
                );
                let left = self.convert(old, &target_type, &operands);
                let right = self.operand(operator, value, &operands);
                let result = self.arithmetic(operator, &operands, left, right);
                self.convert(result, &operands, &target_type)
            }
        };

        if !self.is_terminated() {
            self.store(result, &ptr);
        }
    }

    fn increment(&mut self, target: &'a Expression, decrement: bool) {
        let ty = self.target_type(target);
        let ptr = self.place(target);
        let ir_type = self.ir_type(&ty);
        let old = self.load(ir_type.clone(), &ptr);
        let result = self.temp();
        self.push(Instruction::Binary {
            result: result.clone(),
            op: if decrement {
                BinaryOp::Sub
            } else {
                BinaryOp::Add
            },
            ty: ir_type.clone(),
            lhs: old.value,
            rhs: Value::Int(1),
        });
        self.store(Typed::new(ir_type, Value::Local(result)), &ptr);
    }

    // -------------------------------------------------------------------------
    // Operators
    // -------------------------------------------------------------------------

    /// The right operand, converted to `operands`; shift amounts keep their
    /// own type, so are resized instead.
    fn operand(
        &mut self,
        operator: BinaryOperator,
        right: &'a Expression,
        operands: &Type,
    ) -> Typed {
        if !matches!(operator, BinaryOperator::Shl | BinaryOperator::Shr) {
            return self.expr_as(right, operands);
        }
        let ty = self.natural(right, None);
        let value = self.expr_as(right, &ty);
        match (ty, operands) {
            (Type::Int(from), Type::Int(to)) => self.resize_int(value, from, int_bits(*to)),
            _ => value,
        }
    }

    fn binary(
        &mut self,
        e: &'a Expression,
        operator: BinaryOperator,
        left: &'a Expression,
        right: &'a Expression,
        hint: Option<&Type>,
    ) -> Typed {
        use BinaryOperator as B;

        if matches!(operator, B::LogicalAnd | B::LogicalOr) {
            return self.short_circuit(operator == B::LogicalAnd, left, right);
        }

        let comparison = matches!(operator, B::Eq | B::Neq | B::Lt | B::Gt | B::Le | B::Ge);
        let operands = resolve(
            self.program
                .operand_types
                .get(&e.id)
                .unwrap_or(&Type::Unknown),
            if comparison { None } else { hint },
        );

        let left = self.expr_as(left, &operands);
        let right = self.operand(operator, right, &operands);

        if comparison {
            self.compare(operator, &operands, left, right)
        } else {
            self.arithmetic(operator, &operands, left, right)
        }
    }

    fn arithmetic(
        &mut self,
        operator: BinaryOperator,
        ty: &Type,
        left: Typed,
        right: Typed,
    ) -> Typed {
        use BinaryOperator as B;

        let float = matches!(ty, Type::Float(_));
        let signed = matches!(ty, Type::Int(int) if int.signed);
        let op = match operator {
            B::Add if float => BinaryOp::FAdd,
            B::Sub if float => BinaryOp::FSub,
            B::Mul if float => BinaryOp::FMul,
            B::Div if float => BinaryOp::FDiv,
            B::Mod if float => BinaryOp::FRem,
            B::Add => BinaryOp::Add,
            B::Sub => BinaryOp::Sub,
            B::Mul => BinaryOp::Mul,
            B::Div if signed => BinaryOp::SDiv,
            B::Div => BinaryOp::UDiv,
            B::Mod if signed => BinaryOp::SRem,
            B::Mod => BinaryOp::URem,
            B::Shl => BinaryOp::Shl,
            B::Shr if signed => BinaryOp::AShr,
            B::Shr => BinaryOp::LShr,
            B::And => BinaryOp::And,
            B::Or => BinaryOp::Or,
            _ => BinaryOp::Xor,
        };

        let result = self.temp();
        self.push(Instruction::Binary {
            result: result.clone(),
            op,
            ty: left.ty.clone(),
            lhs: left.value,
            rhs: right.value,
        });
        Typed::new(left.ty, Value::Local(result))
    }

    pub(super) fn compare(
        &mut self,
        operator: BinaryOperator,
        ty: &Type,
        left: Typed,
        right: Typed,
    ) -> Typed {
        use BinaryOperator as B;

        let result = self.temp();
        if matches!(ty, Type::Float(_)) {
            let predicate = match operator {
                B::Eq => FloatPredicate::Oeq,
                B::Neq => FloatPredicate::Une,
                B::Lt => FloatPredicate::Olt,
                B::Gt => FloatPredicate::Ogt,
                B::Le => FloatPredicate::Ole,
                _ => FloatPredicate::Oge,
            };
            self.push(Instruction::FCmp {
                result: result.clone(),
                predicate,
                ty: left.ty,
                lhs: left.value,
                rhs: right.value,
            });
        } else {
            let signed = matches!(ty, Type::Int(int) if int.signed);
            let predicate = match (operator, signed) {
                (B::Eq, _) => IntPredicate::Eq,
                (B::Neq, _) => IntPredicate::Ne,
                (B::Lt, true) => IntPredicate::Slt,
                (B::Gt, true) => IntPredicate::Sgt,
                (B::Le, true) => IntPredicate::Sle,
                (B::Lt, false) => IntPredicate::Ult,
                (B::Gt, false) => IntPredicate::Ugt,
                (B::Le, false) => IntPredicate::Ule,
                (_, true) => IntPredicate::Sge,
                (_, false) => IntPredicate::Uge,
            };
            self.push(Instruction::ICmp {
                result: result.clone(),
                predicate,
                ty: left.ty,
                lhs: left.value,
                rhs: right.value,
            });
        }
        Typed::new(IrType::BOOL, Value::Local(result))
    }

    /// `&&` and `||`, which only evaluate the right side if they must.
    fn short_circuit(&mut self, and: bool, left: &'a Expression, right: &'a Expression) -> Typed {
        let left = self.expr_as(left, &Type::Bool);
        let left_label = self.current_label();
        let right_label = self.label("rhs");
        let end_label = self.label("logic.end");

        if and {
            self.branch_if(left.value, &right_label, &end_label);
        } else {
            self.branch_if(left.value, &end_label, &right_label);
        }

        self.start_block(right_label);
        let right = self.expr_as(right, &Type::Bool);
        let right_end = self.current_label();
        self.branch(&end_label);

        self.start_block(end_label);
        let result = self.temp();
        self.push(Instruction::Phi {
            result: result.clone(),
            ty: IrType::BOOL,
            incoming: vec![
                (Value::Int(i128::from(!and)), left_label),
                (right.value, right_end),
            ],
        });
        Typed::new(IrType::BOOL, Value::Local(result))
    }

    fn unary(
        &mut self,
        e: &'a Expression,
        operator: UnaryOperator,
        operand: &'a Expression,
        hint: Option<&Type>,
    ) -> Option<Typed> {
        match operator {
            UnaryOperator::Neg => {
                let ty = self.natural(e, hint);
                let value = self.expr_as(operand, &ty);
                let float = matches!(ty, Type::Float(_));
                let result = self.temp();
                self.push(Instruction::Binary {
                    result: result.clone(),
                    op: if float { BinaryOp::FSub } else { BinaryOp::Sub },
                    ty: value.ty.clone(),
                    lhs: if float {
                        Value::Float(-0.0)
                    } else {
                        Value::Int(0)
                    },
                    rhs: value.value,
                });
                Some(Typed::new(value.ty, Value::Local(result)))
            }
            UnaryOperator::Not => {
                let value = self.expr_as(operand, &Type::Bool);
                let result = self.temp();
                self.push(Instruction::Binary {
                    result: result.clone(),
                    op: BinaryOp::Xor,
                    ty: IrType::BOOL,
                    lhs: value.value,
                    rhs: Value::Int(1),
                });
                Some(Typed::new(IrType::BOOL, Value::Local(result)))
            }
            UnaryOperator::Deref => {
                let pointer = self.expr(operand, None)?;
                let ty = self.natural(e, None);
                let ir_type = self.ir_type(&ty);
                Some(self.load(ir_type, &pointer.value))
            }
            UnaryOperator::AddressOf => Some(Typed::ptr(self.place(operand))),
        }
    }

    // -------------------------------------------------------------------------
    // Casts
    // -------------------------------------------------------------------------

    /// `value as to`. Float to integer saturates, as in Rust.
    fn cast_value(&mut self, value: Typed, from: &Type, to: &Type) -> Typed {
        let to_ir = self.ir_type(to);
        match (from, to) {
            _ if from == to => value,
            (Type::Int(from), Type::Int(to)) => self.resize_int(value, *from, int_bits(*to)),
            (Type::Int(int), Type::Float(_)) => {
                let op = if int.signed {
                    CastOp::SIToFP
                } else {
                    CastOp::UIToFP
                };
                self.cast(op, value, to_ir)
            }
            (Type::Float(bits), Type::Int(int)) => {
                let name = format!(
                    "llvm.fpto{}i.sat.i{}.f{bits}",
                    if int.signed { 's' } else { 'u' },
                    int_bits(*int)
                );
                self.call_extern(&name, to_ir.clone(), vec![value])
                    .unwrap_or(Typed::new(to_ir, Value::Poison))
            }
            (Type::Float(32), Type::Float(_)) => self.cast(CastOp::FPExt, value, to_ir),
            (Type::Float(_), Type::Float(_)) => self.cast(CastOp::FPTrunc, value, to_ir),
            (Type::Bool, Type::Int(_)) => self.cast(CastOp::ZExt, value, to_ir),
            (Type::Enum(_), Type::Int(to)) => {
                let IrType::Int(bits) = value.ty else {
                    return value;
                };
                let from = IntType::new(true, bits as u8);
                self.resize_int(value, from, int_bits(*to))
            }
            (Type::Pointer(_), Type::Int(_)) => self.cast(CastOp::PtrToInt, value, to_ir),
            (Type::Int(_), Type::Pointer(_)) => self.cast(CastOp::IntToPtr, value, to_ir),
            _ => self.convert(value, from, to),
        }
    }

    // -------------------------------------------------------------------------
    // Calls
    // -------------------------------------------------------------------------

    fn call_expression(&mut self, callee: &'a Expression, args: &'a [Expression]) -> Option<Typed> {
        match self.program.resolutions.get(&callee.id).cloned() {
            Some(Resolution::Function(id)) => {
                let function = &self.program.functions[id];
                let ir_params: Vec<_> = function.params.iter().map(|ty| self.ir_type(ty)).collect();
                let return_type = match &function.return_type {
                    Some(ty) => self.ir_type(ty),
                    None => IrType::Void,
                };
                let name = self.function_names[id].clone();
                // Imported and `extern` functions are defined elsewhere
                if function.module.is_some() || function.c_name.is_some() {
                    self.declare(
                        &name,
                        return_type.clone(),
                        ir_params.clone(),
                        function.variadic,
                    );
                }

                let params = function.params.clone();
                let mut lowered = Vec::new();
                for (index, arg) in args.iter().enumerate() {
                    lowered.push(match params.get(index) {
                        Some(param) => self.expr_as(arg, param),
                        None => self.c_argument(arg),
                    });
                }
                let variadic = function.variadic.then_some(ir_params);
                self.call(return_type, Value::Global(name), lowered, variadic)
            }
            Some(Resolution::Variant { enum_id, index }) => {
                let crate::checker::VariantShape::Tuple(types) =
                    self.program.enums[enum_id].variants[index].1.clone()
                else {
                    return None;
                };
                let EnumLayout::Tagged { payloads, .. } = self.enum_layouts[enum_id].clone() else {
                    return None;
                };
                let fields = args
                    .iter()
                    .zip(&types)
                    .map(|(arg, ty)| self.expr_as(arg, ty))
                    .collect();
                let payload = self.aggregate(payloads[index].clone()?, fields);
                Some(self.variant_value(enum_id, index, Some(payload)))
            }
            _ => None,
        }
    }

    /// An extra argument to a variadic C function, promoted as C does:
    /// integers narrower than 32 bits and `bool`s to `i32`, `f32` to `double`,
    /// and arrays to a pointer to their first element.
    fn c_argument(&mut self, arg: &'a Expression) -> Typed {
        let ty = self.natural(arg, None);
        match &ty {
            Type::Int(int) if int_bits(*int) < 32 => {
                let value = self.expr_as(arg, &ty);
                self.resize_int(value, *int, 32)
            }
            Type::Bool => {
                let value = self.expr_as(arg, &ty);
                self.cast(CastOp::ZExt, value, IrType::I32)
            }
            Type::Float(32) => {
                let value = self.expr_as(arg, &ty);
                self.cast(CastOp::FPExt, value, IrType::Double)
            }
            Type::Array { size: Some(_), .. } => Typed::ptr(self.place(arg)),
            Type::Enum(_) => {
                let value = self.expr_as(arg, &ty);
                match value.ty {
                    IrType::Int(bits) if bits < 32 => {
                        self.resize_int(value, IntType::new(true, bits as u8), 32)
                    }
                    _ => value,
                }
            }
            _ => self.expr_as(arg, &ty),
        }
    }

    // -------------------------------------------------------------------------
    // Constants
    // -------------------------------------------------------------------------

    /// `e` as a constant of type `ty`, if it is one: literals, negated
    /// literals, strings, field-less variants, and struct literals of
    /// constants.
    pub(super) fn constant(&mut self, e: &'a Expression, ty: &Type) -> Option<Value> {
        match &e.kind {
            ExpressionKind::IntegerLiteralExpression { value, .. } => {
                let natural = self.natural(e, Some(ty));
                Some(self.int_constant(value, &natural).value)
            }
            ExpressionKind::FloatLiteralExpression { value, .. } => Some(Value::Float(*value)),
            ExpressionKind::BooleanLiteralExpression { value } => {
                Some(Value::Int(i128::from(*value)))
            }
            ExpressionKind::UnaryExpression {
                operator: UnaryOperator::Neg,
                operand,
            } => match self.constant(operand, ty)? {
                Value::Int(value) => {
                    let IrType::Int(bits) = self.ir_type(ty) else {
                        return None;
                    };
                    Some(Value::Int(wrap_int(&BigInt::from(-value), bits)))
                }
                Value::Float(value) => Some(Value::Float(-value)),
                _ => None,
            },
            ExpressionKind::StringLiteralExpression { value } => match ty {
                Type::Array { size: Some(_), .. } => {
                    let mut bytes = value.as_bytes().to_vec();
                    bytes.push(0);
                    Some(Value::Bytes(bytes))
                }
                _ => Some(self.string_global(value)),
            },
            ExpressionKind::IdentifierExpression { .. } => {
                let &Resolution::Variant { enum_id, index } =
                    self.program.resolutions.get(&e.id)?
                else {
                    return None;
                };
                let discriminant = self.discriminants[enum_id][index];
                match &self.enum_layouts[enum_id] {
                    EnumLayout::Plain(_) => Some(Value::Int(discriminant)),
                    EnumLayout::Tagged { name, tag, .. } => {
                        let payload = self.named[name][1].clone();
                        Some(Value::Aggregate(vec![
                            Typed::int(tag.clone(), discriminant),
                            Typed::new(payload, Value::ZeroInitializer),
                        ]))
                    }
                }
            }
            ExpressionKind::StructLiteralExpression { fields, .. } => {
                let Some(&Resolution::Struct(id)) = self.program.resolutions.get(&e.id) else {
                    return None;
                };
                let mut values = Vec::new();
                for (name, field_type) in self.program.structs[id].fields.clone() {
                    let value = &fields.iter().find(|field| field.name == name)?.value;
                    let constant = self.constant(value, &field_type)?;
                    values.push(Typed::new(self.ir_type(&field_type), constant));
                }
                Some(Value::Aggregate(values))
            }
            _ => None,
        }
    }

    /// The constant value of an array of type `ty` with every element `fill`.
    pub(super) fn constant_fill(&mut self, fill: &'a Expression, ty: &Type) -> Option<Value> {
        let element = ty.base_element().clone();
        let value = self.constant(fill, &element)?;
        if matches!(value, Value::Int(0)) || matches!(value, Value::Float(f) if f == 0.0) {
            return Some(Value::ZeroInitializer);
        }

        fn repeat(lowerer: &Lowerer, ty: &Type, value: &Value) -> Value {
            match ty {
                Type::Array {
                    element,
                    size: Some(size),
                } => {
                    let element_type = lowerer.ir_type(element);
                    let inner = repeat(lowerer, element, value);
                    Value::Aggregate(vec![Typed::new(element_type, inner); *size as usize])
                }
                _ => value.clone(),
            }
        }
        Some(repeat(self, ty, &value))
    }
}
