//! Generators, lowered to LLVM coroutines (switched-resume lowering).
//!
//! A generator `fn g(...) -> iter T` becomes a `presplitcoroutine` function
//! that allocates its frame with `malloc`, suspends straight away (so calling
//! it runs none of its body), and returns the coroutine handle: that handle is
//! the `iter T` value. Each `yield v` stores `v` in the coroutine's promise and
//! suspends. Finishing suspends for the last time, which `coro.done` reports.
//!
//! `for x in it` resumes the handle, stops once `coro.done`, otherwise reads
//! `x` from the promise. Leaving the loop (normally or by `break`) destroys
//! the coroutine, freeing its frame.

use super::{GeneratorState, Lowerer, layout};
use crate::checker::Type;
use crate::codegen::ir::{IrType, Terminator, Typed, Value};
use crate::parser::{Block, Expression};

impl<'a> Lowerer<'a> {
    fn promise_alignment(&self, item: &Type) -> i128 {
        let item = self.ir_type(item);
        i128::from(layout::size_align(&item, &self.named).1)
    }

    /// Sets up the coroutine at the start of a generator's body.
    pub(super) fn begin_generator(&mut self, item: Type) {
        let promise_type = self.ir_type(&item);
        let promise = self.alloca(promise_type);
        let alignment = self.promise_alignment(&item);

        let id = self
            .call_extern(
                "llvm.coro.id",
                IrType::Token,
                vec![
                    Typed::int(IrType::I32, alignment),
                    Typed::ptr(promise.clone()),
                    Typed::ptr(Value::Null),
                    Typed::ptr(Value::Null),
                ],
            )
            .expect("coro.id returns a token");
        let size = self
            .call_extern("llvm.coro.size.i64", IrType::I64, Vec::new())
            .expect("coro.size returns a size");
        let memory = self
            .call_extern("malloc", IrType::Ptr, vec![size])
            .expect("malloc returns a pointer");
        let handle = self
            .call_extern("llvm.coro.begin", IrType::Ptr, vec![id.clone(), memory])
            .expect("coro.begin returns a handle");

        let start_label = self.label("gen.start");
        let state = GeneratorState {
            promise,
            item,
            id: id.value,
            handle: handle.value,
            finish_label: self.label("gen.finish"),
            cleanup_label: self.label("gen.cleanup"),
            suspend_label: self.label("gen.suspend"),
        };
        self.function.generator = Some(state);

        // Start suspended, so calling the generator runs none of its body
        self.suspend(false, &start_label);
        self.start_block(start_label);
    }

    /// Suspends the coroutine; when resumed it continues at `resume_label`.
    fn suspend(&mut self, last: bool, resume_label: &str) {
        let generator = self.function.generator.as_ref().expect("in a generator");
        let (cleanup, suspend) = (
            generator.cleanup_label.clone(),
            generator.suspend_label.clone(),
        );

        let result = self
            .call_extern(
                "llvm.coro.suspend",
                IrType::I8,
                vec![
                    Typed::new(IrType::Token, Value::NoneToken),
                    Typed::bool(last),
                ],
            )
            .expect("coro.suspend returns a state");
        self.terminate(Terminator::Switch {
            value: result,
            default: suspend,
            cases: vec![
                (Typed::int(IrType::I8, 0), resume_label.to_owned()),
                (Typed::int(IrType::I8, 1), cleanup),
            ],
        });
    }

    pub(super) fn lower_yield(&mut self, value: &'a Expression) {
        let Some(generator) = &self.function.generator else {
            return;
        };
        let (item, promise) = (generator.item.clone(), generator.promise.clone());

        let value = self.expr_as(value, &item);
        if self.is_terminated() {
            return;
        }
        self.store(value, &promise);

        let resume_label = self.label("gen.resume");
        self.suspend(false, &resume_label);
        self.start_block(resume_label);
    }

    /// Ends the generator, from a `return` or the end of its body.
    pub(super) fn finish_generator(&mut self) {
        if let Some(generator) = &self.function.generator {
            let label = generator.finish_label.clone();
            self.branch(&label);
        }
    }

    /// Emits the blocks shared by every exit: the final suspend, freeing the
    /// frame when destroyed, and returning the handle.
    pub(super) fn end_generator(&mut self) {
        let Some(generator) = &self.function.generator else {
            return;
        };
        let finish = generator.finish_label.clone();
        let cleanup = generator.cleanup_label.clone();
        let suspend = generator.suspend_label.clone();
        let id = Typed::new(IrType::Token, generator.id.clone());
        let handle = Typed::ptr(generator.handle.clone());

        // Resuming after the final suspend isn't allowed, so its resume path
        // can't happen
        self.start_block(finish);
        let dead = self.label("gen.dead");
        self.suspend(true, &dead);
        self.start_block(dead);
        self.terminate(Terminator::Unreachable);

        self.start_block(cleanup);
        let memory = self
            .call_extern("llvm.coro.free", IrType::Ptr, vec![id, handle.clone()])
            .expect("coro.free returns a pointer");
        self.call_extern("free", IrType::Void, vec![memory]);
        self.branch(&suspend);

        self.start_block(suspend);
        self.call_extern(
            "llvm.coro.end",
            IrType::BOOL,
            vec![
                handle.clone(),
                Typed::bool(false),
                Typed::new(IrType::Token, Value::NoneToken),
            ],
        );
        self.terminate(Terminator::Ret(Some(handle)));
    }

    pub(super) fn lower_for_in(
        &mut self,
        e: &'a Expression,
        iterable: &'a Expression,
        body: &'a Block,
    ) {
        let Some(&variable) = self.program.declarations.get(&e.id) else {
            return;
        };
        let item = self.program.variables[variable].ty.clone();
        let Some(handle) = self.expr(iterable, None) else {
            return;
        };

        let next_label = self.label("for.next");
        let body_label = self.label("for.item");
        let done_label = self.label("for.done");

        self.start_block(next_label.clone());
        self.call_extern("llvm.coro.resume", IrType::Void, vec![handle.clone()]);
        let done = self
            .call_extern("llvm.coro.done", IrType::BOOL, vec![handle.clone()])
            .expect("coro.done returns a bool");
        self.branch_if(done.value, &done_label, &body_label);

        self.start_block(body_label);
        let alignment = self.promise_alignment(&item);
        let promise = self
            .call_extern(
                "llvm.coro.promise",
                IrType::Ptr,
                vec![
                    handle.clone(),
                    Typed::int(IrType::I32, alignment),
                    Typed::bool(false),
                ],
            )
            .expect("coro.promise returns a pointer");
        let item_type = self.ir_type(&item);
        let value = self.load(item_type.clone(), &promise.value);
        let slot = self.alloca(item_type);
        self.store(value, &slot);
        self.function.variables.insert(variable, slot);

        // `break` also goes to the end, which frees the generator
        self.in_loop(&done_label, Some(&next_label), |lowerer| {
            lowerer.lower_block(body, None);
        });
        self.branch(&next_label);

        self.start_block(done_label);
        self.call_extern("llvm.coro.destroy", IrType::Void, vec![handle]);
    }
}
