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

use crate::lowering::{FileDiagnostic, LoweredCrate, ManifestDiagnostic};
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

/// Lowers Python source files to HIR.
#[cfg(feature = "python")]
pub(crate) fn lower_files(files: &[String]) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut ctx = smelt_frontend_py::HirCtx::new();
    let mut modules = Vec::new();

    for (idx, file) in files.iter().enumerate() {
        let source = std::fs::read_to_string(file)?;
        let file_id = u32::try_from(idx).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("too many source files: {error}"),
            )
        })?;
        let module = smelt_frontend_py::to_hir_with_path(&source, FileId(file_id), file, &mut ctx)
            .map_err(|errors| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}:\n{errors:#?}", std::path::Path::new(file).display()),
                )
            })?;
        modules.push((file.clone(), module));
    }

    Ok((ctx.krate, modules))
}

/// Lowers one Python file for isolated diagnostics collection.
#[cfg(feature = "python")]
pub(crate) fn diagnostics_for(source: &str, file: &str) -> Vec<FileDiagnostic> {
    let mut ctx = smelt_frontend_py::HirCtx::new();
    smelt_frontend_py::to_hir_with_path(source, FileId(0), file, &mut ctx)
        .err()
        .map(|errors| {
            errors
                .into_iter()
                .map(|error| FileDiagnostic {
                    category: error.category,
                    code: error.code,
                    message: error.message,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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
        smelt_frontend_py::FrontendOptions { specialization },
    )
    .map_err(|errors| {
        crate::lowering::manifest_diagnostics(
            file_string,
            errors.iter().map(|e| (e.category, e.code, e.message.clone())),
        )
    });
    (ctx.krate, ctx.module_namespaces, ctx.enum_members, outcome)
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

// ---------------------------------------------------------------------------
// Feature disabled: TypeScript-only build stubs.
// ---------------------------------------------------------------------------

/// Reports that Python support was not compiled into this build.
#[cfg(not(feature = "python"))]
pub(crate) fn dump_ast(_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(PYTHON_NOT_COMPILED_MESSAGE.into())
}

/// Reports that Python support was not compiled into this build.
#[cfg(not(feature = "python"))]
pub(crate) fn lower_files(_files: &[String]) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    Err(PYTHON_NOT_COMPILED_MESSAGE.into())
}

/// Returns a single diagnostic explaining Python support is not compiled in.
#[cfg(not(feature = "python"))]
pub(crate) fn diagnostics_for(_source: &str, _file: &str) -> Vec<FileDiagnostic> {
    vec![FileDiagnostic {
        category: smelt_stdlib::DiagnosticCategory::Internal,
        code: PYTHON_NOT_COMPILED_CODE,
        message: PYTHON_NOT_COMPILED_MESSAGE.to_owned(),
    }]
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
    }];
    (krate, module_namespaces, enum_members, Err(diagnostics))
}

/// Reports that Python support was not compiled into this build.
#[cfg(not(feature = "python"))]
pub(crate) fn scan_imports(_source: &str) -> Result<Vec<ManifestImport>, String> {
    Err(PYTHON_NOT_COMPILED_MESSAGE.to_owned())
}
