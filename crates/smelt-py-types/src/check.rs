//! Whole-program Python type checking with ty, for the build's type gate.
//!
//! Type *resolution* ([`crate::resolve_module_types`]) checks one module in
//! isolation and never asks ty for diagnostics. The type gate needs the
//! opposite: every source of the program checked together, so an import
//! between two of the program's modules resolves, and every error ty reports.
//!
//! [`check_sources`] lays the program's sources out in a fresh temporary
//! directory at their paths relative to the project root, makes each source's
//! directory that is not itself a package (has no `__init__.py`) a first-party
//! search root, the way `python path/to/main.py` puts `path/to` on `sys.path`,
//! and runs ty's own whole-file check on each source.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use ruff_db::diagnostic::Severity;
use ruff_db::files::system_path_to_file;
use ruff_db::system::{OsSystem, SystemPathBuf};
use ty_python_semantic::Db as _;

use crate::db::SmeltTyDb;

/// One Python source of the program to check.
#[derive(Debug, Clone, Copy)]
#[expect(clippy::exhaustive_structs, reason = "a source is exactly its path and its text")]
pub struct CheckSource<'source> {
    /// The path the build names the source by; diagnostics carry it back.
    pub path: &'source Path,
    /// The text the build lowers (which may differ from the file on disk).
    pub text: &'source str,
}

/// How bad one reported problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(clippy::exhaustive_enums, reason = "infos and hints are dropped; what is left is an error or a warning")]
pub enum CheckSeverity {
    /// ty would fail the check: the program is ill-typed.
    Error,
    /// ty warned; the program still type-checks.
    Warning,
}

/// One problem ty reported in a checked source.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckDiagnostic {
    /// The source's path, exactly as given in its [`CheckSource`].
    pub path: PathBuf,
    /// Byte offset of the problem in the source text, when ty located it.
    pub offset: Option<u32>,
    /// ty's rule name (`invalid-assignment`, `invalid-syntax`, …).
    pub rule: String,
    /// ty's one-line message.
    pub message: String,
    /// Whether ty counts it as an error.
    pub severity: CheckSeverity,
}

/// Type-checks `sources` as one program whose project directory is
/// `project_root`, with `declarations` (stubs of modules the program imports
/// from another language) laid out beside them and read but not checked.
///
/// Returns every error and warning ty reports in the given sources; hints and
/// infos say nothing wrong and are dropped. A source outside `project_root` is
/// laid out under a private directory of its own, so it still resolves its own
/// relative imports.
///
/// # Errors
/// Returns an error when the temporary program cannot be written or ty cannot
/// open one of its files; never for an ill-typed program.
pub fn check_sources(
    sources: &[CheckSource<'_>],
    declarations: &[CheckSource<'_>],
    project_root: &Path,
) -> Result<Vec<CheckDiagnostic>> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let dir = crate::unique_temp_dir();
    std::fs::create_dir_all(&dir).context("create ty check project dir")?;
    let guard = crate::TempDirGuard(dir.clone());
    let base = std::path::absolute(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let mut laid = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().chain(declarations).enumerate() {
        let relative = relative_location(source.path, &base, index);
        let location = dir.join(&relative);
        if let Some(parent) = location.parent() {
            std::fs::create_dir_all(parent).context("create ty check source dir")?;
        }
        std::fs::write(&location, source.text).context("write ty check source")?;
        laid.push((source, location));
    }
    let roots = search_roots(&dir, laid.iter().map(|(_, location)| location.as_path()))?;
    // Declarations are read, not checked: they are generated, not the program's.
    laid.truncate(sources.len());
    let system_root = utf8_path(dir)?;
    let db = SmeltTyDb::with_roots(OsSystem::new(&system_root), roots)?;
    let mut diagnostics = Vec::new();
    for (source, location) in laid {
        let file = system_path_to_file(&db, &utf8_path(location)?).context("resolve file in ty db")?;
        for diagnostic in db.check_file(file) {
            let severity = match diagnostic.severity() {
                Severity::Error | Severity::Fatal => CheckSeverity::Error,
                Severity::Warning => CheckSeverity::Warning,
                Severity::Hint | Severity::Info => continue,
            };
            diagnostics.push(CheckDiagnostic {
                path: source.path.to_path_buf(),
                offset: diagnostic.range().map(|range| range.start().to_u32()),
                rule: diagnostic.id().to_string(),
                message: diagnostic.concise_message().to_string(),
                severity,
            });
        }
    }
    drop(guard);
    Ok(diagnostics)
}

/// Where a source is laid out inside the temporary program.
///
/// Its path relative to `base`, when it lies under it without `..`; otherwise
/// `__smelt_external/<index>/<file name>`, which keeps it apart from the
/// program's own tree.
fn relative_location(path: &Path, base: &Path, index: usize) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    absolute
        .strip_prefix(base)
        .ok()
        .filter(|relative| relative.components().all(|component| matches!(component, Component::Normal(_))))
        .map_or_else(
            || {
                Path::new("__smelt_external")
                    .join(index.to_string())
                    .join(path.file_name().unwrap_or_else(|| std::ffi::OsStr::new("module.py")))
            },
            Path::to_path_buf,
        )
}

/// The first-party search roots of the laid-out program: its root, then every
/// directory holding a source that is not a package (no `__init__.py`).
fn search_roots<'path>(
    root: &Path,
    locations: impl Iterator<Item = &'path Path>,
) -> Result<Vec<SystemPathBuf>> {
    let directories = locations
        .filter_map(Path::parent)
        .filter(|directory| *directory != root && !directory.join("__init__.py").is_file())
        .map(Path::to_path_buf)
        .collect::<BTreeSet<_>>();
    std::iter::once(root.to_path_buf())
        .chain(directories)
        .map(utf8_path)
        .collect()
}

