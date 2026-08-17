//! JavaScript constructor-function lowering into synthesized HIR classes.
//!
//! TypeScript/JavaScript lets a plain `function` declaration act as a class:
//! `new Foo()` constructs an instance whose own properties are the `this.x = …`
//! assignments in the function body, while `Foo.prototype.m = function () { … }`
//! adds instance methods. This module materializes that shape once so ordinary
//! class construction and `instanceof` lowering need no special use-site path.

use crate::lowering::{
    AssignmentTarget, BinaryOperator, BindingPattern, Body, Class, Declaration, Expression, Field,
    Function, FunctionOwner, HashSet, Item, LocalDecl, MethodSig, ModuleBuilder, Param, ParamSig,
    Program, SmeltError, Span, Statement, Type, Visibility,
};

impl ModuleBuilder<'_> {
    /// Return whether a statement list constructs, type-tests, or extends the
    /// prototype of a function named `name`, marking it as a constructor function.
    ///
    /// A bare `function Foo(){}` that is only ever called normally is left as a
    /// plain function; it only becomes a class when the surrounding scope uses it
    /// with `new`, `instanceof`, or `Foo.prototype.x = …`.
    pub(in crate::lowering) fn statements_use_function_as_constructor(
        name: &str,
        statements: &[Statement<'_>],
    ) -> bool {
        statements
            .iter()
            .any(|statement| Self::statement_uses_constructor_function(name, statement))
    }

    /// Collect module top-level `function Foo(){}` declarations that this module
    /// uses as constructor functions.
    ///
    /// Module scope differs from a function body in two ways, and both defeat
    /// the plain sibling-statement scan: the declaration may be wrapped in
    /// `export`, and the `new Foo()` use normally lives inside *another*
    /// top-level function's body rather than beside the declaration. Names are
    /// collected in a prepass so [`Self::synthesize_constructor_function_class`]
    /// can run instead of ordinary function-item lowering, and so `new Foo()`
    /// sites lowered before the synthesis still resolve nominally through
    /// `pending_class_names`.
    pub(in crate::lowering) fn module_constructor_function_names(
        program: &Program<'_>,
    ) -> HashSet<String> {
        let mut names = HashSet::new();
        for function in Self::module_function_declarations(program) {
            let Some(id) = &function.id else {
                continue;
            };
            // An ambient `declare function` has no body to derive fields from.
            if function.declare || function.body.is_none() {
                continue;
            }
            let name = id.name.as_str();
            if names.contains(name) {
                continue;
            }
            if program
                .body
                .iter()
                .any(|statement| Self::module_statement_uses_constructor_function(name, statement))
            {
                names.insert(name.to_owned());
            }
        }
        names
    }

    /// Return whether a module top-level function declaration was recorded as a
    /// constructor function by [`Self::module_constructor_function_names`].
    pub(in crate::lowering) fn is_module_constructor_function(
        &self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> bool {
        function.id.as_ref().is_some_and(|id| {
            self.module_constructor_functions
                .contains(id.name.as_str())
        })
    }

    /// Iterate module top-level function declarations, including exported ones.
    fn module_function_declarations<'a>(
        program: &'a Program<'a>,
    ) -> impl Iterator<Item = &'a oxc::ast::ast::Function<'a>> {
        program.body.iter().filter_map(|statement| match statement {
            Statement::FunctionDeclaration(function) => Some(function.as_ref()),
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::FunctionDeclaration(function)) => Some(function.as_ref()),
                _ => None,
            },
            _ => None,
        })
    }

    /// Scan a module top-level statement for constructor uses of `name`.
    ///
    /// Extends [`Self::statement_uses_constructor_function`] across the two
    /// shapes that only occur at module scope — `export` declaration wrappers
    /// and sibling function declarations whose bodies hold the `new` site — then
    /// delegates to the shared scan for everything else. It is deliberately a
    /// separate entry point: widening the shared scan would also change which
    /// *inner-scope* functions become synthesized classes.
    fn module_statement_uses_constructor_function(name: &str, statement: &Statement<'_>) -> bool {
        match statement {
            Statement::FunctionDeclaration(function) => {
                Self::function_body_uses_constructor_function(name, function)
            }
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::FunctionDeclaration(function)) => {
                    Self::function_body_uses_constructor_function(name, function)
                }
                Some(Declaration::VariableDeclaration(variable)) => {
                    variable.declarations.iter().any(|declarator| {
                        declarator.init.as_ref().is_some_and(|init| {
                            Self::expression_uses_constructor_function(name, init)
                        })
                    })
                }
                _ => false,
            },
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                oxc::ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                    Self::function_body_uses_constructor_function(name, function)
                }
                other => other.as_expression().is_some_and(|expression| {
                    Self::expression_uses_constructor_function(name, expression)
                }),
            },
            other => Self::statement_uses_constructor_function(name, other),
        }
    }

    /// Scan a function declaration's body for constructor uses of `name`.
    fn function_body_uses_constructor_function(
        name: &str,
        function: &oxc::ast::ast::Function<'_>,
    ) -> bool {
        function.body.as_ref().is_some_and(|body| {
            body.statements
                .iter()
                .any(|statement| Self::module_statement_uses_constructor_function(name, statement))
        })
    }

    /// Recursively scan a statement for constructor uses of `name`.
    fn statement_uses_constructor_function(name: &str, statement: &Statement<'_>) -> bool {
        match statement {
            Statement::ExpressionStatement(statement) => {
                Self::expression_uses_constructor_function(name, &statement.expression)
            }
            Statement::VariableDeclaration(declaration) => {
                declaration.declarations.iter().any(|declarator| {
                    declarator
                        .init
                        .as_ref()
                        .is_some_and(|init| Self::expression_uses_constructor_function(name, init))
                })
            }
            Statement::ReturnStatement(statement) => statement
                .argument
                .as_ref()
                .is_some_and(|argument| Self::expression_uses_constructor_function(name, argument)),
            Statement::BlockStatement(block) => {
                Self::statements_use_function_as_constructor(name, &block.body)
            }
            Statement::IfStatement(statement) => {
                Self::expression_uses_constructor_function(name, &statement.test)
                    || Self::statement_uses_constructor_function(name, &statement.consequent)
                    || statement.alternate.as_ref().is_some_and(|alternate| {
                        Self::statement_uses_constructor_function(name, alternate)
                    })
            }
            Statement::ForStatement(statement) => {
                Self::statement_uses_constructor_function(name, &statement.body)
            }
            _ => false,
        }
    }

    /// Recursively scan an expression for constructor uses of `name`:
    /// `new name(...)`, `value instanceof name`, or `name.prototype.x = …`.
    fn expression_uses_constructor_function(name: &str, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::NewExpression(new_expr) => {
                if matches!(
                    &new_expr.callee,
                    Expression::Identifier(callee) if callee.name == name
                ) {
                    return true;
                }
                Self::expression_uses_constructor_function(name, &new_expr.callee)
                    || new_expr.arguments.iter().any(|argument| {
                        argument.as_expression().is_some_and(|argument_expr| {
                            Self::expression_uses_constructor_function(name, argument_expr)
                        })
                    })
            }
            Expression::BinaryExpression(binary)
                if binary.operator == BinaryOperator::Instanceof =>
            {
                if matches!(
                    &binary.right,
                    Expression::Identifier(target) if target.name == name
                ) {
                    return true;
                }
                Self::expression_uses_constructor_function(name, &binary.left)
                    || Self::expression_uses_constructor_function(name, &binary.right)
            }
            Expression::AssignmentExpression(assign) => {
                Self::assignment_target_is_prototype_member(name, &assign.left)
                    || Self::expression_uses_constructor_function(name, &assign.right)
            }
            Expression::CallExpression(call) => {
                // Test scaffolding (`describe`/`it`) passes its body as a
                // callback argument, so the construction site is frequently
                // nested inside an arrow or function expression. Recurse into
                // those bodies so a `function Foo` declared in an enclosing
                // scope is still recognized as a constructor.
                Self::expression_uses_constructor_function(name, &call.callee)
                    || call.arguments.iter().any(|argument| {
                        argument.as_expression().is_some_and(|argument_expr| {
                            Self::expression_uses_constructor_function(name, argument_expr)
                        })
                    })
            }
            Expression::ArrowFunctionExpression(arrow) => {
                Self::statements_use_function_as_constructor(name, &arrow.body.statements)
            }
            Expression::FunctionExpression(function) => function
                .body
                .as_ref()
                .is_some_and(|body| {
                    Self::statements_use_function_as_constructor(name, &body.statements)
                }),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::expression_uses_constructor_function(name, &parenthesized.expression)
            }
            Expression::BinaryExpression(binary) => {
                Self::expression_uses_constructor_function(name, &binary.left)
                    || Self::expression_uses_constructor_function(name, &binary.right)
            }
            Expression::LogicalExpression(logical) => {
                Self::expression_uses_constructor_function(name, &logical.left)
                    || Self::expression_uses_constructor_function(name, &logical.right)
            }
            _ => false,
        }
    }

    /// Collect `this.<name>` members assigned anywhere in a constructor body.
    ///
    /// These become the synthesized class's own fields. Both `this.x = …`
    /// statement assignments and nested blocks/branches are traversed so that
    /// conditionally assigned fields are still declared.
    fn collect_this_assigned_field_names(
        &mut self,
        statement: &Statement<'_>,
        names: &mut HashSet<smelt_hir::Symbol>,
    ) {
        match statement {
            Statement::ExpressionStatement(statement) => {
                self.collect_this_assigned_field_names_from_expression(
                    &statement.expression,
                    names,
                );
            }
            Statement::BlockStatement(block) => {
                for inner in &block.body {
                    self.collect_this_assigned_field_names(inner, names);
                }
            }
            Statement::IfStatement(statement) => {
                self.collect_this_assigned_field_names(&statement.consequent, names);
                if let Some(alternate) = &statement.alternate {
                    self.collect_this_assigned_field_names(alternate, names);
                }
            }
            Statement::ForStatement(statement) => {
                self.collect_this_assigned_field_names(&statement.body, names);
            }
            _ => {}
        }
    }

    /// Collect `this.<name>` assignment targets from an expression subtree.
    fn collect_this_assigned_field_names_from_expression(
        &mut self,
        expression: &Expression<'_>,
        names: &mut HashSet<smelt_hir::Symbol>,
    ) {
        if let Expression::AssignmentExpression(assign) = expression {
            if let AssignmentTarget::StaticMemberExpression(member) = &assign.left
                && matches!(&member.object, Expression::ThisExpression(_))
            {
                names.insert(self.intern_source_name(member.property.name.as_str()));
            }
            self.collect_this_assigned_field_names_from_expression(&assign.right, names);
        }
    }

    /// Return whether an expression assigns a member of a synthesized
    /// constructor class — either `Foo.prototype.x = …` (instance member) or
    /// `Foo.x = …` (static member) where `Foo` is a synthesized class.
    ///
    /// Prototype members were consumed when the class was built, and static
    /// members on a constructor are not modeled on synthesized classes, so both
    /// assignment statements are dropped rather than lowered as runtime writes.
    pub(in crate::lowering) fn is_synthesized_prototype_assignment(
        &self,
        expression: &Expression<'_>,
    ) -> bool {
        let Expression::AssignmentExpression(assign) = expression else {
            return false;
        };
        let AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
            return false;
        };
        // `Foo.prototype.x = …`: receiver is `Foo.prototype`.
        if let Expression::StaticMemberExpression(prototype) = &member.object
            && prototype.property.name == "prototype"
            && matches!(
                &prototype.object,
                Expression::Identifier(object) if self.classes.contains_key(object.name.as_str())
            )
        {
            return true;
        }
        // `Foo.x = …`: receiver is the synthesized class binding itself (static
        // members such as `Foo.c = function () {}`).
        matches!(
            &member.object,
            Expression::Identifier(object) if self.classes.contains_key(object.name.as_str())
        )
    }

    /// Return whether an assignment target writes `name.prototype.<member>`.
    fn assignment_target_is_prototype_member(name: &str, target: &AssignmentTarget<'_>) -> bool {
        let AssignmentTarget::StaticMemberExpression(member) = target else {
            return false;
        };
        let Expression::StaticMemberExpression(prototype) = &member.object else {
            return false;
        };
        prototype.property.name == "prototype"
            && matches!(
                &prototype.object,
                Expression::Identifier(object) if object.name == name
            )
    }

    /// Find a `const Foo = function (…) { … }` constructor binding in a
    /// declarator: a variable initialized with a function expression whose name
    /// is used as a constructor in `body_statements`.
    ///
    /// Returns the binding name and the function expression. The function
    /// expression's own optional `id` is ignored; the binding name is what `new`
    /// and `instanceof` reference.
    pub(in crate::lowering) fn const_constructor_function<'a>(
        declarator: &'a oxc::ast::ast::VariableDeclarator<'a>,
    ) -> Option<(&'a str, &'a oxc::ast::ast::Function<'a>)> {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return None;
        };
        let mut init = declarator.init.as_ref()?;
        // Unwrap `as`/`satisfies`/parenthesized wrappers around the function
        // expression (constructor functions are frequently cast, e.g.
        // `const Foo = function () {} as any as FooConstructor`).
        loop {
            match init {
                Expression::TSAsExpression(as_expr) => init = &as_expr.expression,
                Expression::TSSatisfiesExpression(satisfies) => init = &satisfies.expression,
                Expression::ParenthesizedExpression(parenthesized) => {
                    init = &parenthesized.expression;
                }
                Expression::FunctionExpression(function) => {
                    return Some((binding.name.as_str(), function.as_ref()));
                }
                _ => return None,
            }
        }
    }

    /// Synthesize classes for `const Foo = function () { … }` constructor
    /// bindings declared in a statement list and used as constructors there.
    pub(in crate::lowering) fn synthesize_const_constructor_functions(
        &mut self,
        statements: &[Statement<'_>],
    ) -> Result<(), SmeltError> {
        for statement in statements {
            let Statement::VariableDeclaration(declaration) = statement else {
                continue;
            };
            for declarator in &declaration.declarations {
                let Some((name, function)) = Self::const_constructor_function(declarator) else {
                    continue;
                };
                if self.classes.contains_key(name) || self.locals.contains_key(name) {
                    continue;
                }
                if !Self::statements_use_function_as_constructor(name, statements) {
                    continue;
                }
                // A `const bound = function (...) { … }` that captures enclosing
                // locals (the curry/bind/partial family's returned closures) is
                // a *constructable function value*, not a static class: its
                // identity and captured state belong to the runtime closure, so
                // synthesizing a top-level class would drop the captures (e.g.
                // `partialArgs`) and break the closure body. Leave it as a
                // function value so the closure path captures correctly; `new
                // bound()` on the value is handled by the closure-construct path.
                if self.const_constructor_captures_enclosing_locals(function) {
                    continue;
                }
                let prototype_methods =
                    Self::collect_prototype_member_functions(name, statements.iter());
                self.synthesize_named_constructor_function_class(
                    name,
                    function,
                    prototype_methods,
                )?;
            }
        }
        Ok(())
    }

    /// Return whether a `const Foo = function (…) { … }` body references any
    /// local bound in the enclosing scope (a free-variable capture).
    ///
    /// Such a binding is a closure value rather than a static constructor
    /// class. The existing closure capture-name collectors are reused so the
    /// same expression/statement coverage decides capture here and during
    /// closure lowering.
    fn const_constructor_captures_enclosing_locals(
        &self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> bool {
        let Some(function_body) = &function.body else {
            return false;
        };
        let mut param_names = HashSet::new();
        for param in &function.params.items {
            let mut names = Vec::new();
            Self::binding_pattern_names(&param.pattern, &mut names);
            param_names.extend(names);
        }
        if let Some(rest) = &function.params.rest {
            let mut names = Vec::new();
            Self::binding_pattern_names(&rest.rest.argument, &mut names);
            param_names.extend(names);
        }
        let mut captures = Vec::new();
        for statement in &function_body.statements {
            self.collect_statement_capture_names(statement, &param_names, &mut captures);
        }
        !captures.is_empty()
    }

    /// Synthesize classes for constructor functions declared in a test's
    /// inherited `describe`-level setup statements.
    ///
    /// A `describe` body's `function Foo(){}` and `Foo.prototype.x = …` are
    /// replayed as setup before each test case, while the `new Foo()` use can
    /// live in a different `it` callback body. This scans the setup declarations
    /// and synthesizes a class whenever the function is used as a constructor in
    /// either the setup itself or the test body, before the setup is lowered.
    pub(in crate::lowering) fn synthesize_setup_constructor_functions(
        &mut self,
        setup: &[&Statement<'_>],
        body_statements: &[Statement<'_>],
    ) -> Result<(), SmeltError> {
        for statement in setup {
            let Statement::FunctionDeclaration(function) = statement else {
                continue;
            };
            let Some(id) = &function.id else {
                continue;
            };
            if self.classes.contains_key(id.name.as_str())
                || self.locals.contains_key(id.name.as_str())
            {
                continue;
            }
            let used_in_setup = setup.iter().any(|setup_statement| {
                Self::statement_uses_constructor_function(id.name.as_str(), setup_statement)
            });
            let used_in_body =
                Self::statements_use_function_as_constructor(id.name.as_str(), body_statements);
            if !used_in_setup && !used_in_body {
                continue;
            }
            // `Foo.prototype.x = …` assignments live among the setup statements,
            // so the prototype scan walks the borrowed setup slice directly.
            self.synthesize_constructor_function_class_from_refs(function, setup)?;
        }
        Ok(())
    }

    /// Synthesize a class for a constructor function declared in a block or
    /// module body, scanning the same statement list for prototype assignments.
    pub(in crate::lowering) fn synthesize_constructor_function_class(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        siblings: &[Statement<'_>],
    ) -> Result<(), SmeltError> {
        let Some(id) = &function.id else {
            return Ok(());
        };
        let prototype_methods =
            Self::collect_prototype_member_functions(id.name.as_str(), siblings.iter());
        let name = id.name.to_string();
        self.synthesize_named_constructor_function_class(&name, function, prototype_methods)
    }

    /// Synthesize a class for a constructor function declared in inherited test
    /// setup, scanning the borrowed setup slice for prototype assignments.
    fn synthesize_constructor_function_class_from_refs(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        siblings: &[&Statement<'_>],
    ) -> Result<(), SmeltError> {
        let Some(id) = &function.id else {
            return Ok(());
        };
        let prototype_methods = Self::collect_prototype_member_functions(
            id.name.as_str(),
            siblings.iter().copied(),
        );
        let name = id.name.to_string();
        self.synthesize_named_constructor_function_class(&name, function, prototype_methods)
    }

    /// Synthesize and register an HIR class for a constructor function under a
    /// caller-provided class name.
    ///
    /// `class_text` is the source-level name `new`/`instanceof` reference (the
    /// function declaration name or the `const` binding name), `function` is the
    /// constructor function body, and `prototype_methods` are the
    /// `Foo.prototype.x = function () { … }` members collected from the
    /// surrounding statement list. The class is registered into
    /// `self.classes`/`self.class_fields`/`self.class_methods` so subsequent
    /// `new`/`instanceof`/field lookups resolve it like a declared class.
    fn synthesize_named_constructor_function_class<'a>(
        &mut self,
        class_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        prototype_methods: Vec<(String, &'a oxc::ast::ast::Function<'a>)>,
    ) -> Result<(), SmeltError> {
        let class_text = class_text.to_owned();
        if self.classes.contains_key(class_text.as_str()) {
            return Ok(());
        }
        let class_name = self.intern_type_name(class_text.as_str());
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: Vec::new(),
        });
        let span = self.span(function.span.start, function.span.end);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);

        // Derive own fields from `this.<name> = …` assignments in the body. The
        // shared `collect_this_member_names_*` helpers only traverse value
        // positions, so the constructor's own field writes are collected here
        // from assignment targets as well.
        let mut field_names = HashSet::new();
        if let Some(body) = &function.body {
            for statement in &body.statements {
                self.collect_this_assigned_field_names(statement, &mut field_names);
            }
        }
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        let mut field_order = field_names.into_iter().collect::<Vec<_>>();
        field_order.sort_by_key(|symbol| symbol.0);
        for field in field_order {
            if seen.insert(field) {
                fields.push(Field {
                    name: field,
                    ty: unknown_ty,
                    visibility: Visibility::Public,
                    optional: false,
                    span,
                });
            }
        }

        // Register field metadata before lowering the constructor body so that
        // `this.<field>` reads/writes resolve against the class shape.
        self.class_fields.insert(class_text.clone(), fields.clone());
        self.class_methods.insert(class_text.clone(), Vec::new());

        let constructor = self.synthesize_constructor_function_body(
            class_text.as_str(),
            class_name,
            class_ty,
            function,
            span,
        )?;

        let mut methods = Vec::new();
        let mut method_sigs = Vec::new();
        for (method_name, method_function) in prototype_methods {
            let (item, sig) = self.synthesize_prototype_method(
                class_text.as_str(),
                class_name,
                class_ty,
                method_name.as_str(),
                method_function,
            )?;
            methods.push(item);
            method_sigs.push(sig);
        }
        self.class_methods.insert(class_text.clone(), method_sigs);

        let item = self.ctx.krate.push_item(Item::Class(Class {
            name: class_name,
            span,
            kind: smelt_hir::ClassKind::Plain,
            type_params: Vec::new(),
            base: None,
            base_args: Vec::new(),
            fields,
            constructor: Some(constructor),
            methods,
            static_methods: Vec::new(),
            abstract_methods: Vec::new(),
            implements: Vec::new(),
            static_fields: Vec::new(),
            descriptors: Vec::new(),
        }));
        self.classes.insert(class_text, item);
        Ok(())
    }

    /// Collect `Foo.prototype.<name> = function () { … }` assignments whose
    /// right-hand side is a function expression, returning name/function pairs.
    ///
    /// Non-function prototype data (`Foo.prototype.b = 2`) is JavaScript
    /// inherited state that `Object.keys`/`values`/`has` treat as non-own, so it
    /// does not contribute own fields; only callable prototype members are
    /// materialized, as methods.
    fn collect_prototype_member_functions<'a>(
        name: &str,
        statements: impl Iterator<Item = &'a Statement<'a>>,
    ) -> Vec<(String, &'a oxc::ast::ast::Function<'a>)> {
        let mut methods = Vec::new();
        for statement in statements {
            let Statement::ExpressionStatement(statement) = statement else {
                continue;
            };
            let Expression::AssignmentExpression(assign) = &statement.expression else {
                continue;
            };
            let AssignmentTarget::StaticMemberExpression(member) = &assign.left else {
                continue;
            };
            let Expression::StaticMemberExpression(prototype) = &member.object else {
                continue;
            };
            if prototype.property.name != "prototype"
                || !matches!(
                    &prototype.object,
                    Expression::Identifier(object) if object.name == name
                )
            {
                continue;
            }
            if let Expression::FunctionExpression(function) = &assign.right {
                methods.push((member.property.name.to_string(), function.as_ref()));
            }
        }
        methods
    }

    /// Resolve the HIR type of a synthesized constructor function's parameter.
    ///
    /// An annotated or default-initialized parameter keeps the ordinary
    /// [`Self::function_parameter_type`] resolution. An *unannotated,
    /// non-defaulted* constructor parameter, however, is a genuinely dynamic
    /// boundary: a synthesized constructor's own fields are all typed `unknown`
    /// (their shapes come from whatever `new Foo(args)` passes and the
    /// `this.x = …` / `Object.assign(this, …)` writes Smelt cannot see
    /// statically), so the value the parameter carries into those writes has no
    /// recoverable static shape either. It defaults to `unknown`, matching both
    /// the `unknown` field ABI and TypeScript's own implicit-any inference for
    /// the identical `function Foo(object) { Object.assign(this, object); }`
    /// idiom (es-toolkit spells the very same shape `object: any` in its `merge`
    /// spec). This fallback stays inside the constructor-function synthesis path,
    /// so ordinary untyped function declarations still require explicit
    /// annotations via [`Self::function_parameter_type`].
    fn constructor_parameter_type(
        &mut self,
        param: &oxc::ast::ast::FormalParameter<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if param.type_annotation.is_some() || param.initializer.is_some() {
            return self.function_parameter_type(param);
        }
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        if param.optional {
            Ok(self.ctx.krate.types.intern(Type::Optional(unknown_ty)))
        } else {
            Ok(unknown_ty)
        }
    }

    /// Lower a constructor function body into the class constructor item.
    ///
    /// Mirrors the constructor path of `class_function`: `this` is bound as a
    /// mutable local of the class type so `this.<field> = …` assignments in the
    /// body lower to field writes, and the constructor returns the constructed
    /// instance.
    fn synthesize_constructor_function_body(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        function: &oxc::ast::ast::Function<'_>,
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                span,
                "constructor functions must have a body",
            ));
        };
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = false;
        self.current_return_ty = Some(class_ty);
        let mut body = Body::new(None, span);
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let this_local = body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span,
        });
        self.locals.insert("this".to_owned(), this_local);

        let mut params = Vec::new();
        let mut errors = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = match self.constructor_parameter_type(param) {
                Ok(ty) => ty,
                Err(error) => {
                    errors.push(error);
                    self.ctx.krate.types.intern(Type::Unknown)
                }
            };
            let (param_name, param_span, source_name) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.to_string()),
                    )
                } else {
                    (
                        self.intern_source_name(&format!("__param{index}")),
                        self.span(param.span.start, param.span.end),
                        None,
                    )
                };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: param_span,
            });
            body.params.push(local);
            if let Some(source_name) = source_name {
                self.locals.insert(source_name, local);
            }
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: param_span,
            });
        }

        if let Err(error) =
            self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        if let Err(error) =
            self.predeclare_local_function_declarations(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }

        self.locals = saved_locals;
        self.current_class = saved_class;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let body_id = self.ctx.krate.push_body(body);
        let name = self.ctx.krate.symbols.intern("new");
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            // Class constructors take their generics from the owning class, so
            // the function item itself declares no free-function type params.
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: None,
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_name },
        })))
    }

    /// Lower a prototype method (`Foo.prototype.m = function () { … }`) into a
    /// class method item, returning the item and its signature metadata.
    ///
    /// Mirrors the non-constructor path of `class_function`: `this` is the first
    /// parameter so method bodies can read instance fields.
    fn synthesize_prototype_method(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        method_name: &str,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<(smelt_hir::ItemId, MethodSig), SmeltError> {
        let method_symbol = self.intern_source_name(method_name);
        let span = self.span(function.span.start, function.span.end);
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                span,
                "prototype methods must have a body",
            ));
        };
        let return_ty = function
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = false;
        self.current_return_ty = Some(return_ty);
        let mut body = Body::new(None, span);
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let this_local = body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span,
        });
        self.locals.insert("this".to_owned(), this_local);
        body.params.push(this_local);
        let mut params = vec![Param {
            name: this_symbol,
            local: this_local,
            ty: class_ty,
            span,
        }];
        let mut param_sigs = Vec::new();
        let mut errors = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = match self.function_parameter_type(param) {
                Ok(ty) => ty,
                Err(error) => {
                    errors.push(error);
                    self.ctx.krate.types.intern(Type::Unknown)
                }
            };
            let (param_name, param_span, source_name) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.to_string()),
                    )
                } else {
                    (
                        self.intern_source_name(&format!("__param{index}")),
                        self.span(param.span.start, param.span.end),
                        None,
                    )
                };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: param_span,
            });
            body.params.push(local);
            if let Some(source_name) = source_name {
                self.locals.insert(source_name, local);
            }
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: param_span,
            });
            param_sigs.push(ParamSig {
                name: param_name,
                ty,
                span: param_span,
            });
        }

        if let Err(error) =
            self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        if let Err(error) =
            self.predeclare_local_function_declarations(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }

        self.locals = saved_locals;
        self.current_class = saved_class;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let resolved_return_ty = self
            .last_return_type(&body)
            .unwrap_or(return_ty);
        let body_id = self.ctx.krate.push_body(body);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name: method_symbol,
            span,
            // Method generics come from the owning class; the function item
            // declares no free-function type params.
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: None,
            return_ty: resolved_return_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::ClassMethod {
                class: class_name,
                method: method_symbol,
            },
        }));
        let sig = MethodSig {
            name: method_symbol,
            params: param_sigs,
            rest: None,
            required_params: None,
            return_ty: resolved_return_ty,
            visibility: Visibility::Public,
            is_async: false,
            span,
        };
        Ok((item, sig))
    }
}
