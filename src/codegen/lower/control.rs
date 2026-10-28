//! Blocks, `let`, `if`, loops and jumps.

use super::{LoopTarget, Lowerer};
use crate::checker::Type;
use crate::codegen::ir::{BinaryOp, Instruction, IntPredicate, IrType, Terminator, Typed, Value};
use crate::parser::{Block, Expression, ExpressionKind};

impl<'a> Lowerer<'a> {
    /// Lowers a block's statements, then its result converted to `want`.
    pub(super) fn lower_block(&mut self, block: &'a Block, want: Option<&Type>) -> Option<Typed> {
        for statement in &block.body {
            self.expr(statement, None);
        }
        let result = block.result.as_deref()?;
        match want {
            Some(want) if !matches!(want, Type::Void | Type::Never | Type::Unknown) => {
                Some(self.expr_as(result, want))
            }
            _ => self.expr(result, None),
        }
    }

    pub(super) fn block_as(&mut self, block: &'a Block, want: Option<&Type>) -> Option<Typed> {
        self.lower_block(block, want)
    }

    pub(super) fn lower_let(
        &mut self,
        e: &'a Expression,
        fill: Option<&'a Expression>,
        value: Option<&'a Expression>,
    ) {
        let Some(&variable) = self.program.declarations.get(&e.id) else {
            return;
        };
        let ty = self.program.variables[variable].ty.clone();
        let ir_type = self.ir_type(&ty);
        let slot = self.alloca(ir_type);
        self.function.variables.insert(variable, slot.clone());
        self.initialize(&slot, &ty, value, fill);
    }

    /// Sets a new variable at `ptr`: first filling it from its type's array
    /// initializer, then storing its value. A value of just `@` is the
    /// variable itself, so needs no store.
    pub(super) fn initialize(
        &mut self,
        ptr: &Value,
        ty: &Type,
        value: Option<&'a Expression>,
        fill: Option<&'a Expression>,
    ) {
        if let Some(fill) = fill {
            self.fill_array(ptr, ty, fill);
        }
        let Some(value) = value else {
            return;
        };
        if matches!(value.kind, ExpressionKind::AtExpression) {
            return;
        }
        let value = self.expr_as(value, ty);
        if !self.is_terminated() {
            self.store(value, ptr);
        }
    }

    fn fill_array(&mut self, ptr: &Value, ty: &Type, fill: &'a Expression) {
        if let Some(constant) = self.constant_fill(fill, ty) {
            let ir_type = self.ir_type(ty);
            self.store(Typed::new(ir_type, constant), ptr);
            return;
        }

        // Evaluate the fill once, then store it into every element of the
        // (possibly nested) array, viewed as one flat run of elements
        let element = ty.base_element().clone();
        let mut count = 1;
        let mut layer = ty;
        while let Type::Array {
            element,
            size: Some(size),
        } = layer
        {
            count *= size;
            layer = element;
        }

        let value = self.expr_as(fill, &element);
        let element = self.ir_type(&element);
        let counter = self.alloca(IrType::I64);
        self.store(Typed::int(IrType::I64, 0), &counter);

        let condition_label = self.label("fill.cond");
        let body_label = self.label("fill.body");
        let end_label = self.label("fill.end");

        self.start_block(condition_label.clone());
        let index = self.load(IrType::I64, &counter);
        let more = self.temp();
        self.push(Instruction::ICmp {
            result: more.clone(),
            predicate: IntPredicate::Ult,
            ty: IrType::I64,
            lhs: index.value.clone(),
            rhs: Value::Int(i128::from(count)),
        });
        self.branch_if(Value::Local(more), &body_label, &end_label);

        self.start_block(body_label);
        let element_ptr = self.gep(element, ptr, vec![index.clone()]);
        self.store(value, &element_ptr);
        let next = self.temp();
        self.push(Instruction::Binary {
            result: next.clone(),
            op: BinaryOp::Add,
            ty: IrType::I64,
            lhs: index.value,
            rhs: Value::Int(1),
        });
        self.store(Typed::new(IrType::I64, Value::Local(next)), &counter);
        self.branch(&condition_label);

        self.start_block(end_label);
    }

