//! Local declarations: `var`/`let`/`const` statements, `catch` bindings, and the
//! forward declarations a body needs before its statements run.

use crate::lowering::erasure::NotNewErasure;
use crate::lowering::ModuleBuilder;
use crate::SmeltError;
use crate::lowering::js_text::{js_str_text, static_name_text};
use oxc::ast::ast::{BindingPattern, Expression, Statement};
use smelt_hir::{Body, ErasureReason, FunctionType, LocalDecl, Pattern, Stmt, Type};
use std::collections::{HashMap, HashSet};

impl ModuleBuilder<'_> {
    /// Lower a catch parameter to an optional HIR local binding.
    pub(in crate::lowering) fn catch_binding(
        &mut self,
        param: &oxc::ast::ast::CatchParameter<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::LocalId, SmeltError> {
        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
            return Err(SmeltError::unsupported(
                self.span(param.span.start, param.span.end),
                "destructured catch bindings are not lowered yet",
            ));
        };
        let ty = param
            .type_annotation
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| self.erase_with(ErasureReason::ExternalType, "catch binding"));
        let name = binding.name.as_str();
        let symbol = self.intern_source_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(binding.span.start, binding.span.end),
        });
        self.scope.bind(name.to_owned(), local);
        Ok(local)
    }

    /// Return whether an initializer needs its declaration binding before evaluation.
    ///
    /// A valid TypeScript initializer can refer to the binding receiving its
    /// result only through deferred execution, for example
    /// `const recursive = wrap(() => recursive())`. Temporarily making that
    /// binding visible lets the existing capture walk identify this case
    /// without assigning premature types to unrelated factory results.
    pub(in crate::lowering) fn initializer_needs_deferred_self_binding(
        &mut self,
        initializer: &Expression<'_>,
        name: &str,
    ) -> bool {
        if matches!(
            initializer,
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
        ) {
            return false;
        }
        let previous = self.scope.bind(name.to_owned(), smelt_hir::LocalId(u32::MAX));
        let mut captures = Vec::new();
        self.collect_expression_capture_names(initializer, &HashSet::new(), &mut captures);
        if let Some(previous) = previous {
            self.scope.bind(name.to_owned(), previous);
        } else {
            self.scope.unbind(name);
        }
        captures.iter().any(|capture| capture == name)
    }

    /// Predeclare function-local bindings an earlier statement already reads.
    ///
    /// Smelt lowers statements in source order, but JavaScript execution order
    /// is not source order: any function body runs when it is CALLED, so a
    /// closure written before a `const`/`let` may legitimately read it. Three
    /// shapes reach that:
    ///
    /// * a `const` arrow callback another closure in the same body calls;
    /// * an initializer that mentions the binding receiving its own result
    ///   (`const recursive = wrap(() => recursive())`);
    /// * any `const`/`let` a closure in an EARLIER statement of this same list
    ///   captures -- `const abort = () => clearTimeout(id); const id = setTimeout(..)`,
    ///   the shape es-toolkit's `timeout` uses to disarm its own timer. Without
    ///   the reservation the earlier read found no binding and fell through to
    ///   the module-global fallback, which FABRICATED an empty object for an
    ///   `unknown` type: `clearTimeout({})` cleared nothing, silently.
    ///
    /// This pass reserves the local; declaration lowering later fills in the
    /// runtime value, and the earlier closure captures it through the ordinary
    /// shared-cell capture path.
    pub(in crate::lowering) fn predeclare_forward_referenced_locals(
        &mut self,
        statements: &[Statement<'_>],
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let first_reads = self.first_capture_read_indices(statements);
        for (index, statement) in statements.iter().enumerate() {
            let Statement::VariableDeclaration(decl) = statement else {
                continue;
            };
            let is_const = decl.kind == oxc::ast::ast::VariableDeclarationKind::Const;
            if !is_const && decl.kind != oxc::ast::ast::VariableDeclarationKind::Let {
                continue;
            }
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let Some(initializer) = &declarator.init else {
                    continue;
                };
                // The `const`-only shapes: an arrow value, and a self-mentioning
                // initializer. A `let` is reserved only for a genuine forward
                // read, where the reservation is what makes the program lower at
                // all.
                let is_deferred_self_binding = is_const
                    && self.initializer_needs_deferred_self_binding(
                        initializer,
                        binding.name.as_str(),
                    );
                let direct_arrow = match initializer {
                    Expression::ArrowFunctionExpression(arrow) if is_const => Some(arrow),
                    _ => None,
                };
                let read_earlier = direct_arrow.is_none()
                    && !is_deferred_self_binding
                    && first_reads
                        .get(binding.name.as_str())
                        .is_some_and(|first| *first < index);
                if direct_arrow.is_none() && !is_deferred_self_binding && !read_earlier {
                    continue;
                }
                if self.scope.is_bound(binding.name.as_str()) {
                    continue;
                }
                let annotated_ty = declarator
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                let ty = if let Some(arrow) = direct_arrow {
                    self.local_arrow_function_type(arrow, annotated_ty)
                        .unwrap_or_else(|_| {
                            let unknown = self.erase_with(
                                ErasureReason::MissingAnnotation,
                                binding.name.as_str(),
                            );
                            let return_ty = self
                                .contextual_function_type(annotated_ty)
                                .map_or(unknown, |function| function.return_ty);
                            self.ctx.krate.types.intern(Type::Function(FunctionType {
                                params: vec![unknown; arrow.params.items.len()],
                                rest: None,
                                required_params: None,
                                mutable_params: Vec::new(),
                                return_ty,
                                is_async: arrow.r#async,
                                may_throw: false,
                            }))
                        })
                } else {
                    annotated_ty.unwrap_or_else(|| {
                        self.erase_with(ErasureReason::MissingAnnotation, binding.name.as_str())
                    })
                };
                let symbol = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: !is_const,
                    span: self.span(binding.span.start, binding.span.end),
                });
                self.scope.bind(binding.name.as_str().to_owned(), local);
                if read_earlier {
                    // The declaration must write into THIS slot: a second local
                    // would leave the earlier capture reading a slot nothing
                    // ever assigns. The arrow and self-binding shapes are
                    // already routed there by their own declaration paths.
                    self.reserve_forward_local(binding.name.as_str().to_owned());
                }
            }
        }
        Ok(())
    }

    /// For every `let`/`const` name this statement list declares, the index of
    /// the first statement whose deferred code (a closure or a local function
    /// body) reads it.
    ///
    /// "Is `name` read by a statement before index `i`" is then
    /// `first < i`. Asking that per declaration (the previous shape) re-walked
    /// the whole prefix for every declaration, which is
    /// quadratic in the length of a function body (measured by
    /// `tests/compile_scaling.rs` in `smelt-codegen-rust`: a 1024-statement body
    /// spent two thirds of its frontend time here). This walks each statement
    /// once, with every candidate bound to the sentinel the per-name walk used.
    /// TypeScript rejects a plain use-before-declaration of a block-scoped
    /// binding, so an earlier reference that compiles is inside deferred code;
    /// the capture walk reports exactly the BOUND names such code reads, and
    /// whether it reports one name does not depend on whether another is
    /// bound, so each candidate's answer is the one the per-name walk gave.
    fn first_capture_read_indices(&mut self, statements: &[Statement<'_>]) -> HashMap<String, usize> {
        let mut candidates = Vec::new();
        for statement in statements {
            let Statement::VariableDeclaration(decl) = statement else {
                continue;
            };
            if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const
                && decl.kind != oxc::ast::ast::VariableDeclarationKind::Let
            {
                continue;
            }
            for declarator in &decl.declarations {
                if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                    && declarator.init.is_some()
                    && !candidates.contains(&binding.name.as_str())
                {
                    candidates.push(binding.name.as_str());
                }
            }
        }
        let mut first_reads = HashMap::new();
        if candidates.is_empty() {
            return first_reads;
        }
        let previous = candidates
            .iter()
            .map(|name| (*name, self.scope.bind((*name).to_owned(), smelt_hir::LocalId(u32::MAX))))
            .collect::<Vec<_>>();
        for (index, statement) in statements.iter().enumerate() {
            let mut captures = Vec::new();
            self.collect_statement_capture_names(statement, &HashSet::new(), &mut captures);
            self.collect_function_declaration_capture_names(statement, &mut captures);
            for capture in captures {
                if candidates.contains(&capture.as_str()) {
                    first_reads.entry(capture).or_insert(index);
                }
            }
        }
        for (name, previous) in previous.into_iter().rev() {
            if let Some(previous) = previous {
                self.scope.bind(name.to_owned(), previous);
            } else {
                self.scope.unbind(name);
            }
        }
        first_reads
    }

    /// Collect the enclosing names a local `function` DECLARATION's body reads.
    ///
    /// A hoisted local function is deferred code exactly as an arrow is: its
    /// body runs when it is called, so it may read a `const` declared below it.
    /// The shared capture walk does not descend into a function declaration --
    /// a function item is not a closure at every site that walk serves -- so
    /// the forward-reference pass descends itself. Its own parameters are
    /// excluded, since those are not enclosing names.
    fn collect_function_declaration_capture_names(
        &self,
        statement: &Statement<'_>,
        captures: &mut Vec<String>,
    ) {
        let Statement::FunctionDeclaration(function) = statement else {
            return;
        };
        let Some(function_body) = &function.body else {
            return;
        };
        let mut param_names = HashSet::new();
        for param in &function.params.items {
            let mut binding_names = Vec::new();
            Self::binding_pattern_names(&param.pattern, &mut binding_names);
            param_names.extend(binding_names);
        }
        if let Some(rest) = &function.params.rest {
            let mut binding_names = Vec::new();
            Self::binding_pattern_names(&rest.rest.argument, &mut binding_names);
            param_names.extend(binding_names);
        }
        for child in &function_body.statements {
            self.collect_statement_capture_names(child, &param_names, captures);
        }
    }

    /// Predeclare nested function declarations before source-order statement lowering.
    ///
    /// JavaScript hoists `function name(...) {}` declarations within a function
    /// body. Reserving callable locals here lets earlier sibling functions call
    /// declarations that appear later in the same block.
    pub(in crate::lowering) fn predeclare_local_function_declarations(
        &mut self,
        statements: &[Statement<'_>],
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        // `const Foo = function () { … }` constructor bindings used with
        // `new`/`instanceof`/`Foo.prototype.x = …` in this block are synthesized
        // into classes here, before the binding is treated as a function value.
        self.synthesize_const_constructor_functions(statements)?;
        // Which sibling functions the block constructs, in one scoped walk of
        // the block for all of them.
        let sibling_names: HashSet<&str> = statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => function.id.as_ref().map(|id| id.name.as_str()),
                _ => None,
            })
            .collect();
        let mut constructed = HashSet::new();
        if !sibling_names.is_empty() {
            for statement in statements {
                constructed.extend(
                    crate::lowering::scoped_references::statement_name_uses(statement, &sibling_names).constructed,
                );
            }
        }
        for statement in statements {
            let Statement::FunctionDeclaration(function) = statement else {
                continue;
            };
            let Some(id) = &function.id else {
                continue;
            };
            if self.scope.is_bound(id.name.as_str()) {
                continue;
            }
            // A `function Foo(){}` used as `new Foo()` / `instanceof Foo` /
            // `Foo.prototype.x = …` in this block is a JavaScript constructor
            // function: synthesize a class for it instead of a plain local
            // function so the construction and prototype-chain sites resolve.
            if !self.classes.contains(id.name.as_str()) && constructed.contains(id.name.as_str()) {
                self.synthesize_constructor_function_class(function, statements)?;
                continue;
            }
            self.push_type_parameter_scope(function.type_parameters.as_deref())?;
            let result = (|| {
                // The same ABI the declaration itself lowers to (see
                // `local_function_declaration`), so a call lowered before the
                // declaration statement already passes `Optional<T>` for a
                // defaulted parameter.
                let mut params = Vec::new();
                for param in &function.params.items {
                    params.push(self.function_parameter_abi(param)?.0);
                }
                let required_params = function
                    .params
                    .items
                    .iter()
                    .position(|param| param.optional || Self::formal_parameter_has_default(param))
                    .unwrap_or(function.params.items.len());
                let return_ty = function
                    .return_type
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| {
                        // not a new erasure: provisional return, replaced by the
                        // inferred one once the body lowers
                        let unknown = self.unknown_type(NotNewErasure::ErasedUpstream);
                        if function.r#async {
                            self.ctx.krate.types.intern(Type::Future(unknown))
                        } else {
                            unknown
                        }
                    });
                let fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params,
                    rest: None,
                    required_params: Some(required_params),
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: function.r#async,
                    may_throw: false,
                }));
                let symbol = self.intern_source_name(id.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: fn_ty,
                    mutable: false,
                    span: self.span(id.span.start, id.span.end),
                });
                self.scope.bind(id.name.as_str().to_owned(), local);
                Ok(())
            })();
            self.pop_type_parameter_scope();
            result?;
        }
        Ok(())
    }

    /// Lower a variable declaration statement.
    pub(in crate::lowering) fn variable_declaration(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        // An ambient declaration (`declare let process: …`) ASSERTS that the
        // host already provides the binding; it never creates one. Lowering it
        // as an ordinary declaration minted a runtime local seeded with the
        // declared type's default (`None` for an optional annotation), so
        // `typeof process !== 'undefined'` answered `false` before any host
        // lookup could happen. Reads of the name instead fall through to the
        // host/global resolution path in `expr::references`.
        if decl.declare {
            return Ok(());
        }
        // A block-local arrow or predicate-typed binding narrows its callers'
        // arguments exactly like a top-level one (see `type_predicates`).
        self.register_variable_type_predicates(decl);
        for declarator in &decl.declarations {
            if self.conditional_base_declarator(declarator, body, block)? {
                continue;
            }
            // A module-level `let`/`var` binding lifted to a mutable global is
            // fully represented by its thread-local item (its literal
            // initializer is the cell's initial value), so lowering the SAME
            // source declaration — in the module body or replayed as top-level
            // test setup — must not re-declare it as a shadowing local. The
            // declaration is recognized by its binding span; a same-named
            // binding declared elsewhere (a function or block scope) has a
            // different span and still creates its ordinary shadowing local.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && self.is_lifted_global_declarator(binding.name.as_str(), binding.span)
            {
                // A NON-literal initializer could not be lowered by the
                // classification pass (it runs before imports and function
                // items resolve), so this is where it happens: the expression
                // becomes a synthesized nullary initializer function the cell
                // calls lazily. A literal initializer is already stored on the
                // item and this is a no-op.
                if let Some(init) = &declarator.init {
                    self.lower_pending_mutable_global_init(binding.name.as_str(), init)?;
                    self.force_module_slot_init(binding.name.as_str(), binding.span, body, block);
                }
                continue;
            }
            // A `const Foo = function () { … }` binding recognized as a
            // constructor function was already synthesized into a class during
            // the block prepass; its declarator contributes no runtime binding.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && Self::const_constructor_function(declarator).is_some()
                && self.classes.contains(binding.name.as_str())
            {
                continue;
            }
            // `const Foo = class { … }` is a class declaration named `Foo`: it
            // lowers as one and contributes no runtime binding, exactly like the
            // constructor-function case above. Lowered as a value instead, the
            // binding held a placeholder instance and `new Foo()` erased into a
            // dynamic construction whose methods answered `null` (H68).
            if let Some((name, class)) = Self::const_class_expression(declarator) {
                self.class_declaration_named(class, Some(name))?;
                continue;
            }
            // `const { placeholder } = partial;` destructures a static property off
            // a FUNCTION, which the ordinary record-destructuring path cannot type.
            // See `lowering::function_statics`.
            if self.destructure_function_statics(declarator, body, block)? {
                continue;
            }
            // `const { A: B } = await import('./mod')` re-imports statically
            // known module members (a vitest module-reset idiom). Compiled Rust
            // has no module registry to reset, so the fresh namespace is the
            // same module: each destructured member binds as a compile-time
            // alias of the statically resolved import, exactly like
            // `import { A as B } from './mod'`.
            if let BindingPattern::ObjectPattern(object) = &declarator.id
                && let Some(Expression::AwaitExpression(await_expr)) = &declarator.init
                && let Expression::ImportExpression(import_expr) = &await_expr.argument
                && let Expression::StringLiteral(source) = &import_expr.source
            {
                self.dynamic_import_destructure_aliases(object, &js_str_text(source.value))?;
                continue;
            }
            // `const g = globalThis;` records a local global-object alias so that
            // later `g.Object.keys(x)` / `"Map" in g` normalize and erase exactly
            // like the bare `globalThis` spelling. The alias is purely a
            // compile-time name-tracking aid; no HIR local is emitted because the
            // global object is never materialized in Phase 1. A `let` is allowed:
            // a later reassignment off the alias is handled where assignments are
            // lowered (the name is cleared), and a write *through* the alias stays
            // on the erasure denylist and produces an honest blocker.
            //
            // The same holds for the portable global-detection *chain*
            // (`(typeof globalThis === 'object' && globalThis) || (typeof window
            // === 'object' && window) || …`): the profile already folds it to the
            // one present global object, so a binding initialized by it aliases
            // the global object just as literally as `const g = globalThis;`. This
            // is the shape shipped by every "universal global" shim, so
            // recognizing only the bare spelling made the alias depend on which
            // spelling the source happened to use.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && let Some(initializer) = &declarator.init
                && (self.expr_is_global_alias(initializer)
                    || self.expr_folds_to_global_alias(initializer))
            {
                self.imports.mark_global_object_alias(binding.name.as_str().to_owned());
                continue;
            }
            // A `const f = (…) => …` arrow is an immutable closure local, sound
            // because a `const` can never be reassigned. A `let`/`var` arrow may
            // be reassigned later (`let cmp = defaultCmp; … cmp = other;`). Only lift
            // `const` arrows here; mutable-binding arrows fall through to the
            // general initializer path, which binds a mutable local holding the
            // arrow as a closure value so later reassignments lower to a plain
            // local write.
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init
                // Since oxc 0.147 the declaration kind lives on the parent
                // `VariableDeclaration`, not on each declarator.
                && decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
            {
                let annotated_ty = declarator
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                self.local_arrow_callback_declaration(
                    binding.name.as_str(),
                    binding.span.start,
                    binding.span.end,
                    arrow,
                    annotated_ty,
                    body,
                    block,
                )?;
                self.record_declared_callable_interface(binding.name.as_str(), annotated_ty);
                continue;
            }
            let annotated_ty = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            // An unannotated `= []` takes the type TypeScript evolves it to
            // through later pushes, as if that were its annotation.
            let annotated_ty = match (&declarator.id, &declarator.init) {
                (BindingPattern::BindingIdentifier(binding), Some(Expression::ArrayExpression(array)))
                    if annotated_ty.is_none() && array.elements.is_empty() =>
                {
                    self.checker_evolved_array_type(binding.span.start, binding.span.end)
                }
                _ => annotated_ty,
            };
            // Both shapes assign into a local reserved before this statement
            // list was lowered: an initializer that mentions its own binding,
            // and a binding a closure in an earlier statement already reads.
            let reserved_forward_local = if let BindingPattern::BindingIdentifier(binding) =
                &declarator.id
            {
                self.take_forward_local_reservation(binding.name.as_str())
            } else {
                false
            };
            let deferred_self_local = if let BindingPattern::BindingIdentifier(binding) =
                &declarator.id
                && (reserved_forward_local
                    || declarator.init.as_ref().is_some_and(|initializer| {
                        self.initializer_needs_deferred_self_binding(
                            initializer,
                            binding.name.as_str(),
                        )
                    }))
            {
                self.local_arrow_existing_body_local(binding.name.as_str(), body)
            } else {
                None
            };
            let predeclared_self = if let BindingPattern::BindingIdentifier(binding) =
                &declarator.id
                && matches!(declarator.init, Some(Expression::FunctionExpression(_)))
            {
                // not a new erasure: provisional self-binding while the initializer lowers
                let ty = annotated_ty.unwrap_or_else(|| self.unknown_type(NotNewErasure::ErasedUpstream));
                let symbol = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                self.scope.bind(binding.name.as_str().to_owned(), local)
            } else {
                None
            };
            // Postfix stores move after the binding only when no later part of
            // the initializer can observe them; otherwise each one stores where
            // JavaScript evaluates it (see `postfix_deferral`).
            let mut lower_init = |this: &mut Self| {
                declarator
                    .init
                    .as_ref()
                    .map(|init| this.expression_with_hint(init, body, annotated_ty))
                    .transpose()
            };
            let (value_result, deferred_updates) = if declarator
                .init
                .as_ref()
                .is_none_or(crate::lowering::postfix_deferral::initializer_can_defer_postfix_updates)
            {
                self.with_deferred_updates(lower_init)
            } else {
                (lower_init(self), Vec::new())
            };
            let value = value_result?;
            let annotated_ty = self.presence_widened_binding_type(declarator, annotated_ty, value, body);
            let value = value.map(|value| self.keyed_read_into_optional_slot(value, annotated_ty, body));
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                && value.is_none()
                && let Some(previous) = predeclared_self
            {
                self.scope.bind(binding.name.as_str().to_owned(), previous);
            }
            match (deferred_self_local, value) {
                (Some(local), Some(value)) => {
                    let local_index = usize::try_from(local.0).map_err(|err| {
                        SmeltError::unsupported(
                            self.span(declarator.span.start, declarator.span.end),
                            format!("deferred callable local id does not fit in usize: {err}"),
                        )
                    })?;
                    let ty = body
                        .locals
                        .get(local_index)
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(declarator.span.start, declarator.span.end),
                                "deferred callable local is missing from its function body",
                            )
                        })?
                        .ty;
                    let pat = body.push_pattern(Pattern::Binding(local));
                    body.push_stmt_to_block(
                        block,
                        Stmt::Let {
                            pat,
                            ty,
                            value: Some(value),
                        },
                    );
                }
                (_, value) => {
                    self.binding_declaration(
                        &declarator.id,
                        value,
                        annotated_ty,
                        matches!(decl.kind, oxc::ast::ast::VariableDeclarationKind::Let),
                        body,
                        block,
                    )?;
                }
            }
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id {
                self.record_declared_callable_interface(binding.name.as_str(), annotated_ty);
            }
            for update in deferred_updates {
                body.push_stmt_to_block(block, update);
            }
        }
        Ok(())
    }

    /// The declared type of a local the program assigns a keyed read and
    /// tests for presence: `T | undefined` in place of an always-truthy `T`.
    ///
    /// See `lowering::presence_tested_locals` for why such a local must be able
    /// to hold the absent value. The widened type is taken from the annotation
    /// when there is one and from the initializer otherwise; a type that is
    /// already nullishable is left alone, and so is one with falsy inhabitants
    /// (whose Rust defaults already read as falsy) unless the program compares
    /// the local with `undefined`/`null` explicitly.
    fn presence_widened_binding_type(
        &mut self,
        declarator: &oxc::ast::ast::VariableDeclarator<'_>,
        annotated_ty: Option<smelt_hir::TypeId>,
        value: Option<smelt_hir::ExprId>,
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return annotated_ty;
        };
        let Some(test) = self.presence_widened_bindings.get(&binding.span.start).copied() else {
            return annotated_ty;
        };
        let Some(declared) = annotated_ty.or_else(|| value.map(|value| Self::expr_ty(body, value)))
        else {
            return annotated_ty;
        };
        // A falsy-inhabited type answers a truthiness test through its Rust
        // default, but not an explicit `=== undefined` comparison.
        // The explicit comparison widens only a concrete scalar in addition:
        // an erased or generic type (`unknown`, `T`) already represents
        // `undefined` itself, and wrapping it in `Option` would only add a
        // second encoding of absence.
        let always_truthy = self.type_is_always_truthy_object_surface(declared);
        let widened_by_comparison = test
            == super::super::presence_tested_locals::PresenceTest::NullishComparison
            && matches!(
                self.ctx.krate.types.get(declared),
                Some(Type::String | Type::Float | Type::Int | Type::Bool)
            );
        if self.is_nullishable_type(declared) || !(always_truthy || widened_by_comparison) {
            return annotated_ty;
        }
        Some(self.ctx.krate.types.intern(Type::Optional(declared)))
    }

    /// Remember that a local was declared at a callable-interface type.
    ///
    /// The annotation on `const debounced: DebounceFunction<TArgs> = …` is the
    /// only place that says which struct the property writes collected onto that
    /// local belong to. A value that leaves through a position with a type hint
    /// (an annotated return, an annotated parameter) rediscovers it from the
    /// hint, but a `return` from a function with an *inferred* return type has
    /// no hint at all — without this the collected writes would be dropped. Only
    /// a callable interface is recorded; every other annotation is irrelevant to
    /// the callable-object construction and is ignored.
    pub(in crate::lowering) fn record_declared_callable_interface(
        &mut self,
        name: &str,
        annotated_ty: Option<smelt_hir::TypeId>,
    ) {
        let Some(annotated_ty) = annotated_ty else {
            return;
        };
        if !self.type_is_callable_interface(annotated_ty) {
            return;
        }
        let Some(local) = self.scope.lookup(name) else {
            return;
        };
        self.scope.record_callable_local_interface(local, annotated_ty);
    }

    /// Register `const { A: B } = await import('./mod')` bindings as import aliases.
    ///
    /// Each destructured member re-binds a statically resolvable export of the
    /// imported module under the local pattern name, reusing the same alias
    /// machinery as `import { A as B } from './mod'`. Computed keys, nested
    /// patterns, and rest elements have no static import equivalent and stay
    /// unsupported.
    fn dynamic_import_destructure_aliases(
        &mut self,
        object: &oxc::ast::ast::ObjectPattern<'_>,
        source: &str,
    ) -> Result<(), SmeltError> {
        if object.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(object.span.start, object.span.end),
                "dynamic import destructuring does not support rest elements",
            ));
        }
        for property in &object.properties {
            let (Some(imported), BindingPattern::BindingIdentifier(local)) =
                (property.key.static_name(), &property.value)
            else {
                return Err(SmeltError::unsupported(
                    self.span(property.span.start, property.span.end),
                    "dynamic import destructuring requires static keys and identifier bindings",
                ));
            };
            self.alias_imported_item(source, &static_name_text(imported), local.name.as_str());
            self.imports.mark_value(local.name.as_str().to_owned());
        }
        Ok(())
    }
}
