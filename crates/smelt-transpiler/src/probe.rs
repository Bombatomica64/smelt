//! `smelt probe` — report how far a manifest transpiles and why it stops.
//!
//! The probe answers the same questions as the external library-probe scripts,
//! but natively: it attempts a whole-crate build, and
//!
//! - on success, optionally runs the generated test suite and reports how many
//!   `cargo test` cases pass/fail;
//! - on failure, performs recoverable manifest-aware lowering and enumerates
//!   the distinct blocker classes grouped by [`DiagnosticCategory`].
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
    /// Message with quoted specifics and Debug payloads erased, so distinct
    /// identifiers and AST dumps collapse onto one class.
    shape: String,
    /// Total occurrences across all scanned files.
    occurrences: usize,
    /// Number of distinct files in which the class appears.
    files: usize,
    /// One source file where the class first appeared, to locate it from the
    /// report without re-running the scan.
    example_file: String,
    /// Full unnormalized first-seen message, recorded only when normalization
    /// elided detail (so the readable table and the raw AST stay separate).
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
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

    // Recoverable manifest lowering enumerates every distinct blocker class.
    let files = lowering::discover_source_files(options.config, manifest_dir)?;
    let ScanResult {
        occurrences,
        file_sets,
        examples,
        category_counts,
        files_with_blockers,
    } = scan_files(&files, options.config, options.manifest_path)?;

    let mut blockers: Vec<BlockerGroup> = occurrences
        .into_iter()
        .map(|((category, code, shape), count)| {
            let key = (category, code.clone(), shape.clone());
            let file_count = file_sets.get(&key).copied().unwrap_or(0);
            let example = examples.get(&key);
            let example_file = example
                .map(|sample| sample.file.clone())
                .unwrap_or_default();
            // Keep the raw message only when bracket elision actually hid an
            // AST dump or type payload (the shape carries the `…` marker). A
            // quote-only collapse (`` `window` `` -> `` `X` ``) loses nothing
            // worth a detail entry, so those classes stay out of the section.
            let detail = if shape.contains('…') {
                example.map(|sample| sample.message.clone())
            } else {
                None
            };
            BlockerGroup {
                category,
                code,
                shape,
                occurrences: count,
                files: file_count,
                example_file,
                detail,
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

/// First-seen example of a blocker class, used to locate it and to recover the
/// raw (un-normalized) message for the detail section.
struct BlockerExample {
    /// Source file where the class was first observed.
    file: String,
    /// Original, unnormalized diagnostic message.
    message: String,
}

/// Aggregated per-file scan tallies.
struct ScanResult {
    /// Total occurrences per blocker class.
    occurrences: BTreeMap<BlockerKey, usize>,
    /// Distinct file count per blocker class.
    file_sets: BTreeMap<BlockerKey, usize>,
    /// First-seen example per blocker class.
    examples: BTreeMap<BlockerKey, BlockerExample>,
    /// Total occurrences per category (by stable string id).
    category_counts: BTreeMap<String, usize>,
    /// Number of files with at least one blocker.
    files_with_blockers: usize,
}

/// Lower the discovered manifest with shared declaration context and tally blockers.
///
/// Per-file isolation misclassifies valid cyclic imports because TypeScript
/// aliases and class method surfaces are manifest-scoped. The diagnostic
/// collector still recovers file-by-file, but seeds the same declaration pass
/// as a real build so probe counts describe actual compiler blockers.
fn scan_files(
    files: &[std::path::PathBuf],
    config: &Config,
    manifest_path: &Path,
) -> CliResult<ScanResult> {
    let mut occurrences: BTreeMap<BlockerKey, usize> = BTreeMap::new();
    let mut file_sets: BTreeMap<BlockerKey, usize> = BTreeMap::new();
    let mut examples: BTreeMap<BlockerKey, BlockerExample> = BTreeMap::new();
    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut blocked_files = std::collections::BTreeSet::new();
    let mut seen_by_file: BTreeMap<String, std::collections::BTreeSet<BlockerKey>> =
        BTreeMap::new();

    let mut diagnostics = lowering::collect_manifest_diagnostics(config, manifest_path)?;
    diagnostics.sort_by(|left, right| left.file.cmp(&right.file));
    for diagnostic in diagnostics {
            let path = diagnostic.file.clone();
            blocked_files.insert(path.clone());
            let key = (
                diagnostic.category,
                diagnostic.code.to_owned(),
                normalize_message(&diagnostic.message),
            );
            bump(&mut occurrences, key.clone());
            bump_str(&mut category_counts, diagnostic.category.as_str().to_owned());
            examples.entry(key.clone()).or_insert_with(|| BlockerExample {
                file: path.clone(),
                message: diagnostic.message,
            });
            seen_by_file.entry(path).or_default().insert(key);
    }
    for keys in seen_by_file.into_values() {
        for key in keys {
            bump(&mut file_sets, key);
        }
    }

    Ok(ScanResult {
        occurrences,
        file_sets,
        examples,
        category_counts,
        files_with_blockers: blocked_files.len().min(files.len()),
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

/// Normalize a diagnostic message into a stable, short blocker-class key.
///
/// Two kinds of per-site specifics are erased so distinct call sites collapse
/// onto the same class:
///
/// - quoted identifiers (`` `x` `` and `'x'`) become `` `X` `` / `'X'`;
/// - bracketed Debug payloads (`(…)`, `{…}`, `[…]`) — the multi-hundred-line
///   `oxc` AST dumps and `TypeId`/receiver detail that some `not lowered yet`
///   diagnostics append — are replaced by a single elision marker, keeping the
///   variant name that immediately precedes the bracket (e.g.
///   `FunctionExpression(…)`), which is the informative part.
///
/// The full original message is preserved separately for the detail section,
/// so locating the offending source is still possible; see [`BlockerGroup`].
fn normalize_message(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
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
            '(' | '{' | '[' => {
                let close = matching_close(ch);
                out.push(ch);
                if skip_bracketed(&mut chars, ch) {
                    out.push('…');
                }
                out.push(close);
            }
            other => out.push(other),
        }
    }
    out
}

/// Return the closing bracket that pairs with an opening one.
const fn matching_close(open: char) -> char {
    match open {
        '(' => ')',
        '{' => '}',
        _ => ']',
    }
}

/// Consume a balanced bracketed group from `chars` after its opening bracket was
/// already read, honoring nesting and skipping over quoted string contents so
/// brackets inside string literals do not unbalance the scan.
///
/// Returns `true` when the group held any content (so the caller can insert an
/// elision marker), and `false` for an empty `()`/`{}`/`[]`.
fn skip_bracketed(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, open: char) -> bool {
    let close = matching_close(open);
    let mut depth = 1usize;
    let mut had_content = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                had_content = true;
                // Skip a double-quoted string body, honoring escapes.
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '(' | '{' | '[' => {
                had_content = true;
                depth = depth.saturating_add(1);
            }
            ')' | '}' | ']' if ch == close && depth == 1 => {
                return had_content;
            }
            ')' | '}' | ']' => {
                depth = depth.saturating_sub(1);
            }
            _ => had_content = true,
        }
    }
    had_content
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
        let _ = writeln!(
            out,
            "| Occurrences | Files | Category | Blocker class | Example |"
        );
        let _ = writeln!(out, "| ---: | ---: | --- | --- | --- |");
        for blocker in &result.blockers {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | `{}` |",
                blocker.occurrences,
                blocker.files,
                blocker.category.label(),
                escape_cell(&blocker.shape),
                escape_cell(&blocker.example_file),
            );
        }
        let _ = writeln!(out);

        render_blocker_details(&mut out, &result.blockers);
    }
    out
}

