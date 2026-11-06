//! Name facts read from a module's AST once, before lowering.
//!
//! Lowering often asks about a declaration or a read that sits later in the
//! file than the question, so the registries cannot answer yet. [`SourceFacts`]
//! answers those questions from the parsed program: a name in a comment or a
//! string is no declaration, and `class URLParser` does not declare `URL`.

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::{
    BindingPattern, Class, Declaration, Function, IdentifierReference, Program, Statement,
    TSEnumDeclaration, TSInterfaceDeclaration, TSTypeAliasDeclaration, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc::ast_visit::{Visit, walk};
use oxc::span::Span;
use oxc::syntax::scope::ScopeFlags;

/// What a module declares and where it reads each name.
#[derive(Debug, Default)]
pub(super) struct SourceFacts {
    /// Plain named functions and `const` declarations at any depth.
    forward_callables: HashSet<String>,
    /// The values the module level binds: functions, classes and variables.
    module_values: HashSet<String>,
    /// Named classes declared at any depth.
    classes: HashSet<String>,
    /// Named types declared at any depth: classes, interfaces, aliases, enums.
    types: HashSet<String>,
    /// The start offset of each identifier reference, ascending per name.
    references: HashMap<String, Vec<u32>>,
}

impl SourceFacts {
    /// Collect the facts of `program`.
    pub(super) fn collect(program: &Program<'_>) -> Self {
        let mut facts = Self::default();
        for statement in &program.body {
            facts.record_module_value(statement);
        }
        facts.visit_program(program);
        #[expect(clippy::iter_over_hash_type, reason = "each list sorts independently")]
        for positions in facts.references.values_mut() {
            positions.sort_unstable();
        }
        facts
    }

    /// Record the value names a module-level statement binds.
    fn record_module_value(&mut self, statement: &Statement<'_>) {
        let declaration = match statement {
            Statement::ExportDeclaration(export) => &export.declaration,
            Statement::FunctionDeclaration(function) => {
                self.module_values.extend(function.id.as_ref().map(|id| id.name.to_string()));
                return;
            }
            Statement::ClassDeclaration(class) => {
                self.module_values.extend(class.id.as_ref().map(|id| id.name.to_string()));
                return;
            }
            Statement::VariableDeclaration(variable) => {
                self.record_module_variables(variable);
                return;
            }
            _ => return,
        };
        match declaration {
            Declaration::FunctionDeclaration(function) => {
                self.module_values.extend(function.id.as_ref().map(|id| id.name.to_string()));
            }
            Declaration::ClassDeclaration(class) => {
                self.module_values.extend(class.id.as_ref().map(|id| id.name.to_string()));
            }
            Declaration::VariableDeclaration(variable) => self.record_module_variables(variable),
            _ => {}
        }
    }

    /// Record the plainly bound names of a module-level variable statement.
    fn record_module_variables(&mut self, variable: &VariableDeclaration<'_>) {
        for declarator in &variable.declarations {
            if let BindingPattern::BindingIdentifier(binding) = &declarator.id {
                self.module_values.insert(binding.name.to_string());
            }
        }
    }

    /// Record the `const` a statement declares, if any.
    fn record_const_statement(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::VariableDeclaration(variable) => self.record_const(variable),
            Statement::ExportDeclaration(export) => {
                if let Declaration::VariableDeclaration(variable) = &export.declaration {
                    self.record_const(variable);
                }
            }
            _ => {}
        }
    }

    /// Record a plain named function, declared or used as an expression.
    ///
    /// `async`, generator and generic functions resolve through their own item
    /// signatures; the forward-callable answer is only for plain ones.
    fn record_function(&mut self, function: &Function<'_>) {
        if function.r#async || function.generator || function.type_parameters.is_some() {
            return;
        }
        self.forward_callables.extend(function.id.as_ref().map(|id| id.name.to_string()));
    }

    /// Record the name of a `const` whose first binding is plain.
    fn record_const(&mut self, variable: &VariableDeclaration<'_>) {
        if variable.kind != VariableDeclarationKind::Const {
            return;
        }
        if let Some(BindingPattern::BindingIdentifier(binding)) =
            variable.declarations.first().map(|declarator| &declarator.id)
        {
            self.forward_callables.insert(binding.name.to_string());
        }
    }

    /// Whether a plain function or `const` called `name` is declared.
    pub(super) fn declares_forward_callable(&self, name: &str) -> bool {
        self.forward_callables.contains(name)
    }

    /// Whether the module level binds the value `name`.
    pub(super) fn declares_module_value(&self, name: &str) -> bool {
        self.module_values.contains(name)
    }

    /// Whether the module declares a class called `name`, at any depth.
    pub(super) fn declares_class(&self, name: &str) -> bool {
        self.classes.contains(name)
    }

    /// Whether the module declares a type called `name`, at any depth.
    pub(super) fn declares_type(&self, name: &str) -> bool {
        self.types.contains(name)
    }

    /// Whether `name` is read inside `span`.
    pub(super) fn mentions(&self, span: Span, name: &str) -> bool {
        self.references.get(name).is_some_and(|positions| {
            let first = positions.partition_point(|position| *position < span.start);
            positions.get(first).is_some_and(|position| *position < span.end)
        })
    }
}

impl<'a> Visit<'a> for SourceFacts {
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        self.record_const_statement(statement);
        walk::walk_statement(self, statement);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.record_function(function);
        walk::walk_function(self, function, flags);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        if let Some(id) = &class.id {
            self.classes.insert(id.name.to_string());
            self.types.insert(id.name.to_string());
        }
        walk::walk_class(self, class);
    }

    fn visit_ts_interface_declaration(&mut self, interface: &TSInterfaceDeclaration<'a>) {
        self.types.insert(interface.id.name.to_string());
        walk::walk_ts_interface_declaration(self, interface);
    }

    fn visit_ts_type_alias_declaration(&mut self, alias: &TSTypeAliasDeclaration<'a>) {
        self.types.insert(alias.id.name.to_string());
        walk::walk_ts_type_alias_declaration(self, alias);
    }

    fn visit_ts_enum_declaration(&mut self, declaration: &TSEnumDeclaration<'a>) {
        self.types.insert(declaration.id.name.to_string());
        walk::walk_ts_enum_declaration(self, declaration);
    }

    fn visit_identifier_reference(&mut self, reference: &IdentifierReference<'a>) {
        self.references
            .entry(reference.name.to_string())
            .or_default()
            .push(reference.span.start);
    }
}
