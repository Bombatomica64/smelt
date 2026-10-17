//! Crate-wide predeclaration of module-level unique-symbol constants.
//!
//! `export const K: unique symbol = Symbol()` binds one symbol for the
//! program's lifetime, so `K` is a static member NAME: `class C { get [K]() }`
//! declares the member and `c[K]` reads it (see
//! `ty::computed_key_symbols::unique_symbol_key`). Ordinarily the consumer
//! learns `K`'s value from the lowered `const` item of `K`'s module.
//!
//! An import cycle breaks that: the manifest orders modules by a depth-first
//! walk, so in `a -> b -> (type-only) c -> a` the module `c` can lower before
//! the module that declares `K`, and `c[K]` would have no member name to
//! resolve to — the read fell to the erased runtime-keyed path. The member
//! names a cycle-early consumer needs are fixed by the source text alone (the
//! symbol's identity is its declaring site), so this pre-pass records every
//! such binding before any module body lowers, and
//! [`ModuleBuilder::predeclared_unique_symbol`] answers from it.
//!
//! Only the exact shape whose value is known without evaluation is recorded:
//! a top-level `const` whose initializer is a direct `Symbol()` /
//! `Symbol('description')` call. Anything else keeps the existing path.

use crate::HirCtx;
use crate::lowering::ModuleBuilder;
use crate::lowering::stdlib::call_dispatch::unique_symbol_spelling;
use oxc::ast::ast::{
    Argument, BindingPattern, Declaration, Expression, Program, Statement, VariableDeclarationKind,
};
use smelt_hir::{FileId, Span};
use std::collections::HashMap;

/// Record the module-level unique-symbol consts `program` declares into
/// `ctx.predeclared_unique_symbols`, under every key an importer's specifier
/// can resolve to (the path as given and its canonical form).
pub(in crate::lowering) fn record_unique_symbol_consts(
    program: &Program<'_>,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
) {
    let mut bindings = HashMap::new();
    for statement in &program.body {
        let declaration = match statement {
            Statement::VariableDeclaration(declaration) => declaration,
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => declaration,
                _ => continue,
            },
            _ => continue,
        };
        if declaration.kind != VariableDeclarationKind::Const {
            continue;
        }
        for declarator in &declaration.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            let Some(Expression::CallExpression(call)) = &declarator.init else {
                continue;
            };
            if !matches!(&call.callee, Expression::Identifier(callee) if callee.name == "Symbol")
            {
                continue;
            }
            let description = match call.arguments.as_slice() {
                [] => "",
                [Argument::StringLiteral(literal)] => literal.value.as_str(),
                _ => continue,
            };
            let site = Span::new(file_id, call.span.start, call.span.end);
            bindings.insert(
                id.name.as_str().to_owned(),
                unique_symbol_spelling(description, site),
            );
        }
    }
    if bindings.is_empty() {
        return;
    }
    let mut keys = Vec::new();
    ModuleBuilder::push_module_export_key(&mut keys, path);
    if let Some(canonical) = ModuleBuilder::canonical_module_path(path) {
        ModuleBuilder::push_module_export_key(&mut keys, &canonical);
    }
    for key in keys {
        ctx.predeclared_unique_symbols
            .entry(key)
            .or_default()
            .extend(bindings.iter().map(|(name, spelling)| (name.clone(), spelling.clone())));
    }
}

impl ModuleBuilder<'_> {
    /// The value spelling of the module-level unique symbol `name` denotes in
    /// this module, from the crate-wide pre-pass (module docs).
    ///
    /// `name` is either declared at this module's top level or imported; an
    /// import is followed to its declaring module through the specifier the
    /// import names. A name bound by an enclosing local scope never answers
    /// here: it shadows the module binding.
    pub(in crate::lowering) fn predeclared_unique_symbol(&self, name: &str) -> Option<String> {
        if self.scope.is_bound(name) {
            return None;
        }
        let table = &self.ctx.predeclared_unique_symbols;
        if let Some(source) = self.imports.import_source(name) {
            let exported = self.imports.imported_name(name).unwrap_or(name);
            return self
                .resolved_module_export_keys(source)
                .iter()
                .find_map(|key| table.get(key)?.get(exported).cloned());
        }
        table.get(&self.path)?.get(name).cloned()
    }
}
