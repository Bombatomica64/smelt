//! Lowering for bounded host-global override slots.
//!
//! Implements the write/read/presence/`new`-dispatch lowering for modeled host
//! constructors the crate reassigns via `globalThis.<Name> = ...` (see the
//! host-global override plan and [`crate::HirCtx::written_host_globals`]). Only
//! names in the modeled host-object registry that the crate actually writes
//! somewhere activate this machinery; every other name keeps today's byte
//! -identical folding and native construction (pay-for-use).

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{AssignmentTarget, Expression};
use oxc::syntax::operator::AssignmentOperator;
use smelt_hir::{
    Body, ClosureExpr, Expr, ExprKind, FunctionType, Item, LocalDecl, Param, Span, Stmt, Type,
    UnaryOp, UnknownKind,
};

impl ModuleBuilder<'_> {
    /// Return whether `name` is a modeled host constructor the crate reassigns
    /// somewhere via `globalThis.<name> =` (recorded by the crate-level pre-pass).
    pub(in crate::lowering) fn is_written_host_global(&self, name: &str) -> bool {
        self.ctx.written_host_globals.contains(name)
            && smelt_stdlib::host_object_by_class(name).is_some()
    }

    /// Build a [`ExprKind::HostGlobalRead`] for a written host constructor.
    ///
    /// Reads the override slot: a native-handle marker record when the slot is
    /// `Native`, the stored constructor when overridden, or `undefined` when set
    /// absent. Typed `unknown`: the read is a genuine dynamic boundary whose
    /// concrete shape depends on runtime slot state.
    pub(in crate::lowering) fn host_global_read_expr(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let class = self.intern_type_name(name);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        body.push_expr(Expr {
            kind: ExprKind::HostGlobalRead { class },
            ty: unknown_ty,
            span: self.span(start, end),
        })
    }

    /// Return the written host constructor name a `typeof <operand>` presence
    /// guard probes, if any. Recognizes the bare identifier (`typeof Blob`) and
    /// the global-alias member spelling (`typeof globalThis.Blob`).
    pub(in crate::lowering) fn typeof_operand_written_host_global<'a>(
        &self,
        operand: &'a Expression<'a>,
    ) -> Option<&'a str> {
        match operand {
            Expression::Identifier(identifier) => {
                self.is_written_host_global(identifier.name.as_str())
                    .then_some(identifier.name.as_str())
            }
            Expression::StaticMemberExpression(member) if !member.optional => {
                let name = member.property.name.as_str();
                (self.expr_is_global_alias(&member.object) && self.is_written_host_global(name))
                    .then_some(name)
            }
            _ => None,
        }
    }

    /// Build a presence probe for a written host constructor's override slot.
    ///
    /// `negated` selects the sense of a `typeof X === 'undefined'` guard: when
    /// `true` the result is `!present` (matching `=== 'undefined'`), otherwise it
    /// is `present` (matching `!== 'undefined'`).
    pub(in crate::lowering) fn host_global_present_expr(
        &mut self,
        name: &str,
        negated: bool,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let class = self.intern_type_name(name);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let present = body.push_expr(Expr {
            kind: ExprKind::HostGlobalPresent { class },
            ty: bool_ty,
            span: self.span(span.start, span.end),
        });
        if negated {
            self.unary_bool_expr(UnaryOp::Not, present, span, body)
        } else {
            present
        }
    }

    /// Try to lower `globalThis.<Name> = <value>` for a written modeled host
    /// constructor into a [`ExprKind::HostGlobalWrite`].
    ///
    /// Handles the plain `=` assignment only (a compound `globalThis.X += ...`
    /// is not a whole-global reassignment and is left to ordinary lowering). The
    /// stored value is classified at runtime by the write helper; only
    /// `undefined`, a class/function constructor value, or a saved native handle
    /// are v1-supported, so any other concrete value is a named blocker rather
    /// than a silent `SmeltUnknown` erasure. Returns `Ok(None)` when the target
    /// is not a written host global so ordinary lowering proceeds.
    pub(in crate::lowering) fn try_host_global_write_expression(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(name) = self.assignment_target_written_host_global(&assign.left) else {
            return Ok(None);
        };
        let name = name.to_owned();
        if assign.operator != AssignmentOperator::Assign {
            return Err(SmeltError::unsupported(
                self.span(assign.span.start, assign.span.end),
                format!(
                    "compound assignment to host global `{name}` is not supported; \
                     only whole-global reassignment `globalThis.{name} = <value>` is modeled"
                ),
            ));
        }
        let value = match &assign.right {
            Expression::ClassExpression(class) => {
                self.host_global_ctor_closure_value(class, body)?
            }
            other => self.expression_with_hint(other, body, None)?,
        };
        let value_ty = Self::expr_ty(body, value);
        if !self.host_global_write_value_is_supported(value_ty) {
            return Err(SmeltError::unsupported(
                self.span(assign.span.start, assign.span.end),
                format!(
                    "host global `{name}` may only be reassigned to `undefined`, a \
                     class/function constructor, or a saved native handle"
                ),
            ));
        }
        let class = self.intern_type_name(&name);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::HostGlobalWrite { class, value },
            ty: unknown_ty,
            span: self.span(assign.span.start, assign.span.end),
        })))
    }

    /// Return whether a stored host-global value type is a v1-supported override.
    ///
    /// Accepts JS `undefined` (`None`), a constructor value (`Function` or a
    /// class value), and an erased saved handle (`Unknown`, produced by
    /// [`ExprKind::HostGlobalRead`]). Any other concrete shape is rejected as a
    /// named blocker so the write never routes ordinary data through the slot.
    fn host_global_write_value_is_supported(&self, value_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(value_ty),
            Some(Type::None | Type::Unknown | Type::Function(_) | Type::Class { .. })
        )
    }

    /// Return the modeled host constructor name a `globalThis.<Name>` assignment
    /// target names, if it is a written host global. Recognizes the static
    /// member spelling and a string-literal computed key off a global alias.
    fn assignment_target_written_host_global<'a>(
        &self,
        target: &'a AssignmentTarget<'a>,
    ) -> Option<&'a str> {
        let (object, property) = match target {
            AssignmentTarget::StaticMemberExpression(member) if !member.optional => {
                (&member.object, member.property.name.as_str())
            }
            AssignmentTarget::ComputedMemberExpression(member) if !member.optional => {
                let Expression::StringLiteral(key) = &member.expression else {
                    return None;
                };
                (&member.object, key.value.as_str())
            }
            _ => return None,
        };
        if !self.expr_is_global_alias(object) {
            return None;
        }
        self.is_written_host_global(property).then_some(property)
    }

    /// Lower a `class <Name> extends <Host> { ... }` expression assigned into a
    /// host override slot into a first-class constructor closure value.
    ///
    /// The class is lowered normally (so its instances construct through their
    /// own ctor and carry the host base marker via the erasure path), then
    /// wrapped in a closure `(a0, a1, ...) => new Class(a0, a1, ...)` whose arity
    /// matches the declared constructor. Storing a *function* value (rather than
    /// the bare class placeholder) is what lets the `new X(...)` dispatch tell a
    /// `Ctor` override from a `Native` slot at runtime (`typeof slot === 'function'`).
    fn host_global_ctor_closure_value(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item = self.class_declaration(class)?;
        let span = self.span(class.span.start, class.span.end);
        let (class_symbol, param_tys) = {
            let Item::Class(lowered) = self.item_ref(item) else {
                return Err(SmeltError::unsupported(
                    span,
                    "class expression did not lower to a class item",
                ));
            };
            let param_tys = lowered
                .constructor
                .and_then(|ctor| match self.item_ref(ctor) {
                    Item::Function(function) => {
                        Some(function.params.iter().map(|param| param.ty).collect::<Vec<_>>())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            (lowered.name, param_tys)
        };
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_symbol,
            args: Vec::new(),
        });
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut arg_exprs = Vec::new();
        for (index, &param_ty) in param_tys.iter().enumerate() {
            let param_name = self.intern_source_name(&format!("arg{index}"));
            let local = closure_body.push_local(LocalDecl {
                name: Some(param_name),
                ty: param_ty,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: param_name,
                local,
                ty: param_ty,
                span,
            });
            arg_exprs.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty: param_ty,
                span,
            }));
        }
        let constructed = closure_body.push_expr(Expr {
            kind: ExprKind::New {
                class: class_symbol,
                args: arg_exprs,
            },
            ty: class_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(constructed)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: param_tys,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: class_ty,
            is_async: false,
            may_throw: false,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: closure_params,
                rest: None,
                required_params: None,
                return_ty: class_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Build the *native* construction expression for a modeled host
    /// constructor `name` (the slot-`Native` branch of a `new X(...)` dispatch).
    ///
    /// Routes to the same dedicated constructor lowerings the unwritten-name path
    /// uses, so the native branch of an overridden `new X(...)` is byte-identical
    /// to today's construction. Returns `Ok(None)` for a host name without a
    /// dedicated native constructor here (the caller then leaves ordinary
    /// lowering to handle it).
    pub(in crate::lowering) fn native_host_constructor_expression(
        &mut self,
        name: &str,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let expr = match name {
            "Blob" => self.blob_constructor_expression(new_expr, body)?,
            "File" => self.file_constructor_expression(new_expr, body)?,
            "Buffer" => self.buffer_constructor_expression(new_expr, body)?,
            // The byte-backed host objects other than `Buffer` construct through
            // the shared host constructor (`ExprKind::HostConstruct`).
            _ if smelt_stdlib::byte_buffer_role(name).is_some() => {
                self.byte_buffer_constructor_expression(new_expr, name, body)?
            }
            _ => {
                let Some(marker) = Self::marker_only_builtin_marker(name) else {
                    return Ok(None);
                };
                self.marker_only_builtin_constructor_expression(new_expr, body, marker)?
            }
        };
        Ok(Some(expr))
    }

    /// Wrap a native host construction expression in a slot-checked `new X(...)`
    /// dispatch, for a written host constructor.
    ///
    /// Emits both paths, selected at runtime: when the override slot holds a
    /// constructor (`typeof slot === 'function'`) the stored constructor is
    /// closure-called with the lowered arguments (an erased call); otherwise the
    /// existing native construction runs (the `Native` slot, and — pragmatically
    /// — the `Absent` slot, which the specs never construct through). Returns the
    /// bare `native` expression unchanged for names the crate never writes.
    pub(in crate::lowering) fn wrap_host_global_new(
        &mut self,
        name: &str,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        native: smelt_hir::ExprId,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if !self.is_written_host_global(name) {
            return Ok(native);
        }
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let callee = self.host_global_read_expr(name, new_expr.span.start, new_expr.span.end, body);
        let is_ctor = body.push_expr(Expr {
            kind: ExprKind::UnknownIs {
                value: callee,
                kind: UnknownKind::Function,
            },
            ty: bool_ty,
            span,
        });
        let args = new_expr
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ctor_call = body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: unknown_ty,
            span,
        });
        // Both branches evaluate to the erased dynamic value; the native
        // construction is already `unknown`-typed.
        let native_unknown = self.coerce_to_unknown(native, span, body);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: is_ctor,
                then_expr: ctor_call,
                else_expr: native_unknown,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Erase an expression to `unknown` when it is not already, for a
    /// slot-dispatch branch whose sibling branch is erased.
    fn coerce_to_unknown(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        if matches!(self.ctx.krate.types.get(value_ty), Some(Type::Unknown)) {
            return value;
        }
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })
    }
}
