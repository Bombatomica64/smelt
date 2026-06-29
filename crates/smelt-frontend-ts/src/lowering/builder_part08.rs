impl ModuleBuilder<'_> {
    /// Lower a `new ...` expression, including stdlib containers and class construction.
    fn new_expression_with_hint(
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
            if let Some(expr) = self.dynamic_date_constructor_expression(new_expr, body)? {
                return Ok(expr);
            }
            if let Expression::StaticMemberExpression(member) = &new_expr.callee {
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
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new expressions require a direct class name",
            ));
        };
        if callee.name == "Date" {
            return self.new_date_expression(new_expr, body);
        }
        if callee.name == "RegExp" {
            return self.regexp_constructor_expression(new_expr, body);
        }
        if callee.name == "Array" {
            return self.array_constructor_expression(new_expr, body);
        }
        if callee.name == "String" {
            return self.string_constructor_expression(new_expr, body);
        }
        if callee.name == "Object" && !self.classes.contains_key("Object") {
            return self.object_constructor_expression(new_expr, body, type_hint);
        }
        if callee.name == "ArrayBuffer" && !self.classes.contains_key("ArrayBuffer") {
            return self.arraybuffer_constructor_expression(new_expr, body);
        }
        if callee.name == "Blob" && !self.classes.contains_key("Blob") {
            return self.blob_constructor_expression(new_expr, body);
        }
        if callee.name == "Number" && !self.classes.contains_key("Number") {
            return self.boxed_number_constructor_expression(new_expr, body);
        }
        if callee.name == "Proxy" && !self.classes.contains_key("Proxy") {
            return self.proxy_constructor_expression(new_expr, body);
        }
        if callee.name == "AbortController" && !self.classes.contains_key("AbortController") {
            return self.abort_controller_constructor_expression(new_expr, body);
        }
        if Self::is_numeric_typed_array_constructor(callee.name.as_str()) {
            return self.numeric_typed_array_constructor_expression(new_expr, body);
        }
        if callee.name == "URLSearchParams" {
            return self.url_search_params_constructor_expression(new_expr, body);
        }
        if matches!(callee.name.as_str(), "WeakMap" | "WeakSet") {
            return self.opaque_builtin_constructor_expression(new_expr, body, callee.name.as_str());
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
        let Some(item) = self.classes.get(callee.name.as_str()).copied() else {
            if self.pending_class_names.contains(callee.name.as_str()) {
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
            if self.value_imports.contains(callee.name.as_str())
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

    /// Lower `new URL(text)` to its full URL string for string-oriented URL APIs.
    fn url_constructor_expression(
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

    /// Lower boxed `new String(value)` as its primitive string payload.
    fn string_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new String(...) supports at most one argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        let Some(argument) = new_expr.arguments.first() else {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
            }));
        };
        let value = self.argument(argument, body)?;
        if Self::expr_ty(body, value) == ty {
            return Ok(value);
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value },
            ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Return whether a global constructor creates a numeric typed array.
    fn is_numeric_typed_array_constructor(name: &str) -> bool {
        matches!(
            name,
            "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float32Array"
                | "Float64Array"
        )
    }

    /// Lower `new URLSearchParams(init)` to an object carrying observable `size`.
    fn url_search_params_constructor_expression(
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

    /// Lower a supported opaque builtin constructor to an unknown object value.
    fn opaque_builtin_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        class_text: &str,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let _class_name = self.intern_type_name(class_text);
        let _args = new_expr
            .arguments
            .iter()
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let value = body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty: dict_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span: self.span(new_expr.span.start, new_expr.span.end),
        }))
    }

    /// Return true for built-in JavaScript Error constructors with Error identity.
    fn is_builtin_error_constructor(class_text: &str) -> bool {
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
    fn error_object_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let message = self.error_constructor_expression(new_expr, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_error".to_owned())),
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
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(marker_key, marker_value), (message_key, message)]),
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

    /// Lower `new ArrayBuffer(byteLength)` to a concrete marker-bearing record.
    ///
    /// JavaScript `ArrayBuffer` is a host binary-buffer object. es-toolkit only
    /// constructs it and inspects it via `value instanceof ArrayBuffer` (the
    /// `isArrayBuffer` predicate over an erased `unknown`). Rather than erase it
    /// to a shapeless `SmeltUnknown` (which would lose its identity), model it as
    /// a record carrying a dedicated `__smelt_arraybuffer` marker plus its
    /// `byteLength`, mirroring how `Date`/`Error` keep a distinct identity for
    /// later dynamic `instanceof` checks (see `instance_of_text`).
    fn arraybuffer_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let byte_length = match new_expr.arguments.first() {
            Some(argument) => self.argument(argument, body)?,
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: number_ty,
                span,
            }),
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_arraybuffer".to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let byte_length_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("byteLength".to_owned())),
            ty: string_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![
                (marker_key, marker_value),
                (byte_length_key, byte_length),
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

    /// Lower `new Blob(parts?, options?)` to a concrete marker-bearing record.
    ///
    /// JavaScript `Blob` is a host binary-data object. es-toolkit only
    /// constructs it and inspects it via `value instanceof Blob` (the `isBlob`
    /// predicate over an erased `unknown`, plus the `cloneDeepWith` clone path).
    /// Rather than erase it to a shapeless `SmeltUnknown` (which would lose its
    /// identity), model it as a record carrying a dedicated `__smelt_blob`
    /// marker plus its observable `type` string, mirroring the `ArrayBuffer`
    /// model so a later dynamic `instanceof Blob` resolves through the marker
    /// (see `instance_of_text`). The constructor arguments are still lowered so
    /// their effects/types are validated, but only `type` is retained.
    fn blob_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // Lower the constructor arguments for their side effects and type checks
        // even though only the MIME `type` ends up on the modeled record.
        for argument in &new_expr.arguments {
            let _ = self.argument(argument, body)?;
        }
        let blob_type = self.blob_options_type_string(new_expr.arguments.get(1), body, span);
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_blob".to_owned())),
            ty: string_ty,
            span,
        });
        let marker_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let type_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("type".to_owned())),
            ty: string_ty,
            span,
        });
        let object = body.push_expr(Expr {
            kind: ExprKind::DictLit(vec![(marker_key, marker_value), (type_key, blob_type)]),
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

    /// Resolve a `Blob` constructor `options.type` string literal when present.
    ///
    /// Only a directly-spelled `{ type: "..." }` literal is carried onto the
    /// modeled record; any other options shape falls back to the empty MIME
    /// string that a real `Blob` reports when no type is supplied.
    fn blob_options_type_string(
        &mut self,
        options_argument: Option<&Argument<'_>>,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let blob_type = options_argument.and_then(|argument| {
            let Argument::ObjectExpression(object) = argument else {
                return None;
            };
            object.properties.iter().find_map(|property| {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    return None;
                };
                let PropertyKey::StaticIdentifier(key) = &property.key else {
                    return None;
                };
                if key.name != "type" {
                    return None;
                }
                match &property.value {
                    Expression::StringLiteral(literal) => Some(literal.value.to_string()),
                    _ => None,
                }
            })
        });
        body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(blob_type.unwrap_or_default())),
            ty: string_ty,
            span,
        })
    }

    /// Lower the boxed-object form `new Number(value)` to a marker-bearing record.
    ///
    /// This is the boxed `Number` **object**, distinct from the `Number(x)`
    /// coercion call (which already lowers to a numeric value elsewhere). The
    /// boxed object has `typeof === "object"`, so es-toolkit's `isNumber`
    /// (`typeof x === "number"`) must report `false` for it: modeling it as a
    /// record erased to `SmeltUnknown::Object` makes the runtime `typeof`
    /// narrowing (`SmeltUnknown::Number(_)`) correctly miss. The wrapped value
    /// is retained alongside a dedicated `__smelt_number` marker so a later
    /// dynamic `instanceof Number` resolves through the marker, mirroring the
    /// `ArrayBuffer` model.
    fn boxed_number_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "new Number(...) supports at most one value argument",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let number_ty = self.ctx.krate.types.intern(Type::Float);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let value = match new_expr.arguments.first() {
            Some(argument) => self.argument(argument, body)?,
            None => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: number_ty,
                span,
            }),
        };
        let marker_key = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("__smelt_number".to_owned())),
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
    fn proxy_constructor_expression(
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
    fn abort_controller_constructor_expression(
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

    /// Lower a thrown expression to the string message carried by HIR throws.
    pub(super) fn throw_message_expression(
        &mut self,
        argument: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Expression::NewExpression(new_expr) = argument
            && matches!(&new_expr.callee, Expression::Identifier(callee) if Self::is_builtin_error_constructor(callee.name.as_str()))
        {
            return self.error_constructor_expression(new_expr, body);
        }
        self.expression(argument, body)
    }

    /// Lower `new Error(message)` to the message expression used by HIR throws.
    fn error_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(new_expr.span.start, new_expr.span.end),
                "Error constructor lowering supports at most one message argument",
            ));
        }
        let Some(message_arg) = new_expr.arguments.first() else {
            let ty = self.ctx.krate.types.intern(Type::String);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("Error".to_owned())),
                ty,
                span: self.span(new_expr.span.start, new_expr.span.end),
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
            "Error constructor message must be a string",
        ))
    }

    /// Lower `Error(message)`-style calls to the message value used by HIR throws.
    fn error_function_call(
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
    fn expression_with_hint(
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
            Expression::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
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
                    BinaryOperator::Exponential | BinaryOperator::BitwiseOR
                    | BinaryOperator::BitwiseXOR
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            format!("binary operator is not lowered yet: {:?}", binary.operator),
                        ));
                    }
                };
                let lhs = self.expression(&binary.left, body)?;
                let rhs = self.expression(&binary.right, body)?;
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
                if let Some(expr) = self.same_value_zero_logical(logical, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
                    return Ok(expr);
                }
                if logical.operator == LogicalOperator::Coalesce {
                    return self.nullish_coalesce_expression(logical, body, type_hint);
                }
                if let Some(expr) = self.logical_and_numeric_value_expression(logical, body)? {
                    return Ok(expr);
                }
                let cond = self.condition_expression(&logical.left, body)?;
                let rhs_narrowing = if logical.operator == LogicalOperator::And {
                    self.guard_narrowing(&logical.left, body)
                } else {
                    None
                };
                if let Some(narrowing) = rhs_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let rhs = self.expression(&logical.right, body)?;
                if rhs_narrowing.is_some() {
                    self.narrowed_locals.pop();
                }
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
                let cond = self.condition_expression(&conditional.test, body)?;
                let then_narrowing = self.guard_narrowing(&conditional.test, body);
                if let Some(narrowing) = then_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let then_expr =
                    self.expression_with_hint(&conditional.consequent, body, type_hint)?;
                if then_narrowing.is_some() {
                    self.narrowed_locals.pop();
                }
                let branch_hint = Some(Self::expr_ty(body, then_expr));
                let else_narrowing = self.inverse_guard_narrowing(&conditional.test, body);
                if let Some(narrowing) = else_narrowing.clone() {
                    self.narrowed_locals.push(narrowing);
                }
                let else_expr =
                    self.expression_with_hint(&conditional.alternate, body, branch_hint)?;
                if else_narrowing.is_some() {
                    self.narrowed_locals.pop();
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
                } else if self.type_contains_unknown(then_ty) || self.type_contains_unknown(else_ty)
                {
                    self.ctx.krate.types.intern(Type::Unknown)
                } else if let Some(hint) = type_hint
                    && !self.concrete_type_requires_never_value(hint)
                {
                    hint
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
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.span.start, await_expr.span.end),
                        "await expressions are only lowered inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.argument, body)?;
                let awaited_ty = Self::expr_ty(body, awaited);
                let Some(ty) = self.future_inner_type(awaited_ty) else {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(await_expr.span.start, await_expr.span.end),
                    }));
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }))
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
            Expression::CallExpression(call) => self.call_expression(call, body),
            Expression::AssignmentExpression(assign) => {
                let (_target, value) = self.assignment_parts(assign, body)?;
                Ok(value)
            }
            Expression::YieldExpression(yield_expr) => {
                if let Some(argument) = &yield_expr.argument {
                    self.expression(argument, body)
                } else {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(yield_expr.span.start, yield_expr.span.end),
                    }))
                }
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
                self.new_expression_with_hint(new_expr, body, type_hint)
            }
            Expression::TemplateLiteral(tpl) => self.template_literal_expression(tpl, body),
            Expression::TaggedTemplateExpression(tagged) => Err(SmeltError::unsupported(
                self.span(tagged.span.start, tagged.span.end),
                "tagged template literals are not supported",
            )),
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }

    /// Lower a TypeScript bigint literal into Smelt's current numeric runtime value.
    fn bigint_literal_expression(
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
    fn typeof_expression(
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
        let kind = self.typeof_type_name(operand_ty).unwrap_or("object");
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(kind.to_owned())),
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower a TypeScript conditional expression when it appears outside normal expression nodes.
    fn conditional_expression(
        &mut self,
        conditional: &oxc::ast::ast::ConditionalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond = self.condition_expression(&conditional.test, body)?;
        let then_expr = self.expression_with_hint(&conditional.consequent, body, type_hint)?;
        let branch_hint = Some(Self::expr_ty(body, then_expr));
        let else_expr = self.expression_with_hint(&conditional.alternate, body, branch_hint)?;
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

    /// Compute the result type for a conditional expression's branches.
    fn conditional_branch_type(
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
        } else if self.compatible_function_branch_types(then_ty, else_ty) {
            Ok(then_ty)
        } else if let Some(function_ty) = self.single_function_branch_type(then_ty, else_ty) {
            Ok(function_ty)
        } else if matches!(self.ctx.krate.types.get(then_ty), Some(Type::Union(items)) if items.contains(&else_ty)) {
            Ok(then_ty)
        } else if matches!(self.ctx.krate.types.get(else_ty), Some(Type::Union(items)) if items.contains(&then_ty)) {
            Ok(else_ty)
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

    /// Return whether a timestamp-backed Date branch can flow into a generic date type.
    fn date_runtime_float_matches_type_param(
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
    fn is_empty_list_expr(body: &Body, expr: smelt_hir::ExprId) -> bool {
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
    fn condition_expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond = self.expression(expression, body)?;
        self.lowered_condition_expression(cond, self.expression_span(expression), body)
    }

    /// Coerce an already lowered JavaScript value into its boolean truthiness result.
    ///
    /// Assignment operators such as `||=` already lower their target as a
    /// writable place. Reusing the resulting expression here avoids lowering a
    /// computed receiver solely to form the condition that selects its value.
    fn lowered_condition_expression(
        &mut self,
        cond: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond_ty = Self::expr_ty(body, cond);
        if self.ctx.krate.types.get(cond_ty) == Some(&Type::Bool) {
            return Ok(cond);
        }
        if matches!(
            self.ctx.krate.types.get(cond_ty),
            Some(Type::Function(_) | Type::Class { .. } | Type::TypeParam { .. })
        ) {
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
    fn optional_known_date_presence_condition(
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
    fn type_is_always_truthy_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
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
    fn type_is_truthy_condition_surface(&self, ty: smelt_hir::TypeId) -> bool {
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
    fn template_literal_expression(
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


    // Continued in the next split builder file.
}
