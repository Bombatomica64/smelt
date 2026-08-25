//! Minimal `ty` database used for Smelt's one-module inference queries.
//!
//! This follows the public database composition demonstrated by
//! `ty_python_semantic`'s `TestDb`, but uses an [`OsSystem`] and deliberately
//! omits project/config discovery. Smelt materializes each source module in an
//! empty temporary root, so ty's default analysis settings plus its vendored
//! typeshed are the complete environment needed by the frontend.

use std::sync::Arc;

use anyhow::{Context, Result};
use ruff_db::Db as RuffDb;
use ruff_db::diagnostic::Diagnostic;
use ruff_db::files::{File, Files};
use ruff_db::system::{OsSystem, System, SystemPathBuf};
use ruff_db::vendored::VendoredFileSystem;
use ty_module_resolver::{Db as ModuleResolverDb, SearchPathSettings};
use ty_python_core::ProgramFile;
use ty_python_core::platform::PythonPlatform;
use ty_python_core::program::{FallibleStrategy, Program, ProgramSettings};
use ty_python_semantic::lint::{LintRegistry, RuleSelection};
use ty_python_semantic::{AnalysisSettings, Db as SemanticDb, default_lint_registry};
use ty_site_packages::PythonVersionWithSource;

/// Concrete salsa database for Smelt's ty-backed type inference.
#[salsa::db]
#[derive(Clone)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent module consumes this focused private-module implementation"
)]
pub(crate) struct SmeltTyDb {
    /// Incremental query storage required by salsa.
    storage: salsa::Storage<Self>,
    /// Ruff's interned source-file registry.
    files: Files,
    /// Operating-system filesystem rooted at the temporary project.
    system: OsSystem,
    /// Typeshed and other files embedded by ty.
    vendored: VendoredFileSystem,
    /// Default lint selection required by the semantic database interface.
    rule_selection: Arc<RuleSelection>,
    /// Default inference and diagnostic analysis settings.
    analysis_settings: Arc<AnalysisSettings>,
    /// Program-wide settings (Python version/platform and resolved search
    /// paths) this database's [`Program`] is derived from.
    ///
    /// Since ty 0.0.10 the `Program` is no longer a database-global singleton
    /// retrieved with `Program::get`; it is interned on demand from these
    /// settings (see [`Self::program`]).
    program_settings: ProgramSettings,
}

impl SmeltTyDb {
    /// Build a database rooted at the temporary directory containing the
    /// source module and initialize ty's program-wide search paths.
    pub(crate) fn new(system: OsSystem, source_root: SystemPathBuf) -> Result<Self> {
        let vendored = ty_vendored::file_system().clone();
        let search_paths = SearchPathSettings::new(vec![source_root])
            .to_search_paths(&system, &vendored, &FallibleStrategy)
            .context("initialize ty search paths")?;

        Ok(Self {
            storage: salsa::Storage::new(None),
            files: Files::default(),
            system,
            vendored,
            rule_selection: RuleSelection::from_registry(default_lint_registry()).into(),
            analysis_settings: AnalysisSettings::default().into(),
            program_settings: ProgramSettings {
                python_version: PythonVersionWithSource::default(),
                python_platform: PythonPlatform::default(),
                search_paths,
            },
        })
    }

    /// Intern this database's [`Program`] from its stored settings.
    ///
    /// ty's own project/test databases memoize this behind a salsa-tracked
    /// query because interning a program hashes every search path. Smelt runs
    /// one small module per database with exactly two search roots (the
    /// temporary source directory and the vendored typeshed), so interning
    /// directly is cheap and avoids depending on ty's `testing`-gated
    /// `TestProgramDb` helper.
    pub(crate) fn program(&self) -> Program<'_> {
        Program::from_settings(self, &self.program_settings)
    }
}

#[salsa::db]
impl RuffDb for SmeltTyDb {
    fn vendored(&self) -> &VendoredFileSystem {
        &self.vendored
    }

    fn system(&self) -> &dyn System {
        &self.system
    }

    fn files(&self) -> &Files {
        &self.files
    }
}

#[salsa::db]
impl ty_python_core::Db for SmeltTyDb {
    fn should_check_file(&self, file: File) -> bool {
        !file.path(self).is_vendored_path()
    }
}

// A marker trait since ty 0.0.10: module resolution is driven by the per-file
// `ResolverEnvironment` carried on `ProgramFile` rather than by a database-wide
// `search_paths()` accessor.
#[salsa::db]
impl ModuleResolverDb for SmeltTyDb {}

#[salsa::db]
impl SemanticDb for SmeltTyDb {
    fn check_file(&self, _file: File) -> Vec<Diagnostic> {
        // Smelt queries inferred node types and never drives ty diagnostics.
        // Keeping this adapter empty avoids doing a redundant whole-file check.
        Vec::new()
    }

    fn program_file(&self, file: File) -> ProgramFile<'_> {
        self.program().program_file(self, file)
    }

    fn python_version_with_source(&self, _file: File) -> &PythonVersionWithSource {
        &self.program_settings.python_version
    }

    fn rule_selection(&self, _file: File) -> &RuleSelection {
        &self.rule_selection
    }

    fn lint_registry(&self) -> &LintRegistry {
        default_lint_registry()
    }

    fn analysis_settings(&self, _file: File) -> &AnalysisSettings {
        &self.analysis_settings
    }

    fn verbose(&self) -> bool {
        false
    }

    /// Smelt is a batch compiler with no editor session, so no file is ever
    /// "open"; ty only uses this to collect string-literal completions.
    fn is_open_file(&self, _file: File) -> bool {
        false
    }

    fn dyn_clone(&self) -> Box<dyn SemanticDb> {
        Box::new(self.clone())
    }
}

#[salsa::db]
impl salsa::Database for SmeltTyDb {}
