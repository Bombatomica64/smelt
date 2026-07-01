//! Property-key, interface-heritage, and HIR lookup helpers.
//!
//! Covers property-key symbol/index lowering, `implements`/`extends` clause
//! resolution, interface satisfaction checks, and small accessors for looking
//! up expression, local, and item types.

use crate::lowering::{ModuleBuilder, field_type_satisfies};
use crate::SmeltError;
use oxc::ast::ast::{Expression, PropertyKey, TSTypeName};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, Interface, Item, Literal, Type};

impl ModuleBuilder<'_> {
    /// Convert a TypeScript property key into the interned HIR symbol it names.
    pub(in crate::lowering) fn property_key_symbol(
        &mut self,
        key: &PropertyKey<'_>,
    ) -> Result<smelt_hir::Symbol, SmeltError> {
        match key {
            PropertyKey::StaticIdentifier(ident) => {
                Ok(self.intern_source_name(ident.name.as_str()))
            }
            PropertyKey::PrivateIdentifier(ident) => {
                Ok(self.intern_source_name(ident.name.as_str()))
            }
            PropertyKey::StringLiteral(lit) => Ok(self.intern_source_name(lit.value.as_str())),
            PropertyKey::NumericLiteral(lit) => {
                // JavaScript coerces numeric property keys to their string form
                // (`{ 0: x }` names the member `"0"`). Prefer the raw spelling
                // when available so canonical integer keys round-trip cleanly.
                let name = lit
                    .raw
                    .as_ref().map_or_else(|| Self::numeric_property_key_name(lit.value), ToString::to_string);
                Ok(self.intern_source_name(&name))
            }
            _ => Err(SmeltError::unsupported(
                self.span(key.span().start, key.span().end),
                "property names must be static identifiers or string literals",
            )),
        }
    }

    /// Render a numeric property-key value as the string member name JavaScript
    /// would use when no raw source spelling is available (e.g. `0` -> "0").
    pub(in crate::lowering) fn numeric_property_key_name(value: f64) -> String {
        if value.fract() == 0.0 && value.is_finite() {
            // Whole, finite key: render without a fractional part (`0` -> "0",
            // `5` -> "5") via precision formatting, avoiding a lossy f64->int cast.
            format!("{value:.0}")
        } else {
            format!("{value}")
        }
    }

    /// Lower a computed property key to a HIR index expression.
    pub(in crate::lowering) fn property_key_index_expression(
        &mut self,
        key: &PropertyKey<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match key {
            PropertyKey::Identifier(identifier) => self.identifier_expression(
                identifier.name.as_str(),
                identifier.span.start,
                identifier.span.end,
                body,
            ),
            PropertyKey::StaticIdentifier(identifier) => self
                .identifier_expression(
                    identifier.name.as_str(),
                    identifier.span.start,
                    identifier.span.end,
                    body,
                ),
            PropertyKey::StringLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(literal.value.to_string())),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            PropertyKey::NumericLiteral(literal) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(literal.value)),
                    ty,
                    span: self.span(literal.span.start, literal.span.end),
                }))
            }
            PropertyKey::StaticMemberExpression(member) => self.static_member(member, body),
            PropertyKey::ComputedMemberExpression(member) => self.computed_member(member, body),
            PropertyKey::CallExpression(call) => self.call_expression(call, body),
            PropertyKey::LogicalExpression(logical) => self.logical_expression(logical, body),
            PropertyKey::BinaryExpression(binary) => self.binary_expression(binary, body),
            PropertyKey::ConditionalExpression(conditional) => {
                self.conditional_expression(conditional, body, None)
            }
            PropertyKey::TSAsExpression(assertion) => self.expression(&assertion.expression, body),
            PropertyKey::TSSatisfiesExpression(assertion) => {
                self.expression(&assertion.expression, body)
            }
            PropertyKey::TSTypeAssertion(assertion) => self.expression(&assertion.expression, body),
            PropertyKey::TSNonNullExpression(assertion) => {
                self.expression(&assertion.expression, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(key.span().start, key.span().end),
                "computed property keys must be identifier, member, string, or numeric expressions",
            )),
        }
    }

    /// Resolve a class `implements` clause entry to an interface symbol.
    ///
    /// Qualified external interface references are opaque to Smelt's local
    /// interface validator, so they are ignored instead of blocking class
    /// lowering. Direct identifiers are still validated against local
    /// interfaces.
    pub(in crate::lowering) fn implements_symbol(
        &mut self,
        item: &oxc::ast::ast::TSClassImplements<'_>,
    ) -> Result<Option<smelt_hir::Symbol>, SmeltError> {
        if item.type_arguments.is_some() {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "generic implements clauses are not lowered yet",
            ));
        }
        let TSTypeName::IdentifierReference(name) = &item.expression else {
            return Ok(None);
        };
        Ok(Some(self.intern_type_name(name.name.as_str())))
    }

    /// Convert an interface heritage clause to the referenced interface symbol and arguments.
    pub(in crate::lowering) fn interface_heritage(
        &mut self,
        item: &oxc::ast::ast::TSInterfaceHeritage<'_>,
    ) -> Result<(smelt_hir::Symbol, Vec<smelt_hir::TypeId>), SmeltError> {
        let name_text = match &item.expression {
            Expression::Identifier(name) => name.name.to_string(),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return Err(SmeltError::unsupported(
                        self.span(item.span.start, item.span.end),
                        "qualified interface inheritance is not lowered yet",
                    ));
                };
                format!("{}.{}", object.name, member.property.name)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(item.span.start, item.span.end),
                    "qualified interface inheritance is not lowered yet",
                ));
            }
        };
        let args = item
            .type_arguments
            .as_ref()
            .map(|args| {
                args.params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok((self.intern_type_name(&name_text), args))
    }

    /// Find the latest previously lowered interface by symbol.
    ///
    /// Manifests may include generated `.d.ts` files before their source `.ts`
    /// counterparts. Looking from the end keeps source declarations from being
    /// shadowed by earlier, weaker declarations while preserving ordinary
    /// dependency-first lookup.
    pub(in crate::lowering) fn find_interface(&self, name: smelt_hir::Symbol) -> Option<&Interface> {
        self.ctx.krate.items.iter().rev().find_map(|item| {
            if let Item::Interface(interface) = item {
                if interface.name == name {
                    return Some(interface);
                }
            }
            None
        })
    }

    /// Find the latest previously lowered type alias by symbol.
    ///
    /// This mirrors interface lookup so `.ts` aliases can refine generated
    /// declaration-file aliases when both appear in one manifest.
    pub(in crate::lowering) fn find_type_alias(&self, name: smelt_hir::Symbol) -> Option<&smelt_hir::TypeAlias> {
        self.ctx.krate.items.iter().rev().find_map(|item| {
            if let Item::TypeAlias(alias) = item
                && alias.name == name
            {
                return Some(alias);
            }
            None
        })
    }

    /// Validate that a lowered class satisfies all declared interfaces.
    pub(in crate::lowering) fn validate_implements(&self, class_item: smelt_hir::ItemId) -> Result<(), SmeltError> {
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(());
        };
        for interface_name in &class.implements {
            let interface = self
                .ctx
                .krate
                .items
                .iter()
                .find_map(|item| {
                    if let Item::Interface(interface) = item
                        && interface.name == *interface_name
                    {
                        return Some(interface);
                    }
                    None
                })
                .ok_or_else(|| {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(*interface_name)
                        .unwrap_or("<unknown>");
                    SmeltError::unsupported(
                        class.span,
                        format!("implemented interface `{name}` is not declared"),
                    )
                })?;
            for required in &interface.fields {
                let Some(actual) = class
                    .fields
                    .iter()
                    .find(|field| field.name == required.name)
                else {
                    if required.optional {
                        continue;
                    }
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        required.span,
                        format!("class is missing implemented interface field `{name}`"),
                    ));
                };
                if !field_type_satisfies(&self.ctx.krate, actual.ty, required) {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        actual.span,
                        format!("implemented interface field `{name}` has a mismatched type"),
                    ));
                }
            }
            for required in &interface.methods {
                let Some(actual_item) = class.methods.iter().find(|method_item| {
                    matches!(self.item_ref(**method_item), Item::Function(function) if function.name == required.name)
                }) else {
                    let name = self.ctx.krate.symbols.get(required.name).unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(required.span, format!("class is missing implemented interface method `{name}`")));
                };
                let Item::Function(actual) = self.item_ref(*actual_item) else {
                    return Err(SmeltError::unsupported(
                        required.span,
                        "implemented interface method has an unexpected item kind",
                    ));
                };
                let actual_params = actual
                    .params
                    .iter()
                    .filter(|param| self.ctx.krate.symbols.get(param.name) != Some("this"))
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                let required_params = required
                    .params
                    .iter()
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                if actual_params != required_params || actual.return_ty != required.return_ty {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        actual.span,
                        format!("implemented interface method `{name}` has a mismatched signature"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Look up the type of an existing expression.
    pub(in crate::lowering) fn expr_ty(body: &Body, expr: smelt_hir::ExprId) -> smelt_hir::TypeId {
        let index = usize::try_from(expr.0).expect("expr id should fit into usize");
        body.exprs
            .get(index)
            .expect("expr id should point to an existing expression")
            .ty
    }

    /// Look up the type of an existing local.
    pub(in crate::lowering) fn local_ty(body: &Body, local: smelt_hir::LocalId) -> smelt_hir::TypeId {
        let index = usize::try_from(local.0).expect("local id should fit into usize");
        body.locals
            .get(index)
            .expect("local id should point to an existing local")
            .ty
    }

    /// Look up the type of a local that may not exist in this `body`.
    ///
    /// The lexical `locals` name map can outlive the body that actually owns a
    /// binding — for example when a default-parameter initializer registers a
    /// name that the enclosing function body never materializes as a local. In
    /// those cases the recorded [`LocalId`] points past `body.locals`, so a
    /// plain [`Self::local_ty`] would panic. Callers performing best-effort type
    /// narrowing use this checked variant and treat a missing local as "no
    /// narrowing information available".
    pub(in crate::lowering) fn local_ty_checked(body: &Body, local: smelt_hir::LocalId) -> Option<smelt_hir::TypeId> {
        let index = usize::try_from(local.0).ok()?;
        body.locals.get(index).map(|local| local.ty)
    }

    /// Look up a lowered item by id.
    pub(in crate::lowering) fn item_ref(&self, item: smelt_hir::ItemId) -> &Item {
        let index = usize::try_from(item.0).expect("item id should fit into usize");
        self.ctx
            .krate
            .items
            .get(index)
            .expect("item id should point to an existing item")
    }
}
