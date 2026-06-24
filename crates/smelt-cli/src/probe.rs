//! `smelt probe` — report how far a manifest transpiles and why it stops.
//!
//! The probe answers the same questions as the external library-probe scripts,
//! but natively: it attempts a whole-crate build, and
//!
//! - on success, optionally runs the generated test suite and reports how many
//!   `cargo test` cases pass/fail;
//! - on failure, lowers every source file in isolation and enumerates the
//!   distinct blocker classes grouped by [`DiagnosticCategory`].
//!
//! Because each diagnostic now carries a category decided in the frontend, the
//! report groups by cause (missing stdlib vs unimplemented lowering vs
//! typed-subset violation) without parsing human-readable message text.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;
use smelt_stdlib::DiagnosticCategory;

use crate::config::Config;
use crate::{CliResult, lowering, pipeline, test_report};

/// Output format for the probe report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeFormat {
    /// Human-readable Markdown.
    Markdown,
    /// Machine-readable JSON.
    Json,
}

/// Options controlling a probe run.
pub(crate) struct ProbeOptions<'a> {
    /// Parsed manifest configuration.
    pub(crate) config: &'a Config,
    /// Path to the manifest, used to resolve relative source roots.
    pub(crate) manifest_path: &'a Path,
    /// When the crate transpiles, also run the generated `cargo test` suite.
    pub(crate) run_tests: bool,
    /// Report serialization format.
    pub(crate) format: ProbeFormat,
}

/// One blocker class: a category plus a normalized message shape, with counts.
#[derive(Debug, Serialize)]
struct BlockerGroup {
    /// Coarse cause of the blocker.
    category: DiagnosticCategory,
    /// Stable per-site diagnostic code.
    code: String,
    /// Message with quoted specifics erased, so distinct identifiers collapse.
    shape: String,
    /// Total occurrences across all scanned files.
    occurrences: usize,
    /// Number of distinct files in which the class appears.
    files: usize,
}

/// Generated test outcome when the crate transpiles.
#[derive(Debug, Serialize)]
struct TestOutcome {
    /// Number of passing generated `cargo test` cases.
    passed: usize,
    /// Number of failing generated `cargo test` cases.
    failed: usize,
}

/// Structured probe result, serialized directly for `--format json`.
#[derive(Debug, Serialize)]
struct ProbeResult {
    /// Project name from the manifest.
    project: String,
    /// Whether the whole-crate build produced a Rust crate.
    transpiled: bool,
    /// First file the whole-crate build aborted on, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    abort_file: Option<String>,
    /// Generated test counts, when the crate transpiled and tests ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    tests: Option<TestOutcome>,
    /// Number of source files scanned for blockers.
    files_scanned: usize,
    /// Number of scanned files with at least one blocker.
    files_with_blockers: usize,
    /// Per-category occurrence counts across all scanned files.
    category_counts: BTreeMap<String, usize>,
    /// Distinct blocker classes, most frequent first.
    blockers: Vec<BlockerGroup>,
}

/// Run a probe and render the report in the requested format.
pub(crate) fn probe_report(options: &ProbeOptions<'_>) -> CliResult<String> {
    let result = run_probe(options)?;
    match options.format {
        ProbeFormat::Json => Ok(serde_json::to_string_pretty(&result)?),
        ProbeFormat::Markdown => Ok(render_markdown(&result)),
    }
}

