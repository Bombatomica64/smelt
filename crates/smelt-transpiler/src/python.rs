//! Python frontend integration, isolated behind the `python` cargo feature.
//!
//! Every direct use of [`smelt_frontend_py`] lives in this module. That crate
//! is optional so lean TypeScript-only builds can disable it; this module then
//! falls back to stubs that report Python support was not compiled in. Keeping
//! all Python glue here means the rest of the CLI never
//! names a `smelt-frontend-py` type and therefore compiles in both
//! configurations without scattered `#[cfg]` attributes.
//!
//! Enable the frontend with `cargo build --features python` (the default).

use std::collections::HashMap;

use smelt_hir::{Crate, FileId, ItemId, ModuleId};

use crate::lowering::ManifestDiagnostic;
use crate::manifest::ManifestImport;

/// Cross-file Python module/package namespaces threaded between manifest entries.
pub(crate) type PyModuleNamespaces = HashMap<String, HashMap<String, ItemId>>;
/// Cross-file Python `IntEnum` member values threaded between manifest entries.
pub(crate) type PyEnumMembers = HashMap<String, HashMap<String, i64>>;

/// Diagnostic code emitted when a Python source reaches a build without the frontend.
#[cfg(not(feature = "python"))]
const PYTHON_NOT_COMPILED_CODE: &str = "smelt::python-not-compiled";

/// User-facing message for TypeScript-only builds asked to handle Python.
#[cfg(not(feature = "python"))]
const PYTHON_NOT_COMPILED_MESSAGE: &str = "Python support is not compiled into this build of smelt. \
Reinstall from a source checkout with the `python` feature enabled \
(`cargo build --features python`); see the project README.";

// ---------------------------------------------------------------------------
// Feature enabled: real Python frontend.
// ---------------------------------------------------------------------------

/// Parses a Python file and dumps the Ruff AST for CLI debugging.
#[cfg(feature = "python")]
pub(crate) fn dump_ast(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;

    let source = std::fs::read_to_string(file)?;
    let module =
        smelt_frontend_py::parse_module(&source, FileId(0)).map_err(|errors| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}:\n{errors:#?}", std::path::Path::new(file).display()),
            )
        })?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{module:#?}")?;
    Ok(())
}

/// Lowers one Python manifest entry, threading cross-file Python state through.
#[cfg(feature = "python")]
#[expect(
    clippy::too_many_arguments,
    reason = "threads the shared crate and cross-file Python state in and out of the frontend"
)]
pub(crate) fn lower_manifest_source(
    krate: Crate,
    module_namespaces: PyModuleNamespaces,
    enum_members: PyEnumMembers,
    source: &str,
    file_id: FileId,
    file_string: &str,
    specialization: Option<&smelt_specialize::SpecializationManifest>,
    is_entry: bool,
) -> (
    Crate,
    PyModuleNamespaces,
    PyEnumMembers,
    Result<ModuleId, Vec<ManifestDiagnostic>>,
) {
    let mut ctx = smelt_frontend_py::HirCtx {
        krate,
        module_namespaces,
        enum_members,
    };
    let outcome = smelt_frontend_py::to_hir_with_options(
        source,
        file_id,
        file_string,
        &mut ctx,
        smelt_frontend_py::FrontendOptions { specialization, is_entry },
    )
    .map_err(|errors| {
        crate::lowering::manifest_diagnostics(
            file_string,
            source,
            errors.iter().map(|e| {
                (
                    e.category,
                    e.code,
                    e.message.clone(),
                    (e.span.file == file_id).then_some(e.span.start),
                )
            }),
        )
    });
    (ctx.krate, ctx.module_namespaces, ctx.enum_members, outcome)
}

/// Whether an absolute Python import of `module` can never name a project
/// file; see `smelt_frontend_py::is_unshadowable_module`.
#[cfg(feature = "python")]
pub(crate) fn is_unshadowable_module(module: &str) -> bool {
    smelt_frontend_py::is_unshadowable_module(module)
}

/// Scans Python `import module` and `from module import name` specifiers with Ruff AST.
#[cfg(feature = "python")]
pub(crate) fn scan_imports(source: &str) -> Result<Vec<ManifestImport>, String> {
    use smelt_frontend_py::ast::{
        Stmt, StmtImport, StmtImportFrom,
        visitor::{Visitor, walk_stmt},
    };

    /// Ruff AST visitor that records Python imports while preserving parser semantics.
    struct PythonImportVisitor {
        /// Import edges discovered while walking the Python AST.
        imports: Vec<ManifestImport>,
    }

    impl<'a> Visitor<'a> for PythonImportVisitor {
        /// Records import statements and walks nested bodies for dependency closure.
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "the AST visitor must delegate every non-import statement to Ruff's walker"
        )]
        fn visit_stmt(&mut self, stmt: &'a Stmt) {
            match stmt {
                Stmt::Import(import) => self.import_stmt(import),
                Stmt::ImportFrom(import) => self.import_from_stmt(import),
                _ => walk_stmt(self, stmt),
            }
        }
    }

    impl PythonImportVisitor {
        /// Adds one manifest import edge for every module in a Python `import` statement.
        fn import_stmt(&mut self, stmt: &StmtImport) {
            for alias in &stmt.names {
                self.imports.push(ManifestImport {
                    module: alias.name.as_str().to_owned(),
                    names: None,
                    python_relative_level: None,
                    type_only: false,
                });
            }
        }

        /// Adds the module edge from a Python `from ... import ...` statement.
        fn import_from_stmt(&mut self, stmt: &StmtImportFrom) {
            self.imports.push(ManifestImport {
                module: stmt
                    .module
                    .as_ref()
                    .map_or_else(String::new, |module| module.as_str().to_owned()),
                names: None,
                python_relative_level: (stmt.level > 0).then_some(stmt.level),
                type_only: false,
            });
        }
    }

    let module = smelt_frontend_py::parse_module(source, FileId(0)).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| format!("{}: {}", error.code, error.message))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let mut visitor = PythonImportVisitor {
        imports: Vec::new(),
    };
    for stmt in &module.body {
        visitor.visit_stmt(stmt);
    }
    Ok(visitor.imports)
}

