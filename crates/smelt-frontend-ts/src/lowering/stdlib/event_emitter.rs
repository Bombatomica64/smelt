//! `EventEmitter` from `node:events`: construction, its methods and their result
//! types.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::Expression;
use smelt_hir::{Body, EventEmitterOp, Expr, ExprKind, Type};

impl ModuleBuilder<'_> {
    /// Lower `new EventEmitter()` into a concrete emitter.
    ///
    /// The constructor's only option is `{ captureRejections }`, which changes
    /// how a rejected promise returned by a listener is reported. Nothing here
    /// can observe that yet, so passing it is a named blocker rather than an
    /// argument that is accepted and ignored.
    pub(in crate::lowering) fn event_emitter_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if !new_expr.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "EventEmitter options are not modeled yet",
            ));
        }
        let ty = self.event_emitter_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::EventEmitterNew,
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Dispatch a modeled `EventEmitter` method on a concrete receiver.
    pub(in crate::lowering) fn dispatch_event_emitter_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::EventEmitter,
            member_name,
        )
        .is_none()
        {
            return Ok(None);
        }
        let Some(receiver) = self.dispatched_receiver(&member.object, body)? else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_event_emitter_type(receiver_ty) {
            return Ok(None);
        }
        // The member spelling picks the operation: `on` and `addListener` are
        // the same operation under two names, as are `off` and
        // `removeListener`.
        let op = match member_name {
            "on" | "addListener" => EventEmitterOp::On,
            "once" => EventEmitterOp::Once,
            "off" | "removeListener" => EventEmitterOp::Off,
            "removeAllListeners" => EventEmitterOp::RemoveAll,
            "emit" => EventEmitterOp::Emit,
            "listenerCount" => EventEmitterOp::ListenerCount,
            _ => return Ok(None),
        };
        let required = match op {
            EventEmitterOp::On | EventEmitterOp::Once | EventEmitterOp::Off => 2,
            _ => 1,
        };
        if call.arguments.len() < required {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`EventEmitter.{member_name}` requires {required} argument(s)"),
            ));
        }
        // `emit` forwards its whole tail to every listener, so every argument is
        // lowered; the others take exactly the arguments they model.
        let taken = if matches!(op, EventEmitterOp::Emit) {
            call.arguments.len()
        } else {
            required
        };
        // A modeled receiver publishes a schema for its own events, so the
        // listener can be lowered at a REAL signature instead of an erased one.
        // `req.on('data', (chunk) => ..)` types `chunk` as a string, which is
        // what `node:http` says it carries; only the registration adapter still
        // erases, and that adapter is the emitter's documented boundary either
        // way. Without this the source's own closure took a `SmeltUnknown` and
        // coerced it by hand at every use — avoidable erasure inside program
        // code, which is precisely what the metric exists to remove.
        let listener_hint = self.modeled_listener_hint(op, receiver_ty, call);
        let args = call
            .arguments
            .iter()
            .take(taken)
            .enumerate()
            .map(|(index, argument)| match listener_hint {
                Some(hint) if index == 1 => self.argument_with_hint(argument, body, Some(hint)),
                _ => self.argument(argument, body),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.event_emitter_op_result_type(op, receiver_ty);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::EventEmitterOp {
                op,
                emitter: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// The listener signature a modeled receiver's event schema publishes.
    ///
    /// Answers `None` — leaving the listener erased, as it has always been —
    /// unless all three of these hold: the operation registers or removes a
    /// listener, the receiver is a modeled class rather than a plain
    /// `EventEmitter`, and the event name is a literal the class publishes a
    /// schema for. Anything else is the open case the emitter's erased store
    /// exists for: a name computed at run time cannot select a signature.
    fn modeled_listener_hint(
        &mut self,
        op: EventEmitterOp,
        receiver_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<smelt_hir::TypeId> {
        if !matches!(
            op,
            EventEmitterOp::On | EventEmitterOp::Once | EventEmitterOp::Off
        ) {
            return None;
        }
        let class = self.stdlib_class_of_type(receiver_ty)?;
        let Some(oxc::ast::ast::Argument::StringLiteral(event)) = call.arguments.first() else {
            return None;
        };
        let params = class.event_listener_string_params(&js_str_text(event.value))?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        Some(
            self.ctx
                .krate
                .types
                .intern(Type::Function(smelt_hir::FunctionType {
                    params: vec![string_ty; params],
                    rest: None,
                    required_params: Some(params),
                    mutable_params: Vec::new(),
                    return_ty: none_ty,
                    is_async: false,
                    // A listener that throws leaves through the emitting call,
                    // which is what `statement_can_throw` already reports for
                    // `emit`; the signature has to agree with that.
                    may_throw: true,
                })),
        )
    }

    /// Return the modeled `EventEmitter` class type.
    pub(in crate::lowering) fn event_emitter_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("EventEmitter");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type HAS a `node:events` listener list.
    ///
    /// Not "is the `EventEmitter` class". Node's `IncomingMessage` extends
    /// `EventEmitter`, and `req.on('data', ..)` is how a request body is read,
    /// so the test that gates the emitter operations has to admit every class
    /// that carries a listener list. The registry answers which those are
    /// (`StdlibClass::has_event_emitter`), and the generated types make the
    /// answer true by COMPOSITION — a `SmeltIncomingMessage` holds a
    /// `SmeltEventEmitter` and forwards the same five methods to it — so one
    /// listener-list implementation serves both receivers and they cannot drift
    /// apart on ordering or on `once` removal.
    ///
    /// The shadow test follows the class the type actually names, so a user
    /// class called `EventEmitter` still shadows the emitter without affecting
    /// `IncomingMessage`, and vice versa.
    pub(in crate::lowering) fn is_event_emitter_type(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(class) = self.stdlib_class_of_type(ty) else {
            return false;
        };
        if !class.has_event_emitter() {
            return false;
        }
        match class {
            smelt_stdlib::StdlibClass::EventEmitter => !self.source_shadows_global("EventEmitter"),
            smelt_stdlib::StdlibClass::IncomingMessage => {
                !self.source_shadows_global("IncomingMessage")
            }
            _ => false,
        }
    }

    /// The HIR type an `EventEmitter` operation answers.
    ///
    /// Every registration and removal answers the RECEIVER, which is what makes
    /// `e.on(..).on(..)` chain as it does in Node. The receiver, not the
    /// emitter class: `req.on('data', ..)` answers the request in Node, so a
    /// chained `req.on(..).on(..)` must keep the `IncomingMessage` type or the
    /// second call would look for `on` on an emitter that has no `url`.
    /// `emit` answers whether a listener ran, and `listenerCount` a number.
    fn event_emitter_op_result_type(
        &mut self,
        op: EventEmitterOp,
        receiver_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        match op {
            EventEmitterOp::On
            | EventEmitterOp::Once
            | EventEmitterOp::Off
            | EventEmitterOp::RemoveAll => receiver_ty,
            EventEmitterOp::Emit => self.ctx.krate.types.intern(Type::Bool),
            EventEmitterOp::ListenerCount => self.ctx.krate.types.intern(Type::Float),
        }
    }
}
