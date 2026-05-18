impl ModuleBuilder<'_> {
    /// Return whether a lowered actual type can be assigned to an expected type.
    ///
    /// This models the parts of TypeScript assignability that survive Smelt's
    /// erased HIR types: bottom `never`, top-like annotations, union inclusion,
    /// nullish optionals, and recursive container/function shapes.
    fn type_assignable_to(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
    ) -> bool {
        self.type_assignable_to_inner(actual, expected, 0)
    }

    /// Recursive implementation for `type_assignable_to` with a depth guard.
    fn type_assignable_to_inner(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
        depth: usize,
    ) -> bool {
        if depth > 32 {
            return actual == expected;
        }
        let actual = self.type_param_constraint_or_self(actual);
        let expected = self.type_param_constraint_or_self(expected);
        if actual == expected {
            return true;
        }
        let Some(actual_ty) = self.ctx.krate.types.get(actual).cloned() else {
            return false;
        };
        let Some(expected_ty) = self.ctx.krate.types.get(expected).cloned() else {
            return false;
        };
        match (actual_ty, expected_ty) {
            (Type::Never, _) | (Type::None, Type::Optional(_)) => true,
            (_, Type::Never) | (Type::Int, Type::Float) => false,
            (_, Type::Unknown) | (Type::Unknown, _) => true,
            (Type::Union(actual_items), _) => actual_items
                .iter()
                .all(|item| self.type_assignable_to_inner(*item, expected, depth + 1)),
            (_, Type::Union(expected_items)) => expected_items
                .iter()
                .any(|item| self.type_assignable_to_inner(actual, *item, depth + 1)),
            (Type::Optional(actual_item), Type::Optional(expected_item))
            | (Type::List(actual_item), Type::List(expected_item))
            | (Type::Set(actual_item), Type::Set(expected_item))
            | (Type::Future(actual_item), Type::Future(expected_item)) => {
                self.type_assignable_to_inner(actual_item, expected_item, depth + 1)
            }
            (Type::Optional(_), _) => false,
            (_, Type::Optional(expected_item)) => {
                self.type_assignable_to_inner(actual, expected_item, depth + 1)
            }
            (Type::Tuple(actual_items), Type::Tuple(expected_items)) => {
                actual_items.len() == expected_items.len()
                    && actual_items
                        .iter()
                        .zip(expected_items.iter())
                        .all(|(actual_item, expected_item)| {
                            self.type_assignable_to_inner(
                                *actual_item,
                                *expected_item,
                                depth + 1,
                            )
                        })
            }
            (Type::Tuple(actual_items), Type::List(expected_item)) => actual_items
                .iter()
                .all(|actual_item| {
                    self.type_assignable_to_inner(*actual_item, expected_item, depth + 1)
                }),
            (Type::Dict(actual_key, actual_value), Type::Dict(expected_key, expected_value)) => {
                self.type_assignable_to_inner(actual_key, expected_key, depth + 1)
                    && self.type_assignable_to_inner(actual_value, expected_value, depth + 1)
            }
            (Type::Function(actual_fn), Type::Function(expected_fn)) => {
                actual_fn.params.len() == expected_fn.params.len()
                    && actual_fn.is_async == expected_fn.is_async
                    && actual_fn
                        .params
                        .iter()
                        .zip(expected_fn.params.iter())
                        .all(|(actual_param, expected_param)| {
                            self.type_assignable_to_inner(
                                *expected_param,
                                *actual_param,
                                depth + 1,
                            )
                        })
                    && self.type_assignable_to_inner(
                        actual_fn.return_ty,
                        expected_fn.return_ty,
                        depth + 1,
                    )
            }
            (
                Type::Class {
                    name: actual_name,
                    args: actual_args,
                },
                Type::Class {
                    name: expected_name,
                    args: expected_args,
                },
            ) => {
                actual_name == expected_name
                    && actual_args.len() == expected_args.len()
                    && actual_args
                        .iter()
                        .zip(expected_args.iter())
                        .all(|(actual_arg, expected_arg)| {
                            self.type_assignable_to_inner(*actual_arg, *expected_arg, depth + 1)
                        })
            }
            (actual_ty, expected_ty) => actual_ty == expected_ty,
        }
    }

    /// Return whether a key argument can be used with a lowered map key type.
    fn map_key_type_compatible(
        &self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
    ) -> bool {
        let expected = self.type_param_constraint_or_self(expected);
        let actual = self.type_param_constraint_or_self(actual);
        if expected == actual {
            return true;
        }
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(expected) {
            return items
                .iter()
                .any(|item| self.map_key_type_compatible(*item, actual));
        }
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(actual) {
            return items
                .iter()
                .all(|item| self.map_key_type_compatible(expected, *item));
        }
        if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(actual) {
            return self.map_key_type_compatible(expected, *inner);
        }
        if self.ctx.krate.types.get(expected) == Some(&Type::String)
            && let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(actual)
            && self.ctx.krate.symbols.get(*name) == Some("PropertyKey")
        {
            return true;
        }
        if let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(expected)
            && self.ctx.krate.symbols.get(*name) == Some("PropertyKey")
        {
            return !matches!(self.ctx.krate.types.get(actual), Some(Type::None));
        }
        false
    }

    /// Return whether numeric literal widening makes two map value types compatible.
    fn numeric_type_compatible(
        &self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
    ) -> bool {
        expected == actual
            || (self.is_numeric_like_type(expected) && self.is_numeric_like_type(actual))
    }

    /// Return whether a type is represented by Smelt's numeric runtime value.
    fn is_numeric_like_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(self.type_param_constraint_or_self(ty)) {
            Some(Type::Float | Type::Int) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.is_numeric_like_type(item)),
            _ => false,
        }
    }

    /// Lower supported string padding calls into HIR string runtime calls.
    fn string_pad_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "padStart" => StringPadOp::Start,
            "padEnd" => StringPadOp::End,
            _ => return Ok(None),
        };
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires target length and optional string padding",
            ));
        }
        let operand = self.expression(&member.object, body)?;
        let Some(target_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires target length",
            ));
        };
        let target_len = self.argument(target_argument, body)?;
        let pad = if let Some(pad_argument) = call.arguments.get(1) {
            self.argument(pad_argument, body)?
        } else {
            let ty = self.ctx.krate.types.intern(Type::String);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(" ".to_owned())),
                ty,
                span: self.span(call.span.start, call.span.end),
            })
        };
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, target_len)) != Some(&Type::Float)
            || self.ctx.krate.types.get(Self::expr_ty(body, pad)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string padding requires a string receiver, number target length, and string padding",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringPad {
                op,
                operand,
                target_len,
                pad,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `String.prototype.charAt` and `charCodeAt`.
    fn string_char_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let returns_code = match member.property.name.as_str() {
            "charAt" => false,
            "charCodeAt" => true,
            _ => return Ok(None),
        };
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires exactly one number argument",
            ));
        };
        let operand = self.expression(&member.object, body)?;
        let index = self.argument(index_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires a string receiver and number argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(if returns_code {
            Type::Float
        } else {
            Type::String
        });
        let kind = if returns_code {
            ExprKind::StringCharCodeAt { operand, index }
        } else {
            ExprKind::StringCharAt { operand, index }
        };
        Ok(Some(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.join` for string arrays.
    fn string_join_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "join" {
            return Ok(None);
        }
        let mut items = self.expression(&member.object, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let items_ty = self.type_param_constraint_or_self(Self::expr_ty(body, items));
        let items_ty = match self.ctx.krate.types.get(items_ty) {
            Some(Type::List(_)) => items_ty,
            Some(Type::Unknown | Type::TypeParam { .. }) => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                items = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: items },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                list_ty
            }
            Some(Type::Union(members))
                if members.iter().any(|union_member| {
                    matches!(
                        self.ctx.krate.types.get(*union_member),
                        Some(Type::List(_) | Type::Unknown | Type::TypeParam { .. })
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                items = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: items },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                list_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join requires an array receiver",
                ));
            }
        };
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(items_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join requires an array receiver",
            ));
        };
        if !self.array_join_item_type_supported(*item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join currently requires primitive or unknown array items",
            ));
        }
        let separator = match call.arguments.as_slice() {
            [] => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(",".to_owned())),
                ty: string_ty,
                span: self.span(call.span.start, call.span.end),
            }),
            [separator_argument] => {
                let separator = self.argument(separator_argument, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, separator)) != Some(&Type::String) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array join separator must be a string",
                    ));
                }
                separator
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join supports zero or one string separator argument",
                ));
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringJoin { items, separator },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether `Array.prototype.join` can stringify this lowered item type.
    fn array_join_item_type_supported(&self, item_ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(item_ty))
        {
            Some(
                Type::Bool
                | Type::String
                | Type::Int
                | Type::Float
                | Type::Unknown
                | Type::TypeParam { .. },
            ) => true,
            Some(Type::Optional(item)) => self.array_join_item_type_supported(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .all(|item| self.array_join_item_type_supported(item)),
            _ => false,
        }
    }

    /// Lower direct TypeScript string containment.
    fn string_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires exactly one argument",
            ));
        }
        let mut haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires exactly one argument",
            ));
        };
        let mut needle = self.argument(needle_argument, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let haystack_ty = Self::expr_ty(body, haystack);
        if self.ctx.krate.types.get(haystack_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(haystack_ty)
        {
            haystack = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: haystack },
                ty: string_ty,
                span: self.span(member.object.span().start, member.object.span().end),
            });
        }
        let needle_ty = Self::expr_ty(body, needle);
        if self.ctx.krate.types.get(needle_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(needle_ty)
        {
            needle = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: needle },
                ty: string_ty,
                span: self.span(needle_argument.span().start, needle_argument.span().end),
            });
        }
        let coerced_haystack_ty = Self::expr_ty(body, haystack);
        let coerced_needle_ty = Self::expr_ty(body, needle);
        if self.ctx.krate.types.get(coerced_haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(coerced_needle_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringContains { haystack, needle },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript array containment.
    fn list_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Ok(None);
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *list_item_ty;
        let Some(item_argument) = call.arguments.first() else {
            return Ok(None);
        };
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), item_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array includes argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListContains { list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Set.prototype.has`.
    fn set_contains_call(
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
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set.has requires exactly one argument",
            ));
        };
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_element_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let element_ty = *set_element_ty;
        let item = self.argument(item_argument, body)?;
        if !self.array_item_type_compatible(Self::expr_ty(body, item), element_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set.has argument must match the set element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetContains { set, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Set` mutation methods.
    fn set_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "add" | "delete" | "clear") {
            return Ok(None);
        }
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_element_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let element_ty = *set_element_ty;
        match method {
            "add" => {
                let [item_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.add requires exactly one item argument",
                    ));
                };
                let item = self.argument(item_argument, body)?;
                if !self.array_item_type_compatible(Self::expr_ty(body, item), element_ty) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.add item must match the set element type",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::SetAdd { set, item },
                    ty: set_ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "delete" => {
                let [item_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.delete requires exactly one item argument",
                    ));
                };
                let item = self.argument(item_argument, body)?;
                if !self.array_item_type_compatible(Self::expr_ty(body, item), element_ty) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.delete item must match the set element type",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::SetRemove {
                        op: SetRemoveOp::Delete,
                        set,
                        item,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clear" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Set.clear requires no arguments",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::SetClear { set },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower direct TypeScript `Map.prototype.has`.
    fn map_has_call(
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
        let [key_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.has requires exactly one key argument",
            ));
        };
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, _)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let key = self.argument(key_argument, body)?;
        if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.has key must match the map key type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictContainsKey { dict, key },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Map.prototype.get`.
    fn map_get_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "get" {
            return Ok(None);
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let [key_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.get requires exactly one key argument",
            ));
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let key = self.argument(key_argument, body)?;
        if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map.get key must match the map key type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Optional(value_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictGet {
                dict,
                key,
                default: None,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Map` mutation methods.
    fn map_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "set" | "delete" | "clear") {
            return Ok(None);
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        match method {
            "set" => {
                let [key_argument, value_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.set requires key and value arguments",
                    ));
                };
                let key = self.argument(key_argument, body)?;
                let value = self.argument(value_argument, body)?;
                if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key))
                    || !self.numeric_type_compatible(value_ty, Self::expr_ty(body, value))
                {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.set key and value must match the map type",
                    ));
                }
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictSet { dict, key, value },
                    ty: dict_ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "delete" => {
                let [key_argument] = call.arguments.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.delete requires exactly one key argument",
                    ));
                };
                let key = self.argument(key_argument, body)?;
                    if !self.map_key_type_compatible(key_ty, Self::expr_ty(body, key)) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.delete key must match the map key type",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictRemoveKey { dict, key },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clear" => {
                if !call.arguments.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "Map.clear requires no arguments",
                    ));
                }
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::DictClear { dict },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            _ => Ok(None),
        }
    }

    // Continued in the next split builder file.
}
