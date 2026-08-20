//! Static properties assigned onto a function declaration.
//!
//! JavaScript functions are objects, so a module can hang values off one. It is a
//! common way to publish a sentinel or a configuration hook next to the function
//! that consumes it, and es-toolkit uses it nine times:
//!
//! ```ts
//! export function partial(func, ...partialArgs) {
//!   return partialImpl(func, partial.placeholder, ...partialArgs);
//! }
//! partial.placeholder = Symbol('compat.partial.placeholder');
//! ```
//!
//! Also `partialRight.placeholder`, `curry.placeholder`,
//! `curryRight.placeholder`, `bind.placeholder`, `bindKey.placeholder` and
//! `memoize.Cache = Map`.
//!
//! # What went wrong
//!
//! The assignment lowered into the module-init function, which nothing ever calls,
//! and the *target* was dropped outright — the generated init body binds the
//! right-hand side to a local and discards it:
//!
//! ```ignore
//! pub(crate) fn curry_1966() -> () {
//!     let curry_placeholder: SmeltUnknown = SmeltUnknown::Symbol("Symbol(curry.placeholder)@10319");
//!     let _ = curry_placeholder;
//! }
//! ```
//!
//! Every read of `partial.placeholder` then answered `SmeltUnknown::Null`, so a
//! call written `partial(fn, placeholder, 'b', placeholder)` passed `null` where a
//! sentinel belonged and the placeholder slots were filled with a real argument
//! instead of being skipped.
//!
//! # The lowering
//!
//! A module-scope `f.prop = <expr>` whose `f` resolves to a function item is
//! recorded as a **const item** under the compound name `f.prop`, and a read of
//! `f.prop` resolves to it. That reuses the const-item machinery
//! (`const_item_expression`, which inlines the recorded initializer at the read
//! site) rather than adding a parallel store, so an imported `partial.placeholder`
//! travels through the same item-visibility path as any other module const.
//!
//! Identity holds for these values because the recorded initializers are
//! *site-stable*: `Symbol('…')` lowers to a string keyed by its source span, and a
//! bare `Map` to an interned builtin namespace, so two inlined copies compare
//! equal. That is the same identity contract module-level `const` bindings already
//! have under const inlining — this feature does not weaken it, and a
//! per-read-fresh initializer (`f.prop = {}`) would be misrepresented by both
//! equally. Making such a value a single allocation needs lazily-initialized
//! module statics, which is recorded as a gap rather than smuggled in here.

use oxc::ast::ast::{AssignmentOperator, AssignmentTarget, BindingPattern, Expression};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{Body, ConstItem, Item};

/// Build the compound `items` key under which `f.prop` is recorded.
///
/// A dot cannot appear in a TypeScript identifier, so the compound key cannot
/// collide with a real binding name in the same map.
pub(in crate::lowering) fn static_property_key(function: &str, property: &str) -> String {
    format!("{function}.{property}")
}

