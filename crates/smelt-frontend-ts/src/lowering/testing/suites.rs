//! `ModuleBuilder` lowering methods (part 05): test-suite (`describe`/hooks)
//! lowering and related HIR construction helpers split out of `lowering.rs`.

use crate::SmeltError;
use crate::lowering::support::sanitize_test_name;
use crate::lowering::state::class_registry::{ClassNameSnapshot, ClassScopeSnapshot};
use crate::lowering::{ModuleBuilder, TableBindingValue};
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, BindingPattern, Expression, ForStatementLeft,
    ObjectPropertyKind, PropertyKey, Statement,
};
use oxc::span::GetSpan;
use smelt_hir::{Body, Function, FunctionOwner, Item, LocalDecl, Pattern, Span, Stmt, Type};
use crate::lowering::support::arrow_block_statements;

impl ModuleBuilder<'_> {
    /// Lower a `describe` test-suite declaration and its inherited hooks.
    pub(in crate::lowering) fn describe_declaration(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        inherited_setup: &[&Statement<'_>],
        inherited_before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        inherited_after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe calls require a string name",
            )
        })?;
        let group_name = self.test_title(name_arg)?;
        let statements = self.test_suite_callback_statements(call, "describe callbacks")?;
        self.describe_body_declarations(
            statements,
            &group_name,
            inherited_setup.to_vec(),
            inherited_before_each.to_vec(),
            inherited_after_each.to_vec(),
            &[],
        )
    }

    /// Return the static title for a supported `describe(...)` call.
    pub(in crate::lowering) fn describe_group_name(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Result<String, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe calls require a string name",
            )
        })?;
        self.test_title(name_arg)
    }

    /// Lower the statements in a `describe`-like suite body.
    ///
    /// This recursively flattens nested suite organization into Rust test
    /// functions while preserving inherited setup statements, lifecycle hooks,
    /// and literal table bindings from enclosing `describe.each` rows.
    pub(in crate::lowering) fn describe_body_declarations<'a>(
        &mut self,
        statements: &'a [Statement<'a>],
        group_name: &str,
        inherited_setup: Vec<&'a Statement<'a>>,
        inherited_before_each: Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
        inherited_after_each: Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let class_scope = self.enter_test_suite_class_scope(statements);
        let mut items = Vec::new();
        let mut setup = inherited_setup;
        let mut before_each = inherited_before_each;
        let mut after_each = inherited_after_each;
        for statement in statements {
            if matches!(
                statement,
                Statement::VariableDeclaration(_) | Statement::FunctionDeclaration(_)
            ) {
                setup.push(statement);
                continue;
            }
            // A suite body may declare helper types shared by its test cases
            // (`describe(() => { class Pair {…}; it(…) })`). Classes, interfaces,
            // and type aliases register globally rather than re-running per test,
            // so lower them as suite-level items and let each test body reference
            // the registered type.
            if let Statement::ClassDeclaration(class) = statement {
                items.push(self.class_declaration(class)?);
                continue;
            }
            if let Statement::TSInterfaceDeclaration(interface) = statement {
                self.interface_declaration(interface)?;
                continue;
            }
            if let Statement::TSTypeAliasDeclaration(alias) = statement {
                self.type_alias_declaration(alias)?;
                continue;
            }
            if let Statement::IfStatement(if_stmt) = statement {
                items.extend(self.describe_if_statement_declarations(
                    if_stmt,
                    group_name,
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            if let Statement::ForOfStatement(for_of) = statement
                && let Some(unrolled) = self.describe_for_of_declarations(
                    for_of,
                    group_name,
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?
            {
                items.extend(unrolled);
                continue;
            }
            let Statement::ExpressionStatement(expr_stmt) = statement else {
                return Err(SmeltError::unsupported(
                    self.statement_span(statement),
                    "describe blocks only support direct it/test/describe calls for now",
                ));
            };
            if let Some(sequence_items) = self.describe_sequence_test_declarations(
                &expr_stmt.expression,
                group_name,
                &setup,
                &before_each,
                &after_each,
                table_bindings,
            )? {
                items.extend(sequence_items);
                continue;
            }
            if let Some(unrolled) = self.describe_foreach_declarations(
                &expr_stmt.expression,
                group_name,
                &setup,
                &before_each,
                &after_each,
                table_bindings,
            )? {
                items.extend(unrolled);
                continue;
            }
            if self.collect_lifecycle_hook(&expr_stmt.expression, &mut before_each, &mut after_each)
            {
                continue;
            }
            if let Some(table_call) = self.table_test_call(&expr_stmt.expression) {
                items.extend(self.table_test_declarations(
                    table_call,
                    Some(group_name),
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            if let Some(nested_describe) = self.describe_call(&expr_stmt.expression) {
                let nested_group_name = format!(
                    "{group_name} {}",
                    self.describe_group_name(nested_describe)?
                );
                items.extend(self.describe_declaration_with_name_and_bindings(
                    nested_describe,
                    &nested_group_name,
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            if self.dynamic_test_alias_call(&expr_stmt.expression) {
                continue;
            }
            if self.skipped_test_case_call(&expr_stmt.expression) {
                continue;
            }
            if let Some(test_call) = self.test_case_call(&expr_stmt.expression) {
                items.push(self.test_case_declaration(
                    test_call,
                    Some(group_name),
                    &setup,
                    &before_each,
                    &after_each,
                    table_bindings,
                )?);
                continue;
            }
            setup.push(statement);
        }
        self.restore_test_suite_class_scope(class_scope);
        Ok(items)
    }

    /// Enter the lexical class scope for a `describe` callback.
    ///
    /// Direct class declarations receive deterministic internal symbols before
    /// lowering. This lets recursive annotations such as `friends: Person[]`
    /// resolve locally without collapsing same-named classes from sibling
    /// callbacks into one emitted Rust type.
    fn enter_test_suite_class_scope(
        &mut self,
        statements: &[Statement<'_>],
    ) -> ClassScopeSnapshot {
        let scope = self.classes.snapshot_scope();
        for statement in statements {
            let Statement::ClassDeclaration(class) = statement else {
                continue;
            };
            let Some(id) = &class.id else {
                continue;
            };
            let internal_name = format!(
                "{}SmeltSuiteF{}S{}",
                id.name, self.file_id.0, class.span.start,
            );
            let symbol = self.intern_type_name(&internal_name);
            self.classes.bind_scoped_type_name(id.name.to_string(), symbol);
        }
        scope
    }

    /// Restore the enclosing lexical class lookup after a suite body.
    fn restore_test_suite_class_scope(&mut self, scope: ClassScopeSnapshot) {
        self.classes.restore_scope(scope);
    }

    /// Expand a comma expression made entirely of test registrations.
    ///
    /// JavaScript permits `test(...), test(...)` as one expression statement.
    /// Suite discovery runs before ordinary statement lowering, so each call must
    /// be registered here as its own generated test. Mixed comma expressions are
    /// rejected explicitly because treating the whole sequence as setup would
    /// silently discard embedded test registrations.
    pub(in crate::lowering) fn describe_sequence_test_declarations<'a>(
        &mut self,
        expression: &'a Expression<'a>,
        group_name: &str,
        setup: &[&'a Statement<'a>],
        before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Option<Vec<smelt_hir::ItemId>>, SmeltError> {
        let Expression::SequenceExpression(sequence) = expression else {
            return Ok(None);
        };
        let registration_count = sequence
            .expressions
            .iter()
            .filter(|item| {
                self.dynamic_test_alias_call(item)
                    || self.skipped_test_case_call(item)
                    || self.test_case_call(item).is_some()
            })
            .count();
        if registration_count == 0 {
            return Ok(None);
        }
        if registration_count != sequence.expressions.len() {
            return Err(SmeltError::unsupported(
                self.span(sequence.span.start, sequence.span.end),
                "mixed test registration and runtime sequence expressions are not supported",
            ));
        }
        let mut items = Vec::new();
        for sequence_item in &sequence.expressions {
            if self.dynamic_test_alias_call(sequence_item)
                || self.skipped_test_case_call(sequence_item)
            {
                continue;
            }
            let Some(test_call) = self.test_case_call(sequence_item) else {
                return Ok(None);
            };
            items.push(self.test_case_declaration(
                test_call,
                Some(group_name),
                setup,
                before_each,
                after_each,
                table_bindings,
            )?);
        }
        Ok(Some(items))
    }

    /// Unroll an `[...].forEach(item => { ...it/test/describe... })` suite loop.
    ///
    /// es-toolkit (and lodash-style suites) parameterize a family of tests over
    /// a literal list: `['a', 'b'].forEach(chr => it(`case ${chr}`, ...))`. Like
    /// `test.each`, Smelt emits one Rust test per element by binding the loop
    /// parameter to each literal element and re-running suite-body lowering over
    /// the callback statements. Returns `None` when the expression is not a
    /// loop-over-literal-array of supported test calls, so the caller can fall
    /// through to its other suite-statement handling.
    pub(in crate::lowering) fn describe_foreach_declarations<'a>(
        &mut self,
        expression: &'a Expression<'a>,
        group_name: &str,
        setup: &[&'a Statement<'a>],
        before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Option<Vec<smelt_hir::ItemId>>, SmeltError> {
        let Expression::CallExpression(call) = expression else {
            return Ok(None);
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "forEach" {
            return Ok(None);
        }
        let Expression::ArrayExpression(array) = &member.object else {
            return Ok(None);
        };
        let Some(Argument::ArrowFunctionExpression(arrow)) = call.arguments.first() else {
            return Ok(None);
        };
        // The callback must bind exactly the iterated element (an optional index
        // parameter is not modeled), and its body must hold suite statements.
        let [param] = arrow.params.items.as_slice() else {
            return Ok(None);
        };
        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
            return Ok(None);
        };
        let param_name = binding.name.as_str();
        self.unroll_test_loop(
            param_name,
            array.elements.iter(),
            arrow_block_statements(arrow),
            group_name,
            setup,
            before_each,
            after_each,
            table_bindings,
        )
        .map(Some)
    }

    /// Unroll a `for (const item of [...]) { ...it/test/describe... }` loop.
    ///
    /// The block-statement counterpart to [`Self::describe_foreach_declarations`];
    /// binds the loop variable to each literal element of the iterated array and
    /// re-runs suite-body lowering. Returns `None` when the loop does not iterate
    /// a literal array with a single identifier binding so the caller reports the
    /// usual unsupported-suite-statement diagnostic.
    pub(in crate::lowering) fn describe_for_of_declarations<'a>(
        &mut self,
        for_of: &'a oxc::ast::ast::ForOfStatement<'a>,
        group_name: &str,
        setup: &[&'a Statement<'a>],
        before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Option<Vec<smelt_hir::ItemId>>, SmeltError> {
        let Expression::ArrayExpression(array) = &for_of.right else {
            return Ok(None);
        };
        let ForStatementLeft::VariableDeclaration(declaration) = &for_of.left else {
            return Ok(None);
        };
        let [declarator] = declaration.declarations.as_slice() else {
            return Ok(None);
        };
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return Ok(None);
        };
        let Statement::BlockStatement(block) = &for_of.body else {
            return Ok(None);
        };
        let param_name = binding.name.as_str();
        self.unroll_test_loop(
            param_name,
            array.elements.iter(),
            &block.body,
            group_name,
            setup,
            before_each,
            after_each,
            table_bindings,
        )
        .map(Some)
    }

    /// Emit one suite-body lowering per literal element of an unrolled loop.
    ///
    /// Shared by `forEach` and `for…of` suite loops: for each array element the
    /// loop parameter is bound (as a `test.each`-style table binding) and the
    /// loop-body statements are lowered as a suite body. Spread/hole elements
    /// are not constant rows, so encountering one aborts unrolling with the
    /// usual unsupported diagnostic rather than silently dropping cases.
    #[expect(
        clippy::too_many_arguments,
        reason = "suite-body lowering threads inherited setup, both lifecycle-hook lists, table bindings, and the unrolled loop parameter"
    )]
    pub(in crate::lowering) fn unroll_test_loop<'a>(
        &mut self,
        param_name: &'a str,
        elements: impl Iterator<Item = &'a ArrayExpressionElement<'a>>,
        body: &'a [Statement<'a>],
        group_name: &str,
        setup: &[&'a Statement<'a>],
        before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let mut items = Vec::new();
        for (case_index, element) in elements.enumerate() {
            if matches!(
                element,
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "describe blocks only support direct it/test/describe calls for now",
                ));
            }
            // Disambiguate per-iteration test-function names: a template name
            // built from the loop variable can sanitize to the same identifier
            // for distinct elements (`` ` `` and `/` both strip to nothing), so
            // give each iteration a distinct group segment as `test.each` does.
            let case_group = format!("{group_name} case {case_index}");
            let mut bindings = table_bindings.to_vec();
            bindings.push((param_name, TableBindingValue::Element(element)));
            items.extend(self.describe_body_declarations(
                body,
                &case_group,
                setup.to_vec(),
                before_each.to_vec(),
                after_each.to_vec(),
                &bindings,
            )?);
        }
        Ok(items)
    }

    /// Lower tests nested inside a suite-level conditional.
    ///
    /// Date-fns uses browser guards in `describe` blocks. When Smelt can prove
    /// the guard is false for the native Rust target, only the alternate branch
    /// is considered; otherwise both branches are scanned for tests.
    pub(in crate::lowering) fn describe_if_statement_declarations<'a>(
        &mut self,
        if_stmt: &'a oxc::ast::ast::IfStatement<'a>,
        group_name: &str,
        setup: &[&'a Statement<'a>],
        before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let mut items = Vec::new();
        let include_consequent = !Self::describe_condition_is_native_false(&if_stmt.test);
        let include_alternate = Self::describe_condition_is_native_false(&if_stmt.test)
            || !Self::describe_condition_is_native_true(&if_stmt.test);
        if include_consequent {
            items.extend(self.describe_branch_declarations(
                &if_stmt.consequent,
                group_name,
                setup,
                before_each,
                after_each,
                table_bindings,
            )?);
        }
        if include_alternate && let Some(alternate) = &if_stmt.alternate {
            items.extend(self.describe_branch_declarations(
                alternate,
                group_name,
                setup,
                before_each,
                after_each,
                table_bindings,
            )?);
        }
        Ok(items)
    }

    /// Lower a statement that appears as one branch of a suite-level `if`.
    pub(in crate::lowering) fn describe_branch_declarations<'a>(
        &mut self,
        statement: &'a Statement<'a>,
        group_name: &str,
        setup: &[&'a Statement<'a>],
        before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        match statement {
            Statement::BlockStatement(block) => self.describe_body_declarations(
                &block.body,
                group_name,
                setup.to_vec(),
                before_each.to_vec(),
                after_each.to_vec(),
                table_bindings,
            ),
            other => Err(SmeltError::unsupported(
                self.statement_span(other),
                "suite-level conditional branches must be blocks",
            )),
        }
    }

    /// Lower a nested `describe` while carrying inherited table bindings.
    pub(in crate::lowering) fn describe_declaration_with_name_and_bindings<'a>(
        &mut self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
        group_name: &str,
        inherited_setup: &[&'a Statement<'a>],
        inherited_before_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        inherited_after_each: &[&'a oxc::ast::ast::ArrowFunctionExpression<'a>],
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let statements = self.test_suite_callback_statements(call, "describe callbacks")?;
        self.describe_body_declarations(
            statements,
            group_name,
            inherited_setup.to_vec(),
            inherited_before_each.to_vec(),
            inherited_after_each.to_vec(),
            table_bindings,
        )
    }

    /// Return whether an expression is a dynamic test alias call.
    pub(in crate::lowering) fn dynamic_test_alias_call(&self, expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return false;
        };
        if self.imports.is_test_builtin(callee.name.as_str()) || call.arguments.len() < 2 {
            return false;
        }
        call.arguments.get(1).is_some_and(|argument| {
            self.test_arrow_callback(argument, "dynamic test alias")
                .is_ok()
        })
    }

    /// Lower a top-level Vitest `test` / `it` call into an HIR test function.
    pub(in crate::lowering) fn test_case_declaration(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        group_name: Option<&str>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        table_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test case calls require a string name",
            )
        })?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test case calls require a callback",
            )
        })?;
        let test_name = self.test_case_name(name_arg, group_name, table_bindings, setup)?;
        match body_arg {
            Argument::ArrowFunctionExpression(arrow) => self.test_function_from_arrow(
                &test_name,
                self.span(call.span.start, call.span.end),
                arrow,
                setup,
                before_each,
                after_each,
                table_bindings,
            ),
            Argument::FunctionExpression(function) => self.test_function_from_function(
                &test_name,
                self.span(call.span.start, call.span.end),
                function,
                setup,
                before_each,
                after_each,
                table_bindings,
            ),
            _ => Err(SmeltError::unsupported(
                self.span(body_arg.span().start, body_arg.span().end),
                "test case callbacks must be functions",
            )),
        }
    }

    /// Snapshot the class-registry names present before a test case is lowered.
    ///
    /// Constructor functions declared inside a test case (`function Foo(){}`
    /// used as `new Foo()` / `instanceof Foo` / `Foo.prototype.x = …`) are
    /// synthesized into the class registry during per-case predeclaration and
    /// setup replay, but they are *block-local*: a differently-shaped
    /// `function Foo` in a sibling `it` must synthesize its own class rather than
    /// reuse a stale one. `self.locals` is already scoped per case; the class
    /// registry is not. Pairing this snapshot with
    /// [`Self::restore_test_case_class_scope`] drops any class added during the
    /// case while preserving module-level and imported classes.
    fn test_case_class_scope(&self) -> ClassNameSnapshot {
        self.classes.snapshot_names()
    }

    /// Drop class-registry entries added while lowering a test case, restoring
    /// the block-local class scope captured by [`Self::test_case_class_scope`].
    fn restore_test_case_class_scope(&mut self, scope: &ClassNameSnapshot) {
        self.classes.retain_names(scope);
    }

    /// Lower a prepared test callback into an HIR test function.
    pub(in crate::lowering) fn test_function_from_arrow(
        &mut self,
        test_name: &str,
        span: Span,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        table_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let saved_locals = self.scope.take_bindings();
        let saved_date_value_locals = self.scope.take_date_values();
        let saved_callable_local_props = self.scope.take_callable_prop_writes();
        let saved_explicit_any_locals = self.scope.take_explicit_any();
        // Flow-narrowing facts are keyed by NAME, so a fact recorded in one test
        // body (e.g. `array1 = /c/.exec(...)` observing `Optional<SmeltMatch>`)
        // must not leak into a sibling test that declares its own same-named
        // binding — a stale optional narrowing would turn its indexed writes
        // into non-assignable optional projections. Scope them per test case
        // exactly like `self.locals`.
        let saved_narrowed_locals = self.scope.take_narrowings();
        let saved_async = self.current_async;
        let class_scope = self.test_case_class_scope();
        self.current_async = arrow.r#async;
        let mut body = Body::new(None, self.arrow_body_span(arrow));
        let mut errors = Vec::new();
        for (name, value) in table_bindings {
            if let Err(error) = self.bind_table_value(name, *value, &mut body) {
                errors.push(error);
            }
        }
        if let Err(error) =
            self.synthesize_setup_constructor_functions(setup, arrow_block_statements(arrow))
        {
            errors.push(error);
        }
        for statement in setup {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in before_each {
            for statement in arrow_block_statements(hook) {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        if let Err(error) =
            self.predeclare_local_function_declarations(arrow_block_statements(arrow), &mut body)
        {
            errors.push(error);
        }
        if let Err(error) = self.predeclare_local_arrow_callbacks(arrow_block_statements(arrow), &mut body)
        {
            errors.push(error);
        }
        for statement in arrow_block_statements(arrow) {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in after_each {
            for statement in arrow_block_statements(hook) {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        self.scope.restore_bindings(saved_locals);
        self.scope.restore_date_values(saved_date_value_locals);
        self.scope.restore_callable_prop_writes(saved_callable_local_props);
        self.scope.restore_explicit_any(saved_explicit_any_locals);
        self.scope.restore_narrowings(saved_narrowed_locals);
        self.current_async = saved_async;
        self.restore_test_case_class_scope(&class_scope);
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        // A test callback is async when its source spelled `async`, or when the
        // lowered body inlined an `await` (e.g. the `expect(...).rejects.toThrow`
        // async matcher desugars into an awaited try/catch even from a non-async
        // `() => expect(...).rejects.toThrow()` callback, which in JavaScript just
        // returns the pending promise for the framework to await).
        let is_async = arrow.r#async || Self::body_contains_await(&body);
        if is_async {
            body.build_async_state_machine();
        }

        let name = self.intern_source_name(test_name);
        let body_id = self.ctx.krate.push_body(body);
        let none = self.ctx.krate.types.intern(Type::None);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            // Synthetic test wrapper carries no free-function type params.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
            return_ty: none,
            is_async,
            is_test: true,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(test_name.to_owned(), item);
        Ok(item)
    }

    /// Lower a prepared `function () { ... }` test callback into an HIR test function.
    pub(in crate::lowering) fn test_function_from_function(
        &mut self,
        test_name: &str,
        span: Span,
        function: &oxc::ast::ast::Function<'_>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        table_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "test case function callbacks must have a body",
            ));
        };
        if !function.params.items.is_empty() || function.params.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(function.params.span.start, function.params.span.end),
                "test case function callbacks with parameters are not lowered yet",
            ));
        }

        let saved_locals = self.scope.take_bindings();
        let saved_date_value_locals = self.scope.take_date_values();
        let saved_callable_local_props = self.scope.take_callable_prop_writes();
        let saved_explicit_any_locals = self.scope.take_explicit_any();
        // Flow-narrowing facts are keyed by NAME, so a fact recorded in one test
        // body (e.g. `array1 = /c/.exec(...)` observing `Optional<SmeltMatch>`)
        // must not leak into a sibling test that declares its own same-named
        // binding — a stale optional narrowing would turn its indexed writes
        // into non-assignable optional projections. Scope them per test case
        // exactly like `self.locals`.
        let saved_narrowed_locals = self.scope.take_narrowings();
        let saved_async = self.current_async;
        let class_scope = self.test_case_class_scope();
        self.current_async = function.r#async;
        // A `function () { ... }` test callback (as opposed to an arrow) has its
        // own `arguments` binding, so make the array-like `arguments` object
        // available while lowering the body — matching the other function-body
        // lowering paths. The callback takes no parameters here, so its arity is
        // zero.
        self.current_arguments_arities
            .push(function.params.items.len());
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut errors = Vec::new();
        for (name, value) in table_bindings {
            if let Err(error) = self.bind_table_value(name, *value, &mut body) {
                errors.push(error);
            }
        }
        if let Err(error) =
            self.synthesize_setup_constructor_functions(setup, &function_body.statements)
        {
            errors.push(error);
        }
        for statement in setup {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in before_each {
            for statement in arrow_block_statements(hook) {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        if let Err(error) =
            self.predeclare_local_function_declarations(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        if let Err(error) =
            self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for statement in &function_body.statements {
            if let Err(error) = self.test_case_statement(statement, &mut body) {
                errors.push(error);
            }
        }
        for hook in after_each {
            for statement in arrow_block_statements(hook) {
                if let Err(error) = self.test_case_statement(statement, &mut body) {
                    errors.push(error);
                }
            }
        }
        self.current_arguments_arities.pop();
        self.scope.restore_bindings(saved_locals);
        self.scope.restore_date_values(saved_date_value_locals);
        self.scope.restore_callable_prop_writes(saved_callable_local_props);
        self.scope.restore_explicit_any(saved_explicit_any_locals);
        self.scope.restore_narrowings(saved_narrowed_locals);
        self.current_async = saved_async;
        self.restore_test_case_class_scope(&class_scope);
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        // See `test_function_from_arrow`: async when spelled `async` or when the
        // lowered body inlined an `await` (async matcher desugaring).
        let is_async = function.r#async || Self::body_contains_await(&body);
        if is_async {
            body.build_async_state_machine();
        }

        let name = self.intern_source_name(test_name);
        let body_id = self.ctx.krate.push_body(body);
        let none = self.ctx.krate.types.intern(Type::None);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            // Synthetic test wrapper carries no free-function type params.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
            return_ty: none,
            is_async,
            is_test: true,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(test_name.to_owned(), item);
        Ok(item)
    }

    /// Lower `test.each` / `it.each` table rows into one Rust test per row.
    pub(in crate::lowering) fn table_test_declarations(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        group_name: Option<&str>,
        setup: &[&Statement<'_>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'_>],
        inherited_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let Expression::CallExpression(each_call) = &call.callee else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "table tests must call test.each(...) or describe.each(...)",
            ));
        };
        let Expression::StaticMemberExpression(each_member) = &each_call.callee else {
            return Err(SmeltError::unsupported(
                self.span(each_call.span.start, each_call.span.end),
                "table tests must call test.each(...) or describe.each(...)",
            ));
        };
        let Expression::Identifier(test_api) = &each_member.object else {
            return Err(SmeltError::unsupported(
                self.span(each_member.span.start, each_member.span.end),
                "table tests must be called on test, it, or describe",
            ));
        };
        let rows = self.table_rows(each_call)?;
        if test_api.name == "describe" {
            return self.describe_each_declarations(
                call,
                group_name,
                &rows,
                setup,
                before_each,
                after_each,
                inherited_bindings,
            );
        }
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test.each calls require a string name",
            )
        })?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "test.each calls require a callback",
            )
        })?;
        let arrow = self.test_arrow_callback_with_params(body_arg, "test.each callbacks", true)?;
        let mut items = Vec::new();
        for (case_index, row) in rows.iter().enumerate() {
            let case_group = group_name.map(|name| format!("{name} case {case_index}"));
            let mut bindings = inherited_bindings.to_vec();
            bindings.extend(self.table_bindings(arrow, row)?);
            let test_name =
                self.test_case_name(name_arg, case_group.as_deref(), &bindings, setup)?;
            items.push(self.test_function_from_arrow(
                &test_name,
                self.span(call.span.start, call.span.end),
                arrow,
                setup,
                before_each,
                after_each,
                &bindings,
            )?);
        }
        Ok(items)
    }

    /// Lower `describe.each` by flattening each row's nested suite body.
    pub(in crate::lowering) fn describe_each_declarations<'a>(
        &mut self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
        group_name: Option<&str>,
        rows: &[Vec<&'a ArrayExpressionElement<'a>>],
        setup: &[&Statement<'a>],
        before_each: &[&oxc::ast::ast::ArrowFunctionExpression<'a>],
        after_each: &[&oxc::ast::ast::ArrowFunctionExpression<'a>],
        inherited_bindings: &[(&'a str, TableBindingValue<'a>)],
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let name_arg = call.arguments.first().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe.each calls require a string name",
            )
        })?;
        let body_arg = call.arguments.get(1).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "describe.each calls require a callback",
            )
        })?;
        let arrow =
            self.test_arrow_callback_with_params(body_arg, "describe.each callbacks", true)?;
        let mut items = Vec::new();
        for (case_index, row) in rows.iter().enumerate() {
            let row_group = self.test_title(name_arg)?;
            let row_group = group_name.map_or_else(
                || format!("{row_group} case {case_index}"),
                |parent| format!("{parent} {row_group} case {case_index}"),
            );
            let mut bindings = inherited_bindings.to_vec();
            bindings.extend(self.table_bindings(arrow, row)?);
            items.extend(self.describe_body_declarations(
                arrow_block_statements(arrow),
                &row_group,
                setup.to_vec(),
                before_each.to_vec(),
                after_each.to_vec(),
                &bindings,
            )?);
        }
        Ok(items)
    }

    /// Parse the table literal from an `.each([...])` call.
    ///
    /// Nested array elements represent multi-argument rows. Scalar elements
    /// use Vitest's shorthand for a one-argument row.
    pub(in crate::lowering) fn table_rows<'a>(
        &self,
        each_call: &'a oxc::ast::ast::CallExpression<'a>,
    ) -> Result<Vec<Vec<&'a ArrayExpressionElement<'a>>>, SmeltError> {
        let [Argument::ArrayExpression(table)] = each_call.arguments.as_slice() else {
            return Ok(Vec::new());
        };
        let mut rows = Vec::new();
        for element in &table.elements {
            match element {
                ArrayExpressionElement::ArrayExpression(row) => {
                    rows.push(row.elements.iter().collect());
                }
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "table test rows do not support spreads or elisions",
                    ));
                }
                _ => rows.push(vec![element]),
            }
        }
        Ok(rows)
    }

    /// Pair callback parameter names with one table row.
    pub(in crate::lowering) fn table_bindings<'a>(
        &self,
        arrow: &'a oxc::ast::ast::ArrowFunctionExpression<'a>,
        row: &[&'a ArrayExpressionElement<'a>],
    ) -> Result<Vec<(&'a str, TableBindingValue<'a>)>, SmeltError> {
        if arrow.params.items.len() != row.len() {
            return Err(SmeltError::unsupported(
                self.span(arrow.params.span.start, arrow.params.span.end),
                "table test callback parameter count must match row width",
            ));
        }
        let mut bindings = Vec::new();
        for (param, value) in arrow.params.items.iter().zip(row) {
            match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => {
                    bindings.push((binding.name.as_str(), TableBindingValue::Element(value)));
                }
                BindingPattern::ObjectPattern(pattern) => {
                    let ArrayExpressionElement::ObjectExpression(object) = value else {
                        return Err(SmeltError::unsupported(
                            self.span(value.span().start, value.span().end),
                            "object table parameters require object literal rows",
                        ));
                    };
                    for property in &pattern.properties {
                        if property.computed {
                            return Err(SmeltError::unsupported(
                                self.span(property.span.start, property.span.end),
                                "computed table test callback parameters are not lowered yet",
                            ));
                        }
                        let key_text = match &property.key {
                            PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                            PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                            _ => {
                                return Err(SmeltError::unsupported(
                                    self.span(property.key.span().start, property.key.span().end),
                                    "table test object parameter keys must be static strings",
                                ));
                            }
                        };
                        let BindingPattern::BindingIdentifier(binding) = &property.value else {
                            return Err(SmeltError::unsupported(
                                self.span(property.value.span().start, property.value.span().end),
                                "nested table test callback parameter destructuring is not lowered yet",
                            ));
                        };
                        let row_value = object.properties.iter().find_map(|row_property| {
                            let ObjectPropertyKind::ObjectProperty(row_property) = row_property
                            else {
                                return None;
                            };
                            let row_key_text = match &row_property.key {
                                PropertyKey::StaticIdentifier(identifier) => {
                                    identifier.name.as_str()
                                }
                                PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                                _ => return None,
                            };
                            (row_key_text == key_text).then_some(&row_property.value)
                        });
                        let Some(row_value) = row_value else {
                            return Err(SmeltError::unsupported(
                                self.span(property.span.start, property.span.end),
                                "table test row object is missing a destructured key",
                            ));
                        };
                        bindings.push((
                            binding.name.as_str(),
                            TableBindingValue::ObjectField(row_value),
                        ));
                    }
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "table test callback parameters must be identifiers or object patterns",
                    ));
                }
            }
        }
        Ok(bindings)
    }

    /// Bind one `test.each` row value to a local used by the callback body.
    pub(in crate::lowering) fn bind_table_value(
        &mut self,
        name: &str,
        value: TableBindingValue<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let expr = match value {
            TableBindingValue::Element(value) => self.array_element(value, body)?,
            TableBindingValue::ObjectField(value) => self.expression(value, body)?,
        };
        let ty = Self::expr_ty(body, expr);
        let symbol = self.intern_source_name(name);
        let span = match value {
            TableBindingValue::Element(value) => value.span(),
            TableBindingValue::ObjectField(value) => value.span(),
        };
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(span.start, span.end),
        });
        self.scope.bind(name.to_owned(), local);
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt(Stmt::Let {
            pat,
            ty,
            value: Some(expr),
        });
        Ok(())
    }

    /// Convert a test-case name argument into a stable Rust function name.
    ///
    /// Suite-level `const NAME = "literal"` declarations from the inherited
    /// `setup` are folded into the title-resolution bindings so a computed name
    /// like `` it(`\`_.${methodName}\` should …`) `` derives a stable Rust
    /// test-function name. These const bindings only feed title folding; they
    /// are never bound as runtime values (the original `const` already lowers
    /// into the test body), so they cannot collide with row/loop bindings.
    pub(in crate::lowering) fn test_case_name<'a>(
        &self,
        argument: &Argument<'a>,
        group_name: Option<&str>,
        table_bindings: &[(&'a str, TableBindingValue<'a>)],
        setup: &[&'a Statement<'a>],
    ) -> Result<String, SmeltError> {
        let mut title_bindings = table_bindings.to_vec();
        title_bindings.extend(Self::suite_const_string_bindings(setup));
        let case_name = self.test_title_with_bindings(argument, &title_bindings)?;
        let full_name = group_name.map_or_else(
            || case_name.clone(),
            |group_name| format!("{group_name} {case_name}"),
        );
        Ok(format!(
            "test_{}",
            sanitize_test_name(&full_name).unwrap_or_else(|| "case".to_owned())
        ))
    }

    /// Collect `const NAME = "literal"` suite-setup declarations as title
    /// bindings.
    ///
    /// Suites commonly alias the function-under-test's name once
    /// (`const methodName = 'pull'`) and interpolate it into each test title.
    /// Each such constant is exposed as an [`TableBindingValue::ObjectField`]
    /// over its initializer expression so [`Self::template_expression_text`]
    /// folds the interpolation to constant text. Only string-literal (and
    /// expression-free template-literal) initializers qualify, matching what
    /// title folding can resolve.
    pub(in crate::lowering) fn suite_const_string_bindings<'a>(
        setup: &[&'a Statement<'a>],
    ) -> Vec<(&'a str, TableBindingValue<'a>)> {
        let mut bindings = Vec::new();
        for statement in setup {
            let Statement::VariableDeclaration(declaration) = statement else {
                continue;
            };
            for declarator in &declaration.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let Some(init) = &declarator.init else {
                    continue;
                };
                // Scalar literals fold directly; array literals are kept so that
                // a `${expected[1]}` index into a const row resolves through the
                // computed-member arm of `template_expression_text`.
                let is_foldable = matches!(
                    init,
                    Expression::StringLiteral(_)
                        | Expression::TemplateLiteral(_)
                        | Expression::NumericLiteral(_)
                        | Expression::BooleanLiteral(_)
                        | Expression::ArrayExpression(_)
                );
                if is_foldable {
                    bindings.push((binding.name.as_str(), TableBindingValue::ObjectField(init)));
                }
            }
        }
        bindings
    }

    /// Extract a string title from a test-framework name argument.
    pub(in crate::lowering) fn test_title(
        &self,
        argument: &Argument<'_>,
    ) -> Result<String, SmeltError> {
        self.test_title_with_bindings(argument, &[])
    }

    /// Extract a string title, resolving template expressions against bindings.
    ///
    /// String literals and identifiers map straight to their text. A template
    /// literal interleaves its static quasis with each interpolated expression,
    /// resolving every expression to constant text through
    /// [`Self::template_expression_text`]. This lets loop-unrolled suites
    /// (`[...].forEach(x => it(`name ${x}`, ...))`) derive a distinct, stable
    /// Rust test-function name per iteration from the bound literal element,
    /// rather than rejecting computed names outright.
    pub(in crate::lowering) fn test_title_with_bindings(
        &self,
        argument: &Argument<'_>,
        table_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Result<String, SmeltError> {
        match argument {
            Argument::StringLiteral(name) => Ok(name.value.to_string()),
            Argument::Identifier(identifier) => Ok(identifier.name.to_string()),
            Argument::TemplateLiteral(template) => {
                let mut text = String::new();
                for (index, quasi) in template.quasis.iter().enumerate() {
                    text.push_str(quasi.value.raw.as_str());
                    if let Some(expression) = template.expressions.get(index) {
                        let Some(resolved) =
                            Self::template_expression_text(expression, table_bindings)
                        else {
                            return Err(SmeltError::unsupported(
                                self.span(expression.span().start, expression.span().end),
                                "test case names must be string literals or identifiers",
                            ));
                        };
                        text.push_str(&resolved);
                    }
                }
                Ok(text)
            }
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "test case names must be string literals or identifiers",
            )),
        }
    }

    /// Resolve one template-literal interpolation to constant text, if possible.
    ///
    /// Literals fold to their own spelling. A bare identifier resolves through
    /// the active table bindings: a `forEach`/`for…of` loop variable bound to a
    /// literal array element folds to that element's constant text. Anything
    /// whose value is not statically known returns `None` so the caller reports
    /// an unsupported computed test name.
    pub(in crate::lowering) fn template_expression_text(
        expression: &Expression<'_>,
        table_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Option<String> {
        match expression {
            Expression::StringLiteral(literal) => Some(literal.value.to_string()),
            Expression::NumericLiteral(literal) => Some(literal.value.to_string()),
            Expression::BooleanLiteral(literal) => Some(literal.value.to_string()),
            // `undefined`/`null` stringify to their JavaScript spellings, which
            // is what a title interpolation embeds (`return \`${expected[1]}\``).
            Expression::NullLiteral(_) => Some("null".to_owned()),
            Expression::TemplateLiteral(template) if template.expressions.is_empty() => Some(
                template
                    .quasis
                    .iter()
                    .map(|quasi| quasi.value.raw.as_str())
                    .collect::<String>(),
            ),
            Expression::Identifier(identifier) => {
                if identifier.name == "undefined" {
                    return Some("undefined".to_owned());
                }
                let value = table_bindings.iter().rev().find_map(|(name, value)| {
                    (*name == identifier.name.as_str()).then_some(value)
                })?;
                match value {
                    TableBindingValue::Element(element) => {
                        Self::array_element_constant_text(element)
                    }
                    TableBindingValue::ObjectField(field) => {
                        Self::template_expression_text(field, table_bindings)
                    }
                }
            }
            // `arr[N]` folds when `arr` is a const array binding and `N` is a
            // literal index: resolve the binding to its array expression and
            // fold the indexed element to constant text.
            Expression::ComputedMemberExpression(member) => {
                let Expression::Identifier(array_ident) = &member.object else {
                    return None;
                };
                let Expression::NumericLiteral(index_literal) = &member.expression else {
                    return None;
                };
                let value = table_bindings.iter().rev().find_map(|(name, value)| {
                    (*name == array_ident.name.as_str()).then_some(value)
                })?;
                let TableBindingValue::ObjectField(Expression::ArrayExpression(array)) = value
                else {
                    return None;
                };
                let raw = index_literal.value;
                if !raw.is_finite() || raw.fract() != 0.0 || raw < 0.0 {
                    return None;
                }
                // `raw` is a non-negative whole finite number, so converting it to
                // an array index is exact (indices are far below 2^53). `usize` has
                // no `TryFrom<f64>`, so a guarded `as` cast is the conversion here.
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "raw is a guarded non-negative whole finite f64, so the index conversion is exact; usize has no TryFrom<f64>"
                )]
                let index = raw as usize;
                let element = array.elements.get(index)?;
                Self::array_element_constant_text(element)
            }
            Expression::ParenthesizedExpression(paren) => {
                Self::template_expression_text(&paren.expression, table_bindings)
            }
            // `${cond ? "a" : "b"}` folds when the guard is a statically known
            // truthiness, as in loop-unrolled suites that vary a title segment by
            // the bound index (`${index ? " and …" : ""}`).
            Expression::ConditionalExpression(conditional) => {
                let branch = if Self::template_expression_truthy(&conditional.test, table_bindings)?
                {
                    &conditional.consequent
                } else {
                    &conditional.alternate
                };
                Self::template_expression_text(branch, table_bindings)
            }
            _ => None,
        }
    }

    /// Evaluate a template-interpolation guard to a static truthiness, if known.
    ///
    /// Only the cases title folding needs are modeled: numeric literals (`0` is
    /// falsy), boolean literals, string literals (empty is falsy), and
    /// identifiers resolved through the active bindings (a loop-bound element).
    /// Returns `None` when truthiness is not statically decidable so the caller
    /// reports an unsupported computed name rather than guessing.
    pub(in crate::lowering) fn template_expression_truthy(
        expression: &Expression<'_>,
        table_bindings: &[(&str, TableBindingValue<'_>)],
    ) -> Option<bool> {
        match expression {
            Expression::NumericLiteral(literal) => Some(literal.value != 0.0),
            Expression::BooleanLiteral(literal) => Some(literal.value),
            Expression::StringLiteral(literal) => Some(!literal.value.is_empty()),
            Expression::ParenthesizedExpression(paren) => {
                Self::template_expression_truthy(&paren.expression, table_bindings)
            }
            Expression::Identifier(_) => {
                let text = Self::template_expression_text(expression, table_bindings)?;
                // Reuse the constant text the binding folds to, then apply
                // JavaScript-style truthiness to the literal it represents.
                Some(!matches!(text.as_str(), "" | "0" | "false"))
            }
            _ => None,
        }
    }

    /// Fold a literal array-literal element to its constant text, if possible.
    pub(in crate::lowering) fn array_element_constant_text(
        element: &ArrayExpressionElement<'_>,
    ) -> Option<String> {
        match element {
            ArrayExpressionElement::StringLiteral(literal) => Some(literal.value.to_string()),
            ArrayExpressionElement::NumericLiteral(literal) => Some(literal.value.to_string()),
            ArrayExpressionElement::BooleanLiteral(literal) => Some(literal.value.to_string()),
            ArrayExpressionElement::NullLiteral(_) => Some("null".to_owned()),
            ArrayExpressionElement::Identifier(identifier) if identifier.name == "undefined" => {
                Some("undefined".to_owned())
            }
            ArrayExpressionElement::TemplateLiteral(template)
                if template.expressions.is_empty() =>
            {
                Some(
                    template
                        .quasis
                        .iter()
                        .map(|quasi| quasi.value.raw.as_str())
                        .collect::<String>(),
                )
            }
            _ => None,
        }
    }

    /// Extract and validate an arrow callback for supported test-framework calls.
    pub(in crate::lowering) fn test_arrow_callback<'a>(
        &self,
        argument: &'a Argument<'a>,
        context: &str,
    ) -> Result<&'a oxc::ast::ast::ArrowFunctionExpression<'a>, SmeltError> {
        self.test_arrow_callback_with_params(argument, context, false)
    }

    /// Extract a suite callback body from `describe(...)`.
    ///
    /// Vitest accepts options between the title and callback, as in
    /// `describe("name", { concurrent: false }, () => {})`. Suite lowering
    /// only needs the callback statements, so this scans arguments after the
    /// title and returns the final function-like callback.
    pub(in crate::lowering) fn test_suite_callback_statements<'a>(
        &self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
        context: &str,
    ) -> Result<&'a [Statement<'a>], SmeltError> {
        for argument in call.arguments.iter().skip(1).rev() {
            match argument {
                Argument::ArrowFunctionExpression(arrow) => {
                    if !arrow.params.items.is_empty() {
                        return Err(SmeltError::unsupported(
                            self.span(arrow.params.span.start, arrow.params.span.end),
                            format!("{context} with parameters are not lowered yet"),
                        ));
                    }
                    return Ok(arrow_block_statements(arrow));
                }
                Argument::FunctionExpression(function) => {
                    if !function.params.items.is_empty() || function.params.rest.is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(function.params.span.start, function.params.span.end),
                            format!("{context} with parameters are not lowered yet"),
                        ));
                    }
                    let Some(function_body) = &function.body else {
                        return Err(SmeltError::unsupported(
                            self.span(function.span.start, function.span.end),
                            format!("{context} must have a body"),
                        ));
                    };
                    return Ok(&function_body.statements);
                }
                _ => {}
            }
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "describe calls require a callback",
        ))
    }

    /// Extract and validate an arrow callback, optionally allowing table-test parameters.
    pub(in crate::lowering) fn test_arrow_callback_with_params<'a>(
        &self,
        argument: &'a Argument<'a>,
        context: &str,
        allow_params: bool,
    ) -> Result<&'a oxc::ast::ast::ArrowFunctionExpression<'a>, SmeltError> {
        let Argument::ArrowFunctionExpression(arrow) = argument else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("{context} must be arrow functions"),
            ));
        };
        if !allow_params && !arrow.params.items.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(arrow.params.span.start, arrow.params.span.end),
                format!("{context} with parameters are not lowered yet"),
            ));
        }
        Ok(arrow)
    }

    /// Lower one supported statement inside a test case callback.
    pub(in crate::lowering) fn test_case_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        if let Statement::ExpressionStatement(expr_stmt) = statement
            && let Expression::CallExpression(call) = &expr_stmt.expression
            && self.expect_matcher_statement(call, body)?
        {
            return Ok(());
        }
        if let Statement::ExpressionStatement(expr_stmt) = statement
            && let Expression::CallExpression(call) = &expr_stmt.expression
            && self.deep_strict_equal_statement(call, body)?
        {
            return Ok(());
        }
        self.statement(statement, body)
    }

    // Continued in the next split builder file.
}
