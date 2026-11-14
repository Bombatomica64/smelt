//! The cross-module export table: what this module publishes (items, type
//! predicates, global-object aliases) and how an importer resolves the exports of
//! a source module.

use super::super::support::item_name;
use super::super::{AssertionNarrowing, ModuleBuilder};
use super::phases::SignaturesDeclared;
use crate::context::{ModuleKey, QualifiedExport};
use crate::lowering::support::source_spelling;
use smelt_hir::{Module, Type};
use std::collections::HashMap;
use std::path::Path;

impl ModuleBuilder<'_> {
    /// Record item exports lowered from the current source path.
    ///
    /// Takes the [`SignaturesDeclared`] token by value: the export-rename
    /// projection below reads every module item, so it runs once, after the
    /// signature phase and the statement lowering that follows it.
    pub(in crate::lowering) fn record_module_exports(
        &mut self,
        module: &Module,
        _lowered: SignaturesDeclared,
    ) {
        // Resolve the renames the export prepass could not: `export { Store as
        // StoreBase }` is visited before a non-exported `class Store {}` has
        // been predeclared, so the alias found no item then and was recorded as
        // a rename only. By the time the module is done its items exist, so the
        // alias is published here — otherwise the importing module sees a name
        // no declaration is bound to, and `class X extends StoreBase` recorded a
        // base class nothing declares (silently dropping every inherited member).
        //
        // Every local resolves before any alias is published: in `export { a as
        // b, b as c }` the alias `b` must not capture the local `b` of `c`.
        let resolved = self
            .export_renames
            .iter()
            .filter(|(exported, _)| {
                !self
                    .ctx
                    .export_aliases
                    .contains_key(&self.export_key(exported.as_str()))
            })
            // A namespace import re-exported under a new name published its
            // members instead (`reexport_namespace_members`).
            .filter(|(_, local)| !self.imports.is_namespace(local))
            .filter_map(|(exported, local)| {
                // Either registry can hold the declaration at this point: a class
                // lives in the class registry, everything else in the item map.
                let item = self
                    .items
                    .get(local)
                    .copied()
                    .or_else(|| self.classes.item(local))?;
                Some((exported.clone(), item))
            })
            .collect::<Vec<_>>();
        for (exported, item) in resolved {
            self.items.insert(exported.clone(), item);
            self.publish_export_alias(exported, item);
        }
        let mut exports = HashMap::new();
        for item_id in &module.items {
            let Some(item) = self
                .ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            if let Some(name) = item_name(&self.ctx.krate, item) {
                exports.insert(name.to_owned(), *item_id);
            }
        }
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed inserts of distinct aliases"
        )]
        for (alias, item_id) in &self.ctx.export_aliases {
            if alias.module == self.module_key {
                exports.insert(alias.name.clone(), *item_id);
            }
        }
        // The module's own `export { local as exported }` specifiers, including
        // re-exports of an import (`import { matcher } from ..; export { matcher }`)
        // that bound no alias of their own.
        let own = self
            .export_renames
            .iter()
            .filter(|(_, local)| !self.imports.is_namespace(local))
            .filter_map(|(exported, local)| {
                Some((exported.clone(), self.items.get(local).copied()?))
            })
            .collect::<Vec<_>>();
        for (exported, item_id) in own {
            exports.entry(exported).or_insert(item_id);
        }
        self.publish_exported_predicates(&exports);
        self.ctx
            .module_exports
            .insert(self.path.clone(), exports.clone());
        if let Some(stripped) = self.path.strip_prefix("./") {
            self.ctx
                .module_exports
                .insert(stripped.to_owned(), exports.clone());
        }
        if let Some(canonical) = Self::canonical_module_path(&self.path) {
            self.ctx.module_exports.insert(canonical, exports);
        }
        self.record_module_global_object_aliases();
    }

    /// The key this module publishes `name` under in the crate-wide export
    /// tables.
    pub(in crate::lowering) fn export_key(&self, name: impl Into<String>) -> QualifiedExport {
        QualifiedExport {
            module: self.module_key.clone(),
            name: name.into(),
        }
    }

    /// Publish `value` for this module's export `name` in one crate-wide
    /// export table (`table` picks it out of the context).
    pub(in crate::lowering) fn publish_export<V: Clone>(
        &mut self,
        table: impl FnOnce(&mut crate::HirCtx) -> &mut crate::context::ExportTable<V>,
        name: impl Into<String>,
        value: V,
    ) {
        let key = self.export_key(name);
        table(self.ctx).write().insert(key, value);
    }

    /// Publish `item` as this module's export `alias`.
    pub(in crate::lowering) fn publish_export_alias(
        &mut self,
        alias: impl Into<String>,
        item: smelt_hir::ItemId,
    ) {
        let key = self.export_key(alias);
        self.ctx.export_aliases.insert(key, item);
    }

    /// The crate module an import specifier resolves to, when the crate has
    /// created a builder for it (in an import cycle, possibly one that has not
    /// finished lowering).
    pub(in crate::lowering) fn source_module_key(&self, source: &str) -> Option<ModuleKey> {
        self.resolved_module_export_keys(source)
            .into_iter()
            .map(|candidate| ModuleKey::of_path(&candidate))
            .find(|key| self.ctx.known_modules.contains(key))
    }

    /// Copy the facts `source` publishes about its export `imported` into this
    /// module's tables under the local binding `local`: an object constant and
    /// its value projection, a collection, enum members, a namespace object,
    /// overloads and a rest parameter.
    ///
    /// The tables are keyed by [`QualifiedExport`], so the facts are the
    /// exporting module's even when another module exports the same name
    ///.
    pub(in crate::lowering) fn adopt_exported_facts(
        &mut self,
        source: &str,
        imported: &str,
        local: &str,
    ) {
        let Some(module) = self.source_module_key(source) else {
            return;
        };
        let key = QualifiedExport {
            module,
            name: imported.to_owned(),
        };
        let local = local.to_owned();
        if let Some(value) = self.ctx.object_consts.get(&key).cloned() {
            self.consts.set_object(local.clone(), value);
        }
        if let Some(value) = self.ctx.object_value_collections.get(&key).cloned() {
            self.consts
                .set_object_value_collection(local.clone(), value);
        }
        if let Some(value) = self.ctx.const_collections.get(&key).cloned() {
            self.consts.set_collection(local.clone(), value);
        }
        if let Some(members) = self.ctx.enum_members.get(&key).cloned() {
            self.consts.set_enum_members(local.clone(), members);
        }
        if let Some(namespace) = self.ctx.object_namespaces.get(&key).cloned() {
            self.object_namespaces
                .write()
                .insert(local.clone(), namespace);
        }
        if let Some(overloads) = self.ctx.overloads.get(&key).cloned() {
            self.functions.set_overloads(local.clone(), overloads);
        }
        if let Some(rest) = self.ctx.function_rests.get(&key).copied() {
            self.functions.set_rest(local, rest);
        }
    }

    /// Publish what each exported type predicate proves, for importing modules.
    ///
    /// A predicate whose target has no concrete carrier (`value is any[]`,
    /// `value is PropertyKey`) gives an importer no type to read at, only a cast
    /// into one more erased value per read, so it stays with the module that
    /// spells it.
    fn publish_exported_predicates(&mut self, exports: &HashMap<String, smelt_hir::ItemId>) {
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed inserts, one per exported item"
        )]
        for (exported, item) in exports {
            let local = self.export_renames.get(exported).unwrap_or(exported);
            let proof = self
                .functions
                .predicate(local)
                .map(|narrowing| (false, narrowing))
                .or_else(|| {
                    self.functions
                        .assertion(local)
                        .map(|narrowing| (true, narrowing))
                });
            if let Some((is_assertion, narrowing)) = proof
                && self.has_concrete_carrier(narrowing.target)
            {
                self.ctx.item_predicates.insert(
                    *item,
                    crate::context::ItemPredicate {
                        is_assertion,
                        param_index: narrowing.param_index,
                        target: narrowing.target,
                    },
                );
            }
        }
    }

    /// Whether a value of `ty` reaches Rust at a concrete type: a primitive, a
    /// class this crate declares, or a list or option of one.
    fn has_concrete_carrier(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::String | Type::Float | Type::Int | Type::Bool) => true,
            Some(Type::List(item) | Type::Optional(item)) => self.has_concrete_carrier(*item),
            Some(Type::Class { name, args }) => {
                args.is_empty()
                    && source_spelling(&self.ctx.krate, *name)
                        .is_some_and(|class| self.classes.item(class).is_some())
            }
            _ => false,
        }
    }

    /// Adopt the type predicate of an imported item under its local name.
    pub(super) fn adopt_imported_predicate(&mut self, item: smelt_hir::ItemId, local: &str) {
        let Some(predicate) = self.ctx.item_predicates.get(&item).copied() else {
            return;
        };
        let narrowing = AssertionNarrowing {
            param_index: predicate.param_index,
            target: predicate.target,
        };
        if predicate.is_assertion {
            self.functions.set_assertion(local.to_owned(), narrowing);
        } else {
            self.functions
                .set_imported_predicate(local.to_owned(), narrowing);
        }
    }

    /// Publish this module's global-object alias names for importing modules.
    ///
    /// A module that binds the ambient global object (`const g = globalThis;` or
    /// the portable `(typeof globalThis === 'object' && globalThis) || …`
    /// detection chain) and exports that binding hands importers the global
    /// object itself. Recording the names under the same path keys as
    /// [`Self::record_module_exports`] lets [`Self::import_declaration`] mark the
    /// imported local as a global alias, so `globalThis.Buffer.isBuffer(x)`
    /// resolves against the modeled global no matter whether the source reaches
    /// it through the bare spelling or through a shim module. Only names this
    /// module can export are ever consulted by an importer, so publishing the
    /// whole alias set is sound.
    pub(in crate::lowering) fn record_module_global_object_aliases(&mut self) {
        let mut aliases = self.imports.global_object_alias_names().clone();
        if aliases.is_empty() {
            return;
        }
        // Project the aliases onto the spellings they are exported under.
        let renamed = self
            .export_renames
            .iter()
            .filter(|(_, local)| aliases.contains(local.as_str()))
            .map(|(exported, _)| exported.clone())
            .collect::<Vec<_>>();
        aliases.extend(renamed);
        let mut keys = vec![self.path.clone()];
        if let Some(stripped) = self.path.strip_prefix("./") {
            keys.push(stripped.to_owned());
        }
        if let Some(canonical) = Self::canonical_module_path(&self.path) {
            keys.push(canonical);
        }
        for key in keys {
            self.ctx
                .module_global_object_aliases
                .entry(key)
                .or_default()
                .extend(aliases.iter().cloned());
        }
    }

    /// Return whether `imported` is a global-object alias exported by `source`.
    pub(in crate::lowering) fn source_exports_global_object_alias(
        &self,
        source: &str,
        imported: &str,
    ) -> bool {
        self.resolved_module_export_keys(source)
            .into_iter()
            .any(|key| {
                self.ctx
                    .module_global_object_aliases
                    .get(&key)
                    .is_some_and(|aliases| aliases.contains(imported))
            })
    }

    /// Return a canonical path string when the path exists on disk.
    pub(in crate::lowering) fn canonical_module_path(path: &str) -> Option<String> {
        std::fs::canonicalize(path)
            .ok()
            .map(|path| path.display().to_string())
    }

    /// Whether `source` exports `name` as a namespace re-export
    /// (`export * as name from ..`): its exports hold `name.<member>` keys.
    pub(super) fn source_exports_namespace(&self, source: &str, name: &str) -> bool {
        let prefix = format!("{name}.");
        self.source_module_exports(source).is_some_and(|exports| {
            !exports.contains_key(name) && exports.keys().any(|key| key.starts_with(&prefix))
        })
    }

    /// Return the exports for an import source resolved relative to this file.
    pub(in crate::lowering) fn source_module_exports(
        &self,
        source: &str,
    ) -> Option<HashMap<String, smelt_hir::ItemId>> {
        self.resolved_module_export_keys(source)
            .into_iter()
            .find_map(|key| self.ctx.module_exports.get(&key).cloned())
    }

    /// Return candidate module-export keys for a TypeScript source specifier.
    pub(in crate::lowering) fn resolved_module_export_keys(&self, source: &str) -> Vec<String> {
        // A bare specifier that names a modeled host module (`crypto`,
        // `path`) IS that module: Node resolves a builtin before any file, so
        // it must never be joined onto the importer's directory, where it
        // would pick a sibling source file of the same stem (Hono's
        // `utils/crypto.test.ts` imports `createHash` from `crypto` next to
        // its own `utils/crypto.ts`).
        if !source.starts_with('.')
            && !Path::new(source).is_absolute()
            && smelt_stdlib::is_host_module(source)
        {
            return Vec::new();
        }
        let mut keys = Vec::new();
        Self::push_module_export_key(&mut keys, source);
        let source_path = Path::new(source);
        let base = if source_path.is_absolute() {
            source_path.to_path_buf()
        } else {
            Path::new(&self.path)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(source_path)
        };
        let candidates = [
            base.clone(),
            base.with_extension("ts"),
            base.with_extension("tsx"),
            base.with_extension("d.ts"),
            base.join("index.ts"),
            base.join("index.d.ts"),
        ];
        for candidate in candidates {
            Self::push_module_export_key(&mut keys, &candidate.display().to_string());
            if let Some(canonical) = Self::canonical_module_path(&candidate.display().to_string()) {
                Self::push_module_export_key(&mut keys, &canonical);
            }
        }
        keys
    }

    /// Push a module-export key and a leading-`./`-less variant.
    pub(in crate::lowering) fn push_module_export_key(keys: &mut Vec<String>, key: &str) {
        keys.push(key.to_owned());
        if let Some(stripped) = key.strip_prefix("./") {
            keys.push(stripped.to_owned());
        }
    }
}
