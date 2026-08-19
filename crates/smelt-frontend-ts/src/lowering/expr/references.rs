//! Identifier, constant-expression, and source-location lowering helpers.

use crate::lowering::{ConstCollectionItem, ConstCollectionValue, ModuleBuilder, ambient_globals};
use crate::{SmeltError, camel_to_snake};
use oxc::ast::ast::{Expression, Statement};
use oxc::span::GetSpan;
use oxc::syntax::operator::{BinaryOperator, LogicalOperator};
use smelt_hir::{
    AsyncOp, Body, ConstItem, Expr, ExprKind, FileId, Function, FunctionOwner, FunctionType, Item,
    Literal, LocalDecl, NumericPredicateOp, Param, PrimitiveCastOp, Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Intern a source identifier name and convert from `camelCase` to `snake_case`.
    pub(in crate::lowering) fn intern_source_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Intern a generated source name without case-folding it.
    ///
    /// Synthetic object function-table entries need exact key spelling because
    /// JavaScript object keys are case-sensitive (`M` and `m` are distinct
    /// date-fns formatters).
    pub(in crate::lowering) fn intern_exact_source_name(
        &mut self,
        name: &str,
    ) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(name);
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Intern a type name symbol.
    pub(in crate::lowering) fn intern_type_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(name);
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Create an identifier expression from a local variable.
    pub(in crate::lowering) fn identifier_expression(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if name == "Infinity" {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(f64::INFINITY)),
                ty,
                span: self.span(start, end),
            }));
        }
        if name == "NaN" {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(f64::NAN)),
                ty,
                span: self.span(start, end),
            }));
        }
        if name == "undefined" {
            let ty = self.ctx.krate.types.intern(Type::None);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Undefined),
                ty,
                span: self.span(start, end),
            }));
        }
        if name == "arguments" {
            return self.arguments_object_expression(start, end, body);
        }
        if name == "Date" {
            let symbol = self.intern_type_name("Date");
            let ty = self.ctx.krate.types.intern(Type::Class {
                name: symbol,
                args: Vec::new(),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(start, end),
            }));
        }
        if self.classes.contains(name) {
            let symbol = self.classes.item(name)
                .and_then(|item| match self.item_ref(item) {
                    Item::Class(class) => Some(class.name),
                    _ => None,
                })
                .unwrap_or_else(|| self.intern_type_name(name));
            let ty = self.ctx.krate.types.intern(Type::Class {
                name: symbol,
                args: Vec::new(),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(start, end),
            }));
        }
        // A typed-array constructor used as a bare *value* rather than through
        // `new` — e.g. an `instanceof Uint8Array` / `toBeInstanceOf(Uint8Array)`
        // target, or a constructor passed to a helper. Mirror the `Date` model:
        // resolve the name to a `Type::Class` marker value so it is a first-class
        // constructor reference instead of an unresolved identifier. A
        // user-declared class of the same name is handled by the branch above and
        // takes priority. Smelt has no first-class class objects, so the value
        // itself carries no callable payload; the `instanceof`/matcher lowering
        // reads the class *name*, not this placeholder.
        if smelt_stdlib::is_typed_array_class_name(name) {
            let symbol = self.intern_type_name(name);
            let ty = self.ctx.krate.types.intern(Type::Class {
                name: symbol,
                args: Vec::new(),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(start, end),
            }));
        }
        if matches!(name, "RegExp" | "Temporal") {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty,
                span: self.span(start, end),
            }));
        }
        // Prefer the materialized closure binding when this callback is being
        // referenced from a nested closure body. Rebuilding the compact
        // callback expression here would retain capture IDs from its defining
        // body (for example `next` closing over `itemsByIndex`) instead of the
        // nested body's remapped local for `next` itself.
        if let (Some(local), Some(callback)) = (
            self.scope.lookup(name),
            self.scope.callback(name),
        ) && body.blocks.first().map(|block| block.span) != callback.defining_body_span
            && usize::try_from(local.0)
                .ok()
                .and_then(|index| body.locals.get(index))
                .is_some_and(|decl| {
                    matches!(self.ctx.krate.types.get(decl.ty), Some(Type::Function(_)))
                })
        {
            let ty = Self::local_ty(body, local);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(start, end),
            }));
        }
        if let Some(callback) = self.scope.callback(name).cloned() {
            return self.callback_expr_to_closure_with_return_ty(
                callback.return_ty,
                &callback.callback,
                &callback.params,
                callback.rest.map(|rest| rest.index),
                callback.required_params,
                self.span(start, end),
                body,
            );
        }
        let Some(local) = self.scope.lookup(name) else {
            // A module-level `let`/`var` binding lifted to a mutable global (and
            // not shadowed by a local) reads through `GlobalGet`, never the
            // const-inline path. This also resolves imported mutated bindings,
            // which appear in `items` as the same `MutableGlobal` item.
            if let Some(item) = self.mutable_global_item(name) {
                let ty = match self.item_ref(item) {
                    Item::MutableGlobal(global_item) => global_item.ty,
                    _ => self.ctx.krate.types.intern(Type::Unknown),
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::GlobalGet { item },
                    ty,
                    span: self.span(start, end),
                }));
            }
            if let Some((pattern, flags, ty)) = self.const_regexps.get(name).cloned() {
                let span = self.span(start, end);
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(pattern)),
                    ty: string_ty,
                    span,
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(flags)),
                    ty: string_ty,
                    span,
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty,
                    span,
                }));
            }
            if let Some(value) = self.const_literals.get(name) {
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(value.literal.clone()),
                    ty: value.ty,
                    span: self.span(start, end),
                }));
            }
            if let Some(value) = self.const_objects.get(name).cloned() {
                return Ok(self.object_const_expression(&value, start, end, body));
            }
            if let Some(item) = self.items.get(name).copied()
                && let Item::Const(const_item) = self.item_ref(item).clone()
            {
                return self.const_item_expression(&const_item, start, end, body);
            }
            if let Some(item) = self.items.get(name).copied()
                && matches!(self.item_ref(item), Item::Function(_))
            {
                return self.item_function_closure_expression(item, start, end, body);
            }
            if let Some(ty) = self.module_globals.get(name).copied() {
                return self.module_global_expression(name, ty, start, end, body);
            }
            if name == "console" {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(start, end),
                }));
            }
            if name == "__dirname" {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(String::new())),
                    ty,
                    span: self.span(start, end),
                }));
            }
            if let Some(expr) = self.builtin_function_value_expression(name, start, end, body) {
                return Ok(expr);
            }
            if let Some(expr) = self.builtin_namespace_value_expression(name, start, end, body) {
                return Ok(expr);
            }
            if self.is_ambient_global_alias(name) {
                return Ok(self.global_object_value_expression(start, end, body));
            }
            if matches!(
                name,
                "Error"
                    | "EvalError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "TypeError"
                    | "URIError"
                    | "AggregateError"
            ) {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(name.to_owned())),
                    ty,
                    span: self.span(start, end),
                }));
            }
            if matches!(
                name,
                "AbortSignal" | "Object" | "Symbol" | "process" | "strapi" | "require" | "this"
            ) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return self.module_global_expression(name, ty, start, end, body);
            }
            if self.value_imports.contains(name) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return self.module_global_expression(name, ty, start, end, body);
            }
            if self.source_contains_forward_callable(name) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return self.module_global_expression(name, ty, start, end, body);
            }
            if self.allow_unknown_index_access {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(start, end),
                }));
            }
            return Err(SmeltError::for_unresolved_name(
                self.span(start, end),
                name,
                format!("unresolved identifier `{name}`"),
            ));
        };
        if usize::try_from(local.0)
            .ok()
            .is_none_or(|index| index >= body.locals.len())
        {
            // A stale local entry from another body must not hide a lifted
            // mutable global: the read still resolves through `GlobalGet`.
            if let Some(item) = self.mutable_global_item(name) {
                let ty = match self.item_ref(item) {
                    Item::MutableGlobal(global_item) => global_item.ty,
                    _ => self.ctx.krate.types.intern(Type::Unknown),
                };
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::GlobalGet { item },
                    ty,
                    span: self.span(start, end),
                }));
            }
            if let Some(value) = self.const_objects.get(name).cloned() {
                return Ok(self.object_const_expression(&value, start, end, body));
            }
            if let Some(value) = self.const_literals.get(name) {
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(value.literal.clone()),
                    ty: value.ty,
                    span: self.span(start, end),
                }));
            }
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty,
                span: self.span(start, end),
            }));
        }
        // A read of a callable local that is still accumulating property writes
        // escapes the partially-built bundle. Consumption at a callable-interface
        // coercion is intercepted before this path (and removes the entry), so
        // reaching here with a live entry means the value flowed out as a bare
        // callable; a later property write onto it is then a documented punt.
        self.scope.mark_callable_local_escaped(local);
        let base_ty = Self::local_ty(body, local);
        let ty = self.narrowed_type(name).unwrap_or(base_ty);
        let local_expr = body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty,
            span: self.span(start, end),
        });
        if ty != base_ty
            && matches!(
                self.ctx.krate.types.get(base_ty),
                Some(Type::Unknown | Type::Union(_))
            )
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: local_expr,
                    target: ty,
                },
                ty,
                span: self.span(start, end),
            }));
        }
        Ok(local_expr)
    }

    /// Lower JavaScript `arguments` to the array-like exotic object, carrying the
    /// enclosing function's actual argument *values*.
    ///
    /// The enclosing function's parameters are the arguments: `body.params` holds
    /// the positional parameter locals followed by the rest parameter's local when
    /// one is declared, and the top of the argument-arity stack is the count of
    /// positional parameters. Reading them recovers the call's argument vector, so
    /// `Object.keys(arguments)` enumerates the real index keys — which is what
    /// `isEqual(toArgs([1, 2, 3]), { 0: 1, 1: 2, 2: 3 })` compares.
    ///
    /// When the body being lowered is *not* the function that owns this
    /// `arguments` — an arrow function nested inside it inherits the binding but
    /// has its own parameter list — the parameter shape does not match the arity
    /// on the stack and there is nothing local to read. Those references keep the
    /// length-only record rather than reporting a nested closure's parameters as
    /// the outer call's arguments.
    pub(in crate::lowering) fn arguments_object_expression(
        &mut self,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(argument_count) = self.current_arguments_arities.last().copied() else {
            return Err(SmeltError::unsupported(
                self.span(start, end),
                "`arguments` is only available inside function bodies",
            ));
        };
        let span = self.span(start, end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        // `body.params` is the positional parameters, optionally followed by the
        // rest parameter, so exactly one of these two lengths belongs to the
        // function that owns this `arguments`.
        if matches!(
            body.params.len().checked_sub(argument_count),
            Some(0 | 1)
        ) {
            let has_rest = body.params.len() > argument_count;
            let param_locals = body.params.clone();
            let mut fixed = Vec::with_capacity(argument_count);
            for local in param_locals.iter().take(argument_count) {
                let ty = Self::local_ty(body, *local);
                fixed.push(body.push_expr(Expr {
                    kind: ExprKind::Local(*local),
                    ty,
                    span,
                }));
            }
            let rest = has_rest.then(|| {
                let local = param_locals[argument_count];
                let ty = Self::local_ty(body, local);
                body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty,
                    span,
                })
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ArgumentsObject { fixed, rest },
                ty: unknown_ty,
                span,
            }));
        }
        let argument_count = u32::try_from(argument_count).map_err(|_error| {
            SmeltError::unsupported(
                self.span(start, end),
                "`arguments.length` exceeds the supported numeric range",
            )
        })?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, unknown_ty));
        let key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("length".to_owned())),
            ty: key_ty,
            span,
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(f64::from(argument_count))),
            ty: self.ctx.krate.types.intern(Type::Float),
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(key, value)]),
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

    /// Lower a bare reference to a recognized global builtin *function* (used as
    /// a value, not called) into a concrete first-class closure.
    ///
    /// JavaScript utility code frequently passes the global coercion and parse
    /// functions around as values — `['6', '8'].map(unary(parseInt))`,
    /// `values.map(Number)`, `xs.filter(Boolean)`. The direct-call form of each
    /// is lowered by `primitive_cast_call` / `primitive_predicate_call`; this
    /// path gives the *value* form the same concrete behavior by synthesizing a
    /// single-argument closure that runs the builtin's existing IR op, rather
    /// than resolving the name as an unresolved identifier or erasing it to a
    /// dynamic `SmeltUnknown` tag.
    ///
    /// Returns `None` for names that are not builtin function values so the
    /// caller can continue its normal resolution chain.
    pub(in crate::lowering) fn builtin_function_value_expression(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        outer_body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let span = self.span(start, end);
        Some(match name {
            // Primitive coercion constructors used as callbacks.
            "String" => self.builtin_cast_closure_expression(
                PrimitiveCastOp::ToString,
                Type::String,
                span,
                outer_body,
            ),
            "Number" => self.builtin_cast_closure_expression(
                PrimitiveCastOp::ToJsNumber,
                Type::Float,
                span,
                outer_body,
            ),
            "Boolean" => self.builtin_cast_closure_expression(
                PrimitiveCastOp::ToBool,
                Type::Bool,
                span,
                outer_body,
            ),
            // String-to-number parse functions used as callbacks. Both expose a
            // single string parameter here; direct `parseFloat` calls separately
            // make JavaScript's implicit `ToString` coercion explicit. Callers
            // that need JavaScript's
            // `(value, index)` arity (e.g. `arr.map(parseInt)`) wrap them in
            // `unary`/`ary` first, so a one-parameter closure is faithful. The
            // parameter is typed `string` so the cast emits the real numeric
            // parse instead of the erased-operand fallback.
            "parseInt" => self.builtin_string_cast_closure_expression(
                PrimitiveCastOp::ToInt,
                span,
                outer_body,
            ),
            "parseFloat" => self.builtin_string_cast_closure_expression(
                PrimitiveCastOp::ParseFloat,
                span,
                outer_body,
            ),
            // Numeric predicates used as callbacks.
            "isNaN" => self.builtin_numeric_predicate_closure_expression(
                NumericPredicateOp::IsNaN,
                span,
                outer_body,
            ),
            "isFinite" => self.builtin_numeric_predicate_closure_expression(
                NumericPredicateOp::IsFinite,
                span,
                outer_body,
            ),
            // Collection constructors used as first-class constructor values
            // (`memoize.Cache = Map`, `new (memoize.Cache || Map)()`). The
            // synthesized closure constructs the default empty collection and
            // erases it at the dynamic boundary; dynamic `new` lowers to a
            // closure call, so the value composes with the erased-new path.
            "Map" | "Set" => {
                self.builtin_collection_constructor_closure_expression(name, span, outer_body)
            }
            // The `Proxy` constructor used as a first-class value (`if (Proxy)`,
            // `isFunction(Proxy)`, a `Proxy` entry in a value table). JavaScript
            // proxies are transparent about their target's identity, and Smelt's
            // construction lowering (`proxy_constructor_expression`) models
            // `new Proxy(target, handler)` as the target itself. The value form
            // mirrors that: an identity closure over the target, so a dynamic
            // `new` through the value (which lowers to a closure call) produces
            // the same transparent result, and `typeof Proxy === 'function'`
            // holds because the value is a real function.
            "Proxy" => self.transparent_proxy_value_closure_expression(span, outer_body),
            // `encodeURI` used as a value (`values.map(encodeURI)`, a native-
            // function table entry). The closure runs the same IR op as the
            // direct-call lowering (`uri_encode_call`), with a concrete `string`
            // parameter so the percent-encoding runs on the real string.
            "encodeURI" => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.builtin_unary_closure_expression(
                    string_ty,
                    string_ty,
                    span,
                    outer_body,
                    |value_expr| ExprKind::UriEncode {
                        operand: value_expr,
                    },
                )
            }
            // `setTimeout` used as a value rather than called directly (e.g.
            // `const original = globalThis.setTimeout;` before mocking). The
            // synthesized `(callback, delayMs) => …` closure runs the same
            // `AsyncOp::SetTimeout` the direct-call lowering emits, so the value
            // form schedules on the shared virtual-time timer queue and returns
            // the timer id.
            "setTimeout" => self.builtin_set_timeout_value_closure_expression(span, outer_body),
            _ => return None,
        })
    }

    /// Build a first-class constructor closure for a bare `Map`/`Set` value.
    ///
    /// The closure takes no parameters (dynamic dispatch tolerates extra
    /// arguments) and returns the empty collection erased to `unknown`, the
    /// same value `new Map()` / `new Set()` would produce before erasure.
    fn builtin_collection_constructor_closure_expression(
        &mut self,
        name: &str,
        span: Span,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let mut closure_body = Body::new(None, span);
        let constructed = if name == "Set" {
            let item_ty = self.ctx.krate.types.intern(Type::Unknown);
            let set_ty = self.ctx.krate.types.intern(Type::Set(item_ty));
            let empty_list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
            let list = closure_body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: empty_list_ty,
                span,
            });
            closure_body.push_expr(Expr {
                kind: ExprKind::ListToSet { list },
                ty: set_ty,
                span,
            })
        } else {
            let key_ty = self.ctx.krate.types.intern(Type::Unknown);
            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
            let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
            closure_body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty: dict_ty,
                span,
            })
        };
        let erased = closure_body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: constructed,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(erased)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: false,
        }));
        outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
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
        })
    }

    /// Build the identity closure used for the bare `Proxy` constructor value.
    ///
    /// The closure is `(target) => target` over the erased ABI: JavaScript
    /// proxies report the identity of their target (`typeof`, plain-object
    /// shape), and Smelt's `new Proxy(target, handler)` lowering already
    /// resolves the construct to the target. Dynamic `new` through a
    /// first-class constructor value lowers to a closure call, so applying
    /// this value reproduces the transparent-construction semantics. Extra
    /// arguments (the handler) are tolerated by dynamic dispatch and ignored,
    /// matching how the direct construction discards the handler.
    fn transparent_proxy_value_closure_expression(
        &mut self,
        span: Span,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        self.builtin_unary_closure_expression(
            unknown_ty,
            unknown_ty,
            span,
            outer_body,
            |value_expr| ExprKind::UnknownCast {
                value: value_expr,
                target: unknown_ty,
            },
        )
    }

    /// Build the `(callback, delayMs) => setTimeout(callback, delayMs)` closure
    /// used when `setTimeout` escapes as a first-class value.
    ///
    /// The closure body runs the same `AsyncOp::SetTimeout` the direct-call
    /// timer lowering produces, so a call through the value registers on the
    /// shared virtual-time timer queue and returns the erased timer id. The
    /// callback parameter is `unknown` (the timer emitter dispatches erased
    /// callables), and the delay parameter is a concrete number.
    fn builtin_set_timeout_value_closure_expression(
        &mut self,
        span: Span,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let callback_name = self.intern_source_name("callback");
        let delay_name = self.intern_source_name("delayMs");
        let mut closure_body = Body::new(None, span);
        let callback_local = closure_body.push_local(LocalDecl {
            name: Some(callback_name),
            ty: unknown_ty,
            mutable: false,
            span,
        });
        let delay_local = closure_body.push_local(LocalDecl {
            name: Some(delay_name),
            ty: float_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(callback_local);
        closure_body.params.push(delay_local);
        let callback_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(callback_local),
            ty: unknown_ty,
            span,
        });
        let delay_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(delay_local),
            ty: float_ty,
            span,
        });
        let result = closure_body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::SetTimeout,
                args: vec![callback_expr, delay_expr],
            },
            ty: unknown_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![unknown_ty, float_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: false,
        }));
        outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![
                    Param {
                        name: callback_name,
                        local: callback_local,
                        ty: unknown_ty,
                        span,
                    },
                    Param {
                        name: delay_name,
                        local: delay_local,
                        ty: float_ty,
                        span,
                    },
                ],
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
        })
    }

    /// Lower a bare reference to a recognized global *namespace object*
    /// (`Math`, `JSON`, `Reflect`, `Atomics`, `Intl`, `Promise`, `Array`,
    /// `Function`) used as a value into a concrete marker-bearing host-object
    /// record.
    ///
    /// These intrinsics are normally consumed through recognized member calls
    /// (`Math.max(...)`, `JSON.parse(...)`, `Promise.resolve(...)`); this path
    /// only fires when the bare name is used as a first-class *value*, e.g.
    /// `isPlainObject(JSON)` or `isPlainObject(Math)`. Modeling them as a record
    /// carrying a dedicated `__smelt_builtin_namespace` marker (plus the source
    /// `name`) keeps them honest host objects rather than leaving an unresolved
    /// identifier or erasing them to a shapeless `SmeltUnknown::Object` that a
    /// plain-object check would mistake for `{}`. The marker is a genuine dynamic
    /// boundary value: predicates that inspect it at runtime (`typeof === object`
    /// is true, but it is not a plain object) observe the host identity.
    ///
    /// Returns `None` for names that are not builtin namespace objects so the
    /// caller continues its normal resolution chain.
    pub(in crate::lowering) fn builtin_namespace_value_expression(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if !matches!(
            name,
            "Math" | "JSON" | "Reflect" | "Atomics" | "Intl" | "Promise" | "Array" | "Function"
        ) && !Self::is_known_defined_global_constructor(name)
            && name != "Blob"
            && name != "ArrayBuffer"
            && name != "DOMException"
        {
            return None;
        }
        let span = self.span(start, end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        // Interned, not a fresh record per reference: JavaScript exposes one object
        // per global name, so `Blob === Blob` and `blob.constructor === Blob` both
        // hold. A record literal mints an identity on construction, which made both
        // comparisons `false`.
        Some(body.push_expr(Expr {
            kind: ExprKind::BuiltinNamespace {
                name: name.to_owned(),
            },
            ty: unknown_ty,
            span,
        }))
    }

    /// Lower a bare reference to the ambient global object (`globalThis` /
    /// `global` / `self`, when not shadowed) *used as a value* into a concrete
    /// marker-bearing host-object record.
    ///
    /// Most ambient-global usage is feature-detection that Phase-1 erasure folds
    /// before this point (`typeof globalThis`, `"Map" in globalThis`,
    /// `globalThis.Object.keys(x)` namespace normalization). This path handles
    /// the residual escaping-identity case where the global object itself flows
    /// as a value — e.g. es-toolkit's `_internal/globalThis.ts` shim
    /// `(typeof globalThis === 'object' && globalThis) || ...`, whose result is
    /// re-exported and later read as `globalThis.Buffer` (an absent member that
    /// resolves to `undefined`). Modeling it as a record carrying a dedicated
    /// `__smelt_global_object` marker keeps a concrete host-object value instead
    /// of leaving an unresolved identifier or erasing it to a shapeless object.
    ///
    /// This is the pragmatic first step toward the plan's full Phase-2/3 runtime
    /// `SmeltGlobalObject` (shared identity + dynamic property store). es-toolkit's
    /// in-scope residual usage never compares global-object identity, writes a
    /// dynamic property onto the global, or reads a user-defined dynamic slot, so
    /// a per-read marker record is sufficient and faithful here; the shared-handle
    /// runtime object remains the correct model once identity or dynamic mutation
    /// becomes observable.
    /// Fold the global-object detection chain
    /// `(typeof X === 'object' && X) || (typeof Y === 'object' && Y) || ...`
    /// to the global-object value, short-circuiting per JavaScript semantics.
    ///
    /// This is the canonical UMD/`globalThis` shim idiom (es-toolkit's
    /// `_internal/globalThis.ts`): a left-nested `||` chain whose clauses each
    /// guard a global alias with its own `typeof` existence probe. JavaScript
    /// evaluates the clauses left to right and never evaluates the right operand
    /// of a `&&` whose guard is falsy, so a clause for an *absent* alias (e.g.
    /// `window` in the non-DOM profile) must be skipped *without lowering* its
    /// dead `window` reference (which would otherwise be an unresolved
    /// identifier). The first clause for a *present* alias (`globalThis`,
    /// `global`, `self`) is truthy and yields the global object, collapsing the
    /// whole chain to the single global-object value.
    ///
    /// Returns `None` for anything that is not this exact guarded-alias chain
    /// shape, so ordinary `||` lowering keeps handling every other case (and any
    /// honest blocker it would otherwise raise).
    pub(in crate::lowering) fn global_detection_chain_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if logical.operator != LogicalOperator::Or {
            return None;
        }
        // Only fold when the chain actually resolves to a *present* global object;
        // a chain that resolves to none of the modeled aliases is not this idiom
        // and must fall through to ordinary `||` lowering. The outer `||` splits
        // into its left spine and right clause exactly like the recursive walker.
        let resolves = self.or_chain_resolves_to_global(&logical.left)
            || self.clause_is_present_global_guard(&logical.right);
        if !resolves {
            return None;
        }
        Some(self.global_object_value_expression(logical.span.start, logical.span.end, body))
    }

    /// Return whether an `||` chain of `(typeof X === 'object' && X)` clauses
    /// resolves to a present global object under the active profile.
    ///
    /// Walks the left-nested `||` spine. Each clause must be a
    /// `typeof X === 'object' && X` guard whose two `X` spellings agree and name
    /// a global alias. A clause for a *present* alias resolves the whole chain
    /// (`Some(true)`); a clause for an *absent* alias is skipped; a final
    /// non-guard fallback (e.g. the `((function(){return this})())` or a literal)
    /// is allowed only after at least one present alias was found. Any other shape
    /// returns `false`, leaving the expression to ordinary lowering.
    pub(in crate::lowering) fn or_chain_resolves_to_global(
        &self,
        expression: &Expression<'_>,
    ) -> bool {
        match Self::unparenthesized_expression(expression) {
            Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::Or => {
                // `(left) || (right)`: a present global anywhere in the spine wins.
                self.or_chain_resolves_to_global(&logical.left)
                    || self.clause_is_present_global_guard(&logical.right)
            }
            other => self.clause_is_present_global_guard(other),
        }
    }

    /// Return whether a single `typeof X === 'object' && X` clause guards a
    /// *present* global alias (so the clause is truthy and yields the global).
    ///
    /// An absent-alias clause (e.g. `typeof window === 'object' && window`) is
    /// statically falsy, so it returns `false` here — the chain walker skips it
    /// rather than treating it as the resolving clause, and crucially the dead
    /// `window` operand is never lowered.
    pub(in crate::lowering) fn clause_is_present_global_guard(
        &self,
        expression: &Expression<'_>,
    ) -> bool {
        let Expression::LogicalExpression(clause) = Self::unparenthesized_expression(expression)
        else {
            return false;
        };
        if clause.operator != LogicalOperator::And {
            return false;
        }
        // Right operand must be a bare alias identifier.
        let Expression::Identifier(value) = Self::unparenthesized_expression(&clause.right) else {
            return false;
        };
        // Left operand must be `typeof <same-alias> === 'object'`.
        let Expression::BinaryExpression(guard) = Self::unparenthesized_expression(&clause.left)
        else {
            return false;
        };
        if !matches!(
            guard.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return false;
        }
        let guard_name = ambient_globals::typeof_identifier_name(&guard.left)
            .or_else(|| ambient_globals::typeof_identifier_name(&guard.right));
        let Some(guard_name) = guard_name else {
            return false;
        };
        let compared_object = matches!(&guard.left, Expression::StringLiteral(s) if s.value == "object")
            || matches!(&guard.right, Expression::StringLiteral(s) if s.value == "object");
        if !compared_object || guard_name != value.name.as_str() {
            return false;
        }
        // The clause's two spellings agree and name a global alias; it is a
        // present global only when the alias is present in this profile and is
        // not shadowed by a module-local binding.
        self.is_ambient_global_alias(guard_name)
            && ambient_globals::global_alias_object_presence(guard_name) == Some(true)
    }

    /// Build the concrete marker-record expression used for bare global-object
    /// values such as `globalThis`, `global`, and `self`.
    pub(in crate::lowering) fn global_object_value_expression(
        &mut self,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let span = self.span(start, end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Dict(string_ty, unknown_ty));
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_global_object".to_owned())),
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
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// Build a `(value) => <PrimitiveCast op>(value)` closure value.
    ///
    /// The single parameter is typed `unknown` so any argument the closure is
    /// later applied to is accepted; the result type matches the cast's output.
    pub(in crate::lowering) fn builtin_cast_closure_expression(
        &mut self,
        op: PrimitiveCastOp,
        return_type: Type,
        span: Span,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let result_ty = self.ctx.krate.types.intern(return_type);
        self.builtin_unary_closure_expression(value_ty, result_ty, span, outer_body, |value_expr| {
            ExprKind::PrimitiveCast {
                op,
                operand: value_expr,
            }
        })
    }

    /// Build a `(value: string) => <PrimitiveCast op>(value)` closure value.
    ///
    /// Used for the string-to-number parse builtins (`parseInt`, `parseFloat`),
    /// whose single argument is a string. A concrete `string` parameter routes
    /// the cast through its real numeric-parse emission rather than the erased
    /// `unknown` fallback (which would yield a constant `0`).
    pub(in crate::lowering) fn builtin_string_cast_closure_expression(
        &mut self,
        op: PrimitiveCastOp,
        span: Span,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let result_ty = self.ctx.krate.types.intern(Type::Float);
        self.builtin_unary_closure_expression(
            string_ty,
            result_ty,
            span,
            outer_body,
            |value_expr| ExprKind::PrimitiveCast {
                op,
                operand: value_expr,
            },
        )
    }

    /// Build a `(value) => <NumericPredicate op>(value)` closure value.
    pub(in crate::lowering) fn builtin_numeric_predicate_closure_expression(
        &mut self,
        op: NumericPredicateOp,
        span: Span,
        outer_body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = self.ctx.krate.types.intern(Type::Float);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        self.builtin_unary_closure_expression(value_ty, bool_ty, span, outer_body, |value_expr| {
            ExprKind::NumericPredicate {
                op,
                operand: value_expr,
            }
        })
    }

    /// Synthesize a single-argument closure value whose body returns `make_body`
    /// applied to the parameter read.
    ///
    /// Shared by the builtin coercion/parse/predicate value lowerings so each
    /// only specifies its parameter type, result type, and the IR op to run.
    pub(in crate::lowering) fn builtin_unary_closure_expression(
        &mut self,
        param_ty: smelt_hir::TypeId,
        return_ty: smelt_hir::TypeId,
        span: Span,
        outer_body: &mut Body,
        make_body: impl FnOnce(smelt_hir::ExprId) -> ExprKind,
    ) -> smelt_hir::ExprId {
        let value_name = self.intern_source_name("value");
        let mut closure_body = Body::new(None, span);
        let value_local = closure_body.push_local(LocalDecl {
            name: Some(value_name),
            ty: param_ty,
            mutable: false,
            span,
        });
        closure_body.params.push(value_local);
        let value_expr = closure_body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty: param_ty,
            span,
        });
        let result = closure_body.push_expr(Expr {
            kind: make_body(value_expr),
            ty: return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(result)));
        let body = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![param_ty],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: vec![Param {
                    name: value_name,
                    local: value_local,
                    ty: param_ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty,
                captures: Vec::new(),
                body,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        })
    }

    /// Read the declared return type of a synthesized closure value expression.
    ///
    /// Callback positions need the callback's return type to type the array
    /// method result; the builtin closures built above carry it in their
    /// interned `Type::Function`. Falls back to `unknown` for non-function
    /// values, which never occurs for the builtin closure helpers.
    pub(in crate::lowering) fn closure_value_return_ty(
        &mut self,
        expr: smelt_hir::ExprId,
        body: &Body,
    ) -> smelt_hir::TypeId {
        let ty = Self::expr_ty(body, expr);
        match self.ctx.krate.types.get(ty) {
            Some(Type::Function(function)) => function.return_ty,
            _ => self.ctx.krate.types.intern(Type::Unknown),
        }
    }

    /// Wrap a function item in a first-class closure value for argument positions.
    pub(in crate::lowering) fn item_function_closure_expression(
        &mut self,
        item: smelt_hir::ItemId,
        start: u32,
        end: u32,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.item_function_closure_expression_with_max_params(
            item,
            usize::MAX,
            start,
            end,
            outer_body,
        )
    }

    /// Wrap a function item in a closure value capped at a caller-supplied arity.
    ///
    /// JavaScript callback call sites adapt arity at runtime: a caller that
    /// supplies `max_params` arguments simply leaves any further declared
    /// parameters `undefined`. When the item declares more parameters than the
    /// call site supplies (`xs.map(orderBy)` supplies 3, `orderBy` declares 4),
    /// the wrapper closure therefore only declares the supplied arity and calls
    /// the item with those arguments; the item call pads the remaining
    /// (optional, per the source type checker) parameters with their defaults
    /// downstream, matching the JavaScript `undefined` tail. Items declaring
    /// `max_params` or fewer parameters (including zero) are wrapped at their
    /// own arity, since callback callers only pass what the closure declares.
    pub(in crate::lowering) fn item_function_closure_expression_with_max_params(
        &mut self,
        item: smelt_hir::ItemId,
        max_params: usize,
        start: u32,
        end: u32,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Item::Function(function) = self.item_ref(item).clone() else {
            return Err(SmeltError::unsupported(
                self.span(start, end),
                "only function items can be wrapped as closure values",
            ));
        };
        let span = self.span(start, end);
        let wrapped_params_len = function.params.len().min(max_params);
        let truncated = wrapped_params_len < function.params.len();
        let mut closure_body = Body::new(None, span);
        let mut closure_params = Vec::new();
        let mut call_args = Vec::new();
        for param in function.params.iter().take(wrapped_params_len) {
            let local = closure_body.push_local(LocalDecl {
                name: Some(param.name),
                ty: param.ty,
                mutable: false,
                span: param.span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: param.name,
                local,
                ty: param.ty,
                span: param.span,
            });
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty: param.ty,
                span: param.span,
            }));
        }
        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Item(item),
            ty: self.item_expr_type(item, span)?,
            span,
        });
        let call = closure_body.push_expr(Expr {
            kind: ExprKind::Call {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(call)));
        let body_id = self.ctx.krate.push_body(closure_body);
        // A truncated wrapper drops the item's trailing parameters (and any rest
        // parameter that falls beyond the cap), so its declared surface is the
        // capped prefix with every remaining parameter required.
        let rest = function.rest.filter(|index| *index < wrapped_params_len);
        let required_params = if truncated {
            function
                .required_params
                .map(|required| required.min(wrapped_params_len))
        } else {
            function.required_params
        };
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: function
                .params
                .iter()
                .take(wrapped_params_len)
                .map(|param| param.ty)
                .collect(),
            rest,
            required_params,
            mutable_params: Vec::new(),
            return_ty: function.return_ty,
            is_async: function.is_async,
            may_throw: false,
        }));
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest,
                required_params,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body: body_id,
                // Tag this wrapper with its source function item so codegen caches
                // one runtime wrapper per item, giving every `func1`-as-value
                // reference the same JavaScript reference identity under `===`.
                // Truncated wrappers stay untagged: the per-item wrapper cache is
                // keyed by item alone, and a capped-arity wrapper must not alias
                // the full-arity wrapper emitted for ordinary value positions.
                function_item: if truncated { None } else { Some(item) },
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Synthesize a read value for a known module-level variable.
    pub(in crate::lowering) fn module_global_expression(
        &mut self,
        name: &str,
        ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(collection) = self.const_collections.get(name).cloned() {
            let span = self.span(start, end);
            let items = collection
                .items
                .iter()
                .map(|item| self.const_collection_item_expression(item, span, body))
                .collect::<Vec<_>>();
            let kind = if collection.is_set {
                ExprKind::SetLit(items)
            } else {
                ExprKind::ListLit(items)
            };
            return Ok(body.push_expr(Expr {
                kind,
                ty: collection.ty,
                span,
            }));
        }
        if let Some(Type::Function(function)) = self.ctx.krate.types.get(ty).cloned() {
            return self.module_global_function_expression(&function, ty, start, end, body);
        }
        if matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Class { .. } | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let value_ty = self.ctx.krate.types.intern(Type::Unknown);
            let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
            let value = body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty: dict_ty,
                span: self.span(start, end),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast { value, target: ty },
                ty,
                span: self.span(start, end),
            }));
        }
        let kind = match self.ctx.krate.types.get(ty) {
            Some(Type::Dict(_, _) | Type::JsMap(_, _)) => ExprKind::DictLit(Vec::new()),
            Some(Type::None | Type::Optional(_)) => ExprKind::Literal(Literal::None),
            Some(Type::Bool) => ExprKind::Literal(Literal::Bool(false)),
            Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
            Some(Type::Float) => ExprKind::Literal(Literal::Float(0.0)),
            Some(Type::String) => ExprKind::Literal(Literal::String(String::new())),
            Some(Type::List(_) | Type::Tuple(_)) => ExprKind::ListLit(Vec::new()),
            Some(Type::Set(_)) => ExprKind::SetLit(Vec::new()),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(start, end),
                    "module-level variable type is not lowered for function reads yet",
                ));
            }
        };
        Ok(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(start, end),
        }))
    }

    /// Recreate one module-level constant collection element in the active body.
    pub(in crate::lowering) fn const_collection_item_expression(
        &mut self,
        item: &ConstCollectionItem,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        match &item.value {
            ConstCollectionValue::Expr(kind) => body.push_expr(Expr {
                kind: kind.clone(),
                ty: item.ty,
                span,
            }),
            ConstCollectionValue::UnknownNull => {
                let none = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: self.ctx.krate.types.intern(Type::None),
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: none,
                        target: item.ty,
                    },
                    ty: item.ty,
                    span,
                })
            }
            ConstCollectionValue::UnknownArray => {
                let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
                let array = body.push_expr(Expr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty: list_ty,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: array,
                        target: item.ty,
                    },
                    ty: item.ty,
                    span,
                })
            }
            ConstCollectionValue::UnknownObject => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                let object = body.push_expr(Expr {
                    kind: ExprKind::DictLit(Vec::new()),
                    ty: dict_ty,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: object,
                        target: item.ty,
                    },
                    ty: item.ty,
                    span,
                })
            }
            ConstCollectionValue::UnknownFunction => {
                let param_ty = self.ctx.krate.types.intern(Type::List(item.ty));
                let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params: vec![param_ty],
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty: item.ty,
                    is_async: false,
                    may_throw: false,
                }));
                let function = FunctionType {
                    params: vec![param_ty],
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty: item.ty,
                    is_async: false,
                    may_throw: false,
                };
                match self.module_global_function_expression(
                    &function,
                    function_ty,
                    span.start,
                    span.end,
                    body,
                ) {
                    Ok(function_value) => body.push_expr(Expr {
                        kind: ExprKind::UnknownCast {
                            value: function_value,
                            target: item.ty,
                        },
                        ty: item.ty,
                        span,
                    }),
                    Err(_) => body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty: self.ctx.krate.types.intern(Type::None),
                        span,
                    }),
                }
            }
        }
    }

    /// Synthesize a callable value for a known module-level function constant.
    ///
    /// Date-fns stores helper callbacks in non-exported top-level `const`
    /// bindings and then reads those bindings while building exported locale
    /// objects. The module body owns the real closure, while exported constant
    /// lowering only needs a callable value with the same public type.
    pub(in crate::lowering) fn module_global_function_expression(
        &mut self,
        function: &FunctionType,
        ty: smelt_hir::TypeId,
        start: u32,
        end: u32,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(start, end);
        let mut body = Body::new(None, span);
        let mut params = Vec::new();
        for (index, param_ty) in function.params.iter().copied().enumerate() {
            let name = self.intern_source_name(&format!("arg_{index}"));
            let local = body.push_local(LocalDecl {
                name: Some(name),
                ty: param_ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            params.push(Param {
                name,
                local,
                ty: param_ty,
                span,
            });
        }
        let value = self.default_module_global_value(function.return_ty, span, &mut body)?;
        body.push_stmt(Stmt::Return(Some(value)));
        let body_id = self.ctx.krate.push_body(body);
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: None,
                required_params: None,
                return_ty: function.return_ty,
                captures: Vec::new(),
                body: body_id,
                function_item: None,
                span,
            }),
            ty,
            span,
        }))
    }

    /// Build a conservative default expression for synthesized module globals.
    pub(in crate::lowering) fn default_module_global_value(
        &mut self,
        ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let kind = match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::None) => ExprKind::Literal(Literal::None),
            Some(Type::Bool) => ExprKind::Literal(Literal::Bool(false)),
            Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
            Some(Type::Float) => ExprKind::Literal(Literal::Float(0.0)),
            Some(Type::String) => ExprKind::Literal(Literal::String(String::new())),
            Some(Type::List(_)) => ExprKind::ListLit(Vec::new()),
            Some(Type::Dict(_, _) | Type::JsMap(_, _)) => ExprKind::DictLit(Vec::new()),
            Some(Type::Optional(_)) => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty: none_ty,
                    span,
                }));
            }
            Some(Type::Future(_)) => {
                let duration = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(0.0)),
                    ty: self.ctx.krate.types.intern(Type::Float),
                    span,
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::Sleep,
                        args: vec![duration],
                    },
                    ty,
                    span,
                }));
            }
            Some(Type::Unknown | Type::Class { .. } | Type::TypeParam { .. }) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                let value = body.push_expr(Expr {
                    kind: ExprKind::DictLit(Vec::new()),
                    ty: dict_ty,
                    span,
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::UnknownCast { value, target: ty },
                    ty,
                    span,
                }));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "module-level function return type needs a supported default value",
                ));
            }
        };
        Ok(body.push_expr(Expr { kind, ty, span }))
    }

    /// Inline an importable HIR const item into the current expression body.
    pub(in crate::lowering) fn const_item_expression(
        &self,
        const_item: &ConstItem,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let source_body = self
            .ctx
            .krate
            .bodies
            .get(usize::try_from(const_item.body.0).unwrap_or(usize::MAX))
            .cloned()
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(start, end),
                    "const item body is not available for inlining",
                )
            })?;
        Self::clone_const_body_expr(&source_body, const_item.value, body)
    }

    /// Clone one expression from a const body, remapping nested expression IDs.
    ///
    /// Cloning is fully structural: every expression kind is rebuilt through
    /// [`ExprKind::try_map_child_exprs`], which remaps direct child expression
    /// IDs into the target body while preserving crate-global references
    /// (items, types, symbols, closure bodies). The only shapes rejected here
    /// are the ones that genuinely point into the source body's local arenas —
    /// `Local` variables and statement `Block`s — because those cannot move to
    /// another body by expression cloning alone.
    pub(in crate::lowering) fn clone_const_body_expr(
        source_body: &Body,
        expr_id: smelt_hir::ExprId,
        target_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let expr = source_body
            .exprs
            .get(usize::try_from(expr_id.0).unwrap_or(usize::MAX))
            .cloned()
            .ok_or_else(|| {
                SmeltError::unsupported(
                    source_body
                        .blocks
                        .get(usize::try_from(source_body.root.0).unwrap_or(usize::MAX))
                        .map_or(Span::new(FileId(0), 0, 0), |block| block.span),
                    "const expression is missing",
                )
            })?;
        if matches!(expr.kind, ExprKind::Local(_) | ExprKind::Block(_)) {
            return Err(SmeltError::unsupported(
                expr.span,
                "const item expression references body-local state that cannot be inlined",
            ));
        }
        let kind = expr.kind.try_map_child_exprs(&mut |child| {
            Self::clone_const_body_expr(source_body, child, target_body)
        })?;
        Ok(target_body.push_expr(Expr {
            kind,
            ty: expr.ty,
            span: expr.span,
        }))
    }

    /// Ensure a console.log item exists in the HIR.
    pub(in crate::lowering) fn ensure_console_log_item(&mut self, span: Span) -> smelt_hir::ItemId {
        let name = self.ctx.krate.symbols.intern(smelt_hir::CONSOLE_LOG_SYMBOL);
        let none = self.ctx.krate.types.intern(Type::None);
        self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            // Synthetic reference stub carries no free-function type params.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
            return_ty: none,
            is_async: false,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        }))
    }

    /// Create a Span from byte offsets.
    pub(in crate::lowering) fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file_id, start, end)
    }

    /// Get the span of a statement.
    pub(in crate::lowering) fn statement_span(&self, statement: &Statement<'_>) -> Span {
        let span = statement.span();
        self.span(span.start, span.end)
    }

    /// Get the span of an expression.
    pub(in crate::lowering) fn expression_span(&self, expression: &Expression<'_>) -> Span {
        let span = expression.span();
        self.span(span.start, span.end)
    }

    // Continued in the next split builder file.
}
