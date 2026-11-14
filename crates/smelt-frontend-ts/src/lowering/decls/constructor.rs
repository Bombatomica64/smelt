//! JavaScript constructor-function lowering into synthesized HIR classes.
//!
//! TypeScript/JavaScript lets a plain `function` declaration act as a class:
//! `new Foo()` constructs an instance whose own properties are the `this.x = …`
//! assignments in the function body, while `Foo.prototype.m = function () { … }`
//! adds instance methods. This module materializes that shape once so ordinary
//! class construction and `instanceof` lowering need no special use-site path.

use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use crate::lowering::{
    AssignmentTarget, BindingPattern, Body, Class, Declaration, Expression, Field,
    Function, FunctionOwner, HashSet, Item, LocalDecl, MethodSig, ModuleBuilder, Param, ParamSig,
    Program, SmeltError, Span, Statement, Type, Visibility,
};
use smelt_hir::ErasureReason;

impl ModuleBuilder<'_> {
    /// Return whether a statement list constructs, type-tests, or extends the
    /// prototype of a function named `name`, marking it as a constructor function.
    ///
    /// A bare `function Foo(){}` that is only ever called normally is left as a
    /// plain function; it only becomes a class when the surrounding scope uses it
    /// with `new` or `Foo.prototype.x = …` (see `scoped_references::BindingUses`).
    pub(in crate::lowering) fn statements_use_function_as_constructor(
        name: &str,
        statements: &[Statement<'_>],
    ) -> bool {
        crate::lowering::scoped_references::statements_use(statements, name).constructed
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
    /// `ClassRegistry::is_pending`.
    pub(in crate::lowering) fn module_constructor_function_names(
        program: &Program<'_>,
    ) -> HashSet<String> {
        // An ambient `declare function` has no body to derive fields from.
        let candidates: HashSet<&str> = Self::module_function_declarations(program)
            .filter(|function| !function.declare && function.body.is_some())
            .filter_map(|function| function.id.as_ref().map(|id| id.name.as_str()))
            .collect();
        if candidates.is_empty() {
            return HashSet::new();
        }
        // One scoped walk of the module for every candidate at once: a walk
        // per candidate made a module of `n` functions quadratic.
        let mut constructed = HashSet::new();
        for statement in &program.body {
            constructed.extend(
                crate::lowering::scoped_references::statement_name_uses(statement, &candidates).constructed,
            );
        }
        constructed
    }

    /// Return whether a module top-level function declaration was recorded as a
    /// constructor function by [`Self::module_constructor_function_names`].
    pub(in crate::lowering) fn is_module_constructor_function(
        &self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> bool {
        function.id.as_ref().is_some_and(|id| {
            self.classes.is_constructor_function(id.name.as_str())
        })
    }

    /// Iterate module top-level function declarations, including exported ones.
    fn module_function_declarations<'a>(
        program: &'a Program<'a>,
    ) -> impl Iterator<Item = &'a oxc::ast::ast::Function<'a>> {
        program.body.iter().filter_map(|statement| match statement {
            Statement::FunctionDeclaration(function) => Some(function.as_ref()),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => Some(function.as_ref()),
                _ => None,
            },
            _ => None,
        })
    }


    /// Collect `this.<name>` members assigned anywhere in a constructor body.
    ///
    /// These become the synthesized class's own fields. Both `this.x = …`
    /// statement assignments and every nested statement ([`nested_statements`]:
    /// blocks, branches, loops, labelled statements, `try`, `switch`) are
    /// traversed so that conditionally assigned fields are still declared.
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
            other => {
                for inner in nested_statements(other) {
                    self.collect_this_assigned_field_names(inner, names);
                }
            }
        }
    }

    /// Collect every `this.<name> = <expr>` write, with the field's source
    /// name, through the statements [`Self::collect_this_assigned_field_names`]
    /// walks, so the writes typed are exactly the ones that declared fields.
    fn collect_this_field_writes<'b, 'c>(
        statement: &'b Statement<'c>,
        writes: &mut Vec<(&'b str, &'b Expression<'c>)>,
    ) {
        match statement {
            Statement::ExpressionStatement(statement) => {
                Self::collect_this_field_writes_from_expression(&statement.expression, writes);
            }
            other => {
                for inner in nested_statements(other) {
                    Self::collect_this_field_writes(inner, writes);
                }
            }
        }
    }

    /// [`Self::collect_this_field_writes`] over one expression: a chained
    /// `this.a = this.b = v` writes both fields with `v`'s value.
    fn collect_this_field_writes_from_expression<'b, 'c>(
        expression: &'b Expression<'c>,
        writes: &mut Vec<(&'b str, &'b Expression<'c>)>,
    ) {
        if let Expression::AssignmentExpression(assign) = expression {
            if let AssignmentTarget::StaticMemberExpression(member) = &assign.left
                && matches!(&member.object, Expression::ThisExpression(_))
            {
                writes.push((member.property.name.as_str(), &assign.right));
            }
            Self::collect_this_field_writes_from_expression(&assign.right, writes);
        }
    }

    /// The type of a constructor-function field: the join
    /// ([`Self::joined_inferred_type`]) of the types its writes evaluate to.
    ///
    /// Each write's value is typed by the ordinary expression lowering, in a
    /// scratch body that is rolled back ([`Self::infer_module_global_initializer_type`]).
    /// A field is erased, with the reason it always had, when it has no write
    /// to type or when ANY write cannot be typed there (one reading a
    /// constructor parameter or local, which the scratch body does not bind):
    /// a join missing one write's type would be narrower than what the field
    /// holds.
    ///
    /// A write that reads the field itself (`this.count = this.count + 1`,
    /// where `this` is `any`) is typed by FIXED POINT: assume the field holds
    /// the join of the writes that type on their own, type the self-reading
    /// writes with each `this.<field>` read at that assumption, and repeat
    /// with the widened join until it is stable. The answer is then exactly
    /// what every write stores; a self-reading write that stays untyped (or a
    /// join that keeps widening) erases the field as before.
    fn constructor_function_field_type(
        &mut self,
        field: smelt_hir::Symbol,
        writes: &[(&str, &Expression<'_>)],
        span: Span,
    ) -> smelt_hir::TypeId {
        let mut types = Vec::new();
        let mut self_reading = Vec::new();
        let mut untyped = false;
        for (name, value) in writes {
            if self.intern_source_name(name) != field {
                continue;
            }
            match self.infer_module_global_initializer_type(value) {
                Ok(ty) if !self.type_contains_unknown(ty) => types.push(ty),
                Ok(_) | Err(_) => {
                    let reads = this_field_reads(value, name);
                    if reads.is_empty() {
                        untyped = true;
                    } else {
                        self_reading.push((*value, reads));
                    }
                }
            }
        }
        if untyped || types.is_empty() {
            return self.erase_at(
                ErasureReason::MissingAnnotation,
                span,
                "constructor-function field",
            );
        }
        let mut assumed = self.joined_inferred_type(types.clone());
        for _ in 0_u8..4 {
            let mut all = types.clone();
            for (value, reads) in &self_reading {
                match self.infer_with_pinned_reads(value, reads, assumed) {
                    Some(ty) => all.push(ty),
                    None => {
                        return self.erase_at(ErasureReason::MissingAnnotation, span, "constructor-function field");
                    }
                }
            }
            let joined = self.joined_inferred_type(all);
            if joined == assumed {
                return joined;
            }
            assumed = joined;
        }
        self.erase_at(ErasureReason::MissingAnnotation, span, "constructor-function field")
    }

    /// The type `value` evaluates to with every node in `reads` read as a
    /// value of type `assumed`, or `None` when it does not type (or types
    /// erased) even then. The scratch lowering is rolled back.
    fn infer_with_pinned_reads(
        &mut self,
        value: &Expression<'_>,
        reads: &[usize],
        assumed: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        let mut body = Body::new(None, self.expression_span(value));
        let mark = self.speculation_mark(&body);
        let temp = body.push_local(LocalDecl {
            name: Some(self.intern_synthesized_name("smelt_assumed_field")),
            ty: assumed,
            mutable: false,
            span: self.expression_span(value),
        });
        let pinned = self.pinned_nodes.len();
        self.pinned_nodes.extend(reads.iter().map(|node| (*node, temp, assumed)));
        let result = self.expression_with_hint(value, &mut body, None).map(|expr| Self::expr_ty(&body, expr));
        self.pinned_nodes.truncate(pinned);
        self.roll_back_to(mark, &mut body);
        result.ok().filter(|ty| !self.type_contains_unknown(*ty))
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
                Expression::Identifier(object) if self.classes.contains(object.name.as_str())
            )
        {
            return true;
        }
        // `Foo.x = …`: receiver is the synthesized class binding itself (static
        // members such as `Foo.c = function () {}`). A write to a MUTABLE static
        // the class declares is a real store into its live storage, never
        // dropped.
        if self.static_field_write_storage(member).is_some() {
            return false;
        }
        matches!(
            &member.object,
            Expression::Identifier(object) if self.classes.contains(object.name.as_str())
        )
    }

    /// Find a `const Foo = function (…) { … }` constructor binding in a
    /// declarator: a variable initialized with a function expression whose name
    /// is used as a constructor. The caller checks constructor usage.
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
        // Constructor functions are frequently cast, e.g.
        // `const Foo = function () {} as any as FooConstructor`.
        let Expression::FunctionExpression(function) =
            peel(declarator.init.as_ref()?, Wrappers::Casts)
        else {
            return None;
        };
        Some((binding.name.as_str(), function.as_ref()))
    }

    /// Match `const Foo = class { … }`, answering the binding name and the class.
    ///
    /// A class expression bound to a name IS a class declaration under that
    /// name: TypeScript infers it, and every nominal use of the binding —
    /// `new Foo()`, `Foo` in type position, `x instanceof Foo`, `extends Foo` —
    /// depends on it. Lowered as a plain value instead, the binding held a
    /// placeholder and `new Foo()` erased into a dynamic construction whose
    /// methods answered `null` (H68).
    ///
    /// Wrappers around the class expression are unwrapped exactly as
    /// [`Self::const_constructor_function`] unwraps them, since the same casts
    /// (`as any`, `satisfies`, parentheses) appear on class expressions.
    pub(in crate::lowering) fn const_class_expression<'a>(
        declarator: &'a oxc::ast::ast::VariableDeclarator<'a>,
    ) -> Option<(&'a str, &'a oxc::ast::ast::Class<'a>)> {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return None;
        };
        let Expression::ClassExpression(class) =
            peel(declarator.init.as_ref()?, Wrappers::Casts)
        else {
            return None;
        };
        Some((binding.name.as_str(), class.as_ref()))
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
                if self.classes.contains(name) || self.scope.is_bound(name) {
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
            if self.classes.contains(id.name.as_str())
                || self.scope.is_bound(id.name.as_str())
            {
                continue;
            }
            let used_in_setup = setup.iter().any(|setup_statement| {
                crate::lowering::scoped_references::statement_uses(setup_statement, id.name.as_str()).constructed
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
    /// class registry (item, fields and method signatures) so subsequent
    /// `new`/`instanceof`/field lookups resolve it like a declared class.
    fn synthesize_named_constructor_function_class<'a>(
        &mut self,
        class_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        prototype_methods: Vec<(String, &'a oxc::ast::ast::Function<'a>)>,
    ) -> Result<(), SmeltError> {
        let class_text = class_text.to_owned();
        if self.classes.contains(class_text.as_str()) {
            return Ok(());
        }
        // The registry is scoped, so `contains` above does not see a
        // same-named constructor function from a sibling block (two `it`
        // callbacks each declaring `function Foo() { this.a = 1 }`). Its class
        // item is still in the crate, so this one takes a name of its own,
        // bound to the source spelling for the rest of its scope.
        let span = self.span(function.span.start, function.span.end);
        // The same constructor function lowered again (inherited test setup)
        // keeps the class it already has.
        if let Some((item, existing)) = self.class_item_lowered_at(span) {
            if self.intern_type_name(class_text.as_str()) != existing {
                self.classes.open_shadow(class_text.as_str());
                self.classes.bind_scoped_type_name(class_text.clone(), existing);
            }
            self.classes.register(class_text, item);
            return Ok(());
        }
        let source_symbol = self.intern_type_name(class_text.as_str());
        let class_name = self.unclaimed_type_symbol(source_symbol, class_text.as_str());
        if source_symbol != class_name {
            self.ctx.krate.names.record(class_name, class_text.as_str());
            self.classes.open_shadow(class_text.as_str());
            self.classes.bind_scoped_type_name(class_text.clone(), class_name);
        }
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: Vec::new(),
        });

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
        // Each field takes the join of the types its `this.<name> = <expr>`
        // writes evaluate to, in the constructor body and in the prototype
        // methods. The writes are found by the same traversal that found the
        // field names, so every write that made a field is typed.
        let mut writes = Vec::new();
        if let Some(body) = &function.body {
            for statement in &body.statements {
                Self::collect_this_field_writes(statement, &mut writes);
            }
        }
        for (_, method) in &prototype_methods {
            if let Some(body) = &method.body {
                for statement in &body.statements {
                    Self::collect_this_field_writes(statement, &mut writes);
                }
            }
        }
        let mut fields = Vec::new();
        let mut field_order = field_names.into_iter().collect::<Vec<_>>();
        field_order.sort_by_key(|symbol| symbol.0);
        for field in field_order {
            let field_ty = self.constructor_function_field_type(field, &writes, span);
            fields.push(Field {
                name: field,
                ty: field_ty,
                visibility: Visibility::Public,
                optional: false,
                span,
            });
        }

        // Register field metadata before lowering the constructor body so that
        // `this.<field>` reads/writes resolve against the class shape.
        self.classes.set_fields(class_text.clone(), fields.clone());
        self.classes.set_methods(class_text.clone(), Vec::new());

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
        self.classes.set_methods(class_text.clone(), method_sigs);

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
            protocols: Vec::new(),
            static_fields: Vec::new(),
            descriptors: Vec::new(),
        }));
        self.classes.register(class_text, item);
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
        let param_span = self.span(param.span.start, param.span.end);
        let unknown_ty = self.erase_at(ErasureReason::MissingAnnotation, param_span, "");
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
        let saved_class = self.current_class.replace(class_text.to_owned());
        let ctx = self.nested_fn_ctx(BodyKind::Method, false, Some(class_ty));
        let scope = ScopeGroups::MEMBER_BODY;
        let (body, params, errors) = self.with_nested_body(ctx, scope, |this, _| {
            let mut body = Body::new(None, span);
            let this_symbol = this.ctx.krate.symbols.intern("this");
            let this_local = body.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: class_ty,
                mutable: true,
                span,
            });
            this.scope.bind("this".to_owned(), this_local);
            this.fn_ctx.constructor_this = Some(this_local);

            let mut params = Vec::new();
            let mut errors = Vec::new();
            for (index, param) in function.params.items.iter().enumerate() {
                let ty = match this.constructor_parameter_type(param) {
                    Ok(ty) => ty,
                    Err(error) => {
                        errors.push(error);
                        this.erase(ErasureReason::MissingAnnotation)
                    }
                };
                let (param_name, param_span, source_name) =
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        (
                            this.intern_source_name(binding.name.as_str()),
                            this.span(binding.span.start, binding.span.end),
                            Some(binding.name.to_string()),
                        )
                    } else {
                        (
                            this.intern_source_name(&format!("__param{index}")),
                            this.span(param.span.start, param.span.end),
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
                    this.scope.bind(source_name, local);
                }
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: param_span,
                });
            }

            if let Err(error) =
                this.predeclare_forward_referenced_locals(&function_body.statements, &mut body)
            {
                errors.push(error);
            }
            if let Err(error) =
                this.predeclare_local_function_declarations(&function_body.statements, &mut body)
            {
                errors.push(error);
            }
            for statement in crate::lowering::hoisting::hoisted_statements(&function_body.statements) {
                if let Err(error) = this.statement(statement, &mut body) {
                    errors.push(error);
                }
            }

            (body, params, errors)
        });
        self.current_class = saved_class;
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
            .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));

        let saved_class = self.current_class.replace(class_text.to_owned());
        let ctx = self.nested_fn_ctx(BodyKind::Method, false, Some(return_ty));
        let scope = ScopeGroups::MEMBER_BODY;
        let (body, params, param_sigs, errors) = self.with_nested_body(ctx, scope, |this, _| {
            let mut body = Body::new(None, span);
            let this_symbol = this.ctx.krate.symbols.intern("this");
            let this_local = body.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: class_ty,
                mutable: true,
                span,
            });
            this.scope.bind("this".to_owned(), this_local);
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
                let ty = match this.function_parameter_type(param) {
                    Ok(ty) => ty,
                    Err(error) => {
                        errors.push(error);
                        this.erase(ErasureReason::MissingAnnotation)
                    }
                };
                let (param_name, param_span, source_name) =
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        (
                            this.intern_source_name(binding.name.as_str()),
                            this.span(binding.span.start, binding.span.end),
                            Some(binding.name.to_string()),
                        )
                    } else {
                        (
                            this.intern_source_name(&format!("__param{index}")),
                            this.span(param.span.start, param.span.end),
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
                    this.scope.bind(source_name, local);
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
                this.predeclare_forward_referenced_locals(&function_body.statements, &mut body)
            {
                errors.push(error);
            }
            if let Err(error) =
                this.predeclare_local_function_declarations(&function_body.statements, &mut body)
            {
                errors.push(error);
            }
            for statement in crate::lowering::hoisting::hoisted_statements(&function_body.statements) {
                if let Err(error) = this.statement(statement, &mut body) {
                    errors.push(error);
                }
            }

            (body, params, param_sigs, errors)
        });
        self.current_class = saved_class;
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

