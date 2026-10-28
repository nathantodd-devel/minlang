//! `switch` and the patterns of its cases.
//!
//! The subject is evaluated once into a slot. Each case has a *test* block,
//! which checks its pattern against the slot (jumping to the next case's test
//! on failure) and stores any bound names, and a *body* block. A body that
//! doesn't end in a jump falls through into the next case's body, as in C,
//! and the last one into `default`.

use super::{EnumLayout, Lowerer};
use crate::checker::{Resolution, Type, VariantShape};
use crate::codegen::ir::{Instruction, IntPredicate, IrType, Typed, Value};
use crate::parser::{BinaryOperator, Block, Case, Expression, Pattern, PatternKind};

impl<'a> Lowerer<'a> {
    pub(super) fn lower_switch(
        &mut self,
        subject: &'a Expression,
        cases: &'a [Case],
        default: Option<&'a Block>,
    ) {
        let ty = self.natural(subject, None);
        let ir_type = self.ir_type(&ty);
        let slot = self.alloca(ir_type);
        let value = self.expr_as(subject, &ty);
        self.store(value, &slot);

        let tests: Vec<_> = cases.iter().map(|_| self.label("case.test")).collect();
        let bodies: Vec<_> = cases.iter().map(|_| self.label("case.body")).collect();
        let default_label = self.label("switch.default");
        let end_label = self.label("switch.end");
        let after_cases = if default.is_some() {
            default_label.clone()
        } else {
            end_label.clone()
        };

        self.branch(tests.first().unwrap_or(&after_cases));

        self.in_loop(&end_label, None, |lowerer| {
            for (index, case) in cases.iter().enumerate() {
                let next_test = tests.get(index + 1).unwrap_or(&after_cases);
                let next_body = bodies.get(index + 1).unwrap_or(&after_cases);

                lowerer.start_block(tests[index].clone());
                lowerer.match_pattern(&case.pattern, &slot, &ty, next_test);
                lowerer.branch(&bodies[index]);

                lowerer.start_block(bodies[index].clone());
                lowerer.lower_block(&case.body, None);
                if !lowerer.is_terminated() {
                    lowerer.branch(next_body);
                }
            }

            if let Some(default) = default {
                lowerer.start_block(default_label.clone());
                lowerer.lower_block(default, None);
            }
        });

        self.start_block(end_label);
    }

    /// Checks `pattern` against the value of type `ty` at `ptr`, jumping to
    /// `fail` if it doesn't match. Afterwards the current block is the
    /// matching path, with any names the pattern binds stored.
    fn match_pattern(&mut self, pattern: &'a Pattern, ptr: &Value, ty: &Type, fail: &str) {
        match &pattern.kind {
            PatternKind::WildcardPattern => {}
            PatternKind::BindingPattern { .. } => {
                let Some(&variable) = self.program.declarations.get(&pattern.id) else {
                    return;
                };
                let ir_type = self.ir_type(ty);
                let value = self.load(ir_type.clone(), ptr);
                let slot = self.alloca(ir_type);
                self.store(value, &slot);
                self.function.variables.insert(variable, slot);
            }
            PatternKind::ValuePattern { value } => {
                let matches = match self.program.resolutions.get(&value.id) {
                    // A field-less variant of an enum with data: compare tags
                    Some(&Resolution::Variant { enum_id, index })
                        if matches!(self.enum_layouts[enum_id], EnumLayout::Tagged { .. }) =>
                    {
                        self.tag_matches(enum_id, index, ptr)
                    }
                    _ => {
                        let ir_type = self.ir_type(ty);
                        let current = self.load(ir_type, ptr);
                        let expected = self.expr_as(value, ty);
                        self.compare(BinaryOperator::Eq, ty, current, expected)
                    }
                };
                self.continue_if(matches.value, fail);
            }
            PatternKind::TupleVariantPattern { elements, .. } => {
                let Some(&Resolution::Variant { enum_id, index }) =
                    self.program.resolutions.get(&pattern.id)
                else {
                    return;
                };
                let matches = self.tag_matches(enum_id, index, ptr);
                self.continue_if(matches.value, fail);

                let VariantShape::Tuple(types) =
                    self.program.enums[enum_id].variants[index].1.clone()
                else {
                    return;
                };
                for (field, (element, field_type)) in elements.iter().zip(&types).enumerate() {
                    let field_ptr = self.payload_field(enum_id, index, ptr, field);
                    self.match_pattern(element, &field_ptr, field_type, fail);
                }
            }
            PatternKind::StructVariantPattern { fields, .. } => {
                let Some(&Resolution::Variant { enum_id, index }) =
                    self.program.resolutions.get(&pattern.id)
                else {
                    return;
                };
                let matches = self.tag_matches(enum_id, index, ptr);
                self.continue_if(matches.value, fail);

                let VariantShape::Struct(declared) =
                    self.program.enums[enum_id].variants[index].1.clone()
                else {
                    return;
                };
                for field in fields {
                    let Some(position) = declared.iter().position(|(name, _)| *name == field.name)
                    else {
                        continue;
                    };
                    let field_ptr = self.payload_field(enum_id, index, ptr, position);
                    let field_type = declared[position].1.clone();
                    self.match_pattern(&field.pattern, &field_ptr, &field_type, fail);
                }
            }
        }
    }

    /// Continues in a new block if `condition` holds, otherwise jumps to
    /// `fail`.
    fn continue_if(&mut self, condition: Value, fail: &str) {
        let matched = self.label("case.match");
        self.branch_if(condition, &matched, fail);
        self.start_block(matched);
    }

    /// Whether the enum value at `ptr` holds variant `index`.
    fn tag_matches(&mut self, enum_id: usize, index: usize, ptr: &Value) -> Typed {
        let discriminant = self.discriminants[enum_id][index];
        let (tag_type, tag) = match self.enum_layouts[enum_id].clone() {
            EnumLayout::Plain(tag_type) => {
                let tag = self.load(tag_type.clone(), ptr);
                (tag_type, tag)
            }
            EnumLayout::Tagged { name, tag, .. } => {
                let tag_ptr = self.field_ptr(IrType::Named(name), ptr, 0);
                let value = self.load(tag.clone(), &tag_ptr);
                (tag, value)
            }
        };

        let result = self.temp();
        self.push(Instruction::ICmp {
            result: result.clone(),
            predicate: IntPredicate::Eq,
            ty: tag_type,
            lhs: tag.value,
            rhs: Value::Int(discriminant),
        });
        Typed::new(IrType::BOOL, Value::Local(result))
    }

    /// The address of field `field` of variant `index`'s payload, in the enum
    /// value at `ptr`.
    fn payload_field(&mut self, enum_id: usize, index: usize, ptr: &Value, field: usize) -> Value {
        let EnumLayout::Tagged { name, payloads, .. } = self.enum_layouts[enum_id].clone() else {
            return ptr.clone();
        };
        let payload_ptr = self.field_ptr(IrType::Named(name), ptr, 1);
        let payload_type = payloads[index]
            .clone()
            .unwrap_or(IrType::Struct(Vec::new()));
        self.field_ptr(payload_type, &payload_ptr, field as u32)
    }
}
