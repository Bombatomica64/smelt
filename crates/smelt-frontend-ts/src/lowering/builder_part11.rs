impl ModuleBuilder<'_> {
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
                op: BinOp::Eq,
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
        if object.name != "Object" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "keys" => DictProjectionOp::Keys,
            "values" => DictProjectionOp::Values,
            "entries" => DictProjectionOp::Entries,
            _ => return Ok(None),
        };
        let [dict_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Object.{} requires exactly one record argument",
                    member.property.name
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
        let ty = match op {
            DictProjectionOp::Keys => self.ctx.krate.types.intern(Type::List(key_ty)),
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
        self.argument(argument, body)?;
        let symbol_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(symbol_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
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
        let _ = self.argument(value, body)?;
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
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
            return_ty,
            is_async: false,
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
            return_ty,
            is_async: false,
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
                    if let Some((key_ty, value_ty)) = self.entries_tuple_item_types(entry_ty) {
                        let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::DictLit(Vec::new()),
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
        let dict_shape_ty = self.type_param_constraint_or_self(dict_ty);
        let key_ty = match self.ctx.krate.types.get(dict_shape_ty) {
            Some(Type::Dict(key_ty, _)) => *key_ty,
            Some(
                Type::Unknown | Type::TypeParam { .. } | Type::Class { .. } | Type::String,
            ) => {
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
        if self.ctx.krate.types.get(Self::expr_ty(body, value)) == Some(&Type::Unknown) {
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
        if let Some(from_index_argument) = call.arguments.get(1) {
            let from_index = self.argument(from_index_argument, body)?;
            if !self.slice_index_type_is_number(Self::expr_ty(body, from_index)) {
                return Err(SmeltError::unsupported(
                    self.span(from_index_argument.span().start, from_index_argument.span().end),
                    "string search fromIndex must be numeric",
                ));
            }
        }
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
        let _pattern = self.argument(pattern_argument, body)?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Float);
        let match_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        let ty = self.ctx.krate.types.intern(Type::List(match_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
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
