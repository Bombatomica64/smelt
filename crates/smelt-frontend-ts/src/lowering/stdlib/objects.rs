//! `Object` statics, `SameValueZero` equality and `Array.isArray`-style type queries.

use crate::lowering::ModuleBuilder;
use crate::SmeltError;
use oxc::ast::ast::{Argument, Expression};
use oxc::syntax::operator::{BinaryOperator, LogicalOperator};
use smelt_hir::{
    BinOp, Body, DictProjectionOp, ErasureReason, Expr, ExprKind, Literal, PropertyLookup, Span,
    Type, UnknownKind,
};

impl ModuleBuilder<'_> {
    /// Lower `a === b || Object.is(a, b)` as JavaScript `SameValueZero` equality.
    pub(in crate::lowering) fn same_value_zero_logical(
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
    pub(in crate::lowering) fn object_is_identifier_pair<'a>(
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
    pub(in crate::lowering) fn argument_identifier<'a>(argument: &'a Argument<'a>) -> Option<&'a str> {
        match argument {
            Argument::Identifier(ident) => Some(ident.name.as_str()),
            _ => None,
        }
    }

    /// Return whether an expression is the requested identifier.
    pub(in crate::lowering) fn expression_is_identifier(expression: &Expression<'_>, name: &str) -> bool {
        matches!(expression, Expression::Identifier(ident) if ident.name == name)
    }

    /// Lower `Object.is(a, b)` as a strict equality expression.
    pub(in crate::lowering) fn object_is_call(
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

    /// Lower direct TypeScript `Object.keys`, `Object.values`, and `Object.entries` calls.
    ///
    /// `Reflect.ownKeys(record)` is routed here too, as its own projection: it
    /// reports symbol-keyed properties as well as string-keyed ones, and a
    /// symbol-keyed property IS stored (under the `__smelt_symbol:` key form),
    /// so it is not interchangeable with `Object.keys`. Its declared result is
    /// `(string | symbol)[]`, which is why that arm alone yields
    /// `List<Unknown>`; see the `DictProjectionOp::OwnKeys` arm of the
    /// result-type match below.
    pub(in crate::lowering) fn object_projection_call(
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
            ("Object", "keys" | "getOwnPropertyNames") => DictProjectionOp::Keys,
            ("Object", "values") => DictProjectionOp::Values,
            ("Object", "entries") => DictProjectionOp::Entries,
            ("Reflect", "ownKeys") => DictProjectionOp::OwnKeys,
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
        // A CONCRETE typed-array view answers its own enumeration: its own
        // properties are exactly its element indices, and both the indices and
        // the elements are on the value. Erasing it to a `Record<string,
        // unknown>` first — which is what the arms below do for any class —
        // would cross the dynamic boundary to compute an answer that is
        // already here, and would type the values `unknown` where they are
        // numbers.
        if let Some(expr) = self.typed_array_projection(op, dict, dict_ty, call.span, body) {
            return Ok(Some(expr));
        }
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
                let value_ty = self.erase_unless_unknown(dict_ty, ErasureReason::UntypedBuiltin);
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
                let value_ty = self.erase(ErasureReason::UntypedBuiltin);
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
                let value_ty = self.erase(ErasureReason::UntypedBuiltin);
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
            DictProjectionOp::FromEntries => return Ok(None),
            DictProjectionOp::Keys | DictProjectionOp::ForInKeys => {
                self.ctx.krate.types.intern(Type::List(key_ty))
            }
            // `Reflect.ownKeys` is declared `(target: object) => (string |
            // symbol)[]` in TypeScript's own lib, and a consumer discriminates
            // the two kinds at run time (`typeof key === 'string'`). A
            // `List<string>` element type is therefore not a narrowing but a
            // falsehood: it drops the symbol keys and const-folds the caller's
            // `typeof` guard. `Unknown` is the representation a symbol value
            // already has everywhere else (see the `Symbols` arm below), and the
            // dynamic index paths map that tag back to the storage key.
            DictProjectionOp::OwnKeys => {
                let own_key_ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
                self.ctx.krate.types.intern(Type::List(own_key_ty))
            }
            // A symbol-keyed property list holds symbol VALUES, not their
            // descriptions: the property key an erased record stores is
            // `__smelt_symbol:<description>`, so handing back the bare
            // description made `source[sym]` miss and `target[sym] = v` write a
            // plain string key. `Type::Unknown` is the representation a symbol
            // already has everywhere else (`Literal::Symbol` ->
            // `SmeltUnknown::Symbol`), and the dynamic index paths map that tag
            // back to the prefixed key. Interned only on this arm — interning
            // `Unknown`/`List<Unknown>` for every projection would change which
            // record backing the other arms pick.
            DictProjectionOp::Symbols => {
                let symbol_key_ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
                self.ctx.krate.types.intern(Type::List(symbol_key_ty))
            }
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
    pub(in crate::lowering) fn object_keys_compatible_type(&self, ty: smelt_hir::TypeId) -> bool {
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
    pub(in crate::lowering) fn object_get_own_property_symbols_call(
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
        let value_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.getOwnPropertySymbols");
        let target = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        let dict = body.push_expr(Expr {
            kind: ExprKind::UnknownCast { value, target },
            ty: target,
            span: self.span(call.span.start, call.span.end),
        });
        // The elements are symbol VALUES (erased `SmeltUnknown::Symbol`), not their
        // descriptions — see `object_projection_call`. A symbol read back out of
        // this list has to index the receiver again (`source[symbols[i]]`), which
        // only works if it still carries the symbol tag the property-key mapping
        // turns into the internal `__smelt_symbol:` key.
        let symbol_ty = self.erase_with(ErasureReason::UnmodeledType, "symbol");
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
    pub(in crate::lowering) fn object_get_prototype_of_call(
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
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.getPrototypeOf");
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::PrototypeSentinel {
                value,
                // `Object.getPrototypeOf` is specified to ignore own properties,
                // unlike the `__proto__` accessor.
                own_slot_shadows: false,
            },
            ty: unknown_ty,
            span,
        })))
    }

    /// Lower `Object(value)` — the boxing call — to a runtime box.
    ///
    /// `Object(1)` / `Object('a')` / `Object(true)` wrap a primitive in the same
    /// marker record `new Number(1)` builds, `Object(obj)` returns `obj`, and
    /// `Object()` / `Object(null)` yield a fresh empty object. Deep-equality code
    /// leans on the wrapper being distinguishable from its primitive while
    /// comparing equal through `valueOf` (es-toolkit `isEqualWith`), so the
    /// distinction cannot be dropped. Which branch applies depends on the runtime
    /// tag, so the decision belongs to `smelt_box_value`.
    ///
    /// A user-declared value named `Object` shadows the global; the recognition
    /// table cannot see that, so bail out and let ordinary call lowering handle it.
    pub(in crate::lowering) fn object_box_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Object"
            || self.classes.contains("Object")
            || self.imports.is_value("Object")
        {
            return Ok(None);
        }
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object(value)");
        let span = self.span(call.span.start, call.span.end);
        let Some(argument) = call.arguments.first() else {
            // `Object()` with no argument is `{}`.
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::DictLit(Vec::new()),
                ty: unknown_ty,
                span,
            })));
        };
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                span,
                "Object(value) accepts at most one argument",
            ));
        }
        let value = self.argument(argument, body)?;
        let value = if Self::expr_ty(body, value) == unknown_ty {
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: unknown_ty,
                },
                ty: unknown_ty,
                span,
            })
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BoxPrimitive { value },
            ty: unknown_ty,
            span,
        })))
    }

    /// Lower `Object.create(proto)` to an erased object shaped from its prototype.
    ///
    /// Every prototype spelling takes the SAME path: the value is erased and
    /// handed to the `smelt_object_from_prototype` runtime helper, which mints a
    /// fresh object, copies the prototype's own keys behind the
    /// `__smelt_proto:` prefix (so they resolve on a read but stay out of the
    /// created object's own-key enumeration), and records the prototype value
    /// itself so `Object.getPrototypeOf` can answer with the very object that
    /// was passed in.
    ///
    /// Two shortcuts used to bypass it and both lost the prototype CHAIN, which
    /// is observable: an object-literal prototype was inlined as
    /// `__smelt_proto:` entries with no record of the parent, and a `null`
    /// prototype folded to an empty object literal whose `[[Prototype]]` then
    /// read back as `Object.prototype`. `Object.getPrototypeOf` must answer the
    /// literal for the first and `null` for the second — the distinction
    /// `isPlainObject` is built on (`Object.create({})` is not a plain object,
    /// `Object.create(null)` is). Dropping both also makes a spread or computed
    /// key in the prototype literal work, which the inlining rejected.
    pub(in crate::lowering) fn object_create_call(
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
        let prototype = self.argument(prototype, body)?;
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.create");
        let span = self.span(call.span.start, call.span.end);
        let prototype = if Self::expr_ty(body, prototype) == unknown_ty {
            prototype
        } else {
            body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: prototype,
                    target: unknown_ty,
                },
                ty: unknown_ty,
                span,
            })
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ObjectFromPrototype { prototype },
            ty: unknown_ty,
            span,
        })))
    }

    /// Lower `Object.defineProperty` / `Object.defineProperties` to a real
    /// property installation.
    ///
    /// The two statics differ only in shape: `defineProperty` names ONE key and
    /// its descriptor, `defineProperties` hands over a whole key-to-descriptor
    /// table. Normalizing the singular form into a one-entry table lets both
    /// spellings share `ExprKind::DefineProperties` and the single runtime
    /// helper behind it, instead of each growing its own rule.
    ///
    /// A non-string property key (`Object.defineProperty(o, Symbol.toStringTag,
    /// d)`) falls through to the opaque metadata path: symbol keys are not
    /// modeled, and inventing a string key for one would collide with a real
    /// property of the same name.
    pub(in crate::lowering) fn object_define_properties_call(
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
        let span = self.span(call.span.start, call.span.end);
        // not a new erasure yet: the call may still decline below, so the
        // decision is recorded only once the rule applies (before the result).
        let unknown_ty = self.unknown_type();
        let (target, descriptors) = match member.property.name.as_str() {
            "defineProperties" => {
                let [target, descriptors] = call.arguments.as_slice() else {
                    return Ok(None);
                };
                let target = self.argument(target, body)?;
                let descriptors = self.argument(descriptors, body)?;
                (target, descriptors)
            }
            "defineProperty" => {
                let [target, key, descriptor] = call.arguments.as_slice() else {
                    return Ok(None);
                };
                let target = self.argument(target, body)?;
                let key = self.argument(key, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, key)) != Some(&Type::String) {
                    return Ok(None);
                }
                let descriptor = self.argument(descriptor, body)?;
                let descriptor = Self::erase_to_unknown(descriptor, unknown_ty, span, body);
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let dict_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
                let descriptors = body.push_expr(Expr {
                    kind: ExprKind::DictLit(Vec::from([(key, descriptor)])),
                    ty: dict_ty,
                    span,
                });
                (target, descriptors)
            }
            _ => return Ok(None),
        };
        let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.defineProperties");
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DefineProperties {
                target,
                descriptors,
            },
            ty: unknown_ty,
            span,
        })))
    }

    /// Wrap `value` in an erasure cast unless it is already `unknown`-typed.
    pub(in crate::lowering) fn erase_to_unknown(
        value: smelt_hir::ExprId,
        unknown_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if Self::expr_ty(body, value) == unknown_ty {
            return value;
        }
        body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// Lower opaque side-effecting `Object` metadata calls.
    pub(in crate::lowering) fn object_metadata_mutation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.object_define_properties_call(call, body)? {
            return Ok(Some(expr));
        }
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
        let ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object metadata mutation");
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `Object.fromEntries([[key, value], ...])` to a dictionary literal.
    pub(in crate::lowering) fn object_from_entries_call(
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
                Some(Type::JsMap(_key, value_ty)) => {
                    // A source `Map` always backs onto `SmeltJsMap`, never a
                    // `Record`, so `Object.fromEntries(map)` must convert its
                    // entries into a `Record<string, V>` (stringifying keys),
                    // even when the map is string-keyed — returning the map
                    // as-is would leave a `SmeltJsMap` where a `SmeltRecord` is
                    // expected. This mirrors the non-string `Dict` arm above; the
                    // `SmeltJsMap`-backed receiver coerces the same way.
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
                        // not a new erasure: the entries list is only re-viewed as the
                        // tagged runtime-helper operand; the result keeps `value_ty`.
                        let unknown = self.unknown_type();
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
                        let value_ty = self.erase_unless_unknown(entry_ty, ErasureReason::UntypedBuiltin);
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
                    let value_ty = self.erase_unless_unknown(Self::expr_ty(body, value), ErasureReason::UntypedBuiltin);
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
                let value_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.fromEntries");
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
    pub(in crate::lowering) fn entries_tuple_item_types(
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

    /// Lower `Object.getOwnPropertyDescriptor(obj, key)` in Smelt's object model.
    ///
    /// Every own property a source program can give a record is a DATA
    /// property that is writable, enumerable and configurable
    /// (`Object.defineProperty` stores the value; accessors live on classes,
    /// not as own entries of a record). So the descriptor is
    /// `Object.hasOwn(obj, key) ? { value: obj[key], writable: true,
    /// enumerable: true, configurable: true } : undefined`, built from the
    /// ownership test and keyed read that already exist. An own key is found
    /// even when it is spelled `__proto__` on a null-prototype record, which is
    /// what Hono's `parseBody` test asks.
    ///
    /// The receiver is read as the string-keyed erased record
    /// `getOwnPropertySymbols` reads, because the spec's `PropertyDescriptor`
    /// types `value` as `any`: the descriptor is a genuine dynamic boundary,
    /// whatever the receiver's own value type.
    ///
    /// The receiver and key are each evaluated twice (the test and the read),
    /// so the rule applies only when both are re-readable without effects
    /// ([`Self::is_effect_free_reread`]); any other shape declines to the
    /// ordinary call path.
    pub(in crate::lowering) fn object_get_own_property_descriptor_call(
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
        if object.name != "Object"
            || member.property.name != "getOwnPropertyDescriptor"
            || self.classes.contains("Object")
        {
            return Ok(None);
        }
        let [receiver_argument, key_argument] = call.arguments.as_slice() else {
            return Ok(None);
        };
        let (Some(receiver_expr), Some(key_expr)) =
            (receiver_argument.as_expression(), key_argument.as_expression())
        else {
            return Ok(None);
        };
        if !Self::is_effect_free_reread(receiver_expr) || !Self::is_effect_free_reread(key_expr) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        // not a new erasure yet: `read_pair` may still decline; the
        // descriptor's `any` value is recorded once both reads succeed.
        let unknown_ty = self.unknown_type();
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let record_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, unknown_ty));
        let read_pair = |this: &mut Self, target: &mut Body| -> Result<Option<(smelt_hir::ExprId, smelt_hir::ExprId)>, SmeltError> {
            let value = this.expression(receiver_expr, target)?;
            let record = target.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: record_ty,
                },
                ty: record_ty,
                span,
            });
            let key = this.expression(key_expr, target)?;
            let key_ty = Self::expr_ty(target, key);
            let key = if key_ty == string_ty {
                key
            } else if this.is_string_compatible_type(key_ty) {
                target.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: key },
                    ty: string_ty,
                    span,
                })
            } else {
                return Ok(None);
            };
            Ok(Some((record, key)))
        };
        let Some((tested_record, tested_key)) = read_pair(self, body)? else {
            return Ok(None);
        };
        let Some((read_record, read_key)) = read_pair(self, body)? else {
            return Ok(None);
        };
        // `PropertyDescriptor.value` is `any` in TypeScript's own lib.
        let _ = self.erase_with(ErasureReason::ExternalType, "PropertyDescriptor.value");
        let is_own = body.push_expr(Expr {
            kind: ExprKind::DictContainsKey {
                dict: tested_record,
                key: tested_key,
                lookup: PropertyLookup::Own,
            },
            ty: bool_ty,
            span,
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::Index {
                receiver: read_record,
                index: read_key,
            },
            ty: unknown_ty,
            span,
        });
        let mut entries = Vec::from([(
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String("value".to_owned())),
                ty: string_ty,
                span,
            }),
            value,
        )]);
        for flag in ["writable", "enumerable", "configurable"] {
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(flag.to_owned())),
                ty: string_ty,
                span,
            });
            let truth = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            });
            let erased = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: truth },
                ty: unknown_ty,
                span,
            });
            entries.push((key, erased));
        }
        let descriptor = body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: record_ty,
            span,
        });
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let absent = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Undefined),
            ty: none_ty,
            span,
        });
        let ty = self.ctx.krate.types.intern(Type::Optional(record_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond: is_own,
                then_expr: descriptor,
                else_expr: absent,
            },
            ty,
            span,
        })))
    }

    /// Return whether evaluating an expression twice is the same as once.
    ///
    /// Identifiers, `this`, literals and non-optional static member chains
    /// over those, looked through parentheses and type assertions. A getter on
    /// a chain could in principle observe the second read, but a source
    /// getter with a side effect is not something a descriptor probe is
    /// written against, and computed members, calls and optional chains are
    /// refused outright.
    fn is_effect_free_reread(expression: &Expression<'_>) -> bool {
        match expression {
            Expression::Identifier(_)
            | Expression::ThisExpression(_)
            | Expression::StringLiteral(_)
            | Expression::NumericLiteral(_) => true,
            Expression::StaticMemberExpression(member) => {
                !member.optional && Self::is_effect_free_reread(&member.object)
            }
            Expression::ParenthesizedExpression(inner) => {
                Self::is_effect_free_reread(&inner.expression)
            }
            Expression::TSAsExpression(inner) => Self::is_effect_free_reread(&inner.expression),
            Expression::TSNonNullExpression(inner) => {
                Self::is_effect_free_reread(&inner.expression)
            }
            _ => false,
        }
    }

    /// Lower direct TypeScript object key ownership checks.
    pub(in crate::lowering) fn object_has_own_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name == "call"
            && Self::is_object_prototype_own_key_probe(&member.object)
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
            && matches!(
                (object.name.as_str(), member.property.name.as_str()),
                ("Object", "hasOwn") | ("Reflect", "has")
            )
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
        if matches!(
            member.property.name.as_str(),
            "hasOwnProperty" | "propertyIsEnumerable"
        ) {
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

    /// Return true for either unbound own-key probe on `Object.prototype`.
    ///
    /// `hasOwnProperty` and `propertyIsEnumerable` differ only for own properties
    /// that are not enumerable, and Smelt's object model has no way to make one
    /// from source: the only non-enumerable entries a record carries are internal
    /// `__smelt_*` marker keys, which no source key can name and which own-key
    /// enumeration already hides. So both spellings lower to the same own-key
    /// check. Modeling `propertyIsEnumerable` matters because es-toolkit's
    /// `getSymbols` gates every symbol-keyed property on it
    /// (`getOwnPropertySymbols(o).filter(s => propertyIsEnumerable.call(o, s))`);
    /// reading it off the opaque `Object.prototype` sentinel yielded `undefined`,
    /// the call answered `null`, and every symbol key was filtered away.
    fn is_object_prototype_own_key_probe(expression: &Expression<'_>) -> bool {
        Self::is_object_prototype_method(expression, "hasOwnProperty")
            || Self::is_object_prototype_method(expression, "propertyIsEnumerable")
    }

    /// Return true for the unshadowed `Object.prototype.<name>` member expression.
    fn is_object_prototype_method(expression: &Expression<'_>, name: &str) -> bool {
        let Expression::StaticMemberExpression(method_member) = expression else {
            return false;
        };
        if method_member.property.name != name {
            return false;
        }
        let Expression::StaticMemberExpression(prototype_member) = &method_member.object else {
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
    pub(in crate::lowering) fn object_has_own_expr(
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
        // `Object.hasOwn(array, index)` with a NUMERIC key checks whether that
        // index is a present element, i.e. `0 <= index < array.length`, which is
        // a comparison rather than a dictionary lookup.
        //
        // A non-numeric key is a different question. A JavaScript array is an
        // exotic object whose own keys are its indices AND its named properties
        // (`Object.hasOwn(/c/.exec(s), 'index')` is `true` — a match result is an
        // array carrying `index`/`input`/`groups`), and only the runtime knows
        // which names the array holds. Such a receiver is therefore erased and
        // asked, exactly as an already-erased array receiver is: the erasure
        // aliases the array's identity, which is what the named-property table is
        // keyed by, so the typed handle and the erased view answer alike. Before
        // this the in-bounds comparison ran on every key, and a string key
        // parsed to `NaN` and answered a constant `false`.
        if matches!(self.ctx.krate.types.get(dict_shape_ty), Some(Type::List(_)))
            && !matches!(
                self.ctx
                    .krate
                    .types
                    .get(self.type_param_constraint_or_self(Self::expr_ty(body, key))),
                Some(Type::Int | Type::Float)
            )
        {
            let span = self.span(call.span.start, call.span.end);
            let unknown_ty = self.erase_with(ErasureReason::ByName, "array named property");
            dict = body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value: dict,
                    target: unknown_ty,
                },
                ty: unknown_ty,
                span,
            });
            dict_shape_ty = unknown_ty;
        }
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
            Some(Type::Function(_)) => {
                // `Object.hasOwn(fn, key)` on a function value (paired with
                // `for (const key in fn)` in es-toolkit's `defaultsDeep` spec).
                // Smelt models a function as a property-less closure with no
                // enumerable own keys, so the ownership check is a constant
                // `false` — the sound answer for Smelt's function representation,
                // and it compiles cleanly without casting the closure to a
                // record. The receiver and key were already lowered above for
                // their effects/types.
                let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(false)),
                    ty: bool_ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            Some(Type::TypeParam { .. } | Type::Class { .. }) => {
                // A generic or erased object is treated as a string-keyed
                // record, matching the `for...in` lowering that casts the same
                // value to `Record<string, unknown>`.
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.hasOwn");
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
                let value_ty = self.erase_with(ErasureReason::UntypedBuiltin, "Object.hasOwn");
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
            kind: ExprKind::DictContainsKey {
                dict,
                key,
                lookup: PropertyLookup::Own,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.isArray` calls using static HIR types.
    pub(in crate::lowering) fn array_is_array_call(
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
        let ty = self.ctx.krate.types.intern(Type::Bool);
        // Fold only when the operand's static type settles the question; every
        // other operand keeps the runtime probe. See `static_array_match`.
        let Some(is_array) = self.static_array_match(Self::expr_ty(body, value)) else {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::UnknownIs {
                    value,
                    kind: UnknownKind::Array,
                },
                ty,
                span: self.span(call.span.start, call.span.end),
            })));
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(is_array)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `ArrayBuffer.isView` calls.
    ///
    /// `ArrayBuffer.isView(x)` is true for a *view* over byte storage and false
    /// for the storage itself. The modeled view identities are exactly the
    /// `ByteBufferRole::View` entries of the shared host registry — the eleven
    /// typed arrays, `DataView`, and Node's `Buffer` (a `Uint8Array` subclass) — so
    /// the call lowers to the `instanceof` disjunction over those classes, reusing
    /// the marker-based `InstanceOf` path. Deriving the set from the registry
    /// rather than naming it here is what keeps this in step with es-toolkit's
    /// `isTypedArray` (`ArrayBuffer.isView(x) && !(x instanceof DataView)`).
    ///
    /// A statically concrete FAMILY value folds at compile time instead — a
    /// typed array and a `DataView` to `true`, byte storage to `false` — and
    /// any other concrete value carries no host marker and folds to `false`.
    pub(in crate::lowering) fn arraybuffer_is_view_call(
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
        if object.name != "ArrayBuffer"
            || member.property.name != "isView"
            || self.classes.contains("ArrayBuffer")
        {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "ArrayBuffer.isView requires exactly one argument",
            ));
        };
        let value = self.argument(argument, body)?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        let span = self.span(call.span.start, call.span.end);
        // A CONCRETE family value answers statically: a view IS a view and byte
        // storage is not. The disjunction below is for an erased value, whose
        // markers are the only thing left to ask.
        let value_ty = Self::expr_ty(body, value);
        if self.is_modeled_class(value_ty, smelt_stdlib::StdlibClass::TypedArray)
            || self.is_modeled_class(value_ty, smelt_stdlib::StdlibClass::ArrayBuffer)
            || self.is_modeled_class(value_ty, smelt_stdlib::StdlibClass::DataView)
        {
            // A `DataView` is a VIEW, which is the whole reason es-toolkit's
            // `isTypedArray` has to exclude it separately.
            let is_view =
                self.is_modeled_class(value_ty, smelt_stdlib::StdlibClass::TypedArray) || self.is_modeled_class(value_ty, smelt_stdlib::StdlibClass::DataView);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(is_view)),
                ty,
                span,
            })));
        }
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let mut checks = Vec::new();
            for entry in smelt_stdlib::HOST_OBJECTS
                .iter()
                .filter(|entry| entry.byte_buffer == Some(smelt_stdlib::ByteBufferRole::View))
            {
                let class = self.intern_type_name(entry.class_name);
                checks.push(body.push_expr(Expr {
                    kind: ExprKind::InstanceOf { value, class },
                    ty,
                    span,
                }));
            }
            // Combine the probes as a *balanced* `||` tree rather than a
            // left-nested chain. `||` is associative over these pure marker
            // probes, so the two shapes are semantically identical, but the
            // nesting depth is what the short-circuit control-flow lowering
            // recurses over: with thirteen modeled views (the eleven typed arrays,
            // `DataView`, and Node `Buffer`) a left-nested chain nests twelve deep
            // and overflowed the stack, where a balanced tree nests four deep.
            while checks.len() > 1 {
                let mut combined = Vec::with_capacity(checks.len().div_ceil(2));
                let mut pairs = checks.chunks_exact(2);
                for pair in &mut pairs {
                    combined.push(body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Or,
                            lhs: pair[0],
                            rhs: pair[1],
                        },
                        ty,
                        span,
                    }));
                }
                combined.extend(pairs.remainder().iter().copied());
                checks = combined;
            }
            if let Some(expr) = checks.first().copied() {
                return Ok(Some(expr));
            }
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty,
            span,
        })))
    }
}
