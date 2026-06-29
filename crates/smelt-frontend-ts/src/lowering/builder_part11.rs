impl ModuleBuilder<'_> {
    /// Lower `a === b || Object.is(a, b)` as JavaScript `SameValueZero` equality.
    fn same_value_zero_logical(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::Or {
            return Ok(None);
        }
        let Expression::BinaryExpression(left_binary) = &logical.left else {
            return Ok(None);
        };
        if left_binary.operator != BinaryOperator::StrictEquality {
            return Ok(None);
        }
        let Expression::CallExpression(right_call) = &logical.right else {
            return Ok(None);
        };
        let Some((object_left, object_right)) = Self::object_is_identifier_pair(right_call) else {
            return Ok(None);
        };
        if !Self::expression_is_identifier(&left_binary.left, object_left)
            || !Self::expression_is_identifier(&left_binary.right, object_right)
        {
            return Ok(None);
        }
        let lhs = self.expression(&left_binary.left, body)?;
        let rhs = self.expression(&left_binary.right, body)?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::StrictEq,
                lhs,
                rhs,
            },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Return identifier names from an `Object.is(left, right)` call.
    fn object_is_identifier_pair<'a>(
        call: &'a oxc::ast::ast::CallExpression<'a>,
    ) -> Option<(&'a str, &'a str)> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Object" || member.property.name != "is" {
            return None;
        }
        let [left_arg, right_arg] = call.arguments.as_slice() else {
            return None;
        };
        let left = Self::argument_identifier(left_arg)?;
        let right = Self::argument_identifier(right_arg)?;
        Some((left, right))
    }

    /// Return the identifier name carried by a normal call argument.
    fn argument_identifier<'a>(argument: &'a Argument<'a>) -> Option<&'a str> {
        match argument {
            Argument::Identifier(ident) => Some(ident.name.as_str()),
            _ => None,
        }
    }

    /// Return whether an expression is the requested identifier.
    fn expression_is_identifier(expression: &Expression<'_>, name: &str) -> bool {
        matches!(expression, Expression::Identifier(ident) if ident.name == name)
    }

    /// Lower `Object.is(a, b)` as a strict equality expression.
    fn object_is_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" || member.property.name != "is" {
            return Ok(None);
        }
        let [left_arg, right_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.is requires exactly two arguments",
            ));
        };
        let lhs = self.argument(left_arg, body)?;
        let rhs = self.argument_with_hint(right_arg, body, Some(Self::expr_ty(body, lhs)))?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::StrictEq,
                lhs,
                rhs,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a supported `Math.pow` call into a HIR numeric runtime call.
    fn math_pow_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "pow" {
            return Ok(None);
        }
        if call.arguments.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires exactly two arguments",
            ));
        }
        let [base_argument, exponent_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires exactly two arguments",
            ));
        };
        let base = self.argument(base_argument, body)?;
        let exponent = self.argument(exponent_argument, body)?;
        if [base, exponent]
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericPow { base, exponent },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.atan2` calls.
    fn math_atan2_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "atan2" {
            return Ok(None);
        }
        if call.arguments.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.atan2 requires exactly two arguments",
            ));
        }
        let [y_argument, x_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.atan2 requires exactly two arguments",
            ));
        };
        let y_coord = self.argument(y_argument, body)?;
        let x_coord = self.argument(x_argument, body)?;
        if [y_coord, x_coord]
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.atan2 requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericAtan2 {
                y: y_coord,
                x: x_coord,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Object.keys`, `Object.values`, and `Object.entries` calls.
    ///
    /// `Reflect.ownKeys(record)` is also routed here as `Object.keys`: for the
    /// plain-record receivers es-toolkit inspects (the `isJSONValue` key walk and
    /// the `pick` key projection) the two return the same string-key list, since
    /// Smelt records carry no non-enumerable or symbol keys. Modeling it through
    /// the existing `DictProjection` keeps a concrete `List<string>` instead of
    /// leaving `Reflect` an unresolved identifier or erasing the result.
    fn object_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        let op = match (object.name.as_str(), member.property.name.as_str()) {
            ("Object", "keys") => DictProjectionOp::Keys,
            ("Object", "values") => DictProjectionOp::Values,
            ("Object", "entries") => DictProjectionOp::Entries,
            ("Reflect", "ownKeys") => DictProjectionOp::Keys,
            _ => return Ok(None),
        };
        let [dict_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "{}.{} requires exactly one record argument",
                    object.name, member.property.name
                ),
            ));
        };
        let mut dict = self.argument(dict_argument, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let (key_type, value_type) = match self.ctx.krate.types.get(dict_ty) {
            Some(Type::Dict(key_type, value_type)) => (*key_type, *value_type),
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::String
                | Type::Bool,
            ) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast { value: dict, target },
                    ty: target,
                    span: self.span(call.span.start, call.span.end),
                });
                (key_ty, value_ty)
            }
            Some(Type::Union(items)) if items.iter().all(|item| self.object_keys_compatible_type(*item)) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast { value: dict, target },
                    ty: target,
                    span: self.span(call.span.start, call.span.end),
                });
                (key_ty, value_ty)
            }
            _ => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(call.span.start, call.span.end),
                });
                (key_ty, value_ty)
            }
        };
        let key_ty = key_type;
        let value_ty = value_type;
        let symbol_key_ty = self.ctx.krate.types.intern(Type::String);
        let symbol_list_ty = self.ctx.krate.types.intern(Type::List(symbol_key_ty));
        let ty = match op {
            DictProjectionOp::FromEntries => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                self.ctx.krate.types.intern(Type::List(key_ty))
            }
            DictProjectionOp::Symbols => symbol_list_ty,
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a type can use JavaScript object projection through `Object.*`.
    fn object_keys_compatible_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(
                Type::Dict(_, _)
                | Type::Unknown
                | Type::TypeParam { .. }
                | Type::Class { .. }
                | Type::String
                | Type::Bool,
            ) => true,
            Some(Type::Union(items)) => items
                .iter()
                .all(|item| self.object_keys_compatible_type(*item)),
            _ => false,
        }
    }

    /// Lower `Object.getOwnPropertySymbols(value)` to an opaque symbol-key list.
    fn object_get_own_property_symbols_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" || member.property.name != "getOwnPropertySymbols" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.getOwnPropertySymbols requires exactly one object argument",
            ));
        };
        let value = self.argument(argument, body)?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        let dict = body.push_expr(Expr {
            kind: ExprKind::UnknownCast { value, target },
            ty: target,
            span: self.span(call.span.start, call.span.end),
        });
        let symbol_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(symbol_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection {
                op: DictProjectionOp::Symbols,
                dict,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Object.getPrototypeOf(value)` to opaque prototype metadata.
    fn object_get_prototype_of_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" || member.property.name != "getPrototypeOf" {
            return Ok(None);
        }
        let [value] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.getPrototypeOf requires exactly one value",
            ));
        };
        // Distinguish prototypes by runtime kind: arrays, null-prototype values,
        // and class instances each have a different prototype than a plain object,
        // which `isPlainObject` / `isDeepEqual` compare against `Object.prototype`.
        // Defer the discrimination to the `smelt_prototype_sentinel` runtime helper
        // (lowered from `ExprKind::PrototypeSentinel`) so the array/null/object/class
        // branches stay colocated in generated Rust. The helper keeps the existing
        // sentinels (`"__smelt_proto:array"`, `null`, `"__smelt_proto:object"`) for
        // non-class values and returns `"__smelt_proto:class"` for class instances,
        // which carry a hidden `__smelt_class` marker (see `class_unknown_object_text`).
        let value = self.argument(value, body)?;
        let span = self.span(call.span.start, call.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrototypeSentinel { value },
            ty: unknown_ty,
            span,
        })))
    }

    /// Lower `Object.create(proto)` to an erased object shaped from its prototype.
    fn object_create_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object_ident) = &member.object else {
            return Ok(None);
        };
        if object_ident.name != "Object" || member.property.name != "create" {
            return Ok(None);
        }
        let [prototype] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.create requires exactly one prototype argument",
            ));
        };
        if let Argument::ObjectExpression(prototype_object) = prototype {
            return self.object_create_from_literal_prototype(call, prototype_object, body);
        }
        let prototype = self.argument(prototype, body)?;
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        if matches!(self.ctx.krate.types.get(Self::expr_ty(body, prototype)), Some(Type::None)) {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty: unknown_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        if Self::expr_ty(body, prototype) == unknown_ty {
            return Ok(Some(prototype));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: prototype,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Object.create({ ... })` while marking properties as inherited.
    fn object_create_from_literal_prototype(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        prototype_object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call.span.start, call.span.end);
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        let mut entries = Vec::new();
        for property in &prototype_object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "Object.create prototype spread properties are not lowered yet",
                ));
            };
            let Some(key_text) = self.static_object_property_key_text(object_property)? else {
                return self.object_create_call_fallback(call, body);
            };
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(format!("__smelt_proto:{key_text}"))),
                ty: key_ty,
                span: self.span(
                    object_property.key.span().start,
                    object_property.key.span().end,
                ),
            });
            let value = self.object_property_value_expr(object_property, body, Some(value_ty))?;
            entries.push((key, value));
        }
        let object_expr = body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: dict_ty,
            span,
        });
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: object_expr,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })))
    }

    /// Return a static object-literal key when one is available.
    fn static_object_property_key_text(
        &self,
        object_property: &oxc::ast::ast::ObjectProperty<'_>,
    ) -> Result<Option<String>, SmeltError> {
        if object_property.computed {
            return Ok(self.computed_string_literal_key(object_property));
        }
        match &object_property.key {
            PropertyKey::StaticIdentifier(ident) => Ok(Some(ident.name.as_str().to_owned())),
            PropertyKey::StringLiteral(lit) => Ok(Some(lit.value.to_string())),
            PropertyKey::NumericLiteral(lit) => Ok(Some(lit.raw.as_ref().map_or_else(
                || {
                    if lit.value.fract() == 0.0_f64 {
                        format!("{:.0}", lit.value)
                    } else {
                        lit.value.to_string()
                    }
                },
                ToString::to_string,
            ))),
            _ => Err(SmeltError::unsupported(
                self.span(
                    object_property.key.span().start,
                    object_property.key.span().end,
                ),
                "Object.create prototype keys must be static string keys or computed strings",
            )),
        }
    }

    /// Fall back to the broad erased `Object.create` approximation.
    fn object_create_call_fallback(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let [prototype] = call.arguments.as_slice() else {
            return Ok(None);
        };
        let prototype = self.argument(prototype, body)?;
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        if Self::expr_ty(body, prototype) == unknown_ty {
            return Ok(Some(prototype));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: prototype,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower opaque side-effecting `Object` metadata calls.
    fn object_metadata_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        let expected_args = match (object.name.as_str(), member.property.name.as_str()) {
            ("Object", "setPrototypeOf") => 2,
            ("Object", "defineProperty") => 3,
            ("Object", "freeze") => 1,
            _ => return Ok(None),
        };
        if call.arguments.len() != expected_args {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Object.{} requires exactly {expected_args} arguments",
                    member.property.name
                ),
            ));
        }
        for argument in &call.arguments {
            let _ = self.argument(argument, body)?;
        }
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Node `Buffer.alloc(length)` as a zero-filled array-like value.
    fn buffer_alloc_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Buffer" || member.property.name != "alloc" {
            return Ok(None);
        }
        let [length_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.alloc requires exactly one length argument",
            ));
        };
        let length = self.argument(length_arg, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, length)),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(length_arg.span().start, length_arg.span().end),
                "Buffer.alloc length must be numeric",
            ));
        }
        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListFromLength { length },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Node `Buffer.from(value[, encoding])` as an opaque string-producing decode.
    fn buffer_from_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Buffer" || member.property.name != "from" {
            return Ok(None);
        }
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.from requires a value and optional encoding",
            ));
        }
        let Some(source_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.from requires a value argument",
            ));
        };
        let source = self.argument(source_argument, body)?;
        if let Some(encoding) = call.arguments.get(1) {
            let _ = self.argument(encoding, body)?;
        }
        let source_ty = Self::expr_ty(body, source);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        if self.ctx.krate.types.get(source_ty) == Some(&Type::String) {
            return Ok(Some(source));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: source,
                target: string_ty,
            },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower lodash/fp `negate(predicate)` as an opaque boolean predicate function.
    fn lodash_negate_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "negate" || !self.value_imports.contains("negate") {
            return Ok(None);
        }
        let [predicate] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "negate requires exactly one predicate argument",
            ));
        };
        let _ = self.argument(predicate, body)?;
        let param_ty = self.ctx.krate.types.intern(Type::Unknown);
        let return_ty = self.ctx.krate.types.intern(Type::Bool);
        let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![param_ty],
            rest: None,
            required_params: None,
                    mutable_params: Vec::new(),