/// Escape Markdown table-cell metacharacters so multi-line or pipe-containing
/// text never breaks the surrounding table row.
fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Render the collapsible detail section listing the full, un-normalized message
/// for every blocker class whose table shape elided an AST dump or type payload.
///
/// The grouped table above stays readable while the raw `oxc` AST and `TypeId`
/// detail needed to pinpoint a site remains one click away, paired with the
/// example file recorded during the scan.
fn render_blocker_details(out: &mut String, blockers: &[BlockerGroup]) {
    let detailed: Vec<&BlockerGroup> = blockers
        .iter()
        .filter(|blocker| blocker.detail.is_some())
        .collect();
    if detailed.is_empty() {
        return;
    }
    let _ = writeln!(out, "<details>");
    let _ = writeln!(
        out,
        "<summary>Full messages for {} elided blocker class(es)</summary>",
        detailed.len()
    );
    let _ = writeln!(out);
    for blocker in detailed {
        if let Some(detail) = &blocker.detail {
            let _ = writeln!(out, "- **{}**", escape_inline(&blocker.shape));
            let _ = writeln!(out, "  - Example: `{}`", escape_inline(&blocker.example_file));
            let _ = writeln!(out, "  - Message:");
            let _ = writeln!(out, "    ```text");
            for line in detail.lines() {
                let _ = writeln!(out, "    {line}");
            }
            // Debug dumps often have no trailing newline; ensure the single-line
            // case still emits its content.
            if detail.lines().next().is_none() {
                let _ = writeln!(out, "    {detail}");
            }
            let _ = writeln!(out, "    ```");
        }
    }
    let _ = writeln!(out, "</details>");
    let _ = writeln!(out);
}

