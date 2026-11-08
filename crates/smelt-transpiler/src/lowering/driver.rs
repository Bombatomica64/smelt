//! The manifest lowering driver as an explicit state machine.
//!
//! Lowering a manifest moves through three phases, each its own type, so a
//! phase can only be entered from the one before it:
//!
//! 1. [`ManifestSeed`]: crate-wide facts scanned once from the WHOLE ordered
//!    source list (host-global writes, ambiguous type renames, module
//!    identities, project sources the crate lacks). They are fixed before any
//!    file lowers, so no answer depends on lowering order.
//! 2. [`SeededLowering`]: one TypeScript [`smelt_frontend_ts::HirCtx`] carrying
//!    the seed plus empty Python accumulators; nothing is predeclared yet.
//! 3. [`PredeclaredLowering`]: type declarations predeclared; files lower one by
//!    one into the same context.
//!
//! The TypeScript context is kept alive across every file instead of being
//! rebuilt per file from a mirror struct, so no field can be silently reset by
//! a forgotten copy (the old per-file round trip re-listed ~30 fields twice).
//! A frontend panic leaves the context in an unknown state; the recoverable
//! driver models that as [`RecoverableLowering::Poisoned`] rather than an
//! `Option<Crate>` taken and maybe put back.

use std::{
    collections::{HashMap, HashSet},
    io,
};

use smelt_hir::{FileId, ModuleId};

use super::{
    ManifestDiagnostic, SourceLang, manifest_diagnostics, manifest_module_identities,
    manifest_declared_type_names, manifest_type_renames, scan_written_host_globals,
};
use crate::manifest::ManifestSource;
use crate::python::{PyEnumMembers, PyModuleNamespaces};

/// Crate-wide facts computed once, before predeclaration, from every source.
pub(super) struct ManifestSeed {
    /// Modeled host constructor names reassigned via `globalThis.<Name> =`
    /// anywhere in the crate; see `HirCtx::written_host_globals`.
    written_host_globals: HashSet<String>,
    /// Project source files the dependency closure did not reach; see
    /// `HirCtx::project_sources_outside_crate`.
    project_sources_outside_crate: HashSet<String>,
    /// Rust-facing class names for names declared by more than one module;
    /// see `HirCtx::type_renames`.
    type_renames: HashMap<String, HashMap<String, String>>,
    /// Every top-level type name of the crate; see
    /// `HirCtx::top_level_type_names`.
    top_level_type_names: HashSet<String>,
    /// Crate-unique module identity per module path; see
    /// `HirCtx::module_identities`.
    module_identities: HashMap<String, String>,
}

impl ManifestSeed {
    /// Scan the ordered manifest sources for every seed-once fact.
    ///
    /// `module_names` and `outside_crate` are the caller's: the first decides the
    /// entry module, the second needs the manifest configuration.
    pub(super) fn scan(
        sources: &[&ManifestSource],
        module_names: &[String],
        outside_crate: HashSet<String>,
    ) -> Self {
        let declared = manifest_declared_type_names(sources);
        Self {
            written_host_globals: scan_written_host_globals(sources),
            project_sources_outside_crate: outside_crate,
            type_renames: manifest_type_renames(sources, &declared),
            top_level_type_names: declared.into_iter().flatten().collect(),
            module_identities: manifest_module_identities(sources, module_names),
        }
    }
}

/// Python cross-file accumulators carried between manifest entries.
#[derive(Default)]
struct PyLoweringState {
    /// Python module/package namespaces visible through `import package`.
    module_namespaces: PyModuleNamespaces,
    /// Python `IntEnum` member values visible to later manifest entries.
    enum_members: PyEnumMembers,
}

/// A lowering context holding the seed, before type declarations are predeclared.
pub(super) struct SeededLowering {
    /// TypeScript frontend context; owns the shared HIR crate.
    ts: smelt_frontend_ts::HirCtx,
}

