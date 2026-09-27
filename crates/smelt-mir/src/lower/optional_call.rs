//! Optional-chained method calls that must reach an enclosing `catch`.
//!
//! `receiver?.method(args)` normally lowers to a single
//! [`Rvalue::OptionalMethod`], which the emitter renders as a
//! `match receiver { Some(value) => Some(value.method(args)?), None => None }`.
//! That spelling propagates a thrown error with `?` straight out of the Rust
//! function: it is an rvalue, not a [`Terminator::Call`], so it carries no
//! unwind edge. Outside a `try` that is exactly JavaScript's behaviour — the
//! throw leaves the function — but inside one it skips the `catch` entirely
//! (Hono's `#insertPath` wraps `this.#tries![method].insert(..)` in a
//! `try`/`catch` that rethrows a sentinel as `UnsupportedPathError`, and the
//! sentinel escaped unconverted).
//!
//! Inside an active exception handler the call is therefore lowered as the
//! control flow it denotes: test the receiver for presence, call the resolved
//! method on the unwrapped receiver through an ordinary `Terminator::Call`
//! (which records the handler as its unwind edge), and join the two arms with
//! the optional result.

use smelt_hir::{BinOp, ExprId, Span, Symbol, Type, TypeId};

use crate::{Callee, Constant, Operand, Place, Rvalue, Statement, Terminator};

use super::LowerError;
use super::context::LoweringCtx;

/// The parts of one `receiver?.method(args)` expression.
pub(super) struct OptionalMethodCall<'a> {
    /// The optional receiver expression.
    pub(super) receiver: ExprId,
    /// The called method's name.
    pub(super) method: Symbol,
    /// The call's argument expressions.
    pub(super) args: &'a [ExprId],
    /// The chain's result type (`Optional<..>`).
    pub(super) result_ty: TypeId,
    /// The whole expression's span.
    pub(super) span: Span,
}

impl LoweringCtx<'_> {
    /// Lower `receiver?.method(args)` as a presence branch plus a real call
    /// terminator when a `catch` is active, or answer `None` to keep the
    /// single-rvalue lowering.
    ///
    /// Applies only when the receiver's type is `Optional<Class>` whose method
    /// resolves to a known function: that is the shape a direct method call
    /// (`ExprKind::Method`) lowers, so the present arm reuses exactly that call
    /// ABI (the unwrapped receiver is the leading argument). Any other shape
    /// (no handler, an erased or union payload, an unresolvable method) keeps
    /// the existing [`Rvalue::OptionalMethod`], so this changes nothing outside
    /// a `try` body.
    ///
    /// Arguments are evaluated only in the present arm, which is JavaScript's
    /// optional-chain short-circuit: `a?.m(f())` never calls `f` when `a` is
    /// nullish.
    pub(super) fn lower_optional_method_with_unwind(
        &mut self,
        call: &OptionalMethodCall<'_>,
    ) -> Result<Option<Operand>, LowerError> {
        let &OptionalMethodCall {
            receiver,
            method,
            args,
            result_ty,
            span,
        } = call;
        let Some(unwind) = self.current_exception_handler() else {
            return Ok(None);
        };
        let receiver_ty = self.hir_expr(receiver)?.ty;
        let Some(Type::Optional(inner_ty)) = self.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        let Some(Type::Class { name, .. }) = self.krate.types.get(inner_ty).cloned() else {
            return Ok(None);
        };
        let Ok(callee_id) = self.resolve_method_on_class(name, method, span) else {
            return Ok(None);
        };
        let Some(call_ty) = self.optional_method_call_type(name, method, result_ty) else {
            return Ok(None);
        };
        let receiver_operand = self.lower_expr(receiver)?;
        let receiver_local = self.materialize_operand_local(receiver_operand, receiver_ty, span)?;
        let bool_ty = self.loop_bool_ty;
        let present = self.assign_temp(
            bool_ty,
            span,
            Rvalue::Binary {
                op: BinOp::NotEq,
                lhs: Operand::Copy(Place::Local(receiver_local)),
                rhs: Operand::Const(Constant::None),
            },
        )?;
        let dest = self.push_temp(result_ty, span);
        let present_block = self.function.push_block(span);
        let absent_block = self.function.push_block(span);
        self.set_terminator(Terminator::Switch {
            cond: present,
            then_block: present_block,
            else_block: absent_block,
        })?;

        // Present arm: unwrap, call with the handler as the unwind edge, and
        // wrap the result back into the optional destination.
        self.current_block = present_block;
        let unwrapped = self.assign_temp(
            inner_ty,
            span,
            Rvalue::Use(Operand::Copy(Place::Local(receiver_local))),
        )?;
        let mut lowered_args = vec![unwrapped];
        for arg in args {
            lowered_args.push(self.lower_expr(*arg)?);
        }
        let call_dest = self.push_temp(call_ty, span);
        let after_call = self.function.push_block(span);
        self.set_terminator(Terminator::Call {
            callee: Callee::Static(callee_id),
            args: lowered_args,
            dest: call_dest,
            target: after_call,
            unwind: Some(unwind),
        })?;
        self.current_block = after_call;
        self.block_mut()?.statements.push(Statement::Assign {
            dest,
            value: Rvalue::Use(Operand::Copy(Place::Local(call_dest))),
        });
        let present_tail = self.current_block;

        // Absent arm: the chain short-circuits to `undefined`.
        self.current_block = absent_block;
        self.block_mut()?.statements.push(Statement::Assign {
            dest,
            value: Rvalue::Use(Operand::Const(Constant::None)),
        });

        // The join is allocated last so every `goto` into it is a forward
        // edge (see the ordering note on `ExprKind::Conditional`).
        let join_block = self.function.push_block(span);
        self.set_terminator(Terminator::Goto(join_block))?;
        self.current_block = present_tail;
        self.set_terminator(Terminator::Goto(join_block))?;
        self.current_block = join_block;
        Ok(Some(Operand::Copy(Place::Local(dest))))
    }

    /// The type the resolved method call itself produces, given the optional
    /// chain's result type.
    ///
    /// The frontend flattens an optional chain over a method that already
    /// returns an optional (`Optional<Optional<T>>` is `Optional<T>`), so the
    /// call's own type is the chain's type when the declared return is optional
    /// and the chain's payload otherwise. A declared return that is a type
    /// parameter could be either, so it answers `None` and the caller keeps the
    /// rvalue lowering.
    fn optional_method_call_type(
        &self,
        class: Symbol,
        method: Symbol,
        result_ty: TypeId,
    ) -> Option<TypeId> {
        let Some(Type::Optional(payload)) = self.krate.types.get(result_ty).cloned() else {
            return None;
        };
        let declared = self.krate.items.iter().find_map(|item| {
            let smelt_hir::Item::Class(class_item) = item else {
                return None;
            };
            if class_item.name != class {
                return None;
            }
            class_item.methods.iter().find_map(|method_item| {
                if let smelt_hir::Item::Function(function) = self.hir_item(*method_item).ok()?
                    && function.name == method
                {
                    Some(function.return_ty)
                } else {
                    None
                }
            })
        })?;
        match self.krate.types.get(declared) {
            Some(Type::Optional(_)) => Some(result_ty),
            Some(Type::TypeParam { .. } | Type::Unknown | Type::Union(_)) => None,
            _ => Some(payload),
        }
    }
}
