//! Object consts: top-level `const` object literals lowered as object-const
//! items (namespaces of functions, maps, records of literals) whose entries reads
//! can resolve statically.

use crate::{ObjectConst, ObjectConstEntry, ObjectConstEntryValue, ObjectConstValue, SmeltError};
use crate::lowering::body_arena::BodyArena;
use crate::lowering::js_text::{has_lone_surrogate, js_str_literal, js_str_text};
use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::support::source_spelling;
use indexmap::IndexMap;
use oxc::ast::ast::{Argument, ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{Body, ConstItem, ErasureReason, Expr, ExprKind, Item, Literal, Span, Type};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Return a directly written object initializer without stripping assertions.
    pub(in crate::lowering) fn direct_object_initializer<'a>(
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::ObjectExpression<'a>> {
        match expression {
            Expression::ObjectExpression(object) => Some(object),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::direct_object_initializer(&parenthesized.expression)
            }
            _ => None,
        }
    }

    /// Capture a module-level `const NAME = new Map([[key, value], ...])`
    /// initializer whose keys are static string literals into reusable
    /// object-constant metadata.
    ///
    /// A literal `Map` const is semantically a string-keyed dictionary, the same
    /// shape the object-literal const path already re-materializes at every
    /// reference site. Without this, a function that reads a module-level Map
    /// const (e.g. es-toolkit's `deburr` reading `deburrMap`) inlines an empty
    /// default dictionary because the real construction only lives in the
    /// never-called module body. Only entries with static string keys and
    /// capturable values are accepted; any other shape returns `None` and falls
    /// back to the existing runtime lowering, so this stays a general rule rather
    /// than a per-const special case.
    pub(in crate::lowering) fn map_const_from_initializer(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<ObjectConst> {
        let Expression::NewExpression(new_expr) = peel(expression, Wrappers::Transparent)
        else {
            return None;
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return None;
        };
        if !Self::is_ts_stdlib_class_name(callee.name.as_str(), smelt_stdlib::StdlibClass::Map) {
            return None;
        }
        let [Argument::ArrayExpression(array)] = new_expr.arguments.as_slice() else {
            return None;
        };
        let mut entries = Vec::new();
        for element in &array.elements {
            let ArrayExpressionElement::ArrayExpression(pair) = element else {
                return None;
            };
            let [key_element, value_element] = pair.elements.as_slice() else {
                return None;
            };
            let ArrayExpressionElement::StringLiteral(key_lit) = key_element else {
                return None;
            };
            let value_expr = value_element.as_expression()?;
            let (value, value_ty) = self.object_const_entry_value(value_expr).ok()?;
            entries.push(ObjectConstEntry {
                key: js_str_text(key_lit.value).into_owned(),
                key_units: key_lit.value.as_str().is_none().then(|| key_lit.value.encode_utf16().collect()),
                value,
                value_ty,
            });
        }
        let ty = self.object_const_type(&entries, None);
        Some(ObjectConst { entries, ty })
    }

    /// Return the object literal a module-level `const` is initialized with.
    ///
    /// Looks through the annotation wrappers, so a literal does not stop being
    /// recognized because the source wrote `as const` or `satisfies T` on it.
    pub(in crate::lowering) fn object_const_initializer<'a>(
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::ObjectExpression<'a>> {
        match expression {
            Expression::ObjectExpression(object) => Some(object),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::object_const_initializer(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::object_const_initializer(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::object_const_initializer(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::object_const_initializer(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Lower an exported object constant into namespace metadata: each member is either
    /// an existing export or a new method item lowered from a function-valued property.
    pub(in crate::lowering) fn object_namespace_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<bool, SmeltError> {
        let mut members = IndexMap::new();
        let function_hint = self.function_table_value_type(type_hint);
        if type_hint.is_some() && function_hint.is_none() {
            return Ok(false);
        }
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "exported object namespace constants do not support spread properties yet",
                ));
            };
            if object_property.computed {
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "exported object namespace constants require static data properties",
                ));
            }
            // A namespace member is a named item, which a lone surrogate cannot
            // name; such an object stays an ordinary object constant.
            if matches!(&object_property.key, PropertyKey::StringLiteral(lit) if has_lone_surrogate(lit.value)) {
                return Ok(false);
            }
            let key_text = match &object_property.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                PropertyKey::StringLiteral(lit) => js_str_text(lit.value).into_owned(),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                        "exported object namespace keys must be static string keys",
                    ));
                }
            };
            if object_property.method {
                let item = self.object_namespace_method_item(
                    name_text,
                    &key_text,
                    object_property,
                    function_hint,
                )?;
                members.insert(key_text, item);
                continue;
            }
            if matches!(
                object_property.value,
                Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
            ) {
                let item = self.object_namespace_method_item(
                    name_text,
                    &key_text,
                    object_property,
                    function_hint,
                )?;
                members.insert(key_text, item);
                continue;
            }
            let Expression::Identifier(value_ident) = &object_property.value else {
                return Ok(false);
            };
            let value_name = value_ident.name.as_str();
            let Some(item) = self.items.get(value_name).copied() else {
                return Ok(false);
            };
            members.insert(key_text, item);
        }
        self.object_namespaces
            .write().insert(name_text.to_owned(), members.clone());
        self.ctx
            .object_namespaces
            .write().insert(name_text.to_owned(), members);
        Ok(true)
    }

    /// Extract the callable value type from a string-keyed function table hint.
    pub(in crate::lowering) fn function_table_value_type(
        &self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let hint = type_hint?;
        let Type::Dict(_, value_ty) = self.ctx.krate.types.get(hint)? else {
            return None;
        };
        matches!(self.ctx.krate.types.get(*value_ty), Some(Type::Function(_))).then_some(*value_ty)
    }

    /// Lower one object-method member from an exported namespace object.
    ///
    /// Date-fns uses objects such as `lightFormatters` as plain function tables:
    /// each member is written with object method syntax and later called through
    /// `lightFormatters.y(...)`. Smelt models those members as private module
    /// functions and records the function item in namespace metadata.
    pub(in crate::lowering) fn object_namespace_method_item(
        &mut self,
        namespace: &str,
        key: &str,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let name_text = format!("{namespace}_{key}");
        match &property.value {
            Expression::FunctionExpression(function) => {
                self.function_expression_item_with_source_name(&name_text, function, type_hint)
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.arrow_function_const_declaration_with_source_name(&name_text, arrow, type_hint)
            }
            _ => Err(SmeltError::unsupported(
                self.span(property.span.start, property.span.end),
                "object namespace methods must lower from function expressions",
            )),
        }
    }

    /// Lower an exported static object constant into importable const metadata.
    pub(in crate::lowering) fn object_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let value = self.object_const_from_expression(object, type_hint)?;
        let span = self.span(object.span.start, object.span.end);
        let mut body = Body::new(None, span);
        let expr =
            self.object_const_expression(&value, object.span.start, object.span.end, &mut body);
        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name,
            ty: value.ty,
            value: expr,
            body: body_id,
            span,
        }));
        self.items.insert(name_text.to_owned(), item);
        self.ctx.export_aliases.insert(name_text.to_owned(), item);
        self.module_globals.insert(name_text.to_owned(), value.ty);
        if let Some(collection) = self.const_collection_from_object_const(&value) {
            self.consts.set_object_value_collection(name_text.to_owned(), collection.clone());
            self.ctx
                .object_value_collections
                .write().insert(name_text.to_owned(), collection);
        }
        self.consts.set_object(name_text.to_owned(), value.clone());
        self.ctx.object_consts.write().insert(name_text.to_owned(), value);
        Ok(item)
    }

    /// Lower an exported object constant whose fields are runtime expressions.
    ///
    /// Static object constants are kept as reusable metadata. Date-fns locale
    /// tables also export object constants whose fields call local helpers such
    /// as `buildFormatLongFn(...)`; those need a real HIR const body instead of
    /// primitive literal folding.
    pub(in crate::lowering) fn dynamic_object_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let span = self.span(object.span.start, object.span.end);
        let mut body = Body::new(None, span);
        let expr = self.object_expression(object, &mut body, type_hint)?;
        let ty = Self::expr_ty(&body, expr);
        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name,
            ty,
            value: expr,
            body: body_id,
            span,
        }));
        self.items.insert(name_text.to_owned(), item);
        self.ctx.export_aliases.insert(name_text.to_owned(), item);
        self.module_globals.insert(name_text.to_owned(), ty);
        if let Some(collection) = self.const_unknown_value_collection_from_object(object) {
            self.consts.set_object_value_collection(name_text.to_owned(), collection.clone());
            self.ctx
                .object_value_collections
                .write().insert(name_text.to_owned(), collection);
        }
        Ok(item)
    }

    /// Convert a static object expression into reusable literal-object metadata.
    pub(in crate::lowering) fn object_const_from_expression(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<ObjectConst, SmeltError> {
        let mut entries: Vec<ObjectConstEntry> = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object const spread properties are not lowered yet",
                ));
            };
            if object_property.computed || object_property.method {
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "object consts require static data properties",
                ));
            }
            let key = match &object_property.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                PropertyKey::StringLiteral(lit) => js_str_text(lit.value).into_owned(),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                        "object const keys must be static string keys",
                    ));
                }
            };
            let key_units = match &object_property.key {
                PropertyKey::StringLiteral(lit) if lit.value.as_str().is_none() => Some(lit.value.encode_utf16().collect()),
                _ => None,
            };
            let (value, value_ty) = self.object_const_entry_value(&object_property.value)?;
            entries.push(ObjectConstEntry {
                key,
                key_units,
                value,
                value_ty,
            });
        }
        let ty = self.object_const_type(&entries, type_hint);
        if let Some(Type::Dict(_, value_ty)) = self.ctx.krate.types.get(ty).cloned() {
            for entry in &mut entries {
                if let Some((value, value_ty)) =
                    self.retarget_object_const_value(&entry.value, entry.value_ty, value_ty)
                {
                    entry.value = value;
                    entry.value_ty = value_ty;
                }
            }
        }
        Ok(ObjectConst { entries, ty })
    }

    /// Retype one folded object-const value to the type its declaration states.
    ///
    /// The folder infers each value from its literal alone, so `[0, [['id', 0]]]`
    /// under `Record<string, [number, [string, number][]]>` would be stored as an
    /// erased `List<Unknown>`. This walks the value against the declared
    /// `target` type and returns it retyped when every part fits structurally:
    ///
    /// - a value whose inferred type already is `target` is kept;
    /// - an array literal against a tuple of the same arity becomes that tuple
    ///   (recreated as a `TupleLit`), each element retargeted to its slot;
    /// - an array literal against `List<E>` retargets every element to `E`;
    /// - an object literal against `Dict<K, V>` retargets every entry to `V`.
    ///
    /// Anything else (a literal needing a coercion, an arity mismatch) returns
    /// `None`, and the caller keeps the inferred value and type unchanged.
    fn retarget_object_const_value(
        &mut self,
        value: &ObjectConstValue,
        inferred_ty: smelt_hir::TypeId,
        target: smelt_hir::TypeId,
    ) -> Option<(ObjectConstValue, smelt_hir::TypeId)> {
        if inferred_ty == target {
            return Some((value.clone(), target));
        }
        match (value, self.ctx.krate.types.get(target).cloned()?) {
            (ObjectConstValue::List(items), Type::Tuple(slots)) if items.len() == slots.len() => {
                let items = items
                    .iter()
                    .zip(slots)
                    .map(|(item, slot)| {
                        self.retarget_object_const_value(&item.value, item.ty, slot)
                            .map(|(retyped, ty)| ObjectConstEntryValue { value: retyped, ty })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((ObjectConstValue::List(items), target))
            }
            (ObjectConstValue::List(items), Type::List(element_ty)) => {
                let items = items
                    .iter()
                    .map(|item| {
                        self.retarget_object_const_value(&item.value, item.ty, element_ty)
                            .map(|(retyped, ty)| ObjectConstEntryValue { value: retyped, ty })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((ObjectConstValue::List(items), target))
            }
            (ObjectConstValue::Object(object), Type::Dict(_, entry_ty)) => {
                let entries = object
                    .entries
                    .iter()
                    .map(|entry| {
                        self.retarget_object_const_value(&entry.value, entry.value_ty, entry_ty)
                            .map(|(retyped, value_ty)| ObjectConstEntry {
                                key: entry.key.clone(),
                                key_units: entry.key_units.clone(),
                                value: retyped,
                                value_ty,
                            })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((ObjectConstValue::Object(ObjectConst { entries, ty: target }), target))
            }
            _ => None,
        }
    }

    /// Lower one reusable static object-constant value.
    ///
    /// Primitive values are stored as literals. Function-valued lookup tables
    /// such as Remeda's `COMPARATORS` store their closure expression so later
    /// module-global reads can recreate the object with callable entries.
    pub(in crate::lowering) fn object_const_entry_value(
        &mut self,
        expression: &Expression<'_>,
    ) -> Result<(ObjectConstValue, smelt_hir::TypeId), SmeltError> {
        match expression {
            Expression::ParenthesizedExpression(parenthesized) => {
                return self.object_const_entry_value(&parenthesized.expression);
            }
            Expression::TSAsExpression(as_expr) => {
                return self.object_const_entry_value(&as_expr.expression);
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                return self.object_const_entry_value(&satisfies.expression);
            }
            Expression::TSNonNullExpression(non_null) => {
                return self.object_const_entry_value(&non_null.expression);
            }
            _ => {}
        }
        if let Expression::RegExpLiteral(literal) = expression {
            let ty = self.regexp_type();
            return Ok((
                ObjectConstValue::RegExp {
                    pattern: Self::regex_literal_pattern_text_without_flags(literal),
                    flags: literal.regex.flags.to_string(),
                },
                ty,
            ));
        }
        if matches!(expression, Expression::Identifier(identifier) if identifier.name == "undefined")
        {
            let span = self.span(expression.span().start, expression.span().end);
            let ty = self.erase_at(ErasureReason::MissingAnnotation, span, "undefined property");
            return Ok((ObjectConstValue::Literal(Literal::Undefined), ty));
        }
        if let Ok(literal) = self.literal_const_expression(expression) {
            return Ok((ObjectConstValue::Literal(literal.literal), literal.ty));
        }
        if let Expression::ArrayExpression(array) = expression {
            return self.array_object_const_entry_value(array);
        }
        if let Some(object) = Self::object_const_initializer(expression) {
            let value = self.object_const_from_expression(object, None)?;
            let ty = value.ty;
            return Ok((ObjectConstValue::Object(value), ty));
        }
        if matches!(expression, Expression::ArrowFunctionExpression(_)) {
            let span = self.span(expression.span().start, expression.span().end);
            let mut body = Body::new(None, span);
            let expr = self.expression(expression, &mut body)?;
            let value_ty = Self::expr_ty(&body, expr);
            let kind = body.expr_at(expr)
                .map(|expr| expr.kind.clone())
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        span,
                        "object const function value did not produce an expression",
                    )
                })?;
            if !matches!(kind, ExprKind::Closure(_)) {
                return Err(SmeltError::unsupported(
                    span,
                    "object const function values must lower to closures",
                ));
            }
            return Ok((ObjectConstValue::Expr(kind), value_ty));
        }
        Err(SmeltError::unsupported(
            self.span(expression.span().start, expression.span().end),
            "object const values must be literals, arrays, objects, or function expressions",
        ))
    }

    /// Lower a literal array nested inside reusable object-constant metadata.
    pub(in crate::lowering) fn array_object_const_entry_value(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
    ) -> Result<(ObjectConstValue, smelt_hir::TypeId), SmeltError> {
        let mut items = Vec::new();
        for element in &array.elements {
            let expression = match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "object const array values do not support spread or elision yet",
                    ));
                }
                ArrayExpressionElement::NumericLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::Float(literal.value)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::BigIntLiteral(literal) => {
                    let digits = Self::canonical_bigint_digits(literal.value.as_str())
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(literal.span.start, literal.span.end),
                                "bigint literal is not a decimal integer",
                            )
                        })?;
                    let ty = self.ctx.krate.types.intern(Type::BigInt);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::BigInt(digits)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::StringLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(js_str_literal(literal.value)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::BooleanLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::Bool(literal.value)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::NullLiteral(_) => {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::None),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::ArrayExpression(nested_array) => {
                    let (value, ty) = self.array_object_const_entry_value(nested_array)?;
                    items.push(ObjectConstEntryValue { value, ty });
                    continue;
                }
                ArrayExpressionElement::ObjectExpression(object) => {
                    let value = self.object_const_from_expression(object, None)?;
                    let ty = value.ty;
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Object(value),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::RegExpLiteral(literal) => {
                    let ty = self.regexp_type();
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::RegExp {
                            pattern: Self::regex_literal_pattern_text_without_flags(literal),
                            flags: literal.regex.flags.to_string(),
                        },
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::TSAsExpression(as_expr) => &as_expr.expression,
                ArrayExpressionElement::TSSatisfiesExpression(satisfies) => &satisfies.expression,
                ArrayExpressionElement::TSNonNullExpression(non_null) => &non_null.expression,
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "object const array values must be static literal values",
                    ));
                }
            };
            let (value, ty) = self.object_const_entry_value(expression)?;
            items.push(ObjectConstEntryValue { value, ty });
        }
        let item_ty = items
            .first()
            .map(|item| item.ty)
            .filter(|first_ty| items.iter().all(|item| item.ty == *first_ty))
            .unwrap_or_else(|| {
                if items.is_empty() {
                    self.erase_with(ErasureReason::MissingAnnotation, "empty const array")
                } else {
                    self.erase_with(ErasureReason::DynamicOperator, "heterogeneous const array")
                }
            });
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Ok((ObjectConstValue::List(items), ty))
    }

    /// Infer the HIR dictionary type for a static object const.
    pub(in crate::lowering) fn object_const_type(
        &mut self,
        entries: &[ObjectConstEntry],
        type_hint: Option<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        if let Some(ty) = type_hint
            && matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _)))
        {
            return ty;
        }
        // An annotated record (`const defaults: Options = { .. }`) keeps its
        // record type, so every inlined copy of the constant builds the struct.
        // Every key must be one of its fields (TypeScript's excess-property
        // check already rejects others in a fresh literal).
        if let Some(ty) = type_hint
            && let Ok(Some(fields)) = self.plain_record_fields(ty)
            && entries.iter().all(|entry| {
                fields.iter().any(|field| {
                    source_spelling(&self.ctx.krate, field.name)
                        == Some(entry.key.as_str())
                })
            })
        {
            return ty;
        }
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let first_value_ty = entries.first().map(|entry| entry.value_ty);
        let value_ty = first_value_ty
            // Values whose types differ only in their OWN generic parameters
            // (`asc: <T>(x: T, y: T) => .., desc: <T>(..) => ..`) share one
            // type; see `alpha_equivalent_types`.
            .filter(|first_ty| {
                entries
                    .iter()
                    .all(|entry| self.alpha_equivalent_types(entry.value_ty, *first_ty))
            })
            .unwrap_or_else(|| {
                if entries.is_empty() {
                    self.erase_with(ErasureReason::MissingAnnotation, "empty const object")
                } else {
                    self.erase_with(ErasureReason::DynamicOperator, "heterogeneous const object")
                }
            });
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Recreate a static object const inside the currently lowered body.
    pub(in crate::lowering) fn object_const_expression(
        &mut self,
        value: &ObjectConst,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(start, end);
        let entries = value
            .entries
            .iter()
            .map(|entry| {
                let key = body.push_expr(Expr {
                    kind: ExprKind::Literal(entry.key_literal()),
                    ty: key_ty,
                    span,
                });
                let entry_value =
                    self.object_const_value_expression(&entry.value, entry.value_ty, span, body);
                (key, entry_value)
            })
            .collect();
        body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: value.ty,
            span,
        })
    }

    /// Recreate one nested static object-constant value inside the active body.
    pub(in crate::lowering) fn object_const_value_expression(
        &mut self,
        value: &ObjectConstValue,
        ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        match value {
            ObjectConstValue::Literal(literal) => body.push_expr(Expr {
                kind: ExprKind::Literal(literal.clone()),
                ty,
                span,
            }),
            ObjectConstValue::RegExp { pattern, flags } => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(pattern.clone())),
                    ty: string_ty,
                    span,
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(flags.clone())),
                    ty: string_ty,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty,
                    span,
                })
            }
            ObjectConstValue::List(items) => {
                let values = items
                    .iter()
                    .map(|item| {
                        self.object_const_value_expression(&item.value, item.ty, span, body)
                    })
                    .collect();
                // An array literal the declaration types as a tuple (see
                // `retarget_object_const_value`) recreates as that tuple.
                let kind = if matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                    ExprKind::TupleLit(values)
                } else {
                    ExprKind::ListLit(values)
                };
                body.push_expr(Expr { kind, ty, span })
            }
            ObjectConstValue::Object(object) => {
                self.object_const_expression(object, span.start, span.end, body)
            }
            ObjectConstValue::Expr(kind) => body.push_expr(Expr {
                kind: kind.clone(),
                ty,
                span,
            }),
        }
    }
}
