//! Re-export declarations: `export { a } from`, `export * from` and
//! `export * as ns from`, which forward another module's exports under this one.

use crate::lowering::js_text::js_str_text;
use smelt_hir::{Import, Module};
use super::super::ModuleBuilder;
use super::super::support::module_export_name;

impl ModuleBuilder<'_> {
    /// Alias locally declared items re-exported by name: `export { a, b as c }`.
    ///
    /// Since oxc 0.147 this node carries no `source`; the `... from "module"`
    /// form is a separate `ExportFromDeclaration`, handled by
    /// [`Self::reexport_from_declaration`].
    pub(in crate::lowering) fn reexport_named_declaration(
        &mut self,
        export: &oxc::ast::ast::ExportNamedDeclaration<'_>,
    ) {
        for specifier in &export.specifiers {
            let local = module_export_name(&specifier.local);
            let exported = module_export_name(&specifier.exported);
            // `export { globalThis_ as globalThis }` re-exports a local under a
            // second spelling. This prepass runs before statement lowering, so
            // record the rename and let `record_module_global_object_aliases`
            // project any later-discovered global-object alias onto it. A
            // same-name specifier is recorded too: `import { matcher } from ..;
            // export { matcher }` names an import this prepass has not bound
            // yet, and `record_module_exports` publishes it once it is.
            self.export_renames.insert(exported.clone(), local.clone());
            // A namespace import is the value the specifier exports, whatever
            // item of the same name the module also sees.
            if self.imports.is_namespace(&local) {
                self.reexport_namespace_members(&local, &exported);
            } else if let Some(item) = self.items.get(&local).copied() {
                self.items.insert(exported.clone(), item);
                self.publish_export_alias(exported, item);
            }
        }
    }

    /// Publish the members of the namespace import `local` under `exported`
    /// (`import * as Pattern from './patterns'; export { Pattern as P }`), the
    /// `exported.<member>` keys an importer of `exported` resolves through,
    /// exactly as `export * as exported from './patterns'` publishes them.
    pub(super) fn reexport_namespace_members(&mut self, local: &str, exported: &str) {
        let prefix = format!("{local}.");
        let mut members = self
            .items
            .iter()
            .filter_map(|(name, item)| Some((format!("{exported}.{}", name.strip_prefix(&prefix)?), *item)))
            .collect::<Vec<_>>();
        members.sort_by(|left, right| left.0.cmp(&right.0));
        for (alias, item) in members {
            self.items.insert(alias.clone(), item);
            self.publish_export_alias(alias, item);
        }
    }

    /// Lower `export { name } from "module"` metadata and local aliases.
    pub(in crate::lowering) fn reexport_from_declaration(
        &mut self,
        export: &oxc::ast::ast::ExportFromDeclaration<'_>,
        module: &mut Module,
    ) {
        let source_value = js_str_text(export.source.value);
        let source_text = source_value.as_ref();
        let span = self.span(export.span.start, export.span.end);
        for specifier in &export.specifiers {
            let imported = module_export_name(&specifier.local);
            let exported = module_export_name(&specifier.exported);
            let name = self.intern_source_name(&imported);
            let alias = (exported != imported).then(|| self.intern_source_name(&exported));
            module.imports.push(Import {
                module: source_text.to_owned(),
                name,
                alias,
                span,
            });
            self.alias_imported_item(source_text, &imported, &exported);
            if let Some(item) = self.items.get(&exported).copied() {
                self.publish_export_alias(exported.clone(), item);
                let exported_symbol = alias.unwrap_or(name);
                module.reexports.push((exported_symbol, item));
            }
            if let Some(namespace) = self.object_namespaces.get(&exported).cloned() {
                self.publish_export(|ctx| &mut ctx.object_namespaces, exported.clone(), namespace);
            }
            if let Some(value) = self.consts.object(exported.as_str()).cloned() {
                self.publish_export(|ctx| &mut ctx.object_consts, exported.clone(), value);
            }
            if let Some(overloads) = self.functions.overloads(&exported).cloned() {
                self.publish_export(|ctx| &mut ctx.overloads, exported, overloads);
            }
        }
    }

    /// Lower `export * from "module"` metadata for dependency discovery.
    pub(in crate::lowering) fn reexport_all_declaration(
        &mut self,
        export: &oxc::ast::ast::ExportAllDeclaration<'_>,
        module: &mut Module,
    ) {
        let source_text = js_str_text(export.source.value);
        let source = source_text.as_ref();
        let span = self.span(export.span.start, export.span.end);
        let name = self.intern_source_name("*");
        let alias = export
            .exported
            .as_ref()
            .map(|exported| self.intern_source_name(&module_export_name(exported)));
        module.imports.push(Import {
            module: source.to_owned(),
            name,
            alias,
            span,
        });
        if let Some(exported) = export
            .exported
            .as_ref()
            .map(|exported| module_export_name(exported))
        {
            self.alias_source_exports_under_namespace(source, &exported);
        }
    }

    /// Alias exports from one source module under a namespace re-export.
    ///
    /// For `export * as Types from "./types"`, this records aliases such as
    /// `Types.Id` only for exports that came from `./types`. Keeping this
    /// source-scoped prevents namespace imports in large barrel graphs from
    /// repeatedly re-aliasing every previously seen item.
    pub(in crate::lowering) fn alias_source_exports_under_namespace(&mut self, source: &str, namespace: &str) {
        let Some(exports) = self.source_module_exports(source) else {
            return;
        };
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed inserts of distinct names"
        )]
        for (name, item) in exports {
            let alias = format!("{namespace}.{name}");
            self.publish_export_alias(alias.clone(), item);
            self.items.insert(alias.clone(), item);
            if self.classes.has_item(item) {
                self.classes.register(alias.clone(), item);
            }
            if self.interfaces.has_item(item) {
                self.interfaces.register_alias(alias, item);
            }
        }
    }
}
