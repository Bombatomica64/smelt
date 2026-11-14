//! Module globals: the type of each top-level binding a function body may read,
//! inferred from its annotation or initializer before bodies are lowered.

use super::super::{ConstCollection, ModuleBuilder};
use crate::SmeltError;
use oxc::ast::ast::{BindingPattern, Declaration, Expression, Program, Statement};
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Item, Type};
use std::collections::{HashMap, HashSet};

impl ModuleBuilder<'_> {
    /// Collect typed top-level variables that functions may read or write.
    pub(in crate::lowering) fn collect_module_globals(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => {
                    self.collect_module_global_decl(variable);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::VariableDeclaration(variable) = &export.declaration {
                        self.collect_module_global_decl(variable);
                    }
                }
                _ => {}
            }
        }
    }

    /// Register annotated and inferred module-level variables for later function-body lookup.
    pub(in crate::lowering) fn collect_module_global_decl(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) {
        // `declare let process: …` says the HOST provides `process`. The
        // declaration still contributes the name's TYPE — that is all a
        // `declare` is — but recording it as an ordinary module global made
        // every read resolve through `module_global_expression`, which
        // fabricates the declared type's default value (`None`, `false`, `0`,
        // `""`, `{}`, `[]`) for a global whose initializer never ran, turning a
        // host lookup into a wrong compile-time constant. Recording the name
        // here lets `expr::references` consult the host/ambient-global path
        // first; see `ambient_value_declarations`.
        if decl.declare {
            for declarator in &decl.declarations {
                Self::collect_ambient_declaration_names(
                    &declarator.id,
                    &mut self.ambient_value_declarations,
                );
            }
        }
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                if matches!(
                    decl.kind,
                    oxc::ast::ast::VariableDeclarationKind::Const
                        | oxc::ast::ast::VariableDeclarationKind::Let
                ) {
                    let span = self.span(declarator.span.start, declarator.span.end);
                    let ty = self.erase_at(
                        ErasureReason::DynamicOperator,
                        span,
                        "module-level destructuring",
                    );
                    Self::collect_module_global_pattern_bindings(
                        &declarator.id,
                        ty,
                        &mut self.module_globals,
                    );
                }
                continue;
            };
            // A binding whose initializer IS the ambient global object (the bare
            // alias, or the portable `(typeof globalThis === 'object' &&
            // globalThis) || ...` detection chain) names the global object, not a
            // value of its own. Recording the alias in this PREPASS is what makes
            // every body see it — including a function lowered before the
            // declaration — and stops the annotated-global path below from
            // registering the name with its declared type, whose reads fabricate
            // that type's default (an empty record for `any`) instead of
            // resolving against the modeled global.
            if let Some(init) = &declarator.init
                && (self.expr_is_global_alias(init) || self.expr_folds_to_global_alias(init))
            {
                self.imports
                    .mark_global_object_alias(binding.name.as_str().to_owned());
                continue;
            }
            if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                && let Some(init) = &declarator.init
                && let Some(object) = Self::object_const_initializer(init)
                && let Ok(value) = {
                    // The declared annotation types the folded metadata, so a
                    // body that recreates the const (`paths[p][1]`) reads the
                    // declared tuple/list shapes rather than the literal's
                    // erased inference.
                    let type_hint = declarator.type_annotation.as_ref().and_then(|annotation| {
                        self.ts_type_to_hir(&annotation.type_annotation).ok()
                    });
                    self.object_const_from_expression(object, type_hint)
                }
            {
                if let Some(collection) = self.const_collection_from_object_const(&value) {
                    self.consts.set_object_value_collection(
                        binding.name.as_str().to_owned(),
                        collection.clone(),
                    );
                    self.publish_export(
                        |ctx| &mut ctx.object_value_collections,
                        binding.name.as_str().to_owned(),
                        collection,
                    );
                }
                self.consts
                    .set_object(binding.name.as_str().to_owned(), value);
            } else if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                && let Some(init) = &declarator.init
                && let Some(object) = Self::object_const_initializer(init)
                && let Some(collection) = self.const_unknown_value_collection_from_object(object)
            {
                self.consts.set_object_value_collection(
                    binding.name.as_str().to_owned(),
                    collection.clone(),
                );
                self.publish_export(
                    |ctx| &mut ctx.object_value_collections,
                    binding.name.as_str().to_owned(),
                    collection,
                );
            }
            if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                && !self.consts.has_object(binding.name.as_str())
                && let Some(init) = &declarator.init
                && let Some(map_const) = self.map_const_from_initializer(init)
            {
                self.consts
                    .set_object(binding.name.as_str().to_owned(), map_const);
            }
            if matches!(
                decl.kind,
                oxc::ast::ast::VariableDeclarationKind::Const
                    | oxc::ast::ast::VariableDeclarationKind::Let
            ) && let Some(init) = &declarator.init
                && let Expression::RegExpLiteral(literal) = init
            {
                let pattern = Self::regex_literal_pattern_text_without_flags(literal);
                let flags = literal.regex.flags.to_string();
                let ty = self.regexp_type();
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), ty);
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const {
                    self.consts
                        .set_regexp(binding.name.as_str().to_owned(), (pattern, flags, ty));
                }
            }
            if matches!(
                decl.kind,
                oxc::ast::ast::VariableDeclarationKind::Const
                    | oxc::ast::ast::VariableDeclarationKind::Let
            ) && let Some(init) = &declarator.init
                && let Ok(value) = self.literal_const_expression(init)
            {
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), value.ty);
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const {
                    self.consts
                        .set_literal(binding.name.as_str().to_owned(), value);
                }
            }
            let Some(annotation) = &declarator.type_annotation else {
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init
                    && let Ok(ty) = self.local_arrow_function_type(arrow, None)
                {
                    self.module_globals
                        .insert(binding.name.as_str().to_owned(), ty);
                }
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(init) = &declarator.init
                    && (self.is_module_global_array_initializer(init)
                        || Self::object_const_initializer(init).is_some())
                {
                    let ty = self
                        .infer_module_global_initializer_type(init)
                        .unwrap_or_else(|_| {
                            let span = self.span(init.span().start, init.span().end);
                            self.erase_at(
                                ErasureReason::MissingAnnotation,
                                span,
                                "module global initializer",
                            )
                        });
                    self.module_globals
                        .insert(binding.name.as_str().to_owned(), ty);
                    if let Some(collection) = self.const_collection_from_initializer(init, ty) {
                        self.consts
                            .set_collection(binding.name.as_str().to_owned(), collection.clone());
                        self.publish_export(
                            |ctx| &mut ctx.const_collections,
                            binding.name.as_str().to_owned(),
                            collection,
                        );
                    }
                }
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(init) = &declarator.init
                    && !matches!(init, Expression::ArrowFunctionExpression(_))
                {
                    let ty = self
                        .infer_module_global_initializer_type(init)
                        .unwrap_or_else(|_| {
                            let span = self.span(init.span().start, init.span().end);
                            self.erase_at(
                                ErasureReason::MissingAnnotation,
                                span,
                                "module global initializer",
                            )
                        });
                    self.module_globals
                        .insert(binding.name.as_str().to_owned(), ty);
                }
                continue;
            };
            if let Ok(ty) = self.ts_type_to_hir(&annotation.type_annotation) {
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), ty);
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(init) = &declarator.init
                    && let Some(collection) = self.declared_const_collection(init, ty)
                {
                    self.consts
                        .set_collection(binding.name.as_str().to_owned(), collection.clone());
                    self.publish_export(
                        |ctx| &mut ctx.const_collections,
                        binding.name.as_str().to_owned(),
                        collection,
                    );
                }
            }
        }
    }

    /// Drop folded collection metadata for bindings lifted to mutable globals.
    ///
    /// `collect_module_globals` folds an annotated `const xs: T[] = [...]`
    /// before it is known whether the module writes through `xs`. A written
    /// binding lifts to a `MutableGlobal` slot, and every read must see that
    /// slot: a folded copy of the initializer would silently read the stale
    /// initial elements.
    pub(super) fn forget_lifted_const_collections(&mut self) {
        let lifted: Vec<String> = self.mutable_global_items.keys().cloned().collect();
        for name in lifted {
            if self.consts.forget_collection(&name) {
                let key = self.export_key(name);
                self.ctx.const_collections.write().remove(&key);
            }
        }
    }

    /// Fold an ANNOTATED module-level array/`Set` const into collection metadata.
    ///
    /// An unannotated `const xs = [1]` already records its literal elements so
    /// function and closure bodies recreate the value; an annotated
    /// `const xs: number[] = [1]` recorded only its type, and every body read
    /// then fabricated the declared type's default (`[]`, or an erased `none`
    /// inside a closure). This gives the annotated spelling the same metadata,
    /// typed by the annotation, but only when each folded element's own type is
    /// exactly the declared element type — an element that would need a
    /// coercion (`(number | undefined)[]` holding `1`) is left unfolded.
    fn declared_const_collection(
        &mut self,
        init: &Expression<'_>,
        declared_ty: smelt_hir::TypeId,
    ) -> Option<ConstCollection> {
        let element_ty = match self.ctx.krate.types.get(declared_ty)? {
            Type::List(element_ty) | Type::Set(element_ty) => *element_ty,
            _ => return None,
        };
        let mut collection = self.const_collection_from_initializer(init, declared_ty)?;
        collection.ty = declared_ty;
        let set_ty = matches!(self.ctx.krate.types.get(declared_ty), Some(Type::Set(_)));
        (collection.is_set == set_ty && collection.items.iter().all(|item| item.ty == element_ty))
            .then_some(collection)
    }

    /// Record every name an ambient declaration's binding pattern introduces.
    ///
    /// Destructuring an ambient declaration is legal TypeScript, so the whole
    /// pattern is walked rather than only the plain identifier form.
    fn collect_ambient_declaration_names(
        pattern: &BindingPattern<'_>,
        names: &mut HashSet<String>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                names.insert(binding.name.as_str().to_owned());
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    Self::collect_ambient_declaration_names(&property.value, names);
                }
                if let Some(rest) = &object.rest {
                    Self::collect_ambient_declaration_names(&rest.argument, names);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    Self::collect_ambient_declaration_names(element, names);
                }
                if let Some(rest) = &array.rest {
                    Self::collect_ambient_declaration_names(&rest.argument, names);
                }
            }
            BindingPattern::AssignmentPattern(assignment) => {
                Self::collect_ambient_declaration_names(&assignment.left, names);
            }
        }
    }

    /// Register simple names from a non-identifier module-level binding pattern.
    pub(in crate::lowering) fn collect_module_global_pattern_bindings(
        pattern: &BindingPattern<'_>,
        ty: smelt_hir::TypeId,
        module_globals: &mut HashMap<String, smelt_hir::TypeId>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                module_globals.insert(binding.name.as_str().to_owned(), ty);
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    Self::collect_module_global_pattern_bindings(
                        &property.value,
                        ty,
                        module_globals,
                    );
                }
                if let Some(rest) = &object.rest {
                    Self::collect_module_global_pattern_bindings(
                        &rest.argument,
                        ty,
                        module_globals,
                    );
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    Self::collect_module_global_pattern_bindings(element, ty, module_globals);
                }
                if let Some(rest) = &array.rest {
                    Self::collect_module_global_pattern_bindings(
                        &rest.argument,
                        ty,
                        module_globals,
                    );
                }
            }
            BindingPattern::AssignmentPattern(assignment) => {
                Self::collect_module_global_pattern_bindings(&assignment.left, ty, module_globals);
            }
        }
    }

    /// Return true for const array initializers whose values must be visible in functions.
    pub(in crate::lowering) fn is_module_global_array_initializer(
        &self,
        init: &Expression<'_>,
    ) -> bool {
        match init {
            Expression::ArrayExpression(_) => true,
            Expression::NewExpression(new_expr) => matches!(
                &new_expr.callee,
                Expression::Identifier(callee)
                    if self.host_global_named(callee.name.as_str()).is_some_and(|host| matches!(host.name(), "Set" | "Map"))
                        || smelt_stdlib::is_typed_array_class_name(callee.name.as_str())
            ),
            Expression::CallExpression(call) => {
                self.object_values_identifier_argument(call).is_some()
            }
            Expression::TSAsExpression(as_expr) => {
                self.is_module_global_array_initializer(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.is_module_global_array_initializer(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.is_module_global_array_initializer(&non_null.expression)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.is_module_global_array_initializer(&parenthesized.expression)
            }
            _ => false,
        }
    }

    /// Infer the type of a top-level constant initializer for later test-body reads.
    ///
    /// The initializer is lowered into a scratch body only to read its type,
    /// and the lowering is rolled back afterwards: a closure inside it
    /// (`const server = serve({ fetch: async (r) => .. })`) allocates a real
    /// body in the crate, and leaving that orphan behind made MIR lower a body
    /// no expression refers to — for an async closure, one whose `await`s sit
    /// in a function never marked async, which failed the whole build. The real
    /// lowering of the declaration builds the closure again where it belongs.
    pub(in crate::lowering) fn infer_module_global_initializer_type(
        &mut self,
        init: &Expression<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let mut body = Body::new(None, self.expression_span(init));
        let mark = self.speculation_mark(&body);
        let result = self
            .expression_with_hint(init, &mut body, None)
            .map(|expr| Self::expr_ty(&body, expr));
        self.roll_back_to(mark, &mut body);
        result
    }

    /// Re-infer, from its initializer, the type of every unannotated module
    /// `const` that `collect_module_globals` recorded as `Unknown`.
    ///
    /// That pass runs before function items are predeclared (its types feed
    /// the signatures, `typeof x` among them), so an initializer that CALLS a
    /// module function (`const job = makeJob(..)`) cannot be typed there. Run
    /// once the signatures exist, the same probe types it.
    ///
    /// Only a binding the earlier passes did not commit to an erased shape is
    /// refined: one that became a module slot (whose item type is updated with
    /// it) or one no separately lowered body reads. A binding read from an item
    /// context WITHOUT a slot is the erased-call path (`const validate =
    /// wrap(fn)` called as `validate(x)`; see `collect_mutable_globals`), which
    /// relies on its `Unknown` type, so it is left as it was.
    pub(super) fn refine_unknown_module_global_types(
        &mut self,
        program: &Program<'_>,
        item_reads: &HashSet<String>,
    ) {
        // Compared by shape, not by interning `Unknown`: the type table is
        // what decides whether the erased runtime is emitted, so a pass that
        // only LOOKS for erased bindings must not add the type itself.
        let is_unknown = |builder: &Self, ty: smelt_hir::TypeId| {
            matches!(builder.ctx.krate.types.get(ty), Some(Type::Unknown))
        };
        for statement in &program.body {
            let variable = match statement {
                Statement::VariableDeclaration(variable) => variable,
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(variable) => variable,
                    _ => continue,
                },
                _ => continue,
            };
            if variable.declare || variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &variable.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let name = binding.name.as_str();
                let (None, Some(init)) = (&declarator.type_annotation, &declarator.init) else {
                    continue;
                };
                if matches!(init, Expression::ArrowFunctionExpression(_))
                    || !self
                        .module_globals
                        .get(name)
                        .is_some_and(|&ty| is_unknown(self, ty))
                {
                    continue;
                }
                let slot = self.mutable_global_items.get(name).copied();
                if slot.is_none() && item_reads.contains(name) {
                    continue;
                }
                let Ok(ty) = self.infer_module_global_initializer_type(init) else {
                    continue;
                };
                if is_unknown(self, ty) {
                    continue;
                }
                self.module_globals.insert(name.to_owned(), ty);
                if let Some(item) = slot
                    && let Some(Item::MutableGlobal(global)) = self
                        .ctx
                        .krate
                        .items
                        .get_mut(usize::try_from(item.0).unwrap_or(usize::MAX))
                    && matches!(self.ctx.krate.types.get(global.ty), Some(Type::Unknown))
                {
                    global.ty = ty;
                }
            }
        }
    }
}