impl SeededLowering {
    /// Start a fresh crate with the seed installed in the TypeScript context.
    pub(super) fn new(seed: ManifestSeed) -> Self {
        let ts = smelt_frontend_ts::HirCtx {
            written_host_globals: seed.written_host_globals,
            project_sources_outside_crate: seed.project_sources_outside_crate,
            type_renames: seed.type_renames,
            top_level_type_names: seed.top_level_type_names,
            module_identities: seed.module_identities,
            ..smelt_frontend_ts::HirCtx::new()
        };
        Self { ts }
    }

    /// Predeclare type aliases and class method surfaces across the TypeScript manifest.
    ///
    /// This gives strongly connected import components the same declaration
    /// visibility as TypeScript: a barrel cycle may affect runtime
    /// initialization order, but it does not hide a type alias declared by
    /// another module.
    pub(super) fn predeclare(
        mut self,
        sources: &[&ManifestSource],
        file_ids: &[FileId],
    ) -> Result<PredeclaredLowering, Box<dyn std::error::Error>> {
        // Unique-symbol consts first, for EVERY source: a class surface below
        // may key a member by a symbol another module (possibly one ordered
        // later in an import cycle) declares.
        for (source, file_id) in sources.iter().zip(file_ids) {
            if source.lang.is_typescript() {
                let path = source.path.display().to_string();
                smelt_frontend_ts::predeclare_unique_symbol_consts_with_path(
                    &source.source,
                    *file_id,
                    &path,
                    &mut self.ts,
                );
            }
        }
        for (source, file_id) in sources.iter().zip(file_ids) {
            if source.lang.is_typescript() {
                let path = source.path.display().to_string();
                smelt_frontend_ts::predeclare_type_declarations_with_path(
                    &source.source,
                    *file_id,
                    &path,
                    &mut self.ts,
                )
                .map_err(|errors| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{}:\n{errors:#?}", source.path.display()),
                    )
                })?;
            }
        }
        Ok(PredeclaredLowering { ts: self.ts, py: PyLoweringState::default() })
    }
}

/// Whole-crate inputs every module's frontend lowering reads.
#[derive(Clone, Copy)]
pub(super) struct ModuleInputs<'input> {
    /// Materialized specialization data.
    pub(super) specialization: &'input crate::specialization::PreparedSpecialization,
    /// TypeScript 7 checker facts for the crate's TypeScript sources.
    pub(super) ts_facts: Option<&'input smelt_ts_model::TsTypeFacts>,
    /// The checkers' verdict, which decides how far each source is trusted.
    pub(super) check: &'input crate::type_gate::CheckReport,
}

/// A predeclared lowering context that lowers manifest files one at a time.
pub(super) struct PredeclaredLowering {
    /// TypeScript frontend context; owns the shared HIR crate between files.
    ts: smelt_frontend_ts::HirCtx,
    /// Python cross-file accumulators.
    py: PyLoweringState,
}

impl PredeclaredLowering {
    /// The HIR crate lowered so far.
    pub(super) const fn krate(&self) -> &smelt_hir::Crate {
        &self.ts.krate
    }

    /// Mutable access to the HIR crate lowered so far.
    pub(super) const fn krate_mut(&mut self) -> &mut smelt_hir::Crate {
        &mut self.ts.krate
    }

    /// Finish lowering and hand back the crate.
    pub(super) fn into_crate(self) -> smelt_hir::Crate {
        self.ts.krate
    }