/// Attempt the build, scan blockers, and optionally measure tests.
fn run_probe(options: &ProbeOptions<'_>) -> CliResult<ProbeResult> {
    let manifest_dir = options.manifest_path.parent().unwrap_or(Path::new("."));

    // Real-world inputs can panic the frontend; catch panics across the build
    // and per-file scan and report them rather than crashing. Backtraces are
    // silenced for the duration so the report output stays clean.
    let _quiet = lowering::QuietPanics::install();

    // Whole-crate build decides the transpile verdict. A panic counts as a
    // failed transpile attributed to a frontend panic.
    let build = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pipeline::build_rust_crate(options.config, options.manifest_path)
    }));
    let (transpiled, abort_file) = match build {
        Ok(Ok(())) => (true, None),
        Ok(Err(error)) => (false, first_abort_file(&error.to_string())),
        Err(_) => (false, Some("(frontend panic)".to_owned())),
    };

    // Run the generated test suite when the crate built and tests were asked for.
    let tests = if transpiled && options.run_tests {
        Some(measure_tests(options.config, manifest_dir)?)
    } else {
        None
    };

    // Per-file scan enumerates every distinct blocker class.
    let files = lowering::discover_source_files(options.config, manifest_dir)?;
    let ScanResult {
        occurrences,
        file_sets,
        category_counts,
        files_with_blockers,
    } = scan_files(&files)?;

    let mut blockers: Vec<BlockerGroup> = occurrences
        .into_iter()
        .map(|((category, code, shape), count)| {
            let file_count = file_sets
                .get(&(category, code.clone(), shape.clone()))
                .copied()
                .unwrap_or(0);
            BlockerGroup {
                category,
                code,
                shape,
                occurrences: count,
                files: file_count,
            }
        })
        .collect();
    // Most frequent classes first; ties broken by affected file count.
    blockers.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then(b.files.cmp(&a.files))
            .then(a.shape.cmp(&b.shape))
    });

    Ok(ProbeResult {
        project: options.config.project_name().to_owned(),
        transpiled,
        abort_file,
        tests,
        files_scanned: files.len(),
        files_with_blockers,
        category_counts,
        blockers,
    })
}

/// Identity of a blocker class: category, code, and normalized message shape.
type BlockerKey = (DiagnosticCategory, String, String);

/// Increment a blocker-class counter by one (saturating).
fn bump(counts: &mut BTreeMap<BlockerKey, usize>, key: BlockerKey) {
    counts
        .entry(key)
        .and_modify(|count| *count = count.saturating_add(1))
        .or_insert(1);
}

/// Increment a string-keyed counter by one (saturating).
fn bump_str(counts: &mut BTreeMap<String, usize>, key: String) {
    counts
        .entry(key)
        .and_modify(|count| *count = count.saturating_add(1))
        .or_insert(1);
}

/// Aggregated per-file scan tallies.
struct ScanResult {
    /// Total occurrences per blocker class.
    occurrences: BTreeMap<BlockerKey, usize>,
    /// Distinct file count per blocker class.
    file_sets: BTreeMap<BlockerKey, usize>,
    /// Total occurrences per category (by stable string id).
    category_counts: BTreeMap<String, usize>,
    /// Number of files with at least one blocker.
    files_with_blockers: usize,
}

/// Lower every discovered file in isolation and tally its blocker classes.
fn scan_files(files: &[std::path::PathBuf]) -> CliResult<ScanResult> {
    let mut occurrences: BTreeMap<BlockerKey, usize> = BTreeMap::new();
    let mut file_sets: BTreeMap<BlockerKey, usize> = BTreeMap::new();
    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut files_with_blockers = 0usize;

    for file in files {
        let Some(path) = file.to_str() else { continue };
        let diagnostics = lowering::collect_file_diagnostics(path)?;
        if !diagnostics.is_empty() {
            files_with_blockers = files_with_blockers.saturating_add(1);
        }
        let mut seen_here: std::collections::BTreeSet<BlockerKey> = std::collections::BTreeSet::new();
        for diagnostic in diagnostics {
            let key = (
                diagnostic.category,
                diagnostic.code.to_owned(),
                normalize_message(&diagnostic.message),
            );
            bump(&mut occurrences, key.clone());
            bump_str(&mut category_counts, diagnostic.category.as_str().to_owned());
            seen_here.insert(key);
        }
        for key in seen_here {
            bump(&mut file_sets, key);
        }
    }

    Ok(ScanResult {
        occurrences,
        file_sets,
        category_counts,
        files_with_blockers,
    })
}

/// Build then run the generated test suite, extracting pass/fail counts.
fn measure_tests(config: &Config, manifest_dir: &Path) -> CliResult<TestOutcome> {
    let cargo_manifest = manifest_dir
        .join(config.output_target())
        .join("Cargo.toml");
    let report = test_report::rust_test_report_markdown(&test_report::RustTestReportOptions {
        cargo_manifest: &cargo_manifest,
        focus: &[],
        guard: &[],
        full: true,
        baseline_report: None,
        include_diagnostics: false,
        suppress_warnings: true,
    })?;
    let (passed, failed) = parse_test_counts(&report).unwrap_or((0, 0));
    Ok(TestOutcome { passed, failed })
}