return_ty,
            is_async: false,
                            may_throw: false,
        }));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower lodash `_.has(object, path)` as an opaque boolean ownership check.
    fn lodash_has_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "has" {
            return Ok(None);
        }
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "_" || !self.value_imports.contains("_") {
            return Ok(None);
        }
        let [target, path] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "lodash has requires object and path arguments",
            ));
        };
        let _ = self.argument(target, body)?;
        let _ = self.argument(path, body)?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower common curried lodash/fp helpers as opaque callable values.
    fn lodash_fp_curried_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "fp" || !self.value_imports.contains("fp") {
            return Ok(None);
        }
        if !matches!(
            member.property.name.as_str(),
            "map"
                | "filter"
                | "replace"
                | "split"
                | "join"
                | "slice"
                | "trimCharsStart"
                | "identity"
                | "toLower"
                | "pipe"
        ) {
            return Ok(None);
        }
        for argument in &call.arguments {
            let _ = self.argument(argument, body)?;
        }
        let param_ty = self.ctx.krate.types.intern(Type::Unknown);
        let return_ty = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![param_ty],
            rest: None,
            required_params: None,
                    mutable_params: Vec::new(),
return_ty,
            is_async: false,
                            may_throw: false,
        }));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower Node `path.join(...)` and `path.resolve(...)` as string path builders.
    fn node_path_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "path" || !self.value_imports.contains("path") {
            return Ok(None);
        }
        if !matches!(member.property.name.as_str(), "join" | "resolve") {
            return Ok(None);
        }
        if call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("path.{} requires at least one argument", member.property.name),
            ));
        }
        for argument in &call.arguments {
            let _ = self.argument(argument, body)?;
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(String::new())),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `Object.fromEntries([[key, value], ...])` to a dictionary literal.
    fn object_from_entries_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" || member.property.name != "fromEntries" {
            return Ok(None);
        }
        if let [argument] = call.arguments.as_slice() {
            let value = self.argument(argument, body)?;
            match self.ctx.krate.types.get(Self::expr_ty(body, value)).cloned() {
                Some(Type::Dict(key, value_ty)) => {
                    if self.ctx.krate.types.get(key) == Some(&Type::String) {
                        return Ok(Some(value));
                    }
                    let string_ty = self.ctx.krate.types.intern(Type::String);
                    let ty = self.ctx.krate.types.intern(Type::Dict(string_ty, value_ty));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                Some(Type::List(entry_ty)) => {
                    if let Some((_key_ty, value_ty)) = self.entries_tuple_item_types(entry_ty) {
                        let string_ty = self.ctx.krate.types.intern(Type::String);
                        let ty = self.ctx.krate.types.intern(Type::Dict(string_ty, value_ty));
                        let unknown = self.ctx.krate.types.intern(Type::Unknown);
                        let entries = body.push_expr(Expr {
                            kind: ExprKind::TypeAssert { value },
                            ty: unknown,
                            span: self.span(call.span.start, call.span.end),
                        });
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::DictProjection {
                                op: DictProjectionOp::FromEntries,
                                dict: entries,
                            },
                            ty,
                            span: self.span(call.span.start, call.span.end),
                        })));
                    }
                    if self.erased_or_union_surface(entry_ty) {
                        let key_ty = self.ctx.krate.types.intern(Type::String);
                        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                        let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
                            ty,
                            span: self.span(call.span.start, call.span.end),
                        })));
                    }
                }
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                    let key_ty = self.ctx.krate.types.intern(Type::String);
                    let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                    let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::DictLit(Vec::new()),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                _ => {}
            }
            if !matches!(argument, Argument::ArrayExpression(_)) {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictLit(Vec::new()),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
        }
        let [Argument::ArrayExpression(entries_array)] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.fromEntries currently requires one array literal of [key, value] pairs",
            ));
        };
        let entries = self.map_constructor_entries(entries_array, body)?;
        let Some((first_key, first_value)) = entries.first().copied() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Object.fromEntries empty arrays require a target type annotation",
            ));
        };
        let key_ty = Self::expr_ty(body, first_key);
        let value_ty = Self::expr_ty(body, first_value);
        for (entry_key, entry_value) in &entries {
            if Self::expr_ty(body, *entry_key) != key_ty
                || Self::expr_ty(body, *entry_value) != value_ty
            {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Object.fromEntries key and value types must be homogeneous",
                ));
            }
        }
        let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Infer dictionary key/value types from an entry tuple item type.
    fn entries_tuple_item_types(
        &self,
        entry_ty: smelt_hir::TypeId,
    ) -> Option<(smelt_hir::TypeId, smelt_hir::TypeId)> {
        match self.ctx.krate.types.get(entry_ty) {
            Some(Type::Tuple(items)) if items.len() == 2 => Some((*items.first()?, *items.get(1)?)),
            Some(Type::Union(items)) => items
                .iter()
                .find_map(|item| self.entries_tuple_item_types(*item)),
            _ => None,
        }
    }

    /// Lower direct TypeScript object key ownership checks.
    fn object_has_own_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name == "call"
            && Self::is_object_prototype_has_own_property(&member.object)
        {
            let [dict_argument, key_argument] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Object.prototype.hasOwnProperty.call requires record and key arguments",
                ));
            };
            let dict = self.argument(dict_argument, body)?;
            let key = self.argument(key_argument, body)?;
            return self.object_has_own_expr(call, body, dict, key);
        }
        if let Expression::Identifier(object) = &member.object
            && object.name == "Object"
            && member.property.name == "hasOwn"
        {
            let [dict_argument, key_argument] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Object.hasOwn requires record and key arguments",
                ));
            };
            let dict = self.argument(dict_argument, body)?;
            let key = self.argument(key_argument, body)?;
            return self.object_has_own_expr(call, body, dict, key);
        }
        if member.property.name == "hasOwnProperty" {
            let [key_argument] = call.arguments.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "hasOwnProperty requires exactly one key argument",
                ));
            };
            let dict = self.expression(&member.object, body)?;
            let key = self.argument(key_argument, body)?;
            return self.object_has_own_expr(call, body, dict, key);
        }
        Ok(None)
    }

    /// Return true for the canonical unbound ownership helper.
    fn is_object_prototype_has_own_property(expression: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(has_own_member) = expression else {
            return false;
        };
        if has_own_member.property.name != "hasOwnProperty" {
            return false;
        }
        let Expression::StaticMemberExpression(prototype_member) = &has_own_member.object else {
            return false;
        };
        if prototype_member.property.name != "prototype" {
            return false;
        }
        matches!(
            &prototype_member.object,
            Expression::Identifier(object) if object.name == "Object"
        )
    }

    /// Build a HIR expression for a TypeScript record key ownership check.
    fn object_has_own_expr(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        mut dict: smelt_hir::ExprId,
        mut key: smelt_hir::ExprId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let dict_ty = Self::expr_ty(body, dict);
        let mut dict_shape_ty = self.type_param_constraint_or_self(dict_ty);
        // `Object.hasOwn(obj, key)` is commonly called on optionally-typed
        // receivers (`object?: unknown`). Unwrap the optional and assert the
        // value to its inner shape so the ownership check sees the underlying
        // record/erased type instead of rejecting the `Optional` wrapper.
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(dict_shape_ty).cloned() {
            dict = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: dict },
                ty: inner,
                span: self.span(call.span.start, call.span.end),
            });
            dict_shape_ty = self.type_param_constraint_or_self(inner);
        }
        // `Object.hasOwn(array, index)` checks whether a (numeric) index is a
        // present element, i.e. `0 <= index < array.length`. Arrays are not
        // records, so this lowers to an in-bounds comparison rather than a
        // dictionary key lookup.
        if matches!(self.ctx.krate.types.get(dict_shape_ty), Some(Type::List(_))) {
            let span = self.span(call.span.start, call.span.end);
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            let bool_ty = self.ctx.krate.types.intern(Type::Bool);
            let len = body.push_expr(Expr {
                kind: ExprKind::Len { operand: dict },
                ty: float_ty,
                span,
            });
            let zero = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: float_ty,
                span,
            });
            let non_negative = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Gte,
                    lhs: key,
                    rhs: zero,
                },
                ty: bool_ty,
                span,
            });
            let in_bounds = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::Lt,
                    lhs: key,
                    rhs: len,
                },
                ty: bool_ty,
                span,
            });
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::And,
                    lhs: non_negative,
                    rhs: in_bounds,
                },
                ty: bool_ty,
                span,
            })));
        }
        let key_ty = match self.ctx.krate.types.get(dict_shape_ty) {
            Some(Type::Dict(key_ty, _)) => *key_ty,
            Some(Type::Unknown) => Self::expr_ty(body, key),
            Some(Type::String) => self.ctx.krate.types.intern(Type::String),
            Some(Type::TypeParam { .. } | Type::Class { .. }) => {
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(call.span.start, call.span.end),
                });
                key_ty
            }
            Some(Type::Union(items))
                if items
                    .iter()
                    .all(|item| self.object_keys_compatible_type(*item)) =>
            {
                self.ctx.krate.types.intern(Type::String)
            }
            _ if self.erased_or_union_surface(dict_shape_ty) => {
                // A receiver typed through an erased object surface (e.g. a
                // `T extends object` generic) is treated as a string-keyed
                // record, mirroring the explicit TypeParam/Class coercion above.
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                dict = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: dict,
                        target,
                    },
                    ty: target,
                    span: self.span(call.span.start, call.span.end),
                });
                key_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "object key ownership checks require a record receiver",
                ));
            }
        };
        if Self::expr_ty(body, key) != key_ty && self.is_string_compatible_type(Self::expr_ty(body, key)) {
            key = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: key },
                ty: key_ty,
                span: self.span(call.span.start, call.span.end),
            });
        }
        if Self::expr_ty(body, key) != key_ty {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "object key ownership checks require a key matching the record key type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey { dict, key },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.isArray` calls using static HIR types.
    fn array_is_array_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Array" || member.property.name != "isArray" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Array.isArray requires exactly one argument",
            ));
        };
        let value = self.argument(argument, body)?;
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: UnknownKind::Array,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let is_array = matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::List(_))
        );
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(is_array)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string case methods.
    fn string_case_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "toLowerCase" | "toLocaleLowerCase" => StringCaseOp::Lower,
            "toUpperCase" | "toLocaleUpperCase" => StringCaseOp::Upper,
            _ => return Ok(None),
        };
        if call.arguments.is_empty() {
            let operand = self.expression(&member.object, body)?;
            if call.optional || member.optional {
                let operand = self.optionalize_index_receiver(operand, body);
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let ty = self.optional_chain_result_type(string_ty);
                let method = self.intern_source_name(member.property.name.as_str());
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::OptionalMethod {
                        receiver: operand,
                        method,
                        args: Vec::new(),
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            let operand_ty = Self::expr_ty(body, operand);
            let effective_operand_ty = self.type_param_constraint_or_self(operand_ty);
            if self.ctx.krate.types.get(effective_operand_ty) == Some(&Type::String) {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            if self.string_method_erased_receiver(effective_operand_ty) {
                let ty = self.ctx.krate.types.intern(Type::String);
                let operand = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: operand,
                        target: ty,
                    },
                    ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            if self.allow_unknown_index_access {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "string case methods require a string receiver and no arguments",
        ))
    }

    /// Return whether an erased receiver can be treated as a string method target.
    fn string_method_erased_receiver(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => true,
            Some(Type::Optional(inner)) => self.string_method_erased_receiver(*inner),
            Some(Type::Union(items)) => items.iter().any(|item| {
                let item = self.type_param_constraint_or_self(*item);
                matches!(
                    self.ctx.krate.types.get(item),
                    Some(Type::String | Type::Unknown | Type::TypeParam { .. } | Type::Class { .. })
                )
            }),
            _ => false,
        }
    }

    /// Lower direct TypeScript Unicode string normalization.
    fn string_normalize_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "normalize" {
            return Ok(None);
        }
        let form = match call.arguments.as_slice() {
            [] => StringNormalizeForm::Nfc,
            [Argument::StringLiteral(literal)] => match literal.value.as_str() {
                "NFC" => StringNormalizeForm::Nfc,
                "NFD" => StringNormalizeForm::Nfd,
                "NFKC" => StringNormalizeForm::Nfkc,
                "NFKD" => StringNormalizeForm::Nfkd,
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(literal.span.start, literal.span.end),
                        "string normalize form must be NFC, NFD, NFKC, or NFKD",
                    ));
                }
            },
            [_] => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string normalize requires a literal normalization form",
                ));
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "string normalize accepts at most one argument",
                ));
            }
        };
        let operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if !(self.is_string_compatible_type(operand_ty) || self.type_contains_unknown(operand_ty)) {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "string normalize requires a string receiver",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringNormalize { form, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string trimming.
    fn string_trim_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let side = match member.property.name.as_str() {
            "trim" => StringTrimSide::Both,
            "trimStart" => StringTrimSide::Start,
            "trimEnd" => StringTrimSide::End,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Ok(None);
        }
        let operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string trim requires a string receiver",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringTrim { side, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string prefix and suffix tests.
    fn string_affix_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "startsWith" => StringAffixOp::StartsWith,
            "endsWith" => StringAffixOp::EndsWith,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require exactly one argument",
            ));
        }
        let mut haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require exactly one argument",
            ));
        };
        let mut needle = self.argument(needle_argument, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.is_string_compatible_type(haystack_ty) || self.type_contains_unknown(haystack_ty)
        {
            haystack = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: haystack },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let needle_ty = Self::expr_ty(body, needle);
        if self.is_string_compatible_type(needle_ty) || self.type_contains_unknown(needle_ty)
        {
            needle = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: needle },
                ty: string_ty,
                span: self.span(needle_argument.span().start, needle_argument.span().end),
            });
        }
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringAffix {
                op,
                haystack,
                needle,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string search methods.
    fn string_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => StringSearchOp::Find,
            "lastIndexOf" => StringSearchOp::RFind,
            _ => return Ok(None),
        };
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search requires one needle and an optional fromIndex argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search requires one needle and an optional fromIndex argument",
            ));
        };
        let needle = self.argument(needle_argument, body)?;
        let from_index = if let Some(from_index_argument) = call.arguments.get(1) {
            let from_index = self.argument(from_index_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, from_index)) {
                return Err(SmeltError::unsupported(
                    self.span(from_index_argument.span().start, from_index_argument.span().end),
                    "string search fromIndex must be numeric",
                ));
            }
            Some(from_index)
        } else {
            None
        };
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search methods require string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `String.prototype.matchAll` to a match-record list surface.
    pub(super) fn string_match_all_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "matchAll" {
            return Ok(None);
        }
        let [pattern_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string matchAll requires one RegExp argument",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(member.object.span().start, member.object.span().end),
                "string matchAll requires a string receiver",
            ));
        }
        let regex = self.argument(pattern_argument, body)?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Float);
        let match_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        let ty = self.ctx.krate.types.intern(Type::List(match_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::RegexMatchAll { regex, haystack },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript literal string replacement.
    fn string_replace_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "replace" {
            return Ok(None);
        }
        let [pattern_argument, replacement_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string replace requires exactly two string arguments",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        let pattern = self.argument(pattern_argument, body)?;
        let replacement = self.argument(replacement_argument, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let pattern_ty = Self::expr_ty(body, pattern);
        let replacement_ty = Self::expr_ty(body, replacement);
        if !(self.is_string_compatible_type(haystack_ty) || self.type_contains_unknown(haystack_ty))
            || !self.is_string_compatible_type(pattern_ty)
            || !self.is_string_compatible_type(replacement_ty)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string replace requires string-compatible receiver, pattern, and replacement",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringReplace {
                op: StringReplaceOp::First,
                haystack,
                pattern,
                replacement,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string repetition.
    fn string_repeat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "repeat" {
            return Ok(None);
        }
        let [count_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string repeat requires exactly one number argument",
            ));
        };
        let operand = self.expression(&member.object, body)?;
        let count = self.argument(count_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, count)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string repeat requires a string receiver and number argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringRepeat { operand, count },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    // Continued in the next split builder file.
}