/// `path` as ty's UTF-8 path type.
fn utf8_path(path: PathBuf) -> Result<SystemPathBuf> {
    SystemPathBuf::from_path_buf(path).map_err(|original| anyhow!("non-UTF-8 path: {}", original.display()))
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::panic_in_result_fn,
    reason = "tests index results they just asserted the length of, and assert inside Result-returning tests"
)]
mod tests {
    use super::*;

    /// Check `(path, text)` sources under a fictitious project root.
    fn check(files: &[(&str, &str)]) -> Result<Vec<CheckDiagnostic>> {
        let root = Path::new("/smelt-check-project");
        let paths = files.iter().map(|(path, _)| root.join(path)).collect::<Vec<_>>();
        let sources = paths
            .iter()
            .zip(files)
            .map(|(path, (_, text))| CheckSource { path, text })
            .collect::<Vec<_>>();
        check_sources(&sources, &[], root)
    }

    /// A well-typed program reports no error.
    #[test]
    fn a_well_typed_program_has_no_error() -> Result<()> {
        let diagnostics = check(&[("main.py", "def f(x: int) -> int:\n    return x + 1\n\nprint(f(2))\n")])?;
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity != CheckSeverity::Error), "{diagnostics:?}");
        Ok(())
    }

    /// An ill-typed assignment is an error at its offset, with ty's rule name.
    #[test]
    fn an_ill_typed_program_is_an_error() -> Result<()> {
        let text = "x: int = \"three\"\n";
        let diagnostics = check(&[("main.py", text)])?;
        let errors = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == CheckSeverity::Error)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{diagnostics:?}");
        assert_eq!(errors[0].rule, "invalid-assignment");
        assert_eq!(errors[0].path, Path::new("/smelt-check-project/main.py"));
        assert!(errors[0].offset.is_some_and(|offset| offset < 16), "{errors:?}");
        Ok(())
    }

    /// Sibling modules of the program import each other, so a cross-module
    /// call is checked against the callee's real signature.
    #[test]
    fn sibling_modules_resolve_and_are_checked_together() -> Result<()> {
        let util = "def twice(x: int) -> int:\n    return x * 2\n";
        let good = check(&[("src/util.py", util), ("src/main.py", "from util import twice\nprint(twice(2))\n")])?;
        assert!(good.iter().all(|diagnostic| diagnostic.severity != CheckSeverity::Error), "{good:?}");
        let bad = check(&[("src/util.py", util), ("src/main.py", "from util import twice\nprint(twice(\"2\"))\n")])?;
        assert!(
            bad.iter().any(|diagnostic| diagnostic.severity == CheckSeverity::Error
                && diagnostic.path.ends_with("src/main.py")),
            "{bad:?}"
        );
        Ok(())
    }
}
