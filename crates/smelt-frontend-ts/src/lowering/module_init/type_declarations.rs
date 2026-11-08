//! Module type declarations: the names of the program's classes, interfaces and
//! type declarations, enum member literals, and type alias items, all collected
//! before any body is lowered.

use std::collections::HashSet;
use oxc::ast::ast::{Declaration, Program, Statement};
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Collect class names declared in the current module before lowering eager arrow bodies.
    pub(in crate::lowering) fn program_class_names(program: &Program<'_>) -> HashSet<String> {
        let mut names = HashSet::new();
        for statement in &program.body {
            let exported = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => None,
            };
            let class = match (statement, exported) {
                (Statement::ClassDeclaration(class), _) => Some(class.as_ref()),
                (_, Some(Declaration::ClassDeclaration(class))) => Some(class.as_ref()),
                _ => None,
            };
            if let Some(id) = class.and_then(|class| class.id.as_ref()) {
                names.insert(id.name.to_string());
            }
            // `const Foo = class { … }` declares a class named `Foo` (see
            // `ModuleBuilder::class_declaration_named`), so the name is
            // pending from the prepass exactly like a `class Foo {}`
            // declaration and a `new Foo()` lowered earlier in the module still
            // resolves nominally.
            let variable = match (statement, exported) {
                (Statement::VariableDeclaration(variable), _) => Some(variable.as_ref()),
                (_, Some(Declaration::VariableDeclaration(variable))) => Some(variable.as_ref()),
                _ => None,
            };
            for declarator in variable.iter().flat_map(|variable| &variable.declarations) {
                if let Some((name, _)) = Self::const_class_expression(declarator) {
                    names.insert(name.to_owned());
                }
            }
        }
        names
    }

    /// Collect the names of every TYPE the module declares at top level.
    ///
    /// These are the declarations that bind a name in the module's type
    /// namespace: `interface`, `type`, `class` and `enum`, written bare or under
    /// `export`. A `const Foo = class {}` binds only a VALUE, so it is not
    /// collected here even though `program_class_names` registers it as a class.
    /// Ambient `declare` forms bind the name just the same and are included.
    pub(in crate::lowering) fn program_type_declaration_names(program: &Program<'_>) -> HashSet<String> {
        let mut names = HashSet::new();
        for statement in &program.body {
            let declaration = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => None,
            };
            let name = match (statement, declaration) {
                (Statement::TSInterfaceDeclaration(interface), _)
                | (_, Some(Declaration::TSInterfaceDeclaration(interface))) => {
                    Some(interface.id.name.as_str())
                }
                (Statement::TSTypeAliasDeclaration(alias), _)
                | (_, Some(Declaration::TSTypeAliasDeclaration(alias))) => {
                    Some(alias.id.name.as_str())
                }
                (Statement::ClassDeclaration(class), _)
                | (_, Some(Declaration::ClassDeclaration(class))) => {
                    class.id.as_ref().map(|id| id.name.as_str())
                }
                (Statement::TSEnumDeclaration(declaration), _)
                | (_, Some(Declaration::TSEnumDeclaration(declaration))) => {
                    Some(declaration.id.name.as_str())
                }
                _ => None,
            };
            if let Some(name) = name {
                names.insert(name.to_owned());
            }
        }
        names
    }

    /// Collect interface names declared in the current module before lowering.
    ///
    /// This distinguishes lexically local interfaces, including forward
    /// declarations, from imported or ambient names that happen to share a
    /// symbol with an interface lowered from another source file.
    pub(in crate::lowering) fn program_interface_names(program: &Program<'_>) -> HashSet<String> {
        program
            .body
            .iter()
            .filter_map(|statement| {
                let interface = match statement {
                    Statement::TSInterfaceDeclaration(interface) => interface,
                    Statement::ExportDeclaration(export) => {
                        let Declaration::TSInterfaceDeclaration(interface) =
                            &export.declaration
                        else {
                            return None;
                        };
                        interface
                    }
                    _ => return None,
                };
                Some(interface.id.name.to_string())
            })
            .collect()
    }

    /// Const-fold every top-level `enum` declaration so member references and
    /// `case EnumName.Member:` labels can inline the member's literal.
    ///
    /// Runs during the collection phase, before function bodies and switch
    /// statements are lowered, because TypeScript hoists enum declarations: a
    /// member may be referenced textually before the `enum` appears. Handles
    /// both bare `enum E {}` and `export enum E {}` forms.
    pub(in crate::lowering) fn collect_module_enums(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::TSEnumDeclaration(decl) => {
                    self.collect_enum_declaration(decl);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::TSEnumDeclaration(decl) = &export.declaration {
                        self.collect_enum_declaration(decl);
                    }
                }
                _ => {}
            }
        }
    }

    /// Lower type aliases early so hoisted function signatures can use them.
    pub(in crate::lowering) fn predeclare_type_alias_items(&mut self, program: &Program<'_>) {
        for _ in 0_usize..2_usize {
            for statement in &program.body {
                match statement {
                    Statement::TSTypeAliasDeclaration(alias) => {
                        drop(self.type_alias_declaration(alias));
                    }
                    Statement::TSNamespaceDeclaration(module_decl) => {
                        drop(self.type_namespace_declaration(module_decl));
                    }
                    Statement::ExportDeclaration(export) => {
                        if let Declaration::TSTypeAliasDeclaration(alias) =
                            &export.declaration
                        {
                            drop(self.type_alias_declaration(alias));
                        } else if let Declaration::TSNamespaceDeclaration(module_decl) =
                            &export.declaration
                        {
                            drop(self.type_namespace_declaration(module_decl));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
