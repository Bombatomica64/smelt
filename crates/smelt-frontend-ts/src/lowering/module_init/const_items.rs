//! Top-level `const` declarations lowered as const items, and the initializer
//! shapes that reference another module binding.

use crate::SmeltError;
use crate::lowering::peel::{Wrappers, peel};
use oxc::ast::ast::{BindingPattern, Expression};
use smelt_hir::{Body, ConstItem, Expr, ExprKind, Item};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Lower exported const declarations into importable HIR items by initializer shape.
    pub(in crate::lowering) fn const_item_declarations(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        // `export declare const x: T` is an ambient re-export of a host binding:
        // it emits no const item, because there is no initializer to emit.
        if decl.declare {
            return Ok(Vec::new());
        }
        // An exported `let`/`var` is written only by its own module (an
        // import is a read-only binding), so a written one is a lifted module
        // slot (below) and an unwritten one is, observably, a `const`.
        let mut items = Vec::new();
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                return Err(SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "exported const destructuring is not lowered yet",
                ));
            };
            let init = declarator.init.as_ref().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "exported const declarations require an initializer",
                )
            })?;
            // An exported const lifted to a module slot (a container written
            // through, or one read before its declaration) is its slot: its
            // initializer runs once, as the slot's lazy initializer, and it
            // emits no const item.
            if self.is_lifted_global_declarator(binding.name.as_str(), binding.span) {
                self.lower_pending_mutable_global_init(binding.name.as_str(), init)?;
                continue;
            }
            let type_hint = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            // `export const toolkit = ((v) => v) as Toolkit;` — peel
            // transparent TS wrappers (parens, `as`, `satisfies`, `!`) so the
            // arrow/function-initializer dispatch below sees the value
            // expression, matching how the literal folder already unwraps.
            let init = peel(init, Wrappers::Transparent);
            if let Some(object) = Self::direct_object_initializer(init) {
                if self.object_namespace_const_declaration(
                    binding.name.as_str(),
                    object,
                    type_hint,
                )? {
                    continue;
                }
            }
            if let Some(object) = Self::object_const_initializer(init) {
                let item = if let Ok(item) =
                    self.object_const_declaration(binding.name.as_str(), object, type_hint)
                {
                    item
                } else {
                    if let Some(collection) =
                        self.const_unknown_value_collection_from_object(object)
                    {
                        self.consts.set_object_value_collection(binding.name.as_str().to_owned(), collection.clone());
                        self.publish_export(|ctx| &mut ctx.object_value_collections, binding.name.as_str().to_owned(), collection);
                    }
                    self.dynamic_object_const_declaration(binding.name.as_str(), object, type_hint)?
                };
                items.push(item);
                continue;
            }
            if let Expression::ArrowFunctionExpression(arrow) = init {
                let item =
                    self.arrow_function_const_declaration(binding.name.as_str(), arrow, type_hint)?;
                items.push(item);
                continue;
            }
            if let Expression::FunctionExpression(function) = init
                && function.body.is_some()
            {
                // `export const stub = function () { ... }` binds an anonymous
                // function expression to a module name. It is semantically the
                // same as `function stub() { ... }`, so lower it through the
                // shared named-function path rather than the literal folder.
                let item =
                    self.function_declaration_named(function, binding.name.as_str())?;
                items.push(item);
                continue;
            }
            if self.is_module_global_array_initializer(init)
                && self.literal_const_expression(init).is_err()
            {
                let item = self.push_expression_const_item(binding, init)?;
                items.push(item);
                continue;
            }
            if let Some(identifier) = Self::imported_value_identifier(init)
                && self.imports.is_value(identifier.name.as_str())
            {
                let item = self.push_expression_const_item(binding, init)?;
                items.push(item);
                continue;
            }
            if matches!(
                init,
                Expression::CallExpression(_) | Expression::NewExpression(_)
            ) && self.literal_const_expression(init).is_err()
            {
                let item = self.push_expression_const_item(binding, init)?;
                items.push(item);
                continue;
            }
            // A bare reference to another module-level value (`export const A = b;`
            // or `export const A = ns.member;`) is not a foldable primitive when the
            // referenced value is a collection, object, or other non-literal const.
            // Route it through the same general expression lowering that
            // non-exported consts and array/call initializers already use, so the
            // referenced module const is inlined instead of erroring as an
            // unresolved literal. Literal-foldable references (numeric constants,
            // const-from-const primitives) still fold below to keep those regressions.
            if self.literal_const_expression(init).is_err()
                && self.is_resolvable_module_reference(init)
            {
                let item = self.push_expression_const_item(binding, init)?;
                items.push(item);
                continue;
            }
            // A member access that is neither a foldable numeric constant nor a
            // module-local reference (`export const slice = Array.prototype.slice;`,
            // `export const arrayProto = Array.prototype;`) is a statically
            // resolvable member expression on a builtin/global root. General
            // expression lowering already resolves such members — builtin
            // namespace members lower to their concrete or erased-`Unknown`
            // value, so route them through the same expression path instead of
            // rejecting everything but well-known Number/Math constants. Genuine
            // dynamic boundaries (a prototype object, a bound builtin method)
            // stay explicit as the `Unknown` value the shared lowering produces.
            if Self::is_member_access_initializer(init)
                && self.literal_const_expression(init).is_err()
            {
                let item = self.push_expression_const_item(binding, init)?;
                items.push(item);
                continue;
            }
            // `export const enc = encodeURIComponent;` — a bare reference to a
            // JavaScript GLOBAL, aliasing it under a shorter name (Hono's
            // `utils/url.ts` does exactly this, with the comment
            // "`decodeURIComponent` is a long name"). This is the identifier
            // twin of the member-access arm above: general expression lowering
            // already produces the global's value (a builtin function value
            // closure, a namespace, or the erased `Unknown` at a real dynamic
            // boundary), so route it there instead of demanding a literal. The
            // literal folder cannot help — a function is not a literal — and
            // rejecting left the module unbuildable.
            if let Some(identifier) = Self::bare_identifier_initializer(init)
                && smelt_stdlib::globals::is_javascript_global_builtin(identifier)
                && !self.scope.is_bound(identifier)
                && !self.items.contains_key(identifier)
                && self.literal_const_expression(init).is_err()
            {
                let item = self.push_expression_const_item(binding, init)?;
                items.push(item);
                continue;
            }
            let value = match self.literal_const_expression(init) {
                Ok(value) => value,
                Err(error) if self.is_known_non_importable_exported_const(init) => {
                    drop(error);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let span = self.span(binding.span.start, binding.span.end);
            let mut body = Body::new(None, span);
            let expr = body.push_expr(Expr {
                kind: ExprKind::Literal(value.literal.clone()),
                ty: value.ty,
                span,
            });
            let body_id = self.ctx.krate.push_body(body);
            let name_text = binding.name.as_str();
            let name = self.intern_source_name(name_text);
            let item = self.ctx.krate.push_item(Item::Const(ConstItem {
                name,
                ty: value.ty,
                value: expr,
                body: body_id,
                span,
            }));
            self.items.insert(name_text.to_owned(), item);
            self.publish_export_alias(name_text.to_owned(), item);
            self.consts.set_literal(name_text.to_owned(), value);
            items.push(item);
        }
        Ok(items)
    }

    /// Lower an exported-const initializer through general expression lowering
    /// and register the resulting HIR `Item::Const`.
    ///
    /// Initializers that are not foldable primitive literals (arrays, `Set`/`Map`
    /// constructors, calls, imported-value aliases, and references to other
    /// module-level consts) still bind to a real const item. They are lowered by
    /// the same `self.expression` path a non-exported const uses, so a referenced
    /// module const is inlined rather than erroring as an unresolved literal. The
    /// item is registered under its source name in `items`, `export_aliases`, and
    /// `module_globals`, and any recoverable literal-array/set collection shape is
    /// recorded so nested bodies can inline the values. Returns the new item id.
    pub(in crate::lowering) fn push_expression_const_item(
        &mut self,
        binding: &oxc::ast::ast::BindingIdentifier<'_>,
        init: &Expression<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let span = self.span(binding.span.start, binding.span.end);
        let mut body = Body::new(None, span);
        let expr = self.expression(init, &mut body)?;
        let ty = Self::expr_ty(&body, expr);
        let body_id = self.ctx.krate.push_body(body);
        let name_text = binding.name.as_str();
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name,
            ty,
            value: expr,
            body: body_id,
            span,
        }));
        self.items.insert(name_text.to_owned(), item);
        self.publish_export_alias(name_text.to_owned(), item);
        self.module_globals.insert(name_text.to_owned(), ty);
        if let Some(collection) = self.const_collection_from_initializer(init, ty) {
            self.consts.set_collection(name_text.to_owned(), collection.clone());
            self.publish_export(|ctx| &mut ctx.const_collections, name_text.to_owned(), collection);
        }
        Ok(item)
    }

    /// Return whether an exported-const initializer references another
    /// module-level value that general expression lowering can resolve.
    ///
    /// Recognizes a bare identifier (`export const A = b;`) or a static member
    /// access (`export const A = ns.member;`), through the usual `as` / `satisfies`
    /// / non-null / parenthesized wrappers, whose root identifier names a value
    /// Smelt has already lowered in this module: a HIR item, a const object, a
    /// const collection, a const literal, or a typed module global. Such
    /// references inline the referenced const through the general expression path
    /// instead of requiring a foldable primitive literal. Bare identifiers that
    /// resolve to a builtin/global alias (`Number`, `Math`, `globalThis`, ...) are
    /// deliberately excluded so numeric-constant folding and the well-known-member
    /// path keep owning those shapes.
    pub(in crate::lowering) fn is_resolvable_module_reference(&self, init: &Expression<'_>) -> bool {
        let Some(root) = Self::module_reference_root_name(init) else {
            return false;
        };
        self.items.contains_key(root)
            || self.consts.is_folded_const(root)
            || self.module_globals.contains_key(root)
    }

    /// The name of a bare-identifier initializer, unwrapping the type-level
    /// wrappers that never change the value (`as`, `satisfies`, `!`,
    /// parentheses).
    ///
    /// Returns `None` for anything that is not just a name, so a caller can ask
    /// "is this initializer an alias to some other binding?" without
    /// re-implementing the unwrapping each time.
    pub(in crate::lowering) fn bare_identifier_initializer<'a>(init: &'a Expression<'a>) -> Option<&'a str> {
        match init {
            Expression::Identifier(identifier) => Some(identifier.name.as_str()),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::bare_identifier_initializer(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::bare_identifier_initializer(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::bare_identifier_initializer(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::bare_identifier_initializer(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Return whether an initializer reads a MEMBER of something.
    ///
    /// Looks through the wrappers that do not change what the expression reads
    /// -- parentheses, `as`, `satisfies` -- so a member read does not stop
    /// being recognized because the source annotated it.
    pub(in crate::lowering) fn is_member_access_initializer(init: &Expression<'_>) -> bool {
        match init {
            Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_) => true,
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::is_member_access_initializer(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::is_member_access_initializer(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::is_member_access_initializer(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::is_member_access_initializer(&non_null.expression)
            }
            _ => false,
        }
    }

    /// Return the root identifier name of an identifier or static-member
    /// initializer, unwrapping `as` / `satisfies` / non-null / parenthesized
    /// expressions. Returns `None` for any other initializer shape.
    pub(in crate::lowering) fn module_reference_root_name<'a>(init: &'a Expression<'a>) -> Option<&'a str> {
        match init {
            Expression::Identifier(identifier) => Some(identifier.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                Self::module_reference_root_name(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                Self::module_reference_root_name(&member.object)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::module_reference_root_name(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::module_reference_root_name(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::module_reference_root_name(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::module_reference_root_name(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Return the identifier behind a top-level const initializer that aliases an imported value.
    pub(in crate::lowering) fn imported_value_identifier<'a>(
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::IdentifierReference<'a>> {
        match expression {
            Expression::Identifier(identifier) => Some(identifier),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::imported_value_identifier(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::imported_value_identifier(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::imported_value_identifier(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::imported_value_identifier(&non_null.expression)
            }
            _ => None,
        }
    }
}
