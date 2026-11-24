//! The cross-module export table: what this module publishes (items, type
//! predicates, global-object aliases) and how an importer resolves the exports of
//! a source module.

use super::super::support::const_literal_from_item;
use super::super::{AssertionNarrowing, ModuleBuilder};
use crate::context::{ModuleKey, QualifiedExport};
use crate::lowering::support::source_spelling;
use smelt_hir::{Item, Type};
use std::collections::HashMap;
use std::path::Path;

impl ModuleBuilder<'_> {
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
    pub(super) fn publish_exported_predicates(
        &mut self,
        exports: &HashMap<String, smelt_hir::ItemId>,
    ) {
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
            Some(Type::List(item) | Type::Optional(item, _)) => self.has_concrete_carrier(*item),
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

    /// Add a local alias for an imported item when it is already known.
    pub(in crate::lowering) fn alias_imported_item(
        &mut self,
        source: &str,
        imported: &str,
        local: &str,
    ) {
        // A static property hung off an exported function (`partial.placeholder`)
        // is recorded under the compound key `partial.placeholder`, so an importer
        // has to alias those alongside the name itself. The fallback path below
        // already does this for locally-known items; the resolved-exports path
        // returned early and skipped it, which left an imported
        // `partial.placeholder` unresolved.
        if let Some(exports) = self.source_module_exports(source) {
            let imported_prefix = format!("{imported}.");
            let qualified = exports
                .iter()
                .filter_map(|(name, item)| {
                    let member = name.strip_prefix(&imported_prefix)?;
                    Some((format!("{local}.{member}"), *item))
                })
                .collect::<Vec<_>>();
            for (alias, item) in qualified {
                self.items.insert(alias, item);
            }
        }
        // Facts that are not items (an object constant's entries, enum
        // members, overloads, ...) come from the module `source` resolves to,
        // never from whichever module last published the bare name.
        self.adopt_exported_facts(source, imported, local);
        if let Some(exports) = self.source_module_exports(source)
            && let Some(item) = exports.get(imported).copied()
        {
            self.items.insert(local.to_owned(), item);
            self.adopt_imported_predicate(item, local);
            match self.item_ref(item) {
                Item::Class(_) => {
                    self.classes.register(local.to_owned(), item);
                }
                Item::Interface(_) => {
                    self.interfaces.register_alias(local.to_owned(), item);
                }
                // Imported primitive const literals (e.g. `export const stringTag
                // = '[object String]'`) must be foldable in the importer so they
                // can appear in switch case labels and other constant positions.
                // The construction-time `visible_const_literals` snapshot misses
                // imports whose source module is lowered after this one, so fold
                // the resolved crate item here where the dependency is guaranteed
                // present.
                Item::Const(const_item) => {
                    if let Some(value) = const_literal_from_item(&self.ctx.krate, const_item) {
                        self.consts.set_literal(local.to_owned(), value);
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(item) = self.items.get(imported).copied() {
            self.items.insert(local.to_owned(), item);
        }
        let imported_prefix = format!("{imported}.");
        let qualified_item_aliases = self
            .items
            .iter()
            .filter_map(|(name, item)| {
                let member = name.strip_prefix(&imported_prefix)?;
                Some((format!("{local}.{member}"), *item))
            })
            .collect::<Vec<_>>();
        for (alias, item) in qualified_item_aliases {
            self.items.insert(alias, item);
        }
        if let Some(item) = self.classes.item(imported) {
            self.classes.register(local.to_owned(), item);
        }
        let qualified_class_aliases = self
            .classes
            .entries()
            .filter_map(|(name, item)| {
                let member = name.strip_prefix(&imported_prefix)?;
                Some((format!("{local}.{member}"), item))
            })
            .collect::<Vec<_>>();
        for (alias, item) in qualified_class_aliases {
            self.classes.register(alias, item);
        }
        if let Some(item) = self.interfaces.item(imported) {
            self.interfaces.register_alias(local.to_owned(), item);
        }
        let qualified_interface_aliases = self
            .interfaces
            .entries()
            .filter_map(|(name, item)| {
                let member = name.strip_prefix(&imported_prefix)?;
                Some((format!("{local}.{member}"), item))
            })
            .collect::<Vec<_>>();
        for (alias, item) in qualified_interface_aliases {
            self.interfaces.register_alias(alias, item);
        }
        // Rebinding by the imported name only reaches this module's own
        // declarations (`import { a as b } from './self'`): another module's
        // constants were adopted through its qualified exports above.
        self.consts.rebind_import(local, imported);
        if let Some(namespace) = self.object_namespaces.get(imported).cloned() {
            self.object_namespaces
                .write()
                .insert(local.to_owned(), namespace);
        }
        if let Some(overloads) = self.functions.overloads(imported).cloned() {
            self.functions.set_overloads(local.to_owned(), overloads);
        }
        if let Some(rest) = self.functions.rest(imported) {
            self.functions.set_rest(local.to_owned(), rest);
        }
    }

    /// Alias exports from one source module under a namespace re-export.
    ///
    /// For `export * as Types from "./types"`, this records aliases such as
    /// `Types.Id` only for exports that came from `./types`. Keeping this
    /// source-scoped prevents namespace imports in large barrel graphs from
    /// repeatedly re-aliasing every previously seen item.
    pub(in crate::lowering) fn alias_source_exports_under_namespace(
        &mut self,
        source: &str,
        namespace: &str,
    ) {
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