impl ModuleBuilder<'_> {
    /// Return whether `name` resolves to something a static property can hang off.
    ///
    /// A function *item* is the shape es-toolkit uses. A class is excluded: a
    /// `static` member is the source spelling there and has its own lowering.
    fn name_is_static_property_owner(&self, name: &str) -> bool {
        self.items
            .get(name)
            .copied()
            .is_some_and(|item| matches!(self.item_ref(item), Item::Function(_)))
    }

    /// Recognize a `f.prop = <expr>` whose `f` is a function item, returning the
    /// compound `items` key it is recorded under.
    ///
    /// Shared by the collection pass and the module-body skip check, so the two
    /// cannot disagree about which statements were absorbed.
    pub(in crate::lowering) fn function_static_property_key(
        &self,
        expression: &Expression<'_>,
    ) -> Option<String> {
        let Expression::AssignmentExpression(assignment) = expression else {
            return None;
        };
        if assignment.operator != AssignmentOperator::Assign {
            return None;
        }
        let AssignmentTarget::StaticMemberExpression(member) = &assignment.left else {
            return None;
        };
        let Expression::Identifier(owner) = &member.object else {
            return None;
        };
        if member.optional || !self.name_is_static_property_owner(owner.name.as_str()) {
            return None;
        }
        // `f.prototype.m = …` is the constructor-function idiom, which class
        // synthesis claims; only a direct property on the function is a static.
        if member.property.name == "prototype" {
            return None;
        }
        Some(static_property_key(
            owner.name.as_str(),
            member.property.name.as_str(),
        ))
    }

    /// Return whether `expression` is a static-property assignment already recorded.
    ///
    /// The two statement loops that walk the module body both skip these, and both
    /// ask here rather than re-deriving the shape, so they cannot disagree with the
    /// prepass about which statements were absorbed.
    pub(in crate::lowering) fn function_static_property_recorded(
        &self,
        expression: &Expression<'_>,
    ) -> bool {
        self.function_static_property_key(expression)
            .is_some_and(|key| {
                self.items
                    .get(&key)
                    .copied()
                    .is_some_and(|item| matches!(self.item_ref(item), Item::Const(_)))
            })
    }

    /// Collect every module-scope static property in one prepass.
    ///
    /// Runs after the function items are predeclared (so the owner check resolves)
    /// and before any body is lowered (so a function reading its OWN static — which
    /// is a forward reference to a statement below the declaration — resolves).
    /// es-toolkit `partial` does exactly that: its body passes
    /// `partial.placeholder` to `partialImpl`.
    ///
    /// A right-hand side that fails to lower is left alone rather than half-recorded:
    /// the statement then keeps whatever the ordinary path did with it, and the error
    /// surfaces from there with its own span.
    pub(in crate::lowering) fn collect_function_static_properties(
        &mut self,
        program: &oxc::ast::ast::Program<'_>,
        errors: &mut Vec<SmeltError>,
    ) {
        for statement in &program.body {
            let oxc::ast::ast::Statement::ExpressionStatement(expr_stmt) = statement else {
                continue;
            };
            if let Err(error) = self.collect_function_static_property(&expr_stmt.expression) {
                errors.push(error);
            }
        }
    }

    /// Record a module-scope `f.prop = <expr>` as a const item, if it is one.
    ///
    /// Returns `true` when the statement was absorbed, so the caller skips it. The
    /// statement produced no runtime effect before this — the module-init body it
    /// lowered into is never called — so absorbing it removes nothing.
    pub(in crate::lowering) fn collect_function_static_property(
        &mut self,
        expression: &Expression<'_>,
    ) -> Result<bool, SmeltError> {
        let Some(key) = self.function_static_property_key(expression) else {
            return Ok(false);
        };
        let Expression::AssignmentExpression(assignment) = expression else {
            return Ok(false);
        };
        let span = self.span(assignment.span.start, assignment.span.end);
        let mut const_body = Body::new(None, span);
        let value = self.expression(&assignment.right, &mut const_body)?;
        let ty = Self::expr_ty(&const_body, value);
        let body = self.ctx.krate.push_body(const_body);
        let name = self.intern_exact_source_name(&key);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name,
            ty,
            value,
            body,
            span,
        }));
        self.items.insert(key.clone(), item);
        // Exported so an importer's `partial.placeholder` read resolves through the
        // same item-visibility path as any other module const.
        self.ctx.export_aliases.insert(key, item);
        Ok(true)
    }

    /// Resolve a `f.prop` read against the recorded function statics.
    pub(in crate::lowering) fn function_static_property_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(owner) = &member.object else {
            return Ok(None);
        };
        // A local binding of the same name shadows the module function, so the
        // read is not a static-property read at all.
        if self.scope.lookup(owner.name.as_str()).is_some() {
            return Ok(None);
        }
        let key = static_property_key(owner.name.as_str(), member.property.name.as_str());
        let Some(item) = self.items.get(&key).copied() else {
            return Ok(None);
        };
        let Item::Const(const_item) = self.item_ref(item).clone() else {
            return Ok(None);
        };
        self.const_item_expression(&const_item, member.span.start, member.span.end, body)
            .map(Some)
    }

    /// Lower `const { placeholder } = f;` against the recorded function statics.
    ///
    /// This is how a spec reaches the sentinel — `const { placeholder } = partial;`
    /// — and it is an object-destructuring whose source is a *function*, which the
    /// ordinary record-destructuring path cannot type. Each shorthand or renamed
    /// property resolves to the `f.<key>` const, exactly as the member read does.
    ///
    /// Returns `true` when the declarator was absorbed.
    pub(in crate::lowering) fn destructure_function_statics(
        &mut self,
        declarator: &oxc::ast::ast::VariableDeclarator<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let BindingPattern::ObjectPattern(pattern) = &declarator.id else {
            return Ok(false);
        };
        let Some(Expression::Identifier(owner)) = &declarator.init else {
            return Ok(false);
        };
        if self.scope.lookup(owner.name.as_str()).is_some()
            || !self.name_is_static_property_owner(owner.name.as_str())
        {
            return Ok(false);
        }
        if pattern.rest.is_some() {
            return Ok(false);
        }
        // Every property must resolve, otherwise leave the whole declarator to the
        // ordinary path rather than lowering it half-way.
        let mut resolved = Vec::new();
        for property in &pattern.properties {
            let Some(key) = property.key.static_name() else {
                return Ok(false);
            };
            let BindingPattern::BindingIdentifier(binding) = &property.value else {
                return Ok(false);
            };
            let item_key = static_property_key(owner.name.as_str(), key.as_ref());
            let Some(item) = self.items.get(&item_key).copied() else {
                return Ok(false);
            };
            let Item::Const(const_item) = self.item_ref(item).clone() else {
                return Ok(false);
            };
            resolved.push((binding.name.as_str().to_owned(), binding.span, const_item));
        }
        for (name, binding_span, const_item) in resolved {
            let value = self.const_item_expression(
                &const_item,
                binding_span.start,
                binding_span.end,
                body,
            )?;
            let ty = Self::expr_ty(body, value);
            let symbol = self.intern_source_name(&name);
            let local = body.push_local(smelt_hir::LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span: self.span(binding_span.start, binding_span.end),
            });
            let pat = body.push_pattern(smelt_hir::Pattern::Binding(local));
            body.push_stmt_to_block(
                block,
                smelt_hir::Stmt::Let {
                    pat,
                    ty,
                    value: Some(value),
                },
            );
            self.scope.bind(name, local);
        }
        Ok(true)
    }
}
