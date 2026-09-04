//! `new` expression and construction lowering for the TypeScript frontend.
//!
//! Lowers `new ...` expressions, including standard-library container
//! constructors and user class construction, into typed HIR.

use super::ModuleBuilder;
use crate::SmeltError;
use oxc::ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use oxc::syntax::operator::{BinaryOperator, LogicalOperator, UnaryOperator};
use smelt_hir::{
    BinOp, Body, ClosureExpr, Expr, ExprKind, FunctionType, Item, Literal, LocalDecl, Param,
    PrimitiveCastOp, Span, Stmt, Type, UnaryOp, UrlField,
};

/// The lowered positional pieces of a builtin Error construction.
///
/// Produced by `error_constructor_parts` so the throw path (which keeps only
/// the message) and the error-record path (which retains everything) lower the
/// source arguments exactly once.
struct ErrorConstructorParts {
    /// The `string`-typed message expression (`"Error"` literal when absent).
    message: smelt_hir::ExprId,
    /// The retained ES2022 `cause` option value, when the construction spells
    /// an options object literal with a `cause` property.
    cause: Option<smelt_hir::ExprId>,
    /// The retained `AggregateError` leading `errors` argument.
    errors: Option<smelt_hir::ExprId>,
}

impl ModuleBuilder<'_> {
    /// Lower a `new ...` expression, including stdlib containers and class construction.
    pub(super) fn new_expression_with_hint(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.set_constructor_expression(new_expr, body, type_hint)? {
            return Ok(expr);
        }
        if let Some(expr) = self.map_constructor_expression(new_expr, body, type_hint)? {
            return Ok(expr);
        }
        if let Some(expr) = self.promise_constructor_expression(new_expr, body, type_hint)? {
            return Ok(expr);
        }
        let Expression::Identifier(callee) = &new_expr.callee else {
            if let Some(expr) = self.intl_date_time_format_constructor_expression(new_expr, body)? {
                return Ok(expr);
            }
            if let Some(expr) = self.intl_namespace_constructor_expression(new_expr, body)? {
                return Ok(expr);
            }
            if let Some(expr) = self.dynamic_date_constructor_expression(new_expr, body)? {
                return Ok(expr);
            }
            if let Expression::StaticMemberExpression(member) = &new_expr.callee {
                // `new memoize.Cache()` where the member resolves to a typed
                // constructor slot (a `Type::Function` produced from a construct
                // signature, e.g. `Cache: new () => MapCache`). The member is a
                // callable value, so the construction is an ordinary indirect
                // call through it, typed by the constructor's declared return
                // type — the constructed value keeps its concrete shape instead
                // of erasing to `unknown` (issue #54). This mirrors the
                // identifier value-callee path in `new_through_value_expression`.
                if let Some(expr) = self.new_through_member_constructor_slot(new_expr, member, body)?
                {
                    return Ok(expr);
                }
                let class_name = self.intern_type_name(member.property.name.as_str());
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: Vec::new(),
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: class_name,
                        args,
                    },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }));
            }
            // `new <expr>(...)` over a dynamic callee (`new object[key](...
            // args)` in lodash-compat `bindKey`, `new (C as any)()` wherever
            // TypeScript needs the cast to accept a plain function as a
            // constructor). Classes are not first-class values in Smelt, so a
            // computed callee can only hold a function value, and constructing
            // through one is JavaScript `[[Construct]]`: the same
            // `ExprKind::Construct` the named-binding path below uses.
            //
            // The SPREAD form (`new f(...args)`) still lowers as a dynamic call:
            // `ExprKind::Construct` carries positional arguments, and a runtime
            // argument vector needs its own node. Until it has one, `new
            // f(...args)` keeps the pre-construction behavior.
            let callee_value = self.expression(&new_expr.callee, body)?;
            let callee_ty = Self::expr_ty(body, callee_value);
            let callable_ty = self.function_member_type(callee_ty);
            if self.erased_or_union_surface(callee_ty) || callable_ty.is_some() {
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let (params, result_ty) = callable_ty
                    .and_then(|ty| match self.ctx.krate.types.get(ty).cloned() {
                        Some(Type::Function(function)) => {
                            Some((function.params, function.return_ty))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| (Vec::new(), unknown_ty));
                let span = self.span(new_expr.span.start, new_expr.span.end);
                if new_expr.arguments.iter().any(Argument::is_spread) {
                    let args = self.packed_spread_arguments(
                        unknown_ty,
                        &new_expr.arguments,
                        span,
                        body,
                    )?;
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::ClosureCallSpread {
                            callee: callee_value,
                            args,
                        },
                        ty: result_ty,
                        span,
                    }));
                }
                let args = new_expr
                    .arguments
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        self.argument_with_hint(arg, body, params.get(index).copied())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Construct {
                        callee: callee_value,
                        args,
                    },
                    ty: result_ty,
                    span,
                }));
            }
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new expressions require a direct class name",
            ));
        };
        // `new ctor(args)` where `ctor` is a *value* of a constructor type
        // (`ctor: new (message: string) => Error`). A constructor type lowers to
        // an ordinary `Type::Function` (see `constructor_type_to_hir`), so the
        // construction is an ordinary indirect call through that callable value,
        // routed through the same `ClosureCall` machinery a plain call uses. A
        // local binding that shadows a stdlib name takes priority here, matching
        // how the call-expression dispatch resolves callees.
        if self.scope.is_bound(callee.name.as_str()) {
            if let Some(expr) = self.new_through_value_expression(new_expr, callee, body)? {
                return Ok(expr);
            }
        }
        // A reassigned modeled host constructor dispatches `new X(...)` on its
        // override slot: closure-call the stored constructor when the slot holds
        // one, else run the native construction. Intercepts before the native
        // constructor and user-class paths so a same-named override class
        // (`class File extends Blob`) does not shadow the dynamic dispatch.
        if self.is_written_host_global(callee.name.as_str())
            && let Some(native) =
                self.native_host_constructor_expression(callee.name.as_str(), new_expr, body)?
        {
            return self.wrap_host_global_new(callee.name.as_str(), new_expr, native, body);
        }
        if callee.name == "Date" {
            return self.new_date_expression(new_expr, body);
        }
        if callee.name == "RegExp" {
            return self.regexp_constructor_expression(new_expr, body);
        }
        if callee.name == "Array" {
            return self.array_constructor_expression(new_expr, body);
        }
        if callee.name == "String" && !self.classes.contains("String") {
            return self.boxed_primitive_constructor_expression(
                new_expr,
                body,
                "__smelt_string",
                Literal::String(String::new()),
            );
        }
        if callee.name == "Object" && !self.classes.contains("Object") {
            return self.object_constructor_expression(new_expr, body, type_hint);
        }
        // The byte-backed host objects other than Node's `Buffer` (which keeps its
        // own `Buffer.from`/`alloc`/`concat`-shaped lowering) construct through the
        // shared host constructor so their records carry real byte storage.
        if callee.name != "Buffer"
            && smelt_stdlib::byte_buffer_role(callee.name.as_str()).is_some()
            && !self.classes.contains(callee.name.as_str())
        {
            let class_name = callee.name.to_string();
            return self.byte_buffer_constructor_expression(new_expr, &class_name, body);
        }
        if callee.name == "Buffer" && !self.classes.contains("Buffer") {
            return self.buffer_constructor_expression(new_expr, body);
        }
        if callee.name == "Blob" && !self.classes.contains("Blob") {
            return self.blob_constructor_expression(new_expr, body);
        }
        if callee.name == "File" && !self.classes.contains("File") {
            return self.file_constructor_expression(new_expr, body);
        }
        if callee.name == "Number" && !self.classes.contains("Number") {
            return self.boxed_primitive_constructor_expression(
                new_expr,
                body,
                "__smelt_number",
                Literal::Float(0.0),
            );
        }
        if callee.name == "Boolean" && !self.classes.contains("Boolean") {
            return self.boxed_primitive_constructor_expression(
                new_expr,
                body,
                "__smelt_boolean",
                Literal::Bool(false),
            );
        }
        if callee.name == "Proxy" && !self.classes.contains("Proxy") {
            return self.proxy_constructor_expression(new_expr, body);
        }
        if callee.name == "Function" && !self.classes.contains("Function") {
            return self.function_constructor_expression(new_expr, body);
        }
        if callee.name == "AbortController" && !self.classes.contains("AbortController") {
            return self.abort_controller_constructor_expression(new_expr, body);
        }
        // The typed-array views are byte-backed host objects: they share the one
        // byte-buffer construction path with `ArrayBuffer`/`DataView`/`Buffer`, and
        // their element type (resolved at runtime from the marker the class name
        // selects) is what decides whether a source argument is re-viewed
        // byte-for-byte or converted element-by-element.
        if Self::is_numeric_typed_array_constructor(callee.name.as_str())
            && !self.classes.contains(callee.name.as_str())
        {
            return self.byte_buffer_constructor_expression(new_expr, callee.name.as_str(), body);
        }
        if callee.name == "URLSearchParams" {
            return self.url_search_params_constructor_expression(new_expr, body);
        }
        if let Some(marker) = Self::marker_only_builtin_marker(callee.name.as_str()) {
            if !self.classes.contains(callee.name.as_str()) {
                return self.marker_only_builtin_constructor_expression(new_expr, body, marker);
            }
        }
        if callee.name == "DOMException" && !self.classes.contains("DOMException") {
            return self.domexception_object_constructor_expression(new_expr, body);
        }
        if Self::is_builtin_error_constructor(callee.name.as_str()) {
            return self.error_object_constructor_expression(new_expr, body);
        }
        if let Some(expr) = self.dynamic_identifier_constructor_expression(new_expr, body)? {
            return Ok(expr);
        }
        if callee.name == "URL" {
            return self.url_constructor_expression(new_expr, body);
        }
        let Some(item) = self.classes.item(callee.name.as_str()) else {
            if self.classes.is_pending(callee.name.as_str()) {
                let class_name = self.intern_type_name(callee.name.as_str());
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: Vec::new(),
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: class_name,
                        args,
                    },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }));
            }
            if self.imports.is_value(callee.name.as_str())
                || self.module_globals.contains_key(callee.name.as_str())
                || self.source_contains_class(callee.name.as_str())
            {
                let class_name = self.intern_type_name(callee.name.as_str());
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: Vec::new(),
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: class_name,
                        args,
                    },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }));
            }
            return Err(SmeltError::for_unresolved_name(
                self.span(callee.span.start, callee.span.end),
                callee.name.as_str(),
                format!("unresolved class `{}`", callee.name),
            ));
        };
        let Item::Class(class) = self.item_ref(item).clone() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new expressions require a class item",
            ));
        };
        if matches!(class.kind, smelt_hir::ClassKind::Abstract) {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                format!("abstract class `{}` cannot be constructed", callee.name),
            ));
        }
        let class_name = class.name;
        let args = new_expr
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let explicit_type_args = new_expr
            .type_arguments
            .as_ref()
            .map(|type_args| {
                type_args
                    .params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let class_args = if let Some(explicit_type_args) = explicit_type_args {
            let substitutions = self.type_argument_substitution(
                &class.type_params,
                &explicit_type_args,
                self.span(new_expr.span.start, new_expr.span.end),
            )?;
            class
                .type_params
                .iter()
                .map(|param| {
                    substitutions.get(&param.name).copied().unwrap_or_else(|| {
                        self.ctx
                            .krate
                            .types
                            .intern(Type::TypeParam { name: param.name })
                    })
                })
                .collect()
        } else {
            class
                .type_params
                .iter()
                .map(|param| {
                    param.default.unwrap_or_else(|| {
                        self.ctx
                            .krate
                            .types
                            .intern(Type::TypeParam { name: param.name })
                    })
                })
                .collect()
        };
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: class_args,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::New {
                class: class_name,
                args,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower `new ctor(args)` where `ctor` is a callable value (a binding whose
    /// type is a constructor/function type), returning `None` when the callee is
    /// not such a value so the caller can fall through to the stdlib/class
    /// dispatch.
    ///
    /// A constructor-type annotation lowers to an ordinary `Type::Function` (see
    /// `constructor_type_to_hir`), so the callee is reached as a VALUE — but
    /// constructing through a function value is not the same operation as
    /// calling it. JavaScript `[[Construct]]` allocates an object linked to the
    /// callee's `prototype`, runs the callee with that object as its receiver,
    /// and keeps the allocated object unless the callee returned one of its
    /// own; none of that happens for a plain call. So this lowers to
    /// `ExprKind::Construct`, not `ExprKind::ClosureCall`, in both the
    /// concretely-typed and the dynamic case. When the binding is present but
    /// is *not* a callable value, `None` is returned and the caller reports the
    /// existing "unresolved class" / non-callable error.
    ///
    /// The result type stays the callable's declared return type where there is
    /// one: a construct signature (`new () => MapCache`) states exactly what
    /// the construction yields, and a constructor whose body returns an object
    /// yields that object, so the declared type is the honest answer. A callee
    /// with no function type is a genuine dynamic boundary and stays `unknown`.
    pub(super) fn new_through_value_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        callee: &oxc::ast::ast::IdentifierReference<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(local) = self.scope.lookup(callee.name.as_str()) else {
            return Ok(None);
        };
        let local_ty = Self::local_ty(body, local);
        // A local constructor value without a concrete function type (`const
        // CacheConstructor = memoize.Cache || Map; new CacheConstructor()`)
        // dispatches through the dynamic closure-call ABI, mirroring the
        // computed-callee `new` fallback: classes are not first-class values
        // in Smelt, so the binding can only hold a function value, and the
        // construction is a genuine dynamic boundary returning `unknown`.
        if !matches!(self.ctx.krate.types.get(local_ty), Some(Type::Function(_))) {
            let callee_expr = self.identifier_expression(
                callee.name.as_str(),
                callee.span.start,
                callee.span.end,
                body,
            )?;
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let args = new_expr
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Construct {
                    callee: callee_expr,
                    args,
                },
                ty: unknown_ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            })));
        }
        let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned() else {
            return Ok(None);
        };
        let callee_expr = self.identifier_expression(
            callee.name.as_str(),
            callee.span.start,
            callee.span.end,
            body,
        )?;
        let args = new_expr
            .arguments
            .iter()
            .take(function.params.len())
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Construct {
                callee: callee_expr,
                args,
            },
            ty: function.return_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        })))
    }

    /// Lower `new receiver.member(args)` when `receiver.member` resolves to a
    /// typed constructor slot, returning `None` so the caller falls through to
    /// the nested-class construction path otherwise.
    ///
    /// A member such as `memoize.Cache` typed with a construct signature
    /// (`Cache: new () => MapCache`) lowers to a `Type::Function` (see
    /// `interface_construct_slot_type`). Because JavaScript classes *are*
    /// constructor functions, `new memoize.Cache()` is just an indirect call
    /// through that callable value: it reuses the `ExprKind::ClosureCall` path a
    /// plain `memoize.Cache()` call takes, typed by the constructor's declared
    /// return type. Only a concrete callable slot is intercepted; a member that
    /// is not a `Type::Function` (a nested class name, a dynamic bag) is left to
    /// the existing member-`new` handling so its behavior is unchanged.
    pub(super) fn new_through_member_constructor_slot(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // Lower the receiver object once and resolve the member's field type. A
        // constructor slot is a `Type::Function`; anything else (a nested class
        // name, a dynamic bag) is not intercepted, so `body` still holds the
        // lowered receiver but the caller's fallback ignores it — the receiver
        // of `new Foo.Bar()` is a pure name read with no observable effect.
        let receiver_value = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver_value);
        let field_symbol = self.intern_source_name(member.property.name.as_str());
        let Ok(member_ty) = self.class_field_type(receiver_ty, field_symbol) else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(member_ty).cloned() else {
            return Ok(None);
        };
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let callee_value = body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver: receiver_value,
                field: field_symbol,
            },
            ty: member_ty,
            span,
        });
        let args = new_expr
            .arguments
            .iter()
            .take(function.params.len())
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callee_value,
                args,
            },
            ty: function.return_ty,
            span,
        })))
    }

    /// Adapt a class name used as a *value* into a constructor closure when the
    /// contextual type expects a constructor (a `Type::Function`), returning
    /// `None` when `name` is not a constructable class or the hint is not a
    /// callable type so the caller can fall through to plain identifier lowering.
    ///
    /// A class name in value position (`makeError(TypeError, ...)`,
    /// `factory(MyClass)`) is a constructor value. A constructor-type parameter
    /// lowers to a `Type::Function` (see `constructor_type_to_hir`), so the honest
    /// bridge is a synthesized closure `(a0, a1, ...) => new Class(a0, a1, ...)`
    /// whose parameter and return types are exactly the expected constructor
    /// type's — matching how ordinary function items are wrapped as first-class
    /// closure values (`item_function_closure_expression`) and how builtin
    /// functions become closures in value position (the builtins-as-values work).
    /// The closure body reuses the normal construction lowering: user / imported
    /// / forward-declared classes construct through `ExprKind::New`, and the
    /// builtin `Error` family constructs through the same erased-Error record the
    /// direct `new Error(...)` path emits.
    pub(super) fn class_constructor_value_expression(
        &mut self,
        name: &str,
        type_hint: Option<smelt_hir::TypeId>,
        identifier_span: oxc::span::Span,
        outer_body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(hint) = type_hint else {
            return Ok(None);
        };
        let Some(Type::Function(expected)) = self.ctx.krate.types.get(hint).cloned() else {
            return Ok(None);
        };
        // A local binding shadowing the name is an ordinary value, not a class
        // constructor; leave it to the normal identifier path.
        if self.scope.is_bound(name) {
            return Ok(None);
        }
        let is_error_builtin = Self::is_builtin_error_constructor(name);
        let is_user_class = self.classes.contains(name)
            || self.classes.is_pending(name)
            || self.source_contains_class(name);
        if !is_error_builtin && !is_user_class {
            return Ok(None);
        }

        let span = self.span(identifier_span.start, identifier_span.end);
        let class_symbol = self.intern_type_name(name);
        // Build closure parameters mirroring the expected constructor signature so
        // the adapter is assignable to the parameter's constructor type.
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut arg_exprs = Vec::new();
        for (index, &param_ty) in expected.params.iter().enumerate() {
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

        let constructed = if is_error_builtin {
            // Reuse the erased-Error record model. `new Error(message)` keeps
            // only the message argument, so pass the first closure parameter
            // through the shared error-object builder.
            self.error_object_from_message(arg_exprs.first().copied(), name, span, &mut closure_body)
        } else {
            let class_ty = self.ctx.krate.types.intern(Type::Class {
                name: class_symbol,
                args: Vec::new(),
            });
            closure_body.push_expr(Expr {
                kind: ExprKind::New {
                    class: class_symbol,
                    args: arg_exprs,
                },
                ty: class_ty,
                span,
            })
        };
        // The closure returns the constructed value; type the closure by what it
        // actually returns. Builtin `Error` constructs an erased record
        // (`unknown`), while user classes return their concrete class type.
        let return_ty = Self::expr_ty(&closure_body, constructed);
        closure_body.push_stmt(Stmt::Return(Some(constructed)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: expected.params.clone(),
            rest: expected.rest,
            required_params: expected.required_params,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        Ok(Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: closure_params,
                rest: expected.rest,
                required_params: expected.required_params,
                return_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        })))
    }

    /// Push one slot of the fixed `Error` instance layout onto a record.
    ///
    /// `ERROR_MARKER_FIELDS` declares the slots every JavaScript `Error`
    /// instance owns, and Smelt models an error as a record. A slot Smelt has no
    /// value for is therefore present and `undefined`, not missing — exactly
    /// what a hand-written Rust `struct` with an `Option` field would be, and
    /// what a `.stack`/`.cause` read already answers either way.
    ///
    /// Materializing them unconditionally is what keeps an error and a copy of
    /// it structurally equal. Every clone helper writes the whole layout back
    /// (`e2.stack = e.stack`, `new Ctor(e.message, { cause: e.cause })`), and a
    /// property STORE always creates the key — so a record that omitted the slot
    /// came back from a round trip with one property MORE than its source.
    ///
    /// The slots are already hidden from `Object.keys`/for-in by the runtime
    /// error filter, so nothing observable enumerates them.
    fn push_error_layout_entry(
        &mut self,
        entries: &mut Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>,
        name: &str,
        value: Option<smelt_hir::ExprId>,
        span: Span,
        body: &mut Body,
    ) {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(name.to_owned())),
            ty: string_ty,
            span,
        });
        let value = value.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty: unknown_ty,
                span,
            })
        });
        entries.push((key, value));
    }

    /// Build the erased-`Error` record used by `new Error(...)` from an optional
    /// message expression already lowered in `body`.
    ///
    /// Shared by the direct `new Error(message)` path and the constructor-value
    /// adapter (`class_constructor_value_expression`) so both stamp the same
    /// `__smelt_error` marker + `message` shape, keeping `instanceof Error`
    /// identity consistent regardless of how the constructor was invoked. When no
    /// message is supplied the default `"Error"` string is used, matching the
    /// zero-argument `new Error()` behavior.
    pub(super) fn error_object_from_message(
        &mut self,
        message: Option<smelt_hir::ExprId>,
        class_name: &str,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let message = message.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty: string_ty,
                span,
            })
        });
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_error".to_owned())),
            ty: string_ty,
            span,
        });
        // The marker VALUE is the constructor's class name, not a bare `true`.
        // Every error class shared one boolean marker, so `instance_of_text`
        // could not tell them apart: `new Error('x') instanceof AggregateError`
        // answered `true`, which sent es-toolkit `clone` down the AggregateError
        // branch and rebuilt the error as `new Ctor(obj.errors, obj.message, ..)`
        // — putting `errors` in the message slot and losing the message. It also
        // makes `.name` truthful, which `smelt_get_object_field` reads back.
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(class_name.to_owned())),
            ty: string_ty,
            span,
        });
        let message_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("message".to_owned())),
            ty: string_ty,
            span,
        });
        let mut entries = vec![(marker_key, marker_value), (message_key, message)];
        self.push_error_layout_entry(&mut entries, "stack", None, span, body);
        self.push_error_layout_entry(&mut entries, "cause", None, span, body);
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: dict_ty,
            span,
        });
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// Lower `new URL(text)` to its full URL string for string-oriented URL APIs.
    pub(super) fn url_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let [url_arg] = new_expr.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new URL() currently supports exactly one string URL argument",
            ));
        };
        let url = self.url_string_argument(url_arg, body, new_expr.span)?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::UrlField {
                field: UrlField::Href,
                url,
            },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Return whether a global constructor creates a JavaScript typed array.
    ///
    /// Delegates to the shared `smelt_stdlib` recognizer so all eleven typed
    /// array names — including the BigInt-backed `BigInt64Array` /
    /// `BigUint64Array`, which the previous inline `matches!` omitted and which
    /// therefore aborted the es-toolkit build as an "unresolved class" — are
    /// recognized from one registry. Every view is a byte-backed host object with
    /// its own element type, so all eleven share the one byte-buffer construction
    /// path.
    pub(super) fn is_numeric_typed_array_constructor(name: &str) -> bool {
        smelt_stdlib::is_typed_array_class_name(name)
    }

    /// Lower `new URLSearchParams(init)` to an object carrying observable `size`.
    pub(super) fn url_search_params_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "URLSearchParams constructor supports at most one initializer",
            ));
        }
        let size = match new_expr.arguments.first() {
            None => 0.0_f64,
            Some(Argument::StringLiteral(literal)) => {
                if literal.value.trim_start_matches('?').is_empty() {
                    0.0_f64
                } else {
                    1.0_f64
                }
            }
            Some(Argument::ObjectExpression(object)) => {
                let count = object
                    .properties
                    .iter()
                    .filter(|property| matches!(property, ObjectPropertyKind::ObjectProperty(_)))
                    .count();
                f64::from(u32::try_from(count).map_err(|error| {
                    SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        format!("URLSearchParams initializer is too large: {error}"),
                    )
                })?)
            }
            Some(argument) => {
                let _ = self.argument(argument, body)?;
                1.0_f64
            }
        };
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        let key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("size".to_owned())),
            ty: key_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(size)),
            ty: self.ctx.krate.types.intern(Type::Float),
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(key, value)]),
            ty: dict_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Lower a host builtin constructor that Smelt only inspects via
    /// `instanceof` into a marker-bearing record erased to `SmeltUnknown`.
    ///
    /// JavaScript host objects such as `WeakMap`, `WeakSet`, `DataView`,
    /// `SharedArrayBuffer`, and `File` have no useful structural shape that
    /// es-toolkit reads — they are constructed and then only tested with
    /// `value instanceof X` (the `isWeakMap`/`isWeakSet`/`isTypedArray` family
    /// and the `clone` deep-clone dispatch). Rather than erase them to a
    /// shapeless `SmeltUnknown::Object` (which would make every `instanceof`
    /// false and collide each host type with the others), give each a dedicated
    /// `__smelt_<marker>` key so a later dynamic `instanceof` resolves through
    /// the marker (see `instance_of_text`), mirroring the `ArrayBuffer`/`Blob`
    /// models. Constructor arguments are lowered for their effects/types and
    /// then discarded, since none of the retained shape is observed.
    pub(super) fn marker_only_builtin_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        marker: &str,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // Lower arguments for their effects/type checks; the marker record keeps
        // no structural shape from them.
        for argument in &new_expr.arguments {
            self.argument(argument, body)?;
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(marker.to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(marker_key, marker_value)]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Return the dedicated identity marker key for a marker-only host builtin
    /// constructor, or `None` when the name is not such a builtin.
    ///
    /// Shared by the `new X(...)` constructor dispatch (to choose the marker to
    /// stamp) and by `instanceof X` lowering (to know the target is modeled).
    /// The mapping lives in the shared `smelt_stdlib::host_object` registry so the
    /// construct side, the `instanceof` side, and the runtime host-marker registry
    /// cannot drift. "Marker-only" here means host objects with no retained
    /// structural fields (`WeakMap`/`WeakSet`/`Request`); host objects with
    /// retained fields (the byte buffers, `Blob`, `File`, boxed primitives,
    /// `DOMException`) have their own dedicated constructors and are excluded here
    /// even though they share the registry.
    pub(crate) fn marker_only_builtin_marker(name: &str) -> Option<&'static str> {
        match name {
            // `Request` joins the marker-only set: es-toolkit constructs it only
            // to probe host identity (`isPlainObject(new Request(url))`), reading
            // none of its structural surface, exactly like the other entries here.
            "WeakMap" | "WeakSet" | "Request" => smelt_stdlib::host_object_marker(name),
            _ => None,
        }
    }

    /// Return true for built-in JavaScript Error constructors with Error identity.
    pub(super) fn is_builtin_error_constructor(class_text: &str) -> bool {
        matches!(
            class_text,
            "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "AggregateError"
        )
    }

    /// Lower a built-in Error constructor used as a value to an erased Error object.
    ///
    /// The record carries the shared `__smelt_error` identity marker and the
    /// `message`, plus the retained ES2022 `cause` option and the
    /// `AggregateError` `errors` list when the construction spells them (see
    /// `error_constructor_parts`). `cause`/`errors` mirror JavaScript's
    /// non-enumerable own error properties: reads resolve through the record,
    /// and the runtime for-in/`Object.keys` filters hide them alongside
    /// `message` (see `smelt_is_for_in_object_key`).
    pub(super) fn error_object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let parts = self.error_constructor_parts(new_expr, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // Which error class was spelled. Falls back to `Error` for a callee shape
        // that is not a bare identifier, which is the base class anyway.
        let class_name = match &new_expr.callee {
            Expression::Identifier(callee) => callee.name.to_string(),
            _ => "Error".to_owned(),
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_error".to_owned())),
            ty: string_ty,
            span,
        });
        // See `error_object_from_message`: the marker value carries the class name
        // so error subclasses stay distinguishable and `.name` reads truthfully.
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(class_name)),
            ty: string_ty,
            span,
        });
        let message_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("message".to_owned())),
            ty: string_ty,
            span,
        });
        let mut entries = vec![(marker_key, marker_value), (message_key, parts.message)];
        self.push_error_layout_entry(&mut entries, "stack", None, span, body);
        self.push_error_layout_entry(&mut entries, "cause", parts.cause, span, body);
        if let Some(errors) = parts.errors {
            let errors_key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("errors".to_owned())),
                ty: string_ty,
                span,
            });
            entries.push((errors_key, errors));
        }
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new DOMException(message?, name?)` to a concrete marker record.
    ///
    /// `DOMException` is a host error class. es-toolkit re-exports it (with an
    /// `Error` fallback for runtimes without it) and uses it only as the base of
    /// `AbortError`/`TimeoutError` and via `value instanceof DOMException`. Rather
    /// than erase it to a shapeless `SmeltUnknown`, model it like `Error`: a record
    /// carrying a dedicated `__smelt_domexception` marker plus its `message` and
    /// `name`, so the identity survives later dynamic `instanceof` checks (see
    /// `instance_of_text`). The two-argument form is `(message, name)`; the name
    /// defaults to `"Error"` to match the spec fallback path es-toolkit relies on.
    pub(super) fn domexception_object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let message = match new_expr.arguments.first() {
            Some(message_arg) => {
                let message = self.argument(message_arg, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
                    message
                } else if self.is_string_compatible_type(Self::expr_ty(body, message))
                    || self.type_contains_unknown(Self::expr_ty(body, message))
                {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: message },
                        ty: string_ty,
                        span: self.span(message_arg.span().start, message_arg.span().end),
                    })
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(message_arg.span().start, message_arg.span().end),
                        "DOMException constructor message must be a string",
                    ));
                }
            }
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            }),
        };
        let name = match new_expr.arguments.get(1) {
            Some(name_arg) => {
                let name = self.argument(name_arg, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, name)) == Some(&Type::String) {
                    name
                } else if self.is_string_compatible_type(Self::expr_ty(body, name))
                    || self.type_contains_unknown(Self::expr_ty(body, name))
                {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: name },
                        ty: string_ty,
                        span: self.span(name_arg.span().start, name_arg.span().end),
                    })
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(name_arg.span().start, name_arg.span().end),
                        "DOMException constructor name must be a string",
                    ));
                }
            }
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty: string_ty,
                span,
            }),
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_domexception".to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let message_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("message".to_owned())),
            ty: string_ty,
            span,
        });
        let name_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("name".to_owned())),
            ty: string_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (marker_key, marker_value),
                (message_key, message),
                (name_key, name),
            ]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new <ByteBufferHost>(...)` — `ArrayBuffer`, `SharedArrayBuffer`,
    /// `DataView` — through the shared host constructor.
    ///
    /// These are JavaScript's binary-data host objects. Source code constructs
    /// them, probes them with `value instanceof ArrayBuffer`, and *operates on
    /// their bytes*: `slice(0)`, `byteLength`, `byteOffset`, and indexed element
    /// reads all appear in the clone/equality paths of any library that handles
    /// binary data. So an identity-only marker record is not enough; the record
    /// needs real byte storage, which the runtime constructor gives it.
    ///
    /// Routing through `ExprKind::HostConstruct` — the same runtime constructor
    /// the reflected `new Object.getPrototypeOf(x).constructor(...)` path calls —
    /// is what makes a directly-constructed record indistinguishable from a
    /// reflectively-constructed one. es-toolkit's `clone` uses the reflected form
    /// where `cloneDeepWith` uses the direct one, and its specs compare the
    /// results against each other.
    pub(super) fn byte_buffer_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        class_name: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let args = new_expr
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::HostConstruct {
                class_name: class_name.to_owned(),
                args,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new Buffer(arg)` to the concrete modeled `Buffer` byte-buffer
    /// record.
    ///
    /// The `new Buffer(...)` form is deprecated in Node in favor of
    /// `Buffer.from`/`Buffer.alloc`, but es-toolkit specs still probe
    /// `value instanceof Buffer`, which needs the constructed value to carry the
    /// `__smelt_buffer` identity. A numeric-array argument becomes the buffer's
    /// bytes; a numeric `new Buffer(size)` allocates a zero-filled length-backed
    /// list; every other argument is evaluated for its effects and backed by an
    /// empty byte list, mirroring the `Buffer.from`/`Buffer.concat` static
    /// lowerings (see `buffer_record_from_bytes`).
    pub(super) fn buffer_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // `ListFromLength` fills with `list[unknown]`, so its destination type
        // must be `List<Unknown>` for the zero-filled `new Buffer(size)` case.
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let bytes = match new_expr.arguments.first() {
            None => self.buffer_empty_bytes(span, body),
            Some(argument) => {
                let value = self.argument(argument, body)?;
                let value_ty = Self::expr_ty(body, value);
                match self.ctx.krate.types.get(value_ty) {
                    // `new Buffer([1, 2, 3])` reuses the numeric source list.
                    Some(Type::List(_)) => value,
                    // `new Buffer(size)` allocates `size` zero-filled bytes.
                    Some(Type::Int | Type::Float) => body.push_expr(Expr {
                        kind: ExprKind::ListFromLength { length: value },
                        ty: list_ty,
                        span,
                    }),
                    // Strings / opaque sources: retain identity, empty bytes.
                    _ => self.buffer_empty_bytes(span, body),
                }
            }
        };
        // Discard any trailing constructor arguments (e.g. an encoding) after
        // evaluating them for their effects, matching the static-call handlers.
        for argument in new_expr.arguments.iter().skip(1) {
            let _ = self.argument(argument, body)?;
        }
        Ok(self.buffer_record_from_bytes(bytes, span, body))
    }

    /// Lower `new Blob(parts?, options?)` to a concrete marker-bearing record.
    ///
    /// JavaScript `Blob` is a host binary-data object: constructed, inspected
    /// via `value instanceof Blob` (the `isBlob` predicate over an erased
    /// `unknown`), and read through `.type`/`.size` (the `clone`/`cloneDeepWith`
    /// paths). Rather than erase it to a shapeless `SmeltUnknown` (which would
    /// lose its identity), model it as a record carrying a dedicated
    /// `__smelt_blob` marker plus its observable `type`, `size`, and `content`,
    /// built by the `smelt_blob_record_from_parts` runtime helper (see
    /// `ExprKind::BlobFromParts`) so a later dynamic `instanceof Blob` resolves
    /// through the marker (see `instance_of_text`) and field reads observe real
    /// values.
    pub(super) fn blob_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let parts = self.blob_parts_expression(new_expr.arguments.first(), body, span)?;
        let (blob_type, _) =
            self.blob_options_expressions(new_expr.arguments.get(1), "Blob", body, span)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::BlobFromParts {
                parts,
                blob_type,
                name: None,
                last_modified: None,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new File(parts, name, options?)` to a concrete marker-bearing record.
    ///
    /// JavaScript `File` extends `Blob`, so the modeled record carries the
    /// `__smelt_file` marker *on top of* `__smelt_blob` (stamped by the
    /// `smelt_blob_record_from_parts` runtime helper): `file instanceof Blob`
    /// and `file instanceof File` both resolve through their markers, matching
    /// the host subtype relationship that `isBlob`/`isFile` observe. The record
    /// retains `name`, `type`, `lastModified`, `size`, and `content`, so the
    /// `clone`/`cloneDeepWith` paths that rebuild a `File` from those fields
    /// round-trip real values.
    pub(super) fn file_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let parts = self.blob_parts_expression(new_expr.arguments.first(), body, span)?;
        let name = match new_expr.arguments.get(1) {
            Some(name_arg) => {
                let name = self.argument(name_arg, body)?;
                self.blob_string_field_expression(name, name_arg.span(), "File", "name", body)?
            }
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            }),
        };
        let (blob_type, last_modified) =
            self.blob_options_expressions(new_expr.arguments.get(2), "File", body, span)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::BlobFromParts {
                parts,
                blob_type,
                name: Some(name),
                last_modified,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower a `Blob`/`File` constructor `BlobPart` array to an erased value.
    ///
    /// Parts are heterogeneous at runtime (strings and other Blob/File records),
    /// so the lowered array is erased to `SmeltUnknown` and walked by the
    /// `smelt_blob_record_from_parts` runtime helper. A missing argument
    /// (`new Blob()`) lowers to an empty erased list.
    fn blob_parts_expression(
        &mut self,
        parts_argument: Option<&Argument<'_>>,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let parts = if let Some(argument) = parts_argument {
            self.argument(argument, body)?
        } else {
            let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
            body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: list_ty,
                span,
            })
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: parts,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Resolve the `type` (and `File`-only `lastModified`) expressions from a
    /// `Blob`/`File` constructor options argument.
    ///
    /// An object-literal options argument keeps the spelled `type`/`lastModified`
    /// *value expressions* — arbitrary expressions such as `valueToClone.type`,
    /// not just string literals — and lowers every other property for its
    /// effects. A missing or non-literal options argument falls back to the
    /// empty MIME string a real `Blob` reports when no type is supplied.
    fn blob_options_expressions(
        &mut self,
        options_argument: Option<&Argument<'_>>,
        class_name: &str,
        body: &mut Body,
        span: Span,
    ) -> Result<(smelt_hir::ExprId, Option<smelt_hir::ExprId>), SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let mut blob_type = None;
        let mut last_modified = None;
        match options_argument {
            Some(Argument::ObjectExpression(object)) => {
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        continue;
                    };
                    let PropertyKey::StaticIdentifier(key) = &property.key else {
                        let _ = self.expression(&property.value, body)?;
                        continue;
                    };
                    let value = self.expression(&property.value, body)?;
                    let value_span = property.value.span();
                    match key.name.as_str() {
                        "type" => {
                            blob_type = Some(self.blob_string_field_expression(
                                value,
                                value_span,
                                class_name,
                                "options.type",
                                body,
                            )?);
                        }
                        "lastModified" => {
                            let value_ty = Self::expr_ty(body, value);
                            let coerced = if matches!(
                                self.ctx.krate.types.get(value_ty),
                                Some(Type::Int | Type::Float)
                            ) {
                                value
                            } else if self.type_contains_unknown(value_ty) {
                                body.push_expr(Expr {
                                    kind: ExprKind::TypeAssert { value },
                                    ty: float_ty,
                                    span: self.span(value_span.start, value_span.end),
                                })
                            } else {
                                return Err(SmeltError::unsupported(
                                    self.span(value_span.start, value_span.end),
                                    format!(
                                        "{class_name} constructor options.lastModified must be a number"
                                    ),
                                ));
                            };
                            last_modified = Some(coerced);
                        }
                        // Unmodeled options (`endings`, ...) are evaluated for
                        // their effects and dropped from the record.
                        _ => {}
                    }
                }
            }
            // A non-literal options value (a variable, a call) is evaluated for
            // its effects; its fields are not recoverable statically, so the
            // record falls back to the constructor defaults.
            Some(argument) => {
                let _ = self.argument(argument, body)?;
            }
            None => {}
        }
        let blob_type = blob_type.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            })
        });
        Ok((blob_type, last_modified))
    }

    /// Coerce a lowered `Blob`/`File` constructor field to a string expression,
    /// mirroring the `DOMException` message idiom: strings pass through,
    /// string-compatible or erased values get a runtime `TypeAssert`, anything
    /// else is a lowering error naming the offending field.
    fn blob_string_field_expression(
        &mut self,
        value: smelt_hir::ExprId,
        value_span: oxc::span::Span,
        class_name: &str,
        field_name: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = Self::expr_ty(body, value);
        if self.ctx.krate.types.get(value_ty) == Some(&Type::String) {
            return Ok(value);
        }
        if self.is_string_compatible_type(value_ty) || self.type_contains_unknown(value_ty) {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span: self.span(value_span.start, value_span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(value_span.start, value_span.end),
            format!("{class_name} constructor {field_name} must be a string"),
        ))
    }

    /// Lower a boxed primitive wrapper (`new Number(v)`, `new Boolean(v)`,
    /// `new String(v)`) to a marker-bearing record.
    ///
    /// All three wrappers share this one rule: `new` builds an **object**, so the
    /// wrapper has reference identity of its own (`new String('a') !==
    /// new String('a')`), answers `typeof === "object"` — which is why
    /// es-toolkit's `isNumber`/`isBoolean`/`isString` (`typeof x === "number"`)
    /// must report `false` for it, and modeling it as a record erased to
    /// `SmeltUnknown::Object` is what makes the runtime narrowing correctly miss
    /// — and keeps its payload under the wrapper's own identity marker, so a
    /// later dynamic `instanceof Number`/`Boolean`/`String` resolves through that
    /// marker (mirroring the `ArrayBuffer` model) and `valueOf`/member reads
    /// unbox it.
    ///
    /// The `Number(x)` / `Boolean(x)` / `String(x)` CALLS are coercions, not
    /// constructions: they lower to primitive values on their own path and are
    /// unaffected.
    pub(super) fn boxed_primitive_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        marker: &str,
        default_value: Literal,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "boxed primitive constructors support at most one value argument",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let value = if let Some(argument) = new_expr.arguments.first() {
            self.argument(argument, body)?
        } else {
            let default_ty = self.ctx.krate.types.intern(match &default_value {
                Literal::Bool(_) => Type::Bool,
                Literal::String(_) => Type::String,
                _ => Type::Float,
            });
            body.push_expr(Expr {
                kind: ExprKind::Literal(default_value),
                ty: default_ty,
                span,
            })
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(marker.to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let value_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("value".to_owned())),
            ty: string_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(marker_key, marker_value), (value_key, value)]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower `new Function(...)` to a callable that throws when invoked.
    ///
    /// The `Function` constructor compiles JavaScript source at runtime —
    /// dynamic code evaluation no ahead-of-time Rust translation can honor.
    /// The construction itself succeeds (matching JS, where the error would
    /// only surface when the compiled body misbehaves) and the resulting
    /// value raises a descriptive error if it is ever actually called. The
    /// constructor arguments are lowered for their side effects and dropped.
    pub(super) fn function_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        for argument in &new_expr.arguments {
            let _ = self.argument(argument, body)?;
        }
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let mut closure_body = Body::new(None, span);
        let message = closure_body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(
                "dynamic code evaluation via new Function(...) is not supported".to_owned(),
            )),
            ty: string_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Throw(message));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: true,
        }));
        Ok(body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: Vec::new(),
                rest: None,
                required_params: None,
                return_ty: unknown_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Lower `new Proxy(target, handler)` to its target value.
    ///
    /// JavaScript `Proxy` is transparent: `x instanceof Proxy` is a `TypeError`
    /// and a proxy reports the identity (`typeof`, `instanceof`, plain-object
    /// shape) of its target. es-toolkit only constructs `new Proxy(target, {})`
    /// in tests of `isPlainObject`, where the proxy must behave exactly like the
    /// wrapped target. There is no faithful distinct identity to invent, so the
    /// closest correct model is to lower the construct to its `target` operand
    /// (the handler is lowered for its effects/types, then discarded). This
    /// keeps the transparent semantics rather than erasing to a wrong marker.
    pub(super) fn proxy_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(target_argument) = new_expr.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Proxy(target, handler) requires a target argument",
            ));
        };
        let target = self.argument(target_argument, body)?;
        if let Some(handler_argument) = new_expr.arguments.get(1) {
            let _ = self.argument(handler_argument, body)?;
        }
        Ok(target)
    }

    /// Lower `new AbortController()` to a concrete, marker-bearing record whose
    /// `signal` shares a mutable `aborted` flag with the controller.
    ///
    /// JavaScript `AbortController` is a host cancellation primitive used by
    /// es-toolkit's `debounce`/`throttle`: the controller exposes a `signal`,
    /// `controller.abort()` flips `signal.aborted` to `true`, and
    /// `signal.addEventListener('abort', cb)` registers callbacks fired by
    /// `abort()`. Rather than erase it to a shapeless `SmeltUnknown` (which would
    /// lose identity and shared mutability), model it as two records:
    ///
    /// - the controller carries a dedicated `__smelt_abortcontroller` marker and
    ///   a `signal` field;
    /// - the signal carries a `__smelt_abortsignal` marker, a mutable `aborted`
    ///   flag (false at construction), and a `__smelt_abort_listeners` array that
    ///   `addEventListener` appends to and `abort()` drains.
    ///
    /// Both records erase to `SmeltUnknown::Object`, whose backing storage is a
    /// shared `Rc<RefCell<..>>`; cloning the controller (or reading its `signal`)
    /// keeps the same backing store, so `controller.abort()` is observed through
    /// any binding that read `controller.signal` earlier. The method behaviors
    /// (`abort`, `addEventListener`, ...) are surfaced as runtime-helper-bound
    /// closures when those fields are read (see the erased-object field path in
    /// `place.rs` and `smelt_abort_method`); `instanceof AbortController` /
    /// `instanceof AbortSignal` use the markers (see `instance_of_text`).
    pub(super) fn abort_controller_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);

        // Push a `Bool`/`String` literal expression and return its id. Kept as
        // local helpers (not methods) so the constructor reads top-to-bottom.
        let string_literal = |target: &mut Body, value: &str| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(value.to_owned())),
                ty: string_ty,
                span,
            })
        };
        let signal_marker_key = string_literal(body, "__smelt_abortsignal");
        let aborted_key = string_literal(body, "aborted");
        let listeners_key = string_literal(body, "__smelt_abort_listeners");
        let controller_marker_key = string_literal(body, "__smelt_abortcontroller");
        let signal_key = string_literal(body, "signal");

        let bool_literal = |target: &mut Body, value: bool| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(value)),
                ty: bool_ty,
                span,
            })
        };
        let signal_marker_value = bool_literal(body, true);
        let aborted_value = bool_literal(body, false);
        let controller_marker_value = bool_literal(body, true);

        // The shared signal record: marker, mutable `aborted` flag, listeners.
        let listeners_value = body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty: list_ty,
            span,
        });
        let signal_object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (signal_marker_key, signal_marker_value),
                (aborted_key, aborted_value),
                (listeners_key, listeners_value),
            ]),
            ty: dict_ty,
            span,
        });
        let signal_unknown = body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: signal_object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        });

        // The controller record: marker plus the shared signal.
        let controller_object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (controller_marker_key, controller_marker_value),
                (signal_key, signal_unknown),
            ]),
            ty: dict_ty,
            span,
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: controller_object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower a thrown expression, preserving the operand as an ordinary value.
    ///
    /// `throw` in JavaScript is value-preserving for *any* operand:
    /// `throw new TypeError(x)`, `throw 'a string'`, `throw {code: 1}` and
    /// `throw someCaughtValue` all deliver exactly the value that was written to
    /// the `catch`. This function therefore does nothing more than lower the
    /// operand through the normal expression path, which already gives
    /// `new Error(..)` its erased `{ __smelt_error, message, cause?, errors? }`
    /// record (see `error_object_constructor_expression`).
    ///
    /// It previously narrowed a thrown `new Error(msg)` to `msg` alone, so the
    /// error object was destroyed at the throw site: every `catch` saw a bare
    /// `SmeltUnknown::String`, which made `error instanceof Error` false,
    /// `error.message` `undefined`, and `error.name` unreadable. Only the throw
    /// *statement* narrowed -- the same construction used as a value already kept
    /// the record -- so the two spellings disagreed about what an `Error` is.
    pub(super) fn throw_operand_expression(
        &mut self,
        argument: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.expression(argument, body)
    }

    /// Lower the positional pieces of a builtin Error construction exactly once.
    ///
    /// Supported shapes, following ES2022:
    /// - `new Error(message?, options?)` (and the `EvalError`..`URIError`
    ///   subclasses), where `options` is an object literal whose `cause`
    ///   property is retained;
    /// - `new AggregateError(errors, message?, options?)`, whose leading
    ///   `errors` iterable is retained as a list value.
    ///
    /// The message follows the pre-existing model: a missing message lowers to
    /// the literal `"Error"`, a concrete `string` passes through, and a
    /// string-compatible or erased operand is asserted to `string`. A
    /// non-literal `options` expression is rejected with an honest blocker
    /// rather than guessed at, because whether a `cause` is attached depends on
    /// `"cause" in options`, which a general static rule can only answer for a
    /// literal spelling. Options properties other than `cause` are lowered for
    /// their effects and discarded, matching JavaScript (only `cause` is read).
    fn error_constructor_parts(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<ErrorConstructorParts, SmeltError> {
        let is_aggregate = matches!(
            &new_expr.callee,
            Expression::Identifier(callee) if callee.name == "AggregateError"
        );
        let mut arguments = new_expr.arguments.iter();
        let errors = if is_aggregate {
            match arguments.next() {
                Some(errors_arg) => Some(self.argument(errors_arg, body)?),
                None => None,
            }
        } else {
            None
        };
        let message_arg = arguments.next();
        let options_arg = arguments.next();
        if arguments.next().is_some() {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Error constructor lowering supports at most a message and an options argument",
            ));
        }
        let message = match message_arg {
            None => {
                let ty = self.ctx.krate.types.intern(Type::String);
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                })
            }
            Some(message_arg) => {
                let message = self.argument(message_arg, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
                    message
                } else if self.is_string_compatible_type(Self::expr_ty(body, message))
                    || self.type_contains_unknown(Self::expr_ty(body, message))
                {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: message },
                        ty,
                        span: self.span(message_arg.span().start, message_arg.span().end),
                    })
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(message_arg.span().start, message_arg.span().end),
                        "Error constructor message must be a string",
                    ));
                }
            }
        };
        let cause = match options_arg {
            None => None,
            Some(Argument::ObjectExpression(options)) => {
                let mut cause = None;
                for property in &options.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        return Err(SmeltError::unsupported(
                            self.span(options.span.start, options.span.end),
                            "Error constructor options must not use spread properties",
                        ));
                    };
                    let value = self.expression(&property.value, body)?;
                    if matches!(&property.key, PropertyKey::StaticIdentifier(key) if key.name == "cause")
                    {
                        cause = Some(value);
                    }
                }
                cause
            }
            Some(options_arg) => {
                return Err(SmeltError::unsupported(
                    self.span(options_arg.span().start, options_arg.span().end),
                    "Error constructor options must be an object literal with an optional cause property",
                ));
            }
        };
        Ok(ErrorConstructorParts {
            message,
            cause,
            errors,
        })
    }

    /// Lower `Error(message)`-style calls to the message value used by HIR throws.
    pub(super) fn error_function_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Error function lowering supports at most one message argument",
            ));
        }
        let Some(message_arg) = call.arguments.first() else {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        };
        let message = self.argument(message_arg, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, message)) == Some(&Type::String) {
            return Ok(message);
        }
        if self.is_string_compatible_type(Self::expr_ty(body, message))
            || self.type_contains_unknown(Self::expr_ty(body, message))
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: message },
                ty,
                span: self.span(message_arg.span().start, message_arg.span().end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(message_arg.span().start, message_arg.span().end),
            "Error function message must be a string",
        ))
    }

    /// Lower an expression while preserving a caller-supplied type hint when possible.
    pub(super) fn expression_with_hint(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expression {
            Expression::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::BigIntLiteral(lit) => {
                self.bigint_literal_expression(lit.value.as_str(), lit.span, body)
            }
            Expression::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::Identifier(ident) => {
                // A function-typed local that collected `fn.prop = …` writes and
                // now coerces to a callable-interface class is bundled into a
                // typed `CallableObjectAssign` here, at the coercion position,
                // instead of leaking the props (the `debounce`/`throttle` shape).
                if let Some(expr) = self.try_consume_callable_local(
                    ident.name.as_str(),
                    ident.span.start,
                    ident.span.end,
                    type_hint,
                    body,
                )? {
                    return Ok(expr);
                }
                self.identifier_expression(
                    ident.name.as_str(),
                    ident.span.start,
                    ident.span.end,
                    body,
                )
            }
            Expression::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Expression::Super(super_expr) => {
                self.identifier_expression("this", super_expr.span.start, super_expr.span.end, body)
            }
            Expression::RegExpLiteral(literal) => {
                let ty = self.regexp_type();
                let pattern = Self::regex_literal_pattern_text_without_flags(literal);
                let flags = literal.regex.flags.to_string();
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(pattern)),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span: self.span(literal.span.start, literal.span.end),
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(flags)),
                    ty: self.ctx.krate.types.intern(Type::String),
                    span: self.span(literal.span.start, literal.span.end),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            Expression::ArrayExpression(array) => self.array_expression(array, body, type_hint),
            Expression::ObjectExpression(object) => {
                self.object_expression(object, body, type_hint)
            }
            Expression::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::Instanceof {
                    return self.instanceof_expression(binary, body);
                }
                if binary.operator == BinaryOperator::In {
                    return self.in_expression(binary, body);
                }
                if let Some(expr) = self.unknown_typeof_comparison(binary, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.unknown_null_comparison(binary, body)? {
                    return Ok(expr);
                }
                if binary.operator == BinaryOperator::Exponential {
                    let base = self.expression(&binary.left, body)?;
                    let exponent = self.expression(&binary.right, body)?;
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::NumericPow { base, exponent },
                        ty,
                        span: self.span(binary.span.start, binary.span.end),
                    }));
                }
                let op = match binary.operator {
                    BinaryOperator::Addition => BinOp::Add,
                    BinaryOperator::Subtraction => BinOp::Sub,
                    BinaryOperator::Multiplication => BinOp::Mul,
                    BinaryOperator::Division => BinOp::Div,
                    BinaryOperator::Remainder => BinOp::Rem,
                    // `===`/`!==` carry JS reference-identity semantics for
                    // erased objects (`BinOp::JsStrictEq`), distinct from `==`'s
                    // structural/deep `BinOp::Eq` that the deep-equality matchers
                    // and `isDeepEqual` rely on. (`x === null`, `typeof x === …`,
                    // and the `=== || Object.is` idiom are intercepted earlier.)
                    BinaryOperator::StrictEquality => BinOp::JsStrictEq,
                    BinaryOperator::Equality => BinOp::Eq,
                    BinaryOperator::StrictInequality => BinOp::JsStrictNotEq,
                    BinaryOperator::Inequality => BinOp::NotEq,
                    BinaryOperator::LessThan => BinOp::Lt,
                    BinaryOperator::LessEqualThan => BinOp::Lte,
                    BinaryOperator::GreaterThan => BinOp::Gt,
                    BinaryOperator::GreaterEqualThan => BinOp::Gte,
                    BinaryOperator::ShiftLeft => BinOp::Shl,
                    BinaryOperator::ShiftRight => BinOp::Shr,
                    BinaryOperator::ShiftRightZeroFill => BinOp::UShr,
                    BinaryOperator::BitwiseAnd => BinOp::BitAnd,
                    BinaryOperator::BitwiseOR => BinOp::BitOr,
                    BinaryOperator::BitwiseXOR => BinOp::BitXor,
                    BinaryOperator::Exponential
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            format!("binary operator is not lowered yet: {:?}", binary.operator),
                        ));
                    }
                };
                let mut lhs = self.expression(&binary.left, body)?;
                let mut rhs = self.expression(&binary.right, body)?;
                // Strict-equality operands narrowed by a `typeof` guard must stay
                // the erased originals: re-materializing a narrowed function
                // through an adapter builds a fresh `Rc` whose `Rc::ptr_eq` never
                // matches, so `f === f` would wrongly be `false`. See the peel in
                // `binary_expression` for the full rationale.
                if matches!(op, BinOp::JsStrictEq | BinOp::JsStrictNotEq) {
                    lhs = self.peel_narrowing_cast_for_identity(body, lhs);
                    rhs = self.peel_narrowing_cast_for_identity(body, rhs);
                }
                let lhs_ty = Self::expr_ty(body, lhs);
                let rhs_ty = Self::expr_ty(body, rhs);
                let ty = if op == BinOp::Add
                    && type_hint
                        .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::String))
                    && (self.is_string_compatible_type(lhs_ty)
                        || self.is_string_compatible_type(rhs_ty)
                        || self.type_contains_unknown(lhs_ty)
                        || self.type_contains_unknown(rhs_ty))
                {
                    self.ctx.krate.types.intern(Type::String)
                } else {
                    self.binary_result_type(op, lhs_ty, rhs_ty)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(binary.span.start, binary.span.end),
                }))
            }
            Expression::LogicalExpression(logical) => {
                // Fold the `(typeof X === 'object' && X) || ...` global-detection
                // chain before lowering its operands, so dead absent-alias clauses
                // (e.g. `&& window`) never resolve their identifier.
                if let Some(expr) = self.global_detection_chain_expression(logical, body) {
                    return Ok(expr);
                }
                // A guard the target profile already decides short-circuits the
                // whole expression before any operand-shape helper runs: several
                // of them lower the RIGHT operand first, and the dead operand is
                // exactly the one the profile cannot resolve.
                if let Some(expr) = self.short_circuited_static_guard(logical, body) {
                    return Ok(expr);
                }
                if let Some(expr) = self.same_value_zero_logical(logical, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
                    return Ok(expr);
                }
                if logical.operator == LogicalOperator::Coalesce {
                    return self.nullish_coalesce_expression(logical, body, type_hint);
                }
                let lhs = self.expression(&logical.left, body)?;
                let rhs_narrowing = if logical.operator == LogicalOperator::And {
                    self.guard_narrowing(&logical.left, body)
                } else {
                    None
                };
                if let Some(narrowing) = rhs_narrowing.clone() {
                    self.scope.push_narrowing_scope(narrowing);
                }
                let rhs = self.expression(&logical.right, body)?;
                if rhs_narrowing.is_some() {
                    self.scope.pop_narrowing_scope();
                }
                // JavaScript's `&&`/`||` select an OPERAND, so in this value
                // position the result is the union of the operand types, not a
                // boolean. `logical_operand_value_expression` builds that
                // selection and returns `None` only where the boolean shape
                // below is still the right one (both operands already boolean,
                // or two operand types with no common lowered shape).
                if let Some(expr) =
                    self.logical_operand_value_expression(logical, body, lhs, rhs)?
                {
                    return Ok(expr);
                }
                let cond = self.lowered_condition_expression(
                    lhs,
                    self.expression_span(&logical.left),
                    body,
                )?;
                let ty = self.ctx.krate.types.intern(Type::Bool);
                let identity = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(
                        logical.operator == LogicalOperator::Or,
                    )),
                    ty,
                    span: self.expression_span(&logical.left),
                });
                let (then_expr, else_expr) = if logical.operator == LogicalOperator::And {
                    (rhs, identity)
                } else {
                    (identity, rhs)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty,
                    span: self.span(logical.span.start, logical.span.end),
                }))
            }
            Expression::ConditionalExpression(conditional) => {
                // Fold `Ctor ? new Ctor(...) : fallback` where `Ctor` is a host
                // constructor Smelt always models as present (see
                // `identifier_is_always_present_global_constructor`): the probe is
                // always true, so the ternary yields its consequent and keeps its
                // concrete shape instead of forcing the mismatched branches to
                // reconcile through `SmeltUnknown`.
                if let Expression::Identifier(test) = &conditional.test
                    && self.identifier_is_always_present_global_constructor(test.name.as_str())
                {
                    return self.expression_with_hint(&conditional.consequent, body, type_hint);
                }
                let cond = self.condition_expression(&conditional.test, body)?;
                let arm_span = self.span(conditional.span.start, conditional.span.end);
                let then_narrowing = self.guard_narrowing(&conditional.test, body);
                if let Some(narrowing) = then_narrowing.clone() {
                    self.scope.push_narrowing_scope(narrowing);
                }
                let then_expr = self.lower_conditional_arm(body, arm_span, |slf, body| {
                    slf.expression_with_hint(&conditional.consequent, body, type_hint)
                })?;
                if then_narrowing.is_some() {
                    self.scope.pop_narrowing_scope();
                }
                let branch_hint = Some(Self::expr_ty(body, then_expr));
                let else_narrowing = self.inverse_guard_narrowing(&conditional.test, body);
                if let Some(narrowing) = else_narrowing.clone() {
                    self.scope.push_narrowing_scope(narrowing);
                }
                let else_expr = self.lower_conditional_arm(body, arm_span, |slf, body| {
                    slf.expression_with_hint(&conditional.alternate, body, branch_hint)
                })?;
                if else_narrowing.is_some() {
                    self.scope.pop_narrowing_scope();
                }
                let then_ty = Self::expr_ty(body, then_expr);
                let else_ty = Self::expr_ty(body, else_expr);
                let ty = if then_ty == else_ty {
                    then_ty
                } else if self.numeric_type_compatible(then_ty, else_ty) {
                    self.ctx.krate.types.intern(Type::Float)
                } else if matches!(
                    (
                        self.ctx.krate.types.get(then_ty),
                        self.ctx.krate.types.get(else_ty)
                    ),
                    (Some(Type::TypeParam { .. }), Some(Type::TypeParam { .. }))
                ) {
                    then_ty
                } else if self.date_runtime_float_matches_type_param(then_ty, else_ty) {
                    else_ty
                } else if self.date_runtime_float_matches_type_param(else_ty, then_ty) {
                    then_ty
                } else if Self::is_empty_list_expr(body, then_expr) {
                    else_ty
                } else if Self::is_empty_list_expr(body, else_expr) {
                    then_ty
                } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
                    self.ctx.krate.types.intern(Type::Optional(else_ty))
                } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
                    self.ctx.krate.types.intern(Type::Optional(then_ty))
                } else if self.compatible_function_branch_types(then_ty, else_ty) {
                    then_ty
                } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
                    function_ty
                } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
                    then_ty
                } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
                    else_ty
                } else if type_hint
                    .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown))
                    || self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
                    || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
                {
                    self.ctx.krate.types.intern(Type::Unknown)
                } else if self.declared_class_type(then_ty) && self.declared_class_type(else_ty) {
                    // Two *declared* classes have no common Rust struct, but they
                    // do have a concrete common representation: the generated
                    // tagged union. Without this arm both branches fall into the
                    // string-compatible test below — `is_string_compatible_type`
                    // accepts any `Type::Class`, because that variant also spells
                    // an opaque unresolved name — and the conditional unifies to
                    // `String`, emitting a `String` local that the class values
                    // are then assigned into. That output does not compile.
                    self.ctx
                        .krate
                        .types
                        .intern(Type::Union(vec![then_ty, else_ty]))
                } else if self.is_string_compatible_type(then_ty)
                    && (self.is_string_compatible_type(else_ty)
                        || self.union_has_string_compatible_member(else_ty))
                    || self.is_string_compatible_type(else_ty)
                        && self.union_has_string_compatible_member(then_ty)
                {
                    self.ctx.krate.types.intern(Type::String)
                } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Dict(_, _)))
                    && matches!(self.ctx.krate.types.get(else_ty), Some(Type::Dict(_, _)))
                {
                    self.ctx
                        .krate
                        .types
                        .intern(Type::Union(vec![then_ty, else_ty]))
                } else if let (Some(Type::List(then_item)), Some(Type::List(else_item))) = (
                    self.ctx.krate.types.get(then_ty).cloned(),
                    self.ctx.krate.types.get(else_ty).cloned(),
                ) {
                    // Both branches are arrays whose element types differ. Unify
                    // the element types with the same branch rules and keep an
                    // array shape, falling back to a list of the union of the
                    // element types when they have no closer common shape.
                    let item_ty = self.unify_conditional_list_item_type(then_item, else_item);
                    self.ctx.krate.types.intern(Type::List(item_ty))
                } else if self.type_contains_unknown(then_ty) || self.type_contains_unknown(else_ty)
                {
                    self.ctx.krate.types.intern(Type::Unknown)
                } else if let Some(hint) = type_hint
                    && !self.concrete_type_requires_never_value(hint)
                {
                    hint
                } else if self.erased_or_union_surface(then_ty)
                    || self.erased_or_union_surface(else_ty)
                    || matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(_)))
                    || matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(_)))
                    || matches!(
                        (
                            self.ctx.krate.types.get(then_ty),
                            self.ctx.krate.types.get(else_ty)
                        ),
                        (Some(Type::Function(_)), Some(Type::Function(_)))
                    )
                {
                    // One branch keeps a union/erased surface with no single
                    // concrete Rust shape (e.g. `isArrayLike(source) ? source :
                    // Object.values(source)` where `source` stays `ArrayLike<T>
                    // | Record<string, T>`): the merged value is a genuine
                    // dynamic boundary and widens to `unknown`.
                    self.ctx.krate.types.intern(Type::Unknown)
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(conditional.span.start, conditional.span.end),
                        format!(
                            "conditional expression branches must have the same lowered type (then: {:?}, else: {:?})",
                            self.ctx.krate.types.get(then_ty),
                            self.ctx.krate.types.get(else_ty)
                        ),
                    ));
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty,
                    span: self.span(conditional.span.start, conditional.span.end),
                }))
            }
            Expression::UnaryExpression(unary) => {
                if unary.operator == UnaryOperator::Typeof {
                    return self.typeof_expression(unary, body);
                }
                if unary.operator == UnaryOperator::Delete {
                    return self.unary_expression(unary, body);
                }
                if unary.operator == UnaryOperator::Void {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(unary.span.start, unary.span.end),
                    }));
                }
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    UnaryOperator::UnaryPlus => {
                        let operand = self.expression(&unary.argument, body)?;
                        let operand_ty = Self::expr_ty(body, operand);
                        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Int | Type::Float)) {
                            return Ok(operand);
                        }
                        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Bool))
                            || self.is_date_constructor_arg_type(operand_ty)
                        {
                            let ty = self.ctx.krate.types.intern(Type::Float);
                            return Ok(body.push_expr(Expr {
                                kind: ExprKind::PrimitiveCast {
                                    op: PrimitiveCastOp::ToJsNumber,
                                    operand,
                                },
                                ty,
                                span: self.span(unary.span.start, unary.span.end),
                            }));
                        }
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            "unary plus requires a numeric or DateArg-compatible operand",
                        ));
                    }
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!("unary operator is not lowered yet: {:?}", unary.operator),
                        ));
                    }
                };
                let operand = self.expression(&unary.argument, body)?;
                let operand = if matches!(op, UnaryOp::Not) {
                    self.optional_known_date_presence_condition(
                        operand,
                        self.expression_span(&unary.argument),
                        body,
                    )
                    .unwrap_or(operand)
                } else {
                    operand
                };
                let ty = if matches!(op, UnaryOp::Not) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, operand)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnaryOp { op, operand },
                    ty,
                    span: self.span(unary.span.start, unary.span.end),
                }))
            }
            Expression::AwaitExpression(await_expr) => {
                self.await_expression(await_expr, type_hint, body)
            }
            Expression::UpdateExpression(update) => self.update_expression(update, body),
            Expression::StaticMemberExpression(member) => self.static_member(member, body),
            Expression::ComputedMemberExpression(member) => {
                if type_hint.is_some_and(|hint| {
                    matches!(
                        self.ctx.krate.types.get(hint),
                        Some(Type::Unknown | Type::TypeParam { .. })
                    )
                })
                    && let Some(expr) = self.unknown_computed_member_with_hint(member, body)?
                {
                    return Ok(expr);
                }
                self.computed_member(member, body)
            }
            Expression::CallExpression(call) => {
                let value = self.call_expression(call, body)?;
                // A bare `Array(n)` allocation takes the contextual list type
                // when it has one, exactly as the `new Array(n)` spelling does
                // below; the two forms must stay in lockstep.
                let value = self.adopt_contextual_list_allocation_type(value, type_hint, body);
                let statically_callable = body
                    .exprs
                    .get(usize::try_from(value.0).unwrap_or(usize::MAX))
                    .and_then(|lowered_expr| match &lowered_expr.kind {
                        ExprKind::Call { callee, .. } | ExprKind::ClosureCall { callee, .. } => {
                            Some(*callee)
                        }
                        _ => None,
                    })
                    .is_some_and(|callee| {
                        matches!(
                            self.ctx.krate.types.get(Self::expr_ty(body, callee)),
                            Some(Type::Function(_))
                        )
                    });
                if let Some(hint) = type_hint
                    && statically_callable
                    && matches!(self.ctx.krate.types.get(hint), Some(Type::Future(_)))
                    && matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, value)),
                        Some(Type::None | Type::Unknown)
                    )
                    && let Some(contextual_expr) = body
                        .exprs
                        .get_mut(usize::try_from(value.0).unwrap_or(usize::MAX))
                {
                    // Validated contextual future typing recovers an unresolved
                    // conditional/generic call result without changing the call.
                    contextual_expr.ty = hint;
                }
                Ok(value)
            }
            Expression::AssignmentExpression(assign) => {
                if let Some(expr) = self.try_global_assignment_expression(assign, body)? {
                    return Ok(expr);
                }
                let (_target, value) = self.assignment_parts(assign, body)?;
                Ok(value)
            }
            Expression::YieldExpression(yield_expr) => {
                if yield_expr.delegate && self.current_generator_yields.is_some() {
                    return self.generator_delegate_expression(yield_expr, body);
                }
                if self.current_generator_yields.is_some() {
                    return self.generator_yield_expression(yield_expr, body);
                }
                Err(SmeltError::unsupported(
                    self.span(yield_expr.span.start, yield_expr.span.end),
                    "yield is only valid inside a generator",
                ))
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.arrow_function_expression_with_hint(arrow, body, type_hint)
            }
            Expression::FunctionExpression(function) => self.function_expression_value(
                function,
                type_hint,
                function.span,
                body,
            ),
            Expression::ClassExpression(class) => self.class_expression_value(class, body),
            Expression::ChainExpression(chain) => self.chain_expression(chain, body),
            Expression::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Expression::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Expression::TSSatisfiesExpression(satisfies) => {
                self.expression(&satisfies.expression, body)
            }
            Expression::TSNonNullExpression(non_null) => self.non_null_assertion_expression(
                &non_null.expression,
                self.span(non_null.span.start, non_null.span.end),
                body,
            ),
            Expression::ParenthesizedExpression(parenthesized) => {
                self.expression_with_hint(&parenthesized.expression, body, type_hint)
            }
            Expression::NewExpression(new_expr) => {
                let value = self.new_expression_with_hint(new_expr, body, type_hint)?;
                Ok(self.adopt_contextual_list_allocation_type(value, type_hint, body))
            }
            Expression::TemplateLiteral(tpl) => self.template_literal_expression(tpl, body),
            Expression::PrivateFieldExpression(member) => self.private_field_member(
                &member.object,
                member.field.name.as_str(),
                member.span,
                body,
            ),
            Expression::TaggedTemplateExpression(tagged) => {
                self.tagged_template_expression(tagged, body)
            }
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }

    /// Lower a TypeScript bigint literal into Smelt's current numeric runtime value.
    pub(super) fn bigint_literal_expression(
        &mut self,
        value: &str,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = value.parse::<f64>().map_err(|err| {
            SmeltError::unsupported(
                self.span(span.start, span.end),
                format!("bigint literal cannot be represented numerically: {err}"),
            )
        })?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span: self.span(span.start, span.end),
        }))
    }

    /// Lower JavaScript `typeof value` to a string result when used as a value.
    pub(super) fn typeof_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::Identifier(identifier) = &unary.argument
            && identifier.name == "crypto"
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("undefined".to_owned())),
                ty,
                span: self.span(unary.span.start, unary.span.end),
            }));
        }
        // A bare `typeof Blob` references the modeled host constructor, which is
        // a function value in JavaScript. (The `typeof Blob === 'undefined'`
        // support-guard comparison is folded earlier in `unknown_typeof_comparison`.)
        if let Expression::Identifier(identifier) = &unary.argument
            && Self::is_known_defined_global_constructor(identifier.name.as_str())
        {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("function".to_owned())),
                ty,
                span: self.span(unary.span.start, unary.span.end),
            }));
        }
        let operand = self.expression(&unary.argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        let ty = self.ctx.krate.types.intern(Type::String);
        // When the operand has no statically-known `typeof` spelling (erased
        // `unknown`/`any`, unions, type params, futures — everything
        // `typeof_type_name` returns `None` for), the JavaScript `typeof` must be
        // computed at runtime by inspecting the value's tag. Folding to a literal
        // here (the historical `.unwrap_or("object")`) mis-typed erased values as
        // objects, which made `typeof a === typeof b` fold to `"object" ==
        // "object"` and the primitive arms of `switch (typeof a)` dead (see
        // es-toolkit `isEqualWith`). Only fold when the type pins a single spelling.
        if let Some(kind) = self.typeof_type_name(operand_ty) {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(kind.to_owned())),
                ty,
                span: self.span(unary.span.start, unary.span.end),
            }));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::TypeofValue { value: operand },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower a TypeScript conditional expression when it appears outside normal expression nodes.
    /// Lower one arm of a conditional expression, capturing any statement-level
    /// side effects it produces into a per-arm block so they run only when that
    /// arm is taken.
    ///
    /// Effectful sub-expressions such as a postfix `k++` inside `xs[k++]` lower
    /// their store into `self.current_statement_block` (see `update_expression`).
    /// For a ternary operand that block is the *enclosing* statement block, so
    /// without redirection the increment is hoisted out of the branch and runs
    /// unconditionally on every evaluation — the es-toolkit `partial`/
    /// `partialRight` `providedArgs[idx++]` placeholder bug. Redirecting the arm
    /// to a fresh block and wrapping it as an `ExprKind::Block` (tail = the arm
    /// value) keeps the increment inside the arm, where MIR lowers it within the
    /// matching switch branch. Arms with no side effects return their expression
    /// unwrapped so simple ternaries are unchanged.
    fn lower_conditional_arm(
        &mut self,
        body: &mut Body,
        span: Span,
        lower: impl FnOnce(&mut Self, &mut Body) -> Result<smelt_hir::ExprId, SmeltError>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let arm_block = body.push_block(span);
        let previous_block = self.current_statement_block.replace(arm_block);
        let arm_result = lower(self, body);
        self.current_statement_block = previous_block;
        let arm_expr = arm_result?;
        if body.blocks[arm_block.0 as usize].stmts.is_empty() {
            return Ok(arm_expr);
        }
        body.blocks[arm_block.0 as usize].tail = Some(arm_expr);
        let ty = Self::expr_ty(body, arm_expr);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Block(arm_block),
            ty,
            span,
        }))
    }

    pub(super) fn conditional_expression(
        &mut self,
        conditional: &oxc::ast::ast::ConditionalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // A constructor-presence guard `Ctor ? new Ctor(...) : fallback` where
        // `Ctor` is a bare host constructor Smelt always models as present (typed
        // arrays, `ArrayBuffer`/`Blob`/`Buffer`, the marker-only host builtins).
        // The environment probe is always true in Smelt's model, so the ternary
        // yields its consequent; folding to that branch keeps its concrete shape
        // (es-toolkit's `merge` spec writes `Uint8Array ? new Uint8Array([1]) : {
        // buffer: [1] }`, whose `List` and `Dict` arms have no common concrete
        // Rust type — widening the merge to `SmeltUnknown` to reconcile them is
        // exactly the avoidable erasure the ABI rules forbid). A user binding or
        // class of the same name shadows the global and is not folded.
        if let Expression::Identifier(test) = &conditional.test
            && self.identifier_is_always_present_global_constructor(test.name.as_str())
        {
            return self.expression_with_hint(&conditional.consequent, body, type_hint);
        }
        let cond = self.condition_expression(&conditional.test, body)?;
        let arm_span = self.span(conditional.span.start, conditional.span.end);
        let then_expr = self.lower_conditional_arm(body, arm_span, |slf, body| {
            slf.expression_with_hint(&conditional.consequent, body, type_hint)
        })?;
        let branch_hint = Some(Self::expr_ty(body, then_expr));
        let else_expr = self.lower_conditional_arm(body, arm_span, |slf, body| {
            slf.expression_with_hint(&conditional.alternate, body, branch_hint)
        })?;
        let then_ty = Self::expr_ty(body, then_expr);
        let else_ty = Self::expr_ty(body, else_expr);
        let ty = self.conditional_branch_type(
            then_ty,
            else_ty,
            type_hint,
            conditional.span.start,
            conditional.span.end,
        )?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span: self.span(conditional.span.start, conditional.span.end),
        }))
    }

    /// Return whether a bare identifier names a host constructor that Smelt
    /// always models as present, so a `Ctor ? … : …` environment-presence guard
    /// folds to its consequent.
    ///
    /// The set matches the constructors with a concrete construct + `instanceof`
    /// lowering: the typed-array views and
    /// `ArrayBuffer`/`Blob`/`File`/`Buffer`/`DataView` (all byte-backed host
    /// objects), plus the marker-only host builtins.
    /// A local binding or user class of the same name shadows the global, so it
    /// is excluded — the guard then lowers normally.
    fn identifier_is_always_present_global_constructor(&self, name: &str) -> bool {
        if self.scope.is_bound(name) || self.classes.contains(name) {
            return false;
        }
        smelt_stdlib::is_typed_array_class_name(name)
            || Self::is_known_defined_global_constructor(name)
    }

    /// Compute the result type for a conditional expression's branches.
    pub(super) fn conditional_branch_type(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
        type_hint: Option<smelt_hir::TypeId>,
        start: u32,
        end: u32,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if then_ty == else_ty {
            Ok(then_ty)
        } else if self.numeric_type_compatible(then_ty, else_ty) {
            Ok(self.ctx.krate.types.intern(Type::Float))
        } else if matches!(
            (
                self.ctx.krate.types.get(then_ty),
                self.ctx.krate.types.get(else_ty)
            ),
            (Some(Type::TypeParam { .. }), Some(Type::TypeParam { .. }))
        ) {
            Ok(then_ty)
        } else if self.date_runtime_float_matches_type_param(then_ty, else_ty) {
            Ok(else_ty)
        } else if self.date_runtime_float_matches_type_param(else_ty, then_ty) {
            Ok(then_ty)
        } else if self.ctx.krate.types.get(then_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(else_ty)))
        } else if self.ctx.krate.types.get(else_ty) == Some(&Type::None) {
            Ok(self.ctx.krate.types.intern(Type::Optional(then_ty)))
        } else if let Some(unified) = self.unify_optional_conditional_branches(then_ty, else_ty) {
            // One branch is `Optional<inner>` and the other is compatible with
            // `inner` (equal or numerically widenable), e.g. a `number | undefined`
            // flow-typed local vs a `number` literal (`isNaN(x) ? 0 : x`). Merge
            // to `Optional<unified-inner>` so the optional surface is preserved
            // instead of aborting because a bare and an optional numeric differ.
            Ok(unified)
        } else if self.compatible_function_branch_types(then_ty, else_ty) {
            Ok(then_ty)
        } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
            Ok(function_ty)
        } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
            Ok(then_ty)
        } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
            Ok(else_ty)
        } else if let (Some(Type::List(then_item)), Some(Type::List(else_item))) = (
            self.ctx.krate.types.get(then_ty).cloned(),
            self.ctx.krate.types.get(else_ty).cloned(),
        ) {
            // Both branches are arrays whose element types differ (e.g.
            // `index ? numberArray : numberOrNullArray`). Unify the element
            // types and return a list of the unified element type so the array
            // shape is preserved instead of collapsing the whole value to
            // `unknown`.
            let item_ty = self.unify_conditional_list_item_type(then_item, else_item);
            Ok(self.ctx.krate.types.intern(Type::List(item_ty)))
        } else if type_hint
            .is_some_and(|hint| self.ctx.krate.types.get(hint) == Some(&Type::Unknown))
            || self.ctx.krate.types.get(then_ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(else_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(then_ty)
            || self.type_contains_unknown(else_ty)
        {
            Ok(self.ctx.krate.types.intern(Type::Unknown))
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            Ok(hint)
        } else if self.erased_or_union_surface(then_ty)
            || self.erased_or_union_surface(else_ty)
            || matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(_)))
            || matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(_)))
            || matches!(
                (
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
                (Some(Type::Function(_)), Some(Type::Function(_)))
            )
        {
            // One branch keeps a union/erased surface with no single concrete
            // Rust shape (e.g. `isArrayLike(source) ? source :
            // Object.values(source)` where `source` stays `ArrayLike<T> |
            // Record<string, T>`): the merged value is a genuine dynamic
            // boundary and widens to `unknown`.
            Ok(self.ctx.krate.types.intern(Type::Unknown))
        } else {
            Err(SmeltError::unsupported(
                self.span(start, end),
                format!(
                    "conditional expression branches must have the same lowered type (then: {:?}, else: {:?})",
                    self.ctx.krate.types.get(then_ty),
                    self.ctx.krate.types.get(else_ty)
                ),
            ))
        }
    }

    /// Merge a conditional whose branches are a value and an `Optional` of a
    /// compatible value into a single `Optional` result.
    ///
    /// Returns `Some(Optional<inner>)` when exactly one branch is `Optional<a>`
    /// and the other branch's type unifies with `a` (identical, or numerically
    /// widenable so `Float`/`Int` mix), or when both branches are optionals whose
    /// inners unify. This preserves the optional surface produced by flow typing
    /// (`x: number | undefined; cond ? 0 : x`) instead of failing because a bare
    /// numeric and an optional numeric have different lowered shapes. Returns
    /// `None` when neither branch is optional or the inners are unrelated.
    fn unify_optional_conditional_branches(
        &mut self,
        then_ty: smelt_hir::TypeId,
        else_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let then_opt = match self.ctx.krate.types.get(then_ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let else_opt = match self.ctx.krate.types.get(else_ty) {
            Some(Type::Optional(inner)) => Some(*inner),
            _ => None,
        };
        let inner = match (then_opt, else_opt) {
            (Some(then_inner), Some(else_inner)) => {
                self.unify_compatible_branch_inner(then_inner, else_inner)?
            }
            (Some(then_inner), None) => {
                self.unify_compatible_branch_inner(then_inner, else_ty)?
            }
            (None, Some(else_inner)) => {
                self.unify_compatible_branch_inner(then_ty, else_inner)?
            }
            (None, None) => return None,
        };
        Some(self.ctx.krate.types.intern(Type::Optional(inner)))
    }

    /// Return the unified inner type for two conditional-branch inners that are
    /// identical or numerically compatible, or `None` if unrelated.
    fn unify_compatible_branch_inner(
        &mut self,
        left: smelt_hir::TypeId,
        right: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        if left == right {
            Some(left)
        } else if self.numeric_type_compatible(left, right) {
            Some(self.ctx.krate.types.intern(Type::Float))
        } else {
            None
        }
    }

    /// Unify the element types of two array branches of a conditional expression.
    ///
    /// Both branches are already known to be `List<...>`; this picks an element
    /// type for the merged `List<...>` result. It never fails: when the elements
    /// have no closer common shape it widens to their union (or `unknown` when an
    /// element is itself erased), so an array-producing ternary always keeps an
    /// array shape rather than aborting lowering.
    pub(super) fn unify_conditional_list_item_type(
        &mut self,
        then_item: smelt_hir::TypeId,
        else_item: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if then_item == else_item {
            then_item
        } else if self.numeric_type_compatible(then_item, else_item) {
            self.ctx.krate.types.intern(Type::Float)
        } else if self.ctx.krate.types.get(then_item) == Some(&Type::None) {
            self.ctx.krate.types.intern(Type::Optional(else_item))
        } else if self.ctx.krate.types.get(else_item) == Some(&Type::None) {
            self.ctx.krate.types.intern(Type::Optional(then_item))
        } else if let (Some(Type::List(then_inner)), Some(Type::List(else_inner))) = (
            self.ctx.krate.types.get(then_item).cloned(),
            self.ctx.krate.types.get(else_item).cloned(),
        ) {
            let inner = self.unify_conditional_list_item_type(then_inner, else_inner);
            self.ctx.krate.types.intern(Type::List(inner))
        } else if self.type_contains_unknown(then_item) || self.type_contains_unknown(else_item) {
            self.ctx.krate.types.intern(Type::Unknown)
        } else {
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![then_item, else_item]))
        }
    }

    /// Return whether a timestamp-backed Date branch can flow into a generic date type.
    pub(super) fn date_runtime_float_matches_type_param(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        self.ctx.krate.types.get(actual) == Some(&Type::Float)
            && matches!(
                self.ctx.krate.types.get(expected),
                Some(Type::TypeParam { .. })
            )
    }

    /// Return true when an expression is an uninhabited empty array literal.
    pub(super) fn is_empty_list_expr(body: &Body, expr: smelt_hir::ExprId) -> bool {
        matches!(
            body.exprs.get(usize::try_from(expr.0).unwrap_or(usize::MAX)),
            Some(Expr {
                kind: ExprKind::ListLit(items),
                ..
            }) if items.is_empty()
        )
    }

    /// Lower a JavaScript condition to a boolean expression.
    ///
    /// TypeScript permits optional values in truthiness positions. Smelt models
    /// the common `value ? a : b` and `if (value)` optional-object/string cases
    /// as a `value != None` check once the expression has lowered to
    /// `Optional<T>`.
    pub(super) fn condition_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `&&`/`||` yield an OPERAND, not a boolean, so in a value position they
        // lower to a union-typed selection (see
        // `logical_operand_value_expression`). A condition only observes that
        // operand's truthiness, which distributes over the operator, so lower
        // the condition form directly to a boolean instead of building a union
        // value just to test it.
        if let Expression::LogicalExpression(logical) = Self::unparenthesized_expression(expression)
            && let Some(cond) = self.logical_condition_expression(logical, body)?
        {
            return Ok(cond);
        }
        let cond = self.expression(expression, body)?;
        self.lowered_condition_expression(cond, self.expression_span(expression), body)
    }

    /// Lower a JavaScript `await` operand into a HIR await, or into the operand
    /// itself when it provably cannot be a thenable.
    ///
    /// Three cases, in order:
    ///
    /// 1. the operand's type is a future — await it and take the future's
    ///    output type;
    /// 2. the operand's type is erased or a union — it may still hold a runtime
    ///    promise, so assert it to a future and await it through the checked
    ///    extraction path, whose runtime helper drains promise chains and
    ///    passes non-thenables through unchanged. The awaited value takes the
    ///    contextual type when there is one and stays erased otherwise;
    /// 3. the operand's type is concrete and not a future — `await v` is `v`.
    ///
    /// No case discards the operand: doing so silently deletes the awaited
    /// computation, and its side effects, from the program.
    ///
    /// Shared by the ordinary expression path and the call-argument path so
    /// both spellings of `await x` lower through one rule.
    pub(in crate::lowering) fn await_expression(
        &mut self,
        await_expr: &oxc::ast::ast::AwaitExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if !self.current_async {
            return Err(SmeltError::unsupported(
                self.span(await_expr.span.start, await_expr.span.end),
                "await expressions are only lowered inside async functions",
            ));
        }
        // The context describes the resolved value; the operand must therefore
        // produce a future of that value.
        let awaited_hint = type_hint.map(|hint| self.ctx.krate.types.intern(Type::Future(hint)));
        let awaited = self.expression_with_hint(&await_expr.argument, body, awaited_hint)?;
        let awaited_ty = Self::expr_ty(body, awaited);
        let Some(ty) = self.future_inner_type(awaited_ty) else {
            if self.erased_or_union_surface(awaited_ty) {
                let resolved_ty =
                    type_hint.unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                let future_ty = self.ctx.krate.types.intern(Type::Future(resolved_ty));
                let future = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: awaited },
                    ty: future_ty,
                    span: self.span(
                        await_expr.argument.span().start,
                        await_expr.argument.span().end,
                    ),
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(future),
                    ty: resolved_ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }));
            }
            return Ok(awaited);
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::Await(awaited),
            ty,
            span: self.span(await_expr.span.start, await_expr.span.end),
        }))
    }

    /// Coerce an already lowered JavaScript value into its boolean truthiness result.
    ///
    /// Assignment operators such as `||=` already lower their target as a
    /// writable place. Reusing the resulting expression here avoids lowering a
    /// computed receiver solely to form the condition that selects its value.
    pub(super) fn lowered_condition_expression(
        &mut self,
        cond: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond_ty = Self::expr_ty(body, cond);
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::Bool) {
            return Ok(cond);
        }
        // A function or class value is always truthy in JavaScript, and so is a
        // type parameter whose constraint pins it to an object surface. An
        // unconstrained `T`, however, can hold `0`, `-0`, `NaN`, `""`, `false`,
        // `null` or `undefined`, so it must be tested, not folded: it falls
        // through to the `type_is_truthy_condition_surface` cast below.
        if matches!(
            self.ctx.krate.types.get(cond_ty),
            Some(Type::Function(_) | Type::Class { .. })
        ) || (matches!(self.ctx.krate.types.get(cond_ty), Some(Type::TypeParam { .. }))
            && self.type_is_always_truthy_object_surface(cond_ty))
        {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span,
            }));
        }
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::String) {
            let string_ty = self.ctx.krate.types.intern(Type::String);
            let empty = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: empty,
                },
                ty: bool_ty,
                span,
            }));
        }
        if matches!(self.ctx.krate.types.get(cond_ty), Some(Type::Int | Type::Float)) {
            let zero = body.push_expr(Expr {
                kind: match self.ctx.krate.types.get(cond_ty) {
                    Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
                    _ => ExprKind::Literal(Literal::Float(0.0)),
                },
                ty: cond_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: zero,
                },
                ty: bool_ty,
                span,
            }));
        }
        if let Some(condition) = self.optional_known_date_presence_condition(cond, span, body) {
            return Ok(condition);
        }
        if self
            .non_nullish_type(cond_ty)
            .is_some_and(|inner_ty| self.type_is_always_truthy_object_surface(inner_ty))
        {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            let none = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span,
            });
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::NotEq,
                    lhs: cond,
                    rhs: none,
                },
                ty: bool_ty,
                span,
            }));
        }
        if self.is_nullishable_type(cond_ty) || self.type_is_truthy_condition_surface(cond_ty) {
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    operand: cond,
                },
                ty: bool_ty,
                span,
            }));
        }
        Err(SmeltError::unsupported(
            span,
            format!(
                "condition expression must be boolean or optional (got {:?})",
                self.ctx.krate.types.get(cond_ty)
            ),
        ))
    }

    /// Lower truthiness for optional Date values as object presence.
    ///
    /// Date instances are represented by timestamps in Rust, but source
    /// truthiness depends on the Date object existing, not on its timestamp.
    pub(super) fn optional_known_date_presence_condition(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value_ty = Self::expr_ty(body, value);
        if !self.is_nullishable_type(value_ty)
            || !self.expression_is_known_date_value(value, body)
        {
            return None;
        }
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let none = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: none_ty,
            span,
        });
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: none,
            },
            ty: bool_ty,
            span,
        }))
    }

    /// Return whether a present optional value is always truthy in JavaScript.
    pub(super) fn type_is_always_truthy_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(ty)),
            Some(
                Type::Class { .. }
                    | Type::Function(_)
                    | Type::List(_)
                    | Type::Set(_)
                    | Type::Dict(_, _)
                    | Type::Tuple(_)
                    | Type::Future(_)
            )
        )
    }

    /// Return whether a non-boolean type can appear in a JavaScript truthiness guard.
    pub(super) fn type_is_truthy_condition_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Function(_)
                | Type::Class { .. }
                | Type::TypeParam { .. }
                | Type::Unknown
                | Type::Never,
            ) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| {
                    matches!(
                        self.ctx.krate.types.get(item),
                        Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None)
                    ) || self.type_is_truthy_condition_surface(item)
                }),
            _ => false,
        }
    }

    /// Lower a template literal as string concatenation.
    pub(super) fn template_literal_expression(
        &mut self,
        tpl: &oxc::ast::ast::TemplateLiteral<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let str_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(tpl.span.start, tpl.span.end);
        let Some(first_quasi) = tpl.quasis.first() else {
            return Err(SmeltError::unsupported(
                self.span(tpl.span.start, tpl.span.end),
                "template literals must contain at least one quasi",
            ));
        };
        let first_str = first_quasi
            .value
            .cooked
            .as_ref()
            .map_or_else(|| first_quasi.value.raw.as_str(), |c| c.as_str())
            .to_owned();
        let mut acc = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(first_str)),
            ty: str_ty,
            span,
        });

        for (i, interp) in tpl.expressions.iter().enumerate() {
            let part = self.expression(interp, body)?;
            acc = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: acc,
                    rhs: part,
                },
                ty: str_ty,
                span,
            });
            if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                let s = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                if !s.is_empty() {
                    let lit = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(s.to_owned())),
                        ty: str_ty,
                        span,
                    });
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: lit,
                        },
                        ty: str_ty,
                        span,
                    });
                }
            }
        }
        Ok(acc)
    }

    /// Lower a tagged template literal (a `tag` applied to a template literal).
    ///
    /// Only the `String.raw` builtin tag is modeled. `String.raw` has fully
    /// defined semantics for *any* template: it concatenates the template's
    /// **raw** quasis (backslash escapes left verbatim) interleaved with the
    /// string-coerced substitution values. This lowering implements exactly that
    /// — it is a complete, general implementation of the stdlib builtin, not a
    /// per-call-site special case, mirroring how the other `String.*` statics are
    /// modeled.
    ///
    /// General user-defined tags desugar to `tag(cookedStrings, ...subs)` where
    /// the `cookedStrings` argument is a `TemplateStringsArray` that also carries
    /// a `.raw` sibling array. Smelt's array model is homogeneous and cannot yet
    /// attach that `.raw` property to an array value, so custom tags remain an
    /// explicit deferral with a descriptive error rather than a silently wrong
    /// cooked-only call.
    pub(super) fn tagged_template_expression(
        &mut self,
        tagged: &oxc::ast::ast::TaggedTemplateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if Self::is_string_raw_tag(&tagged.tag) {
            return self.string_raw_template_expression(tagged, body);
        }
        Err(SmeltError::unsupported(
            self.span(tagged.span.start, tagged.span.end),
            "tagged template literals are only supported for the `String.raw` \
             builtin; user-defined tags need a `TemplateStringsArray` with a \
             `.raw` sibling, which Smelt's homogeneous array model cannot yet \
             represent",
        ))
    }

    /// Return true when a tagged-template tag is the `String.raw` builtin,
    /// i.e. a static member read of `raw` off the global `String` identifier
    /// that source has not shadowed with its own binding.
    fn is_string_raw_tag(tag: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = tag else {
            return false;
        };
        if member.property.name != "raw" {
            return false;
        }
        matches!(&member.object, Expression::Identifier(ident) if ident.name == "String")
    }

    /// Lower a `String.raw` tagged template to interleaved raw-quasi /
    /// substitution concatenation.
    ///
    /// The result is `raw[0] + str(sub[0]) + raw[1] + … + raw[n]`, using the raw
    /// (unescaped) quasi text exactly as `String.raw` specifies. Substitutions are
    /// lowered through the normal expression path and string-coerced by the `+`
    /// operator, matching the runtime string concatenation the cooked-template
    /// path already relies on.
    fn string_raw_template_expression(
        &mut self,
        tagged: &oxc::ast::ast::TaggedTemplateExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let tpl = &tagged.quasi;
        let str_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(tagged.span.start, tagged.span.end);
        let Some(first_quasi) = tpl.quasis.first() else {
            return Err(SmeltError::unsupported(
                span,
                "template literals must contain at least one quasi",
            ));
        };
        let mut acc = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(first_quasi.value.raw.as_str().to_owned())),
            ty: str_ty,
            span,
        });
        for (i, interp) in tpl.expressions.iter().enumerate() {
            let part = self.expression(interp, body)?;
            acc = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: acc,
                    rhs: part,
                },
                ty: str_ty,
                span,
            });
            if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                let raw = quasi.value.raw.as_str();
                if !raw.is_empty() {
                    let lit = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(raw.to_owned())),
                        ty: str_ty,
                        span,
                    });
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: lit,
                        },
                        ty: str_ty,
                        span,
                    });
                }
            }
        }
        Ok(acc)
    }

    // Continued in the next split builder file.
}