/// ty's verdict over the Python sources among `sources`, checked as one
/// program whose project directory is `project_dir`.
///
/// Errors and warnings are located in the text the build lowers; a check that
/// could not run (ty failed to open the program) vouches for nothing.
#[cfg(feature = "ty")]
pub(crate) fn check_sources(
    sources: &[&crate::manifest::ManifestSource],
    project_dir: &std::path::Path,
    stubs: &crate::check_program::CrossLanguageStubs,
) -> crate::type_gate::LanguageCheck {
    use crate::lowering::{Severity, source_line_column};
    use crate::type_gate::LanguageCheck;
    use smelt_frontend_py::{CheckSeverity, CheckSource};

    let python = python_sources(sources);
    if python.is_empty() {
        return LanguageCheck::Absent;
    }
    let inputs = python
        .iter()
        .map(|source| CheckSource { path: &source.path, text: &source.source })
        .collect::<Vec<_>>();
    let declared = stubs.python_declarations();
    let declarations = declared
        .iter()
        .map(|(path, text)| CheckSource { path, text })
        .collect::<Vec<_>>();
    let reported = match smelt_frontend_py::check_sources(&inputs, &declarations, project_dir) {
        Ok(reported) => reported,
        Err(error) => return LanguageCheck::Unverified(format!("ty could not check the Python sources: {error:#}")),
    };
    let diagnostics = reported
        .into_iter()
        .map(|diagnostic| {
            let located = python.iter().find(|source| source.path == diagnostic.path);
            let (line, column) = located
                .zip(diagnostic.offset)
                .and_then(|(source, offset)| source_line_column(&source.source, offset))
                .unzip();
            ManifestDiagnostic {
                file: diagnostic.path.display().to_string(),
                category: smelt_stdlib::DiagnosticCategory::TypeCheck,
                code: "smelt::py-check",
                message: format!("{}: {}", diagnostic.rule, diagnostic.message),
                line,
                column,
                severity: Some(match diagnostic.severity {
                    CheckSeverity::Error => Severity::Error,
                    CheckSeverity::Warning => Severity::Warning,
                }),
            }
        })
        .collect();
    LanguageCheck::Ran(diagnostics)
}

/// Without ty there is no Python checker: Python sources are unverified.
#[cfg(not(feature = "ty"))]
pub(crate) fn check_sources(
    sources: &[&crate::manifest::ManifestSource],
    _project_dir: &std::path::Path,
    _stubs: &crate::check_program::CrossLanguageStubs,
) -> crate::type_gate::LanguageCheck {
    if python_sources(sources).is_empty() {
        return crate::type_gate::LanguageCheck::Absent;
    }
    crate::type_gate::LanguageCheck::Unverified(
        "this smelt was built without the Python checker (feature `ty`)".to_owned(),
    )
}

/// The Python sources (modules and stubs) among `sources`.
fn python_sources<'source>(
    sources: &[&'source crate::manifest::ManifestSource],
) -> Vec<&'source crate::manifest::ManifestSource> {
    sources
        .iter()
        .copied()
        .filter(|source| {
            matches!(
                source.lang,
                crate::source_files::SourceLang::Python | crate::source_files::SourceLang::PythonDeclaration
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Feature disabled: TypeScript-only build stubs.
// ---------------------------------------------------------------------------

/// Reports that Python support was not compiled into this build.
#[cfg(not(feature = "python"))]
pub(crate) fn dump_ast(_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(PYTHON_NOT_COMPILED_MESSAGE.into())
}

/// Returns the Python state unchanged with a "not compiled in" diagnostic.
#[cfg(not(feature = "python"))]
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the feature-enabled `lower_manifest_source` signature"
)]
pub(crate) fn lower_manifest_source(
    krate: Crate,
    module_namespaces: PyModuleNamespaces,
    enum_members: PyEnumMembers,
    _source: &str,
    _file_id: FileId,
    file_string: &str,
    _specialization: Option<&smelt_specialize::SpecializationManifest>,
    _is_entry: bool,
) -> (
    Crate,
    PyModuleNamespaces,
    PyEnumMembers,
    Result<ModuleId, Vec<ManifestDiagnostic>>,
) {
    let diagnostics = vec![ManifestDiagnostic {
        file: file_string.to_owned(),
        category: smelt_stdlib::DiagnosticCategory::Internal,
        code: PYTHON_NOT_COMPILED_CODE,
        message: PYTHON_NOT_COMPILED_MESSAGE.to_owned(),
        line: None,
        column: None,
        severity: None,
    }];
    (krate, module_namespaces, enum_members, Err(diagnostics))
}

/// Without the Python frontend no Python import is resolved at all.
#[cfg(not(feature = "python"))]
pub(crate) const fn is_unshadowable_module(_module: &str) -> bool {
    false
}

/// Reports that Python support was not compiled into this build.
#[cfg(not(feature = "python"))]
pub(crate) fn scan_imports(_source: &str) -> Result<Vec<ManifestImport>, String> {
    Err(PYTHON_NOT_COMPILED_MESSAGE.to_owned())
}
