//! `new` expression and construction lowering for the TypeScript frontend.
//!
//! Lowers `new ...` expressions into typed HIR: the `new` dispatch, user class
//! construction, construction through a constructor value, and class values.
//! Built-in host classes are `host_constructors`; errors are
//! `error_constructors`.

mod error_constructors;
mod host_constructors;

pub(in crate::lowering) use host_constructors::HostConstructor;

use super::ModuleBuilder;
use crate::SmeltError;
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{
    Body, ClosureExpr, ErasureReason, Expr, ExprKind, FunctionType, Item, LocalDecl, Param, Stmt,
    Type,
};
use std::collections::HashMap;

impl ModuleBuilder<'_> {
    /// `new value.constructor(args)` on an erased value, spelled through casts and
    /// parentheses. A value with a static class keeps the ordinary route.
    fn construct_like_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut callee = &new_expr.callee;
        loop {
            callee = match callee {
                Expression::ParenthesizedExpression(inner) => &inner.expression,
                Expression::TSAsExpression(inner) => &inner.expression,
                Expression::TSNonNullExpression(inner) => &inner.expression,
                _ => break,
            };
        }
        let Expression::StaticMemberExpression(member) = callee else {
            return Ok(None);
        };
        if member.property.name != "constructor"
            || new_expr.arguments.iter().any(Argument::is_spread)
        {
            return Ok(None);
        }
        self.speculate(body, |this, body| {
            let value = this.expression(&member.object, body)?;
            let value_ty = Self::expr_ty(body, value);
            let by_kind = matches!(this.ctx.krate.types.get(value_ty), Some(Type::List(_)));
            if !by_kind && !this.erased_or_union_surface(value_ty) {
                return Ok(None);
            }
            let args = new_expr
                .arguments
                .iter()
                .map(|arg| this.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let ty = this.erase_with(ErasureReason::ByName, "new on dynamic callee");
            Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ConstructLike { value, args },
                ty,
                span: this.span(new_expr.span.start, new_expr.span.end),
            })))
        })
    }

    /// Lower a `new ...` expression, including stdlib containers and class construction.
    pub(super) fn new_expression_with_hint(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // A constructed value is never nullish, so only the present part of
        // a `T | undefined` hint (an optional slot it is stored into:
        // `Box.cache = new Map()` for `static cache?: Map<K, V>`) describes
        // it; the store widens it back.
        let type_hint = type_hint.map(|hint| match self.ctx.krate.types.get(hint) {
            Some(Type::Optional(inner)) => *inner,
            _ => hint,
        });
        if let Some(selected) =
            self.conditional_class_select(Some(&new_expr.callee), span, body, |this, body| {
                this.new_expression_with_hint(new_expr, body, type_hint)
            })?
        {
            return Ok(selected);
        }
        if let Some(expr) = self.construct_like_expression(new_expr, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.new_through_statics_frame(new_expr, body)? {
            return Ok(expr);
        }
        // `Set`, `Map` and `Promise` are host constructors only where the
        // callee resolves to the host: a parameter, local, class or import of
        // one of those names is constructed as that binding.
        let host_callee = match &new_expr.callee {
            Expression::Identifier(callee) => self.host_global_named(callee.name.as_str()),
            _ => None,
        };
        if let Some(host) = host_callee {
            if let Some(expr) = self.set_constructor_expression(host, new_expr, body, type_hint)? {
                return Ok(expr);
            }
            if let Some(expr) = self.map_constructor_expression(host, new_expr, body, type_hint)? {
                return Ok(expr);
            }
            if let Some(expr) =
                self.promise_constructor_expression(host, new_expr, body, type_hint)?
            {
                return Ok(expr);
            }
        }
        let Expression::Identifier(callee) = &new_expr.callee else {
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
                // of erasing to `unknown`. This mirrors the
                // identifier value-callee path in `new_through_value_expression`.
                if let Some(expr) =
                    self.new_through_member_constructor_slot(new_expr, member, body)?
                {
                    return Ok(expr);
                }
                // A member of a value holds a constructor at run time, so
                // `new table.p()` constructs through it below; only a member of
                // a namespace names a class.
                let member_of_value = matches!(
                    &member.object,
                    Expression::Identifier(object) if self.scope.is_bound(object.name.as_str())
                );
                if member_of_value && !new_expr.arguments.iter().any(Argument::is_spread) {
                    let callee = self.expression(&new_expr.callee, body)?;
                    let ty = Self::expr_ty(body, callee);
                    if self.ctx.krate.types.get(ty) == Some(&Type::Unknown) {
                        let args = new_expr
                            .arguments
                            .iter()
                            .map(|arg| self.argument(arg, body))
                            .collect::<Result<Vec<_>, _>>()?;
                        return Ok(body.push_expr(Expr {
                            kind: ExprKind::Construct { callee, args },
                            ty,
                            span: self.span(new_expr.span.start, new_expr.span.end),
                        }));
                    }
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
                let (params, result_ty) = callable_ty
                    .and_then(|ty| match self.ctx.krate.types.get(ty).cloned() {
                        Some(Type::Function(function)) => {
                            Some((function.params, function.return_ty))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        (
                            Vec::new(),
                            self.erase_with(ErasureReason::ByName, "new on dynamic callee"),
                        )
                    });
                if new_expr.arguments.iter().any(Argument::is_spread) {
                    let spread_item_ty =
                        self.erase_with(ErasureReason::DynamicOperator, "spread arguments");
                    let args = self.packed_spread_arguments(
                        spread_item_ty,
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
                let kind = if self.construct_through_value_is_dynamic(result_ty) {
                    ExprKind::Construct {
                        callee: callee_value,
                        args,
                    }
                } else {
                    let args =
                        match callable_ty.and_then(|ty| self.ctx.krate.types.get(ty).cloned()) {
                            Some(Type::Function(function)) => {
                                self.pack_value_call_arguments(&function, args, &[], span, body)
                            }
                            _ => args,
                        };
                    ExprKind::ClosureCall {
                        callee: callee_value,
                        args,
                    }
                };
                return Ok(body.push_expr(Expr {
                    kind,
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
        if let Some(written) = self.written_host_global(callee.name.as_str())
            && let Some(native) =
                self.native_host_constructor_expression(written, new_expr, body)?
        {
            return self.wrap_host_global_new(callee.name.as_str(), new_expr, native, body);
        }
        // A class the program declares is that class, whatever host
        // constructor its name spells (`class URL`, `class Date`, `class Map`):
        // the host constructors are consulted only for an undeclared name.
        if let Some(host) = host_callee
            && let Some(expr) = self.host_constructor_expression(host, new_expr, body, type_hint)?
        {
            return Ok(expr);
        }
        if let Some(expr) = self.dynamic_identifier_constructor_expression(new_expr, body)? {
            return Ok(expr);
        }
        // A class name bound in THIS module's lexical scope wins over a
        // crate-wide item of the same spelling.
        //
        // `classes.item` reads a map seeded with every class item in the crate,
        // and a class's own item is registered only AFTER its members are
        // lowered — so `new Node()` inside `Node`'s own method resolved to
        // another module's `Node`. While both classes shared one symbol that
        // was invisible; H51 gave them distinct symbols and it became a type
        // mismatch in the generated crate (`expected Option<Node_1<T>>, found
        // Node`), which is how it was found.
        //
        // The scoped type name is the module's own binding, bound before its
        // members are lowered, so preferring it is what makes a self-reference
        // resolve to the class being declared. It is only ever set when the
        // spelling is ambiguous crate-wide, so nothing else moves.
        if let Some(scoped_name) = self.classes.scoped_type_name(callee.name.as_str())
            && self
                .classes
                .item(callee.name.as_str())
                .and_then(|item| match self.item_ref(item) {
                    Item::Class(class) => Some(class.name),
                    _ => None,
                })
                != Some(scoped_name)
        {
            let (hints, _) = self.pending_constructor_hints(scoped_name, new_expr)?;
            let args = new_expr
                .arguments
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    self.argument_with_hint(arg, body, hints.get(index).copied().flatten())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let class_args = new_expr
                .type_arguments
                .as_ref()
                .map(|type_args| {
                    type_args
                        .params
                        .iter()
                        .map(|arg| self.ts_type_to_hir(arg))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            let ty = self.ctx.krate.types.intern(Type::Class {
                name: scoped_name,
                args: class_args,
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::New {
                    class: scoped_name,
                    args,
                },
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        }
        let Some(item) = self.classes.item(callee.name.as_str()) else {
            if self.classes.is_pending(callee.name.as_str()) {
                let class_name = self.intern_type_name(callee.name.as_str());
                let declared = self
                    .module_qualified_type_name(callee.name.as_str())
                    .unwrap_or(class_name);
                let (hints, class_args) = self.pending_constructor_hints(declared, new_expr)?;
                let args = new_expr
                    .arguments
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        self.argument_with_hint(arg, body, hints.get(index).copied().flatten())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: class_args,
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
            // `new DatabaseSync(path)` on a host-module class whose surface is
            // only declared is the same blocker as reading the binding as a
            // value (see `expr::references`): construction is a use.
            if let Some(blocker) = self.imports.unresolved_value_import(callee.name.as_str()) {
                return Err(SmeltError::missing_stdlib(
                    self.span(callee.span.start, callee.span.end),
                    blocker.to_owned(),
                ));
            }
            // A module binding that holds a constructor VALUE (`const A =
            // make()`, a class value a factory returned) constructs through
            // that value at run time: no class of that name exists to build.
            // A binding of a class expression (`const Foo = class { .. }`,
            // lowered later) names that class instead.
            if self.module_globals.contains_key(callee.name.as_str())
                && !self.imports.is_value(callee.name.as_str())
                && !self.classes.is_known(callee.name.as_str())
                && !self.source_contains_class(callee.name.as_str())
                && !self
                    .source_facts
                    .binds_class_expression(callee.name.as_str())
            {
                let callee_expr = self.identifier_expression(
                    callee.name.as_str(),
                    callee.span.start,
                    callee.span.end,
                    body,
                )?;
                let unknown_ty =
                    self.erase_with(ErasureReason::ByName, "new on dynamic constructor value");
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Construct {
                        callee: callee_expr,
                        args,
                    },
                    ty: unknown_ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }));
            }
            if self.imports.is_value(callee.name.as_str())
                || self.module_globals.contains_key(callee.name.as_str())
                || self.source_contains_class(callee.name.as_str())
            {
                // The class item is not registered yet (its module lowers
                // later in an import cycle), but its RUST NAME is already
                // known: resolve the spelling exactly as a type annotation
                // would, so an ambiguous imported class names the exporting
                // module's rendering (`Context_1`) rather than the bare
                // spelling another module's type owns. Interning the bare
                // spelling here made Hono's `hono-base.ts` construct the
                // router's `interface Context` whenever it lowered before
                // `context.ts`.
                let class_name = self.resolve_type_reference_symbol(callee.name.as_str());
                // Constructor parameter types are known from the prepass even
                // before the class item exists; see the registered-class path
                // below for why the arguments need them.
                let (hints, _) = self.pending_constructor_hints(class_name, new_expr)?;
                let args = new_expr
                    .arguments
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        self.argument_with_hint(arg, body, hints.get(index).copied().flatten())
                    })
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
        // Each argument is lowered against its constructor parameter's type,
        // exactly as a call argument is against its parameter's: an object
        // literal passed for `options?: ContextOptions<E>` is then built as
        // that struct instead of a `Record<string, unknown>` the constructor
        // has to take apart key by key.
        // The hints come from the prepass's constructor parameter types with
        // the class's type parameters substituted by the explicit type
        // arguments, else their defaults; a hint still naming an unsubstituted
        // type parameter is dropped rather than guessed.
        let (hints, _) = self.pending_constructor_hints(class_name, new_expr)?;
        let mut args = new_expr
            .arguments
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                self.argument_with_hint(arg, body, hints.get(index).copied().flatten())
            })
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
            // Without explicit arguments the class's parameters are inferred
            // from the constructor's arguments, as TypeScript does
            // (`new Holder(x)` with `x: T` is a `Holder<T>` of the CALLER's
            // `T`). A parameter no argument pins takes the contextual type's
            // argument (`this.node = new Node()` into a `Node<T>` field), then
            // its default, else stays the class's own (uninstantiated)
            // parameter.
            let mut inferred = HashMap::new();
            if let Some(hint) = type_hint
                && let Some(Type::Class {
                    name,
                    args: hint_args,
                }) = self.ctx.krate.types.get(hint).cloned()
                && name == class_name
                && hint_args.len() == class.type_params.len()
            {
                for (param, arg) in class.type_params.iter().zip(hint_args) {
                    inferred.insert(param.name, arg);
                }
            }
            let constructor_params = class
                .constructor
                .and_then(|constructor| match self.item_ref(constructor) {
                    Item::Function(function) => Some(
                        function
                            .params
                            .iter()
                            .map(|param| param.ty)
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            for (param_ty, arg) in constructor_params.iter().zip(&args) {
                let arg_ty = Self::expr_ty(body, *arg);
                let _ = self.infer_callee_type_bindings(
                    &class.type_params,
                    *param_ty,
                    arg_ty,
                    &mut inferred,
                );
            }
            let resolved = self.checker_call_type(new_expr.span.start, new_expr.span.end);
            match resolved.and_then(|ty| self.ctx.krate.types.get(ty).cloned()) {
                Some(Type::Class {
                    name,
                    args: resolved_args,
                }) if name == class_name && resolved_args.len() == class.type_params.len() => {
                    for (param, resolved_arg) in class.type_params.iter().zip(resolved_args) {
                        if let Some(bound) = inferred.get_mut(&param.name) {
                            *bound = self.widened_call_type(Some(resolved_arg), *bound);
                        }
                    }
                }
                _ => self.widen_call_bindings(
                    &class.type_params,
                    &constructor_params,
                    &args,
                    body,
                    &mut inferred,
                ),
            }
            // Rust binds the class parameter from a bare `T` argument, so one
            // that widened to `unknown` takes the argument erased: `new Pair(0, h)`
            // with `h` returning `unknown` is a `Pair<unknown>`.
            for (param_ty, arg) in constructor_params.iter().zip(&mut args) {
                let Some(Type::TypeParam { name }) = self.ctx.krate.types.get(*param_ty) else {
                    continue;
                };
                let Some(&bound) = inferred.get(name) else {
                    continue;
                };
                if self.ctx.krate.types.get(bound) == Some(&Type::Unknown)
                    && Self::expr_ty(body, *arg) != bound
                {
                    *arg = body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: *arg },
                        ty: bound,
                        span,
                    });
                }
            }
            class
                .type_params
                .iter()
                .map(|param| {
                    inferred
                        .get(&param.name)
                        .copied()
                        .or(param.default)
                        .unwrap_or_else(|| {
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

    /// Argument hints and class type arguments for `new C<..>(..)` while `C` is
    /// still pending (its class item does not exist yet).
    ///
    /// The hints are the constructor parameter types the prepass recorded
    /// (`collect_class_constructor_param_types`), with the class's type
    /// parameters replaced by the explicit type arguments, else their
    /// defaults. When some parameter has neither, a hint mentioning any type
    /// parameter is dropped rather than guessed.
    /// The class arguments are the explicit ones when written, so
    /// `new Hono<E, S>(..)` inside `Hono` keeps `Hono<E, S>` rather than a bare
    /// `Hono`; without them the result stays as before.
    fn pending_constructor_hints(
        &mut self,
        declared: smelt_hir::Symbol,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
    ) -> Result<(Vec<Option<smelt_hir::TypeId>>, Vec<smelt_hir::TypeId>), SmeltError> {
        let explicit = new_expr
            .type_arguments
            .as_ref()
            .map(|type_args| {
                type_args
                    .params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let type_params = self
            .ctx
            .class_type_params
            .get(&declared)
            .cloned()
            .unwrap_or_default();
        let Some(params) = self.ctx.class_constructor_params.get(&declared).cloned() else {
            return Ok((
                Vec::new(),
                if type_params.len() == explicit.len() {
                    explicit
                } else {
                    Vec::new()
                },
            ));
        };
        let substitutions: HashMap<smelt_hir::Symbol, smelt_hir::TypeId> = type_params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                explicit
                    .get(index)
                    .copied()
                    .or(param.default)
                    .map(|ty| (param.name, ty))
            })
            .collect();
        let all_replaced = type_params
            .iter()
            .all(|param| substitutions.contains_key(&param.name));
        let hints = params
            .into_iter()
            .map(|ty| {
                let ty = self.substitute_type_params(ty, &substitutions);
                (all_replaced || !self.type_mentions_type_param(ty)).then_some(ty)
            })
            .collect();
        let class_args = if !explicit.is_empty() && type_params.len() == explicit.len() {
            explicit
        } else {
            Vec::new()
        };
        Ok((hints, class_args))
    }

    /// Return whether `new <value>(..)` through a callable of this return type
    /// must perform the JavaScript `[[Construct]]` operation.
    ///
    /// A *declared* construct signature (`new (name: string) => Widget`) states
    /// the constructed type, and the value filling it is a typed constructor
    /// SLOT: it may be a class adapter closure with no function object behind
    /// it at all, so there is no `prototype` to link and no receiver to install
    /// — invoking it IS the construction, which is what
    /// `constructor_type_to_hir` models. An ERASED result (`unknown`, a type
    /// parameter) means the source told us nothing about what comes out, so the
    /// value can only be a plain JavaScript function, and constructing through
    /// one is `[[Construct]]`: allocate, link, install the receiver, and prefer
    /// an object the body returned.
    fn construct_through_value_is_dynamic(&self, return_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(return_ty),
            Some(Type::Unknown | Type::TypeParam { .. })
        )
    }

    /// Lower `new ctor(args)` where `ctor` is a local binding. A binding with a
    /// function type whose return type is concrete lowers statically (a
    /// `ClosureCall`); one whose return type is `unknown` or a type parameter,
    /// and any other bound name, takes the dynamic `Construct` path. `None` is
    /// returned only when the name is not a local binding, so the caller can
    /// fall through to the stdlib/class dispatch.
    ///
    /// A constructor-type annotation lowers to an ordinary `Type::Function` (see
    /// `constructor_type_to_hir`), so the callee is reached as a VALUE — but
    /// constructing through a function value is not the same operation as
    /// calling it. JavaScript `[[Construct]]` allocates an object linked to the
    /// callee's `prototype`, runs the callee with that object as its receiver,
    /// and keeps the allocated object unless the callee returned one of its
    /// own; none of that happens for a plain call. So a callee whose declared
    /// return type is erased (`unknown`, a type parameter) lowers to
    /// `ExprKind::Construct`, while a callee with a concrete return type is a
    /// typed constructor slot and lowers to `ExprKind::ClosureCall`. When the
    /// binding is present but is *not* a callable value, it takes the dynamic
    /// `Construct` path and returns `unknown`.
    ///
    /// The result type stays the callable's declared return type where there is
    /// one, and that type also decides WHICH operation this is: see
    /// [`Self::construct_through_value_is_dynamic`]. A callee with no function
    /// type is a genuine dynamic boundary and stays `unknown`.
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
            let unknown_ty =
                self.erase_with(ErasureReason::ByName, "new on dynamic constructor value");
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
            .take(function.rest.map_or(function.params.len(), |_| usize::MAX))
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let spread_positions = new_expr
            .arguments
            .iter()
            .map(Argument::is_spread)
            .collect::<Vec<_>>();
        let call_span = self.span(new_expr.span.start, new_expr.span.end);
        let kind = if self.construct_through_value_is_dynamic(function.return_ty) {
            ExprKind::Construct {
                callee: callee_expr,
                args,
            }
        } else {
            let args =
                self.pack_value_call_arguments(&function, args, &spread_positions, call_span, body);
            ExprKind::ClosureCall {
                callee: callee_expr,
                args,
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind,
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
            .take(function.rest.map_or(function.params.len(), |_| usize::MAX))
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let spread_positions = new_expr
            .arguments
            .iter()
            .map(Argument::is_spread)
            .collect::<Vec<_>>();
        let call_span = self.span(new_expr.span.start, new_expr.span.end);
        let args =
            self.pack_value_call_arguments(&function, args, &spread_positions, call_span, body);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callee_value,
                args,
            },
            ty: function.return_ty,
            span,
        })))
    }

    /// Whether `name` is a lowered `abstract` class.
    fn class_is_abstract(&self, name: &str) -> bool {
        self.classes.item(name).is_some_and(|item| {
            matches!(self.item_ref(item), Item::Class(class) if matches!(class.kind, smelt_hir::ClassKind::Abstract))
        })
    }

    /// The class symbol `name` resolves to in the current lexical scope: the
    /// lowered class's own (possibly scope-qualified) name, or the interned
    /// spelling for a class not lowered yet.
    fn resolved_class_symbol(&mut self, name: &str) -> smelt_hir::Symbol {
        let lowered = self
            .classes
            .item(name)
            .and_then(|item| match self.item_ref(item) {
                Item::Class(class) => Some(class.name),
                _ => None,
            });
        lowered.unwrap_or_else(|| self.intern_type_name(name))
    }

    /// A class name in value position: the runtime constructor object.
    ///
    /// `SmeltUnknown` is the honest carrier: a slot can hold any class, and a read
    /// of it (`e instanceof pair.c`) is answered at run time. A concrete class
    /// type would claim the slot holds an instance.
    pub(in crate::lowering) fn class_value_expression(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let lowered = self
            .classes
            .item(name)
            .and_then(|item| match self.item_ref(item) {
                Item::Class(class) => Some(class.clone()),
                _ => None,
            });
        let class = lowered
            .as_ref()
            .map_or_else(|| self.intern_type_name(name), |class| class.name);
        let params = lowered.as_ref().map(|lowered| {
            lowered
                .constructor
                .map_or_else(Vec::new, |item| match self.item_ref(item) {
                    Item::Function(function) => function
                        .params
                        .iter()
                        .filter(|param| self.ctx.krate.symbols.get(param.name) != Some("this"))
                        .map(|param| param.ty)
                        .collect(),
                    _ => Vec::new(),
                })
        });
        let constructor = match params {
            Some(_) if self.class_is_abstract(name) => None,
            Some(params) => {
                let instance_ty = self.ctx.krate.types.intern(Type::Class {
                    name: class,
                    args: Vec::new(),
                });
                let hint = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params,
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty: instance_ty,
                    is_async: false,
                    may_throw: false,
                }));
                let span = oxc::span::Span::new(start, end);
                self.class_constructor_value_expression(name, Some(hint), span, body)?
            }
            None => None,
        };
        let ty = self.erase_with(ErasureReason::DynamicOperator, "class value");
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClassValue { class, constructor },
            ty,
            span: self.span(start, end),
        }))
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
        let is_error_builtin = smelt_stdlib::is_error_class_name(name);
        let is_user_class = self.classes.is_known(name) || self.source_contains_class(name);
        if !is_error_builtin && !is_user_class {
            return Ok(None);
        }
        // TypeScript rejects `new` on an abstract class, through an `abstract
        // new (..)` constructor type too, so its value constructs nothing.
        if self.class_is_abstract(name) {
            return Ok(None);
        }

        let span = self.span(identifier_span.start, identifier_span.end);
        // The class the name RESOLVES to here, which for a block-local class
        // (`class None` inside a `describe` callback) is its scoped symbol,
        // not the source spelling: interning the spelling named a class that
        // was never lowered and the constructor fell back to an unmodelled
        // external object.
        let class_symbol = self.resolved_class_symbol(name);
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
            self.error_object_from_message(
                arg_exprs.first().copied(),
                name,
                span,
                &mut closure_body,
            )
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
}