/// Extract `N passed; M failed` from a generated test report, if present.
fn parse_test_counts(report: &str) -> Option<(usize, usize)> {
    let (before_passed, after_passed) = report.split_once(" passed;")?;
    let passed = trailing_number(before_passed)?;
    let (before_failed, _) = after_passed.split_once(" failed")?;
    let failed = trailing_number(before_failed)?;
    Some((passed, failed))
}

/// Parse the last run of ASCII digits in `text`.
fn trailing_number(text: &str) -> Option<usize> {
    text.rsplit(|c: char| !c.is_ascii_digit())
        .find(|run| !run.is_empty())?
        .parse()
        .ok()
}

/// Pull the first `path.ts`/`path.py` out of a build error message.
fn first_abort_file(message: &str) -> Option<String> {
    for token in message.split(['"', '\n', ' ', ':']) {
        let extension = Path::new(token).extension().and_then(|ext| ext.to_str());
        if matches!(extension, Some("ts" | "py")) {
            let trimmed = token.trim_start_matches("./");
            return Some(trimmed.replace("/./", "/"));
        }
    }
    None
}

/// Collapse quoted specifics (`` `x` `` and `'x'`) so distinct identifiers map
/// to the same blocker class.
fn normalize_message(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '`' => {
                out.push_str("`X`");
                for inner in chars.by_ref() {
                    if inner == '`' {
                        break;
                    }
                }
            }
            '\'' => {
                out.push_str("'X'");
                for inner in chars.by_ref() {
                    if inner == '\'' {
                        break;
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Render the probe result as Markdown.
fn render_markdown(result: &ProbeResult) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Probe report: {}", result.project);
    let _ = writeln!(out);
    if result.transpiled {
        let _ = writeln!(out, "- Transpile: **yes** — Rust crate emitted");
        match &result.tests {
            Some(tests) => {
                let _ = writeln!(
                    out,
                    "- Generated `cargo test`: **{} passed / {} failed**",
                    tests.passed, tests.failed
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "- Generated `cargo test`: not run (pass `--run-tests`)"
                );
            }
        }
    } else {
        let abort = result.abort_file.as_deref().unwrap_or("(unknown)");
        let _ = writeln!(
            out,
            "- Transpile: **no** — whole-crate build aborts at `{abort}`"
        );
    }
    let _ = writeln!(
        out,
        "- Files scanned: {} · with blockers: {}",
        result.files_scanned, result.files_with_blockers
    );
    let _ = writeln!(out);

    if !result.category_counts.is_empty() {
        let _ = writeln!(out, "## Blockers by category");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Category | Occurrences |");
        let _ = writeln!(out, "| --- | ---: |");
        for (category, count) in &result.category_counts {
            let _ = writeln!(out, "| {category} | {count} |");
        }
        let _ = writeln!(out);
    }

    if !result.blockers.is_empty() {
        let _ = writeln!(out, "## Distinct blocker classes");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Occurrences | Files | Category | Blocker class |");
        let _ = writeln!(out, "| ---: | ---: | --- | --- |");
        for blocker in &result.blockers {
            let shape = blocker.shape.replace('|', "\\|");
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                blocker.occurrences,
                blocker.files,
                blocker.category.label(),
                shape
            );
        }
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quoted identifiers collapse to a shared shape.
    #[test]
    fn normalize_collapses_quoted_specifics() {
        assert_eq!(normalize_message("unresolved class `Array`"), "unresolved class `X`");
        assert_eq!(
            normalize_message("function 'take' must return"),
            "function 'X' must return"
        );
    }

    /// Test counts are parsed out of a libtest result line.
    #[test]
    fn parses_test_counts() {
        let line = "test result: FAILED. 1768 passed; 21 failed; 0 ignored";
        assert_eq!(parse_test_counts(line), Some((1768, 21)));
    }

    /// The first source path is pulled out of a build error string.
    #[test]
    fn extracts_first_abort_file() {
        let message = "\"/work/lib/src/array/at.spec.ts\":\n[ SmeltError ]";
        assert_eq!(
            first_abort_file(message).as_deref(),
            Some("/work/lib/src/array/at.spec.ts")
        );
    }
}
