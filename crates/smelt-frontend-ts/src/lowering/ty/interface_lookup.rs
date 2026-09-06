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
            // A computed key such as `[SOME_CONST]`, `[MyEnum.Member]`, or a
            // well-known `[Symbol.iterator]` names a *static* member once the
            // constant/enum/symbol is resolved. Fold it to that member name so
            // it lowers exactly like the equivalent spelled-out key instead of
            // being rejected. Genuinely dynamic keys still fail below and stay
            // on the explicit runtime-keyed path.
            _ => {
                if let Some((name, is_symbol)) = self.resolve_static_computed_key_name(key) {
                    // Well-known symbol keys use a stable synthetic spelling
                    // (`__smelt_symbol_iterator`) that member access already
                    // reads, so intern it verbatim; folded string/numeric keys
                    // follow the ordinary source-name interning path.
                    return Ok(if is_symbol {
                        self.intern_exact_source_name(&name)
                    } else {
                        self.intern_source_name(&name)
                    });
                }
                Err(SmeltError::unsupported(
                    self.span(key.span().start, key.span().end),
                    "property names must be static identifiers or string literals",
                ))
            }
        }
    }

    /// Resolve a computed property key to its static string member name.
    ///
    /// Returns `Some((name, is_symbol_backed))` when the key is a
    /// statically-resolvable computed key:
    /// * a const reference whose value folds to a string/number literal
    ///   (`const K = "id"; { [K]: v }`),
    /// * an enum member (`enum E { A = "a" }; { [E.A]: v }`) or a foldable
    ///   `Number`/`Math` numeric constant,
    /// * a well-known symbol member (`[Symbol.iterator]`,
    ///   `[Symbol.asyncIterator]`, …), which maps to a stable synthetic member
    ///   spelling that member access already understands, or
    /// * a `Symbol.for(<description>)` registry symbol — spelled inline
    ///   (`[Symbol.for("k")]`), aliased to a `const` (`[matcher]`), or read
    ///   through a namespace import (`[symbols.override]`) — which folds to a
    ///   stable synthetic spelling derived from the registry description because
    ///   registry symbols are globally interned (issue #115).
    ///
    /// The boolean is `true` when the resolved name is a synthetic symbol key
    /// (interned verbatim) rather than an ordinary source-name-folded key.
    ///
    /// Returns `None` for genuinely dynamic keys (arbitrary expressions, unique
    /// `Symbol(...)` brands, boolean/null constants), leaving them on the
    /// explicit runtime-keyed path.
    pub(in crate::lowering) fn resolve_static_computed_key_name(
        &mut self,
        key: &PropertyKey<'_>,
    ) -> Option<(String, bool)> {
        match key {
            // A well-known / registry `Symbol.<name>` or `Symbol.for(...)` member
            // key names a stable synthetic member; otherwise a foldable const
            // member (enum member, Number/Math constant) resolves normally.
            PropertyKey::StaticMemberExpression(member) => {
                if let Some(symbol_key) = self.symbol_member_key(member) {
                    return Some((symbol_key, true));
                }
                let literal = self.member_literal_const_expression(member).ok()?;
                literal.computed_member_name().map(|name| (name, false))
            }
            // An inline `[Symbol.for("k")]` call key folds to its registry key.
            PropertyKey::CallExpression(call) => {
                Self::symbol_for_call_key(call).map(|symbol_key| (symbol_key, true))
            }
            // A bare identifier reference folds to its const value when known;
            // a `Symbol.for(...)`-aliased const yields a synthetic symbol key.
            PropertyKey::Identifier(identifier) => self
                .resolve_const_literal_by_name(identifier.name.as_str())
                .and_then(|literal| Self::const_computed_key_name(&literal)),
            // Erased type-level wrappers around any of the above still name the
            // inner static key (`[K as const]`, `[(K)]`, `[K!]`).
            PropertyKey::ParenthesizedExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            PropertyKey::TSAsExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            PropertyKey::TSSatisfiesExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            PropertyKey::TSNonNullExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            _ => None,
        }
    }

    /// Resolve a computed-key expression (after unwrapping erased assertions) to
    /// its static string member name, mirroring
    /// [`Self::resolve_static_computed_key_name`] for bare `Expression`s.
    pub(in crate::lowering) fn resolve_static_computed_key_name_expr(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<(String, bool)> {
        match expression {
            Expression::StaticMemberExpression(member) => {
                if let Some(symbol_key) = self.symbol_member_key(member) {
                    return Some((symbol_key, true));
                }
                let literal = self.member_literal_const_expression(member).ok()?;
                literal.computed_member_name().map(|name| (name, false))
            }
            Expression::CallExpression(call) => {
                Self::symbol_for_call_key(call).map(|symbol_key| (symbol_key, true))
            }
            Expression::Identifier(identifier) => self
                .resolve_const_literal_by_name(identifier.name.as_str())
                .and_then(|literal| Self::const_computed_key_name(&literal)),
            Expression::StringLiteral(literal) => Some((literal.value.to_string(), false)),
            Expression::ParenthesizedExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            Expression::TSAsExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            Expression::TSSatisfiesExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            Expression::TSNonNullExpression(inner) => {
                self.resolve_static_computed_key_name_expr(&inner.expression)
            }
            _ => None,
        }
    }

    /// Resolve a member expression that names a symbol into its stable synthetic
    /// member key.
    ///
    /// Handles two shapes:
    /// * a well-known symbol access (`Symbol.iterator`, `Symbol.asyncIterator`, …)
    ///   folds to the per-name synthetic key (see
    ///   [`crate::lowering::ty::computed_key_symbols::well_known_symbol_key`]),
    ///   and
    /// * a namespace-member alias of a `Symbol.for(...)` registry const
    ///   (`import * as s from "..."; [s.override]`) folds to the registry key by
    ///   resolving the const behind the qualified name.
    ///
    /// Returns `None` when the member is not a modeled symbol key, leaving the
    /// caller to try const/enum folding or reject the key as dynamic.
    fn symbol_member_key(
        &self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
    ) -> Option<String> {
        if let Expression::Identifier(object) = &member.object
            && object.name == "Symbol"
        {
            return crate::lowering::ty::computed_key_symbols::well_known_symbol_key(
                member.property.name.as_str(),
            );
        }
        // `namespace.member` alias of an imported `Symbol.for(...)` const: resolve
        // the const behind the qualified name and fold its registry description.
        if let Expression::Identifier(object) = &member.object {
            let qualified = format!("{}.{}", object.name, member.property.name);
            if let Some(name) = self
                .resolve_const_literal_by_name(&qualified)
                .and_then(|literal| literal.symbol_registry_name())
            {
                return Some(name);
            }
        }
        None
    }

    /// Resolve an inline `Symbol.for(<string literal>)` call key to its registry
    /// member key.
    ///
    /// Reuses [`Self::symbol_for_call_description`] so the inline-key path and the
    /// const-folding path agree on what a foldable registry call looks like. Only
    /// the registry form with a string-literal description folds; a unique
    /// `Symbol(...)` call or a non-literal description has no stable static
    /// spelling and returns `None`.
    fn symbol_for_call_key(call: &oxc::ast::ast::CallExpression<'_>) -> Option<String> {
        Self::symbol_for_call_description(call)
            .map(crate::lowering::ty::computed_key_symbols::registry_symbol_key)
    }

    /// Return whether a computed property key resolves to a static member name.
    ///
    /// Combines the plain static-key check (`is_static_property_key`) with the
    /// const/enum/symbol folding of [`Self::resolve_static_computed_key_name`],
    /// so class/interface member gates route resolvable computed keys through the
    /// named-member path instead of rejecting them as dynamic.
    pub(in crate::lowering) fn is_resolvable_property_key(&mut self, key: &PropertyKey<'_>) -> bool {
        crate::lowering::support::is_static_property_key(key)
            || self.resolve_static_computed_key_name(key).is_some()
    }

    /// Resolve a const binding's folded literal to the member key it names.
    ///
    /// Shared by the two identifier arms of
    /// [`Self::resolve_static_computed_key_name`] and its expression twin so a
    /// const used as a class-member key and the same const used as a computed
    /// READ (`c[KEY]`) always answer the same name.
    ///
    /// The unique-symbol fallback lives here rather than in
    /// `computed_member_name` because it is only sound for a binding that is
    /// evaluated once: `resolve_const_literal_by_name` answers from the module
    /// const tables, so reaching this point already means the initializer ran
    /// once. A `Symbol()` evaluated per call keeps the runtime-keyed path.
    fn const_computed_key_name(literal: &crate::lowering::ConstLiteral) -> Option<(String, bool)> {
        if let Some(name) = literal.symbol_registry_name() {
            return Some((name, true));
        }
        if let Some(name) = literal.unique_symbol_member_name() {
            return Some((name, true));
        }
        literal.computed_member_name().map(|name| (name, false))
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

    /// Resolve a class `implements` clause entry to a local interface reference.
    ///
    /// Qualified external interface references are opaque to Smelt's local
    /// interface validator, as are direct imported or ambient interfaces that
    /// have no locally lowered structural definition. Local references retain
    /// their type arguments so validation can instantiate generic requirements.
    pub(in crate::lowering) fn implements_reference(
        &mut self,
        item: &oxc::ast::ast::TSClassImplements<'_>,
    ) -> Result<Option<smelt_hir::InterfaceHeritage>, SmeltError> {
        let TSTypeName::IdentifierReference(name) = &item.expression else {
            return Ok(None);
        };
        let local_name = name.name.as_str();
        let qualified_name = self.qualified_type_declaration_name(local_name);
        let parent = self.intern_type_name(&qualified_name);
        if !self.interfaces.resolves_locally(local_name, parent) {
            return Ok(None);
        }
        let args = item
            .type_arguments
            .as_ref()
            .map(|arguments| {
                arguments
                    .params
                    .iter()
                    .map(|argument| self.ts_type_to_hir(argument))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Some(smelt_hir::InterfaceHeritage { parent, args }))
    }

    /// Convert an interface heritage clause to the referenced interface symbol and arguments.
    pub(in crate::lowering) fn interface_heritage(
        &mut self,
        item: &oxc::ast::ast::TSInterfaceHeritage<'_>,
    ) -> Result<(smelt_hir::Symbol, Vec<smelt_hir::TypeId>), SmeltError> {
        // Since oxc 0.147 an interface's heritage carries a `TSTypeName` rather
        // than an `Expression`, so `A.B` is a `QualifiedName` instead of a
        // static member expression.
        let name_text = match &item.type_name {
            TSTypeName::IdentifierReference(name) => name.name.to_string(),
            TSTypeName::QualifiedName(qualified) => {
                let TSTypeName::IdentifierReference(object) = &qualified.left else {
                    return Err(SmeltError::unsupported(
                        self.span(item.span.start, item.span.end),
                        "qualified interface inheritance is not lowered yet",
                    ));
                };
                format!("{}.{}", object.name, qualified.right.name)
            }
            TSTypeName::ThisExpression(_) => {
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
    pub(in crate::lowering) fn validate_implements(&mut self, class_item: smelt_hir::ItemId) -> Result<(), SmeltError> {
        let Item::Class(class) = self.item_ref(class_item).clone() else {
            return Ok(());
        };
        for implemented in &class.implements {
            if !self.interfaces.is_lowered_locally(implemented.parent) {
                continue;
            }
            let Some(interface) = self.find_interface(implemented.parent).cloned() else {
                continue;
            };
            let substitutions = self.type_argument_substitution(
                &interface.type_params,
                &implemented.args,
                class.span,
            )?;
            let required_fields = self.substituted_fields(&interface.fields, &substitutions);
            let required_methods = self.substituted_methods(&interface.methods, &substitutions);
            for required in &required_fields {
                // Method signatures are also recorded as function-typed fields
                // so interface-typed values expose callable members. A class
                // satisfies such a requirement with a real method, which the
                // method loop below validates, so skip the field check here to
                // avoid demanding a matching data field the class never stores.
                if required_methods
                    .iter()
                    .any(|method| method.name == required.name)
                {
                    continue;
                }
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
            for required in &required_methods {
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
                let parameters_match = actual_params.len() == required_params.len()
                    && actual_params.iter().zip(&required_params).all(
                        |(actual_param, required_param)| {
                            self.type_assignable_to(*required_param, *actual_param)
                        },
                    );
                let return_matches =
                    self.type_assignable_to(actual.return_ty, required.return_ty);
                if !parameters_match || !return_matches {
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
    pub(in crate::lowering) fn local_ty_checked(
        body: &Body,
        local: smelt_hir::LocalId,
    ) -> Option<smelt_hir::TypeId> {
        let index = usize::try_from(local.0).ok()?;
        body.locals.get(index).map(|decl| decl.ty)
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