    /// Lower one manifest file into the shared crate.
    ///
    /// Returns the lowered module or the file's categorized diagnostics; in
    /// both cases the context stays usable, so a recoverable driver can keep
    /// lowering the remaining modules with already-lowered ones resolvable.
    /// The source's language was decided when it was read (a laid typed-version
    /// file can carry TypeScript text under a `.js` path), so the path is not
    /// re-classified.
    pub(super) fn lower_source(
        &mut self,
        source: &ManifestSource,
        file_id: FileId,
        inputs: ModuleInputs<'_>,
    ) -> Result<ModuleId, Vec<ManifestDiagnostic>> {
        let file_string = source.path.display().to_string();
        match source.lang {
            SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
                smelt_frontend_ts::to_hir_with_options(
                    &source.source,
                    file_id,
                    &file_string,
                    &mut self.ts,
                    smelt_frontend_ts::FrontendOptions {
                        specialization: inputs.specialization.typescript(),
                        excluded_modules: &source.excluded_imports,
                        ts_facts: inputs.ts_facts,
                        trust: inputs.check.typescript_trust(&file_string),
                    },
                )
                .map_err(|errors| {
                    manifest_diagnostics(
                        &file_string,
                        &source.source,
                        errors.iter().map(|e| {
                            (
                                e.category,
                                e.code,
                                e.message.clone(),
                                (e.span.file == file_id).then_some(e.span.start),
                            )
                        }),
                    )
                })
            }
            SourceLang::Python | SourceLang::PythonDeclaration => {
                // The Python frontend threads the crate by value; it is moved
                // out of the TypeScript context and put back afterwards.
                let (krate, module_namespaces, enum_members, outcome) =
                    crate::python::lower_manifest_source(
                        std::mem::take(&mut self.ts.krate),
                        std::mem::take(&mut self.py.module_namespaces),
                        std::mem::take(&mut self.py.enum_members),
                        &source.source,
                        file_id,
                        &file_string,
                        inputs.specialization.python(),
                    );
                self.ts.krate = krate;
                self.py = PyLoweringState { module_namespaces, enum_members };
                outcome
            }
        }
    }
}

/// The recoverable (diagnostics-collecting) driver's state between files.
pub(super) enum RecoverableLowering {
    /// Lowering continues; the context is consistent.
    Lowering(Box<PredeclaredLowering>),
    /// A frontend panic unwound through the context mid-file; its crate and
    /// accumulators may be half-updated, so nothing further is lowered or read.
    Poisoned,
}

impl RecoverableLowering {
    /// Start recoverable lowering from a predeclared context.
    pub(super) fn new(lowering: PredeclaredLowering) -> Self {
        Self::Lowering(Box::new(lowering))
    }

    /// Lower one file, catching a frontend panic and poisoning the driver.
    ///
    /// Returns `None` when the driver is (now) poisoned, otherwise the file's
    /// lowering outcome. A poisoned driver never lowers again.
    pub(super) fn lower_source(
        &mut self,
        source: &ManifestSource,
        file_id: FileId,
        inputs: ModuleInputs<'_>,
    ) -> Option<Result<ModuleId, Vec<ManifestDiagnostic>>> {
        let Self::Lowering(lowering) = self else {
            return None;
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lowering.lower_source(source, file_id, inputs)
        }));
        match outcome {
            Ok(lowered) => Some(lowered),
            Err(_panic) => {
                *self = Self::Poisoned;
                None
            }
        }
    }

    /// The crate lowered so far, unless a panic poisoned it.
    pub(super) const fn krate(&self) -> Option<&smelt_hir::Crate> {
        match self {
            Self::Lowering(lowering) => Some(lowering.krate()),
            Self::Poisoned => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::read_manifest_source;

    /// A laid typed version carries TypeScript under a `.js` path, so language is
    /// the source's and not the path's: its unique symbols are predeclared. A
    /// module ordered before it in an import cycle reads them from here.
    #[test]
    fn a_laid_js_source_predeclares_its_unique_symbols() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let written = directory.path().join("index.ts");
        std::fs::write(&written, "export const tag: unique symbol = Symbol(\"tag\");\n")?;
        let mut laid = read_manifest_source(written)?;
        laid.path = directory.path().join("index.js");

        let seed = ManifestSeed::scan(&[&laid], &["index".to_owned()], HashSet::new());
        let lowering = SeededLowering::new(seed).predeclare(&[&laid], &[FileId(0)])?;
        let path = laid.path.display().to_string();
        let symbols = lowering.ts.predeclared_unique_symbols.get(&path);
        if symbols.is_some_and(|names| names.contains_key("tag")) {
            return Ok(());
        }
        Err(format!("`tag` was not predeclared for {path}: {symbols:?}").into())
    }
}