/// The statements `statement` directly nests, in source order: a block's body,
/// both arms of an `if`, a loop's or a labelled statement's body, the blocks of
/// a `try`, and every `switch` case's consequent. A walker that collects facts
/// from nested statements uses this one list, so two walkers that must agree
/// (the fields a constructor declares and the writes that type them) cannot
/// descend into different statements.
fn nested_statements<'b, 'c>(statement: &'b Statement<'c>) -> Vec<&'b Statement<'c>> {
    match statement {
        Statement::BlockStatement(block) => block.body.iter().collect(),
        Statement::IfStatement(statement) => std::iter::once(&statement.consequent)
            .chain(statement.alternate.as_ref())
            .collect(),
        Statement::ForStatement(statement) => vec![&statement.body],
        Statement::ForInStatement(statement) => vec![&statement.body],
        Statement::ForOfStatement(statement) => vec![&statement.body],
        Statement::WhileStatement(statement) => vec![&statement.body],
        Statement::DoWhileStatement(statement) => vec![&statement.body],
        Statement::LabeledStatement(statement) => vec![&statement.body],
        Statement::TryStatement(statement) => statement
            .block
            .body
            .iter()
            .chain(statement.handler.iter().flat_map(|handler| handler.body.body.iter()))
            .chain(statement.finalizer.iter().flat_map(|finalizer| finalizer.body.iter()))
            .collect(),
        Statement::SwitchStatement(statement) => statement
            .cases
            .iter()
            .flat_map(|case| case.consequent.iter())
            .collect(),
        _ => Vec::new(),
    }
}

/// The addresses of every `this.<field>` read inside `value`, the nodes a
/// fixed-point typing of a self-reading field write pins to an assumed type.
fn this_field_reads(value: &Expression<'_>, field: &str) -> Vec<usize> {
    use oxc::ast_visit::{Visit, walk};

    /// Collects the `this.<field>` member reads of one expression.
    struct ThisFieldReads<'f> {
        /// The field's source name.
        field: &'f str,
        /// Addresses of the matching `Expression` nodes.
        nodes: Vec<usize>,
    }

    impl<'a> Visit<'a> for ThisFieldReads<'_> {
        fn visit_expression(&mut self, expression: &Expression<'a>) {
            if let Expression::StaticMemberExpression(member) = expression
                && matches!(&member.object, Expression::ThisExpression(_))
                && member.property.name == self.field
            {
                self.nodes.push(std::ptr::from_ref(expression).addr());
                return;
            }
            walk::walk_expression(self, expression);
        }
    }

    let mut reads = ThisFieldReads { field, nodes: Vec::new() };
    reads.visit_expression(value);
    reads.nodes
}