    /// Control flow constructs. Returns the value of an `if`.
    pub(super) fn control(&mut self, e: &'a Expression, hint: Option<&Type>) -> Option<Typed> {
        match &e.kind {
            ExpressionKind::IfExpression {
                condition,
                then,
                otherwise,
            } => self.lower_if(e, condition, then, otherwise.as_deref(), hint),
            ExpressionKind::SwitchExpression {
                subject,
                cases,
                default,
            } => {
                self.lower_switch(subject, cases, default.as_ref());
                None
            }
            ExpressionKind::WhileExpression { condition, body } => {
                let condition_label = self.label("while.cond");
                let body_label = self.label("while.body");
                let end_label = self.label("while.end");

                self.start_block(condition_label.clone());
                let value = self.expr_as(condition, &Type::Bool);
                self.branch_if(value.value, &body_label, &end_label);

                self.start_block(body_label);
                self.in_loop(&end_label, Some(&condition_label), |lowerer| {
                    lowerer.lower_block(body, None);
                });
                self.branch(&condition_label);

                self.start_block(end_label);
                None
            }
            ExpressionKind::LoopExpression { body } => {
                let body_label = self.label("loop.body");
                let end_label = self.label("loop.end");

                self.start_block(body_label.clone());
                self.in_loop(&end_label, Some(&body_label), |lowerer| {
                    lowerer.lower_block(body, None);
                });
                self.branch(&body_label);

                self.start_block(end_label);
                None
            }
            ExpressionKind::DoExpression { body, condition } => {
                let body_label = self.label("do.body");
                let condition_label = self.label("do.cond");
                let end_label = self.label("do.end");

                self.start_block(body_label.clone());
                let continue_label = condition.as_ref().map(|_| condition_label.as_str());
                self.in_loop(&end_label, continue_label, |lowerer| {
                    lowerer.lower_block(body, None);
                });

                if let Some(condition) = condition {
                    self.start_block(condition_label);
                    let value = self.expr_as(condition, &Type::Bool);
                    self.branch_if(value.value, &body_label, &end_label);
                }

                self.start_block(end_label);
                None
            }
            ExpressionKind::ForClauseExpression {
                init,
                condition,
                step,
                each,
                body,
            } => {
                if let Some(init) = init {
                    self.expr(init, None);
                }

                let condition_label = self.label("for.cond");
                let body_label = self.label("for.body");
                let step_label = self.label("for.step");
                let end_label = self.label("for.end");

                self.start_block(condition_label.clone());
                if let Some(condition) = condition {
                    let value = self.expr_as(condition, &Type::Bool);
                    self.branch_if(value.value, &body_label, &end_label);
                }

                self.start_block(body_label);
                self.in_loop(&end_label, Some(&step_label), |lowerer| {
                    if let Some(each) = each {
                        lowerer.expr(each, None);
                    }
                    lowerer.lower_block(body, None);
                });

                self.start_block(step_label);
                if let Some(step) = step {
                    self.expr(step, None);
                }
                self.branch(&condition_label);

                self.start_block(end_label);
                None
            }
            ExpressionKind::ForExpression { iterable, body, .. } => {
                self.lower_for_in(e, iterable, body);
                None
            }
            ExpressionKind::ReturnExpression { value } => {
                self.lower_return(value.as_deref());
                None
            }
            ExpressionKind::YieldExpression { value } => {
                self.lower_yield(value);
                None
            }
            ExpressionKind::BreakExpression => {
                if let Some(target) = self.function.loops.last() {
                    let label = target.break_label.clone();
                    self.branch(&label);
                }
                None
            }
            ExpressionKind::ContinueExpression => {
                let label = self
                    .function
                    .loops
                    .iter()
                    .rev()
                    .find_map(|target| target.continue_label.clone());
                if let Some(label) = label {
                    self.branch(&label);
                }
                None
            }
            _ => None,
        }
    }

    /// Runs `lower` with `break` going to `break_label` and `continue` (if
    /// it can target this) to `continue_label`.
    pub(super) fn in_loop(
        &mut self,
        break_label: &str,
        continue_label: Option<&str>,
        lower: impl FnOnce(&mut Self),
    ) {
        self.function.loops.push(LoopTarget {
            break_label: break_label.to_owned(),
            continue_label: continue_label.map(str::to_owned),
        });
        lower(self);
        self.function.loops.pop();
    }

    fn lower_if(
        &mut self,
        e: &'a Expression,
        condition: &'a Expression,
        then: &'a Block,
        otherwise: Option<&'a Expression>,
        hint: Option<&Type>,
    ) -> Option<Typed> {
        let ty = self.natural(e, hint);
        let has_value = !matches!(ty, Type::Void | Type::Never | Type::Unknown);
        let ir_type = self.ir_type(&ty);
        let slot = has_value.then(|| self.alloca(ir_type.clone()));

        let then_label = self.label("if.then");
        let else_label = self.label("if.else");
        let end_label = self.label("if.end");

        let value = self.expr_as(condition, &Type::Bool);
        let false_label = if otherwise.is_some() {
            &else_label
        } else {
            &end_label
        };
        self.branch_if(value.value, &then_label, false_label);

        self.start_block(then_label);
        let value = self.lower_block(then, has_value.then_some(&ty));
        self.store_branch(slot.as_ref(), value);
        self.branch(&end_label);

        if let Some(otherwise) = otherwise {
            self.start_block(else_label);
            let value = if has_value {
                Some(self.expr_as(otherwise, &ty))
            } else {
                self.expr(otherwise, None)
            };
            self.store_branch(slot.as_ref(), value);
            self.branch(&end_label);
        }

        self.start_block(end_label);
        slot.map(|slot| self.load(ir_type, &slot))
    }

    /// Stores an `if` branch's value, unless the branch never finished.
    fn store_branch(&mut self, slot: Option<&Value>, value: Option<Typed>) {
        if let (Some(slot), Some(value)) = (slot, value)
            && !self.is_terminated()
        {
            self.store(value, slot);
        }
    }

    fn lower_return(&mut self, value: Option<&'a Expression>) {
        if self.function.generator.is_some() {
            self.finish_generator();
            return;
        }

        let return_type = self.function.return_type.clone();
        let value = match (value, &return_type) {
            (Some(value), Some(ty)) => Some(self.expr_as(value, ty)),
            _ if self.function.is_main => Some(Typed::int(IrType::I32, 0)),
            _ => None,
        };
        if !self.is_terminated() {
            self.terminate(Terminator::Ret(value));
        }
    }
}
