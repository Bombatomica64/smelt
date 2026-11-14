//! Import declarations: binding an import's locals to the source module's items,
//! recording import provenance, and classifying host-module imports.

use super::super::support::{const_literal_from_item, module_export_name};
use super::super::{ModuleBuilder, PendingHostImport};
use crate::lowering::js_text::js_str_text;
use crate::test_support;
use oxc::ast::ast::{ImportDeclarationSpecifier, ImportOrExportKind, Program, Statement};
use smelt_hir::{ErasureReason, Import, Item, Module};

impl ModuleBuilder<'_> {
    /// Record where each imported name came from before the predeclaration
    /// pass lowers any type surface.
    ///
    /// The predeclaration pass lowers every module's type aliases (and class
    /// field surfaces) before ANY module body lowers, and a consumer that
    /// lowers before the alias's own module reads that predeclared body — the
    /// real lowering overwrites it only once the declaring module's turn comes.
    /// Its builder is otherwise fresh, so without this the alias body had no
    /// import provenance at all: an ambiguous name it imports (`import type {
    /// Context } from './context'` in a crate that also declares an
    /// `interface Context` elsewhere) could not ask the crate-wide rename map
    /// for the EXPORTING module's rendering, fell through to the bare spelling,
    /// and so named the OTHER module's type. Hono's `types.ts` alias
    /// `NotFoundHandler<E> = (c: Context<E>) => ..` was read that way by
    /// `hono-base.ts`/`compose.ts` whenever the build order lowered them before
    /// `types.ts`, emitting `Context<SmeltUnknown>` against the router's
    /// zero-parameter `struct Context` (4,645 `E0107`).
    ///
    /// Only the name-provenance facts are recorded (the specifier and the
    /// exported spelling); binding the name to an item, value-import
    /// classification and host-import bookkeeping stay with
    /// [`Self::import_declaration`] in the real pass, because the imported
    /// module's items need not exist yet.
    pub(in crate::lowering) fn predeclare_import_provenance(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            let Statement::ImportDeclaration(import) = statement else {
                continue;
            };
            let Some(specifiers) = &import.specifiers else {
                continue;
            };
            let source_text = js_str_text(import.source.value);
            let source = source_text.as_ref();
            for specifier in specifiers {
                let (imported, local) = match specifier {
                    ImportDeclarationSpecifier::ImportSpecifier(data) => (
                        module_export_name(&data.imported),
                        data.local.name.as_str().to_owned(),
                    ),
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(data) => {
                        ("default".to_owned(), data.local.name.as_str().to_owned())
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(data) => {
                        ("*".to_owned(), data.local.name.as_str().to_owned())
                    }
                };
                self.imports
                    .record_import_source(local.clone(), source.to_owned());
                self.imports.record_imported_name(local, imported);
            }
        }
    }

    /// Lower an import declaration into module metadata and local item aliases.
    pub(in crate::lowering) fn import_declaration(
        &mut self,
        import: &oxc::ast::ast::ImportDeclaration<'_>,
        module: &mut Module,
    ) {
        let source_text = js_str_text(import.source.value);
        let source = source_text.as_ref();
        let span = self.span(import.span.start, import.span.end);
        let Some(specifiers) = &import.specifiers else {
            return;
        };
        for specifier in specifiers {
            let (imported, local) = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier_data) => {
                    let imported = module_export_name(&specifier_data.imported);
                    let local = specifier_data.local.name.as_str().to_owned();
                    if import.import_kind == ImportOrExportKind::Type
                        || specifier_data.import_kind == ImportOrExportKind::Type
                    {
                        self.imports.mark_type_only(local.clone());
                    }
                    (imported, local)
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier_data) => (
                    "default".to_owned(),
                    specifier_data.local.name.as_str().to_owned(),
                ),
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier_data) => {
                    let local = specifier_data.local.name.as_str().to_owned();
                    self.imports.mark_namespace(local.clone());
                    if import.import_kind == ImportOrExportKind::Type {
                        self.imports.mark_type_only(local.clone());
                    }
                    self.alias_source_exports_under_namespace(source, &local);
                    ("*".to_owned(), local)
                }
            };
            if import.import_kind == ImportOrExportKind::Type {
                self.imports.mark_type_only(local.clone());
            } else {
                self.imports.mark_value(local.clone());
                // An imported binding that the source module resolved to the
                // ambient global object *is* the ambient global object here.
                if self.source_exports_global_object_alias(source, &imported) {
                    self.imports.mark_global_object_alias(local.clone());
                }
            }
            self.imports
                .record_import_source(local.clone(), source.to_owned());
            self.imports
                .record_imported_name(local.clone(), imported.clone());
            let name = self.intern_source_name(&imported);
            let alias = (local != imported).then(|| self.intern_source_name(&local));
            module.imports.push(Import {
                module: source.to_owned(),
                name,
                alias,
                span,
            });
            if test_support::is_vitest_compatible_module(source)
                && test_support::is_vitest_builtin_name(&imported)
            {
                self.imports.mark_test_builtin(local.clone());
            } else if imported != "*" {
                self.alias_imported_item(source, &imported, &local);
                // `import { P } from './index'` where the index declares
                // `export * as P from './patterns'`: `P` is that module's
                // namespace object, whose members the line above aliased as `P.*`.
                if !self.import_alias_resolved(&local)
                    && self.source_exports_namespace(source, &imported)
                {
                    self.imports.mark_namespace(local.clone());
                    continue;
                }
                if !self.imports.is_type_only(&local) && !self.import_alias_resolved(&local) {
                    self.pending_host_imports.push(PendingHostImport {
                        module: source.to_owned(),
                        imported: imported.clone(),
                        local: local.clone(),
                    });
                }
            }
        }
    }

    /// Classify the value imports that resolved to no local source item.
    ///
    /// Runs once, after every import statement of the module has been seen, so
    /// the decision can depend on the module as a whole (see the test-tier
    /// carve-out below) rather than on statement order. Each pending binding
    /// falls into one of three cases, decided by the host-module registry
    /// (`smelt_stdlib::host_modules`) rather than by package-name tests:
    ///
    /// - the module models the export: the binding keeps the erased module
    ///   global it has always had, because the rule that models the export
    ///   recognizes the *use* site, not the binding;
    /// - the module is modeled but the export is only declared: the binding
    ///   records the blocker its first use must report, and deliberately gets
    ///   NO module global. Inserting `Type::Unknown` here is what let
    ///   `import { DatabaseSync } from "node:sqlite"` transpile into a crate of
    ///   no-op dynamic lookups with zero blockers;
    /// - the module is not modeled at all: same blocker, *except* in the test
    ///   tier and for relative specifiers (both below).
    ///
    /// # Two deliberate carve-outs
    ///
    /// A **relative or absolute specifier** (`./falsey`, `/abs/x`) never blocks:
    /// by construction it names a source file, which the manifest resolver owns.
    /// A module lowered on its own (a unit test, or one file of a larger graph)
    /// legitimately sees such an import unresolved, and that is not a host-module
    /// gap.
    ///
    /// The one exception is a specifier the manifest *excluded*: there the
    /// resolver deliberately produced no source file, so the carve-out's
    /// premise does not hold, and using an imported value has to report the
    /// exclusion instead of erasing the binding. That check runs first.
    ///
    /// A **test module** never blocks either. Test code routinely reaches for
    /// assertion and fixture libraries Smelt does not model (`chai`, `yup`,
    /// `@date-fns/utc`), and those values only ever flow into matchers that are
    /// already erased. `CLAUDE.md` sanctions exactly this exception ("everything
    /// must lower through general rules, except test functions"). Program code
    /// gets no such pass: a framework import that drives the program is the
    /// false green this classification exists to stop.
    pub(super) fn classify_pending_host_imports(&mut self, test_module: bool) {
        let pending = std::mem::take(&mut self.pending_host_imports);
        for candidate in pending {
            // A specifier the manifest excluded is neither a missing file nor a
            // host package: the scope decision is recorded, so the blocker can
            // name it precisely. This runs BEFORE the relative-specifier
            // carve-out below, which exists for source files the manifest
            // resolver owns — an excluded module is exactly the case where the
            // resolver deliberately did not produce one. Type-only imports
            // never reach here (the push site skips them), so `import type`
            // and `export type` re-exports from an excluded module stay free,
            // which is what keeps a barrel's type surface usable.
            if self.excluded_modules.contains(&candidate.module) {
                let blocker = format!(
                    "`{name}` is imported from `{module}`, which the manifest excludes",
                    name = candidate.imported,
                    module = candidate.module,
                );
                self.imports
                    .mark_unresolved_value_import(candidate.local, blocker);
                continue;
            }
            let bare_package =
                !candidate.module.starts_with('.') && !candidate.module.starts_with('/');
            let blocker = if bare_package {
                smelt_stdlib::host_value_blocker(&candidate.module, &candidate.imported)
            } else {
                None
            };
            // A modeled host module with a declared-only export is a known gap
            // in Smelt's own surface, so it blocks in the test tier too: no
            // matcher erasure can stand in for `node:sqlite`.
            let modeled_module = smelt_stdlib::is_host_module(&candidate.module);
            let blocker = match blocker {
                Some(blocker) if modeled_module || !test_module => Some(blocker),
                _ => None,
            };
            if let Some(blocker) = blocker {
                self.imports
                    .mark_unresolved_value_import(candidate.local, blocker);
                continue;
            }
            let unknown_ty = self.erase_with(ErasureReason::UntypedPackage, &candidate.module);
            self.module_globals.insert(candidate.local, unknown_ty);
        }
    }

    /// Return whether an imported local already resolves to concrete frontend metadata.
    pub(in crate::lowering) fn import_alias_resolved(&self, local: &str) -> bool {
        self.items.contains_key(local)
            || self.classes.contains(local)
            || self.interfaces.contains(local)
            || self.consts.is_folded_const(local)
            || self.object_namespaces.contains_key(local)
            || self.functions.has_overloads(local)
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
}
