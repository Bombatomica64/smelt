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

/// What a module declares and where it reads each name.
#[derive(Debug, Default)]
pub(super) struct SourceFacts {
    /// Plain module-level functions and `const` bindings, exported or not.
    module_callables: HashSet<String>,
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
            facts.record_module_statement(statement);
        }
        facts.visit_program(program);
        #[expect(clippy::iter_over_hash_type, reason = "each list sorts independently")]
        for positions in facts.references.values_mut() {
            positions.sort_unstable();
        }
        facts
    }

    /// Record the callable a module-level statement declares, if any.
    fn record_module_statement(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::FunctionDeclaration(function) => self.record_module_function(function),
            Statement::VariableDeclaration(variable) => self.record_module_const(variable),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => self.record_module_function(function),
                Declaration::VariableDeclaration(variable) => self.record_module_const(variable),
                _ => {}
            },
            _ => {}
        }
    }

    /// Record a plain module-level function.
    ///
    /// `async`, generator and generic functions resolve through their own item
    /// signatures; the forward-callable answer is only for plain ones.
    fn record_module_function(&mut self, function: &Function<'_>) {
        if function.r#async || function.generator || function.type_parameters.is_some() {
            return;
        }
        self.module_callables.extend(function.id.as_ref().map(|id| id.name.to_string()));
    }

    /// Record the name of a module-level `const` whose first binding is plain.
    fn record_module_const(&mut self, variable: &VariableDeclaration<'_>) {
        if variable.kind != VariableDeclarationKind::Const {
            return;
        }
        if let Some(BindingPattern::BindingIdentifier(binding)) =
            variable.declarations.first().map(|declarator| &declarator.id)
        {
            self.module_callables.insert(binding.name.to_string());
        }
    }

    /// Whether the module level declares a function or `const` called `name`.
    pub(super) fn declares_module_callable(&self, name: &str) -> bool {
        self.module_callables.contains(name)
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