/// Escape backticks in inline text so it survives inside Markdown emphasis.
fn escape_inline(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
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

    /// A multi-line `oxc` AST Debug dump collapses to a short, stable shape that
    /// keeps the leading variant name but elides the giant payload.
    #[test]
    fn normalize_elides_ast_debug_dump() {
        let message = "array element kind is not lowered yet: FunctionExpression(Function { span: Span { start: 1, end: 2 }, name: \"x(\" })";
        assert_eq!(
            normalize_message(message),
            "array element kind is not lowered yet: FunctionExpression(…)"
        );
    }

    /// Differing `TypeId`/receiver detail in parentheses collapses so the two
    /// sites map to one blocker class.
    #[test]
    fn normalize_collapses_type_detail_payloads() {
        let left = normalize_message(
            "conditional expression branches must have the same lowered type (then: Some(List(TypeId(0))), else: Some(List(TypeId(7))))",
        );
        let right = normalize_message(
            "conditional expression branches must have the same lowered type (then: Some(Union([TypeId(3)])), else: Some(List(TypeId(0))))",
        );
        assert_eq!(left, right);
        assert_eq!(
            left,
            "conditional expression branches must have the same lowered type (…)"
        );
    }

    /// A truly empty bracket group is preserved verbatim (no elision marker),
    /// while a group whose only content is a nested bracket still elides.
    #[test]
    fn normalize_elision_tracks_bracket_content() {
        assert_eq!(normalize_message("rest: None, items: Vec()"), "rest: None, items: Vec()");
        // The inner `[]` counts as content for the outer parentheses.
        assert_eq!(normalize_message("items: Vec([])"), "items: Vec(…)");
    }

    /// Table cells escape pipes and flatten newlines so a row never breaks.
    #[test]
    fn escape_cell_neutralizes_table_breakers() {
        assert_eq!(escape_cell("a | b\nc"), "a \\| b c");
    }

    /// The detail section lists only classes whose shape elided content, fences
    /// the raw message, and records the example file.
    #[test]
    fn detail_section_lists_only_elided_classes() {
        let blockers = vec![
            BlockerGroup {
                category: DiagnosticCategory::Internal,
                code: "smelt::test-elided".to_owned(),
                shape: "kind is not lowered yet: FunctionExpression(…)".to_owned(),
                occurrences: 1,
                files: 1,
                example_file: "src/a.ts".to_owned(),
                detail: Some(
                    "kind is not lowered yet: FunctionExpression(Function { huge: true })".to_owned(),
                ),
            },
            BlockerGroup {
                category: DiagnosticCategory::Internal,
                code: "smelt::test-plain".to_owned(),
                shape: "call expression is not lowered yet".to_owned(),
                occurrences: 1,
                files: 1,
                example_file: "src/b.ts".to_owned(),
                detail: None,
            },
        ];
        let mut out = String::new();
        render_blocker_details(&mut out, &blockers);
        assert!(out.contains("<details>"), "detail section should open");
        assert!(
            out.contains("Function { huge: true }"),
            "raw message should be preserved in the detail block"
        );
        assert!(out.contains("`src/a.ts`"), "example file should be listed");
        assert!(
            !out.contains("call expression is not lowered yet"),
            "classes without elided detail must be skipped"
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
