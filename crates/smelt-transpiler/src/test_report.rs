//! Reports for focused and full generated Rust test investigations.
//!
//! This command surface keeps repetitive generated-crate execution out of an
//! agent's reasoning context while preserving the facts needed to choose and
//! validate general transpiler fixes.
//!
//! [`TestReport`] is what a run found. Markdown is its rendering for people and
//! JSON its rendering for tools; nothing reads the Markdown back.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::{
    CliResult, diagnostics, report,
    report::{Schema, SchemaVersion, push_fmt},
};

/// Options for one generated Rust test investigation report.
pub(crate) struct RustTestReportOptions<'a> {
    /// Generated Rust crate manifest passed to Cargo.
    pub(crate) cargo_manifest: &'a Path,
    /// Focused filters whose failures are being investigated.
    pub(crate) focus: &'a [String],
    /// Regression filters that must remain passing.
    pub(crate) guard: &'a [String],
    /// Whether to execute the complete generated test suite.
    pub(crate) full: bool,
    /// Earlier JSON report used for test-inventory comparison.
    pub(crate) baseline_report: Option<PathBuf>,
    /// Whether to append grouped Rust compiler diagnostics.
    pub(crate) include_diagnostics: bool,
    /// Whether generated test Cargo calls should suppress warnings.
    pub(crate) suppress_warnings: bool,
}

/// What one investigation found.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TestReport {
    /// Format version; reading another is an error.
    pub(crate) version: SchemaVersion<Self>,
    /// Generated Rust crate manifest.
    pub(crate) cargo_manifest: PathBuf,
    /// Focused runs.
    pub(crate) focus: Vec<TestRun>,
    /// Guard runs.
    pub(crate) guards: Vec<TestRun>,
    /// The whole suite, when it ran.
    pub(crate) full: Option<FullSuite>,
    /// Grouped compiler diagnostics as Markdown, when requested.
    pub(crate) diagnostics: Option<String>,
}

impl Schema for TestReport {
    const NAME: &'static str = "rust test report";
    const VERSION: u32 = 1;
}

/// The whole generated suite.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FullSuite {
    /// The run itself.
    pub(crate) run: TestRun,
    /// Failing test groups, largest first.
    pub(crate) groups: Vec<(String, usize)>,
    /// Comparison with the baseline report, when one was given.
    pub(crate) delta: Option<Delta>,
}

/// How a full run differs from an earlier report.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Delta {
    /// The earlier report.
    pub(crate) baseline: PathBuf,
    /// Tests that failed there and do not now.
    pub(crate) resolved: usize,
    /// Tests that fail now and did not there.
    pub(crate) newly_failing: usize,
}

/// One generated Cargo test invocation.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TestRun {
    /// User-supplied test filter or `full suite`.
    pub(crate) label: String,
    /// Whether Cargo returned successful test status.
    pub(crate) succeeded: bool,
    /// The totals over every test binary, as `N passed; M failed across K
    /// test binaries`, when tests executed.
    pub(crate) summary: Option<String>,
    /// The pass and fail totals over every test binary.
    pub(crate) counts: Option<TestCounts>,
    /// Fully-qualified tests listed as failed by Cargo.
    pub(crate) failing_tests: Vec<String>,
    /// The end of the output, for a failed run.
    pub(crate) excerpt: Vec<String>,
}

/// Pass and fail counts of a test run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TestCounts {
    /// Tests that passed.
    pub(crate) passed: usize,
    /// Tests that failed.
    pub(crate) failed: usize,
}

/// How many trailing output lines a failed run keeps.
const EXCERPT_LINES: usize = 30;

/// Run the requested generated Rust tests.
pub(crate) fn rust_test_report(options: &RustTestReportOptions<'_>) -> CliResult<TestReport> {
    if options.focus.is_empty() && options.guard.is_empty() && !options.full {
        return Err("rust-test-report requires --focus, --guard, or --full".into());
    }
    let focus = options
        .focus
        .iter()
        .map(|filter| run_test(options, Some(filter.as_str())))
        .collect::<Result<Vec<_>, _>>()?;
    let guards = options
        .guard
        .iter()
        .map(|filter| run_test(options, Some(filter.as_str())))
        .collect::<Result<Vec<_>, _>>()?;
    let full = options
        .full
        .then(|| full_suite(run_test(options, None)?, options.baseline_report.as_deref()))
        .transpose()?;
    let diagnostics = options
        .include_diagnostics
        .then(|| diagnostics::rust_diagnostics_markdown(options.cargo_manifest))
        .transpose()?;
    Ok(TestReport {
        version: SchemaVersion::CURRENT,
        cargo_manifest: options.cargo_manifest.to_path_buf(),
        focus,
        guards,
        full,
        diagnostics,
    })
}

/// Execute one Cargo test run, optionally restricted by one test filter.
fn run_test(options: &RustTestReportOptions<'_>, filter: Option<&str>) -> CliResult<TestRun> {
    let mut command = Command::new("cargo");
    command.arg("test").arg("--quiet").arg("--manifest-path");
    command.arg(options.cargo_manifest);
    if let Some(selected_filter) = filter {
        command.arg(selected_filter);
    }
    command.arg("--").arg("--nocapture");
    if options.suppress_warnings {
        command.env("RUSTFLAGS", "-Awarnings");
    }
    let output = command.output()?;
    // A test may print bytes that are not UTF-8; the report still counts it.
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let succeeded = output.status.success();
    let totals = SuiteTotals::of(&text);
    let excerpt = if succeeded {
        Vec::new()
    } else {
        let lines = text.lines().collect::<Vec<_>>();
        let start = lines.len().saturating_sub(EXCERPT_LINES);
        lines.get(start..).unwrap_or_default().iter().map(|line| (*line).to_owned()).collect()
    };
    Ok(TestRun {
        label: filter.map_or_else(|| "full suite".to_owned(), ToOwned::to_owned),
        succeeded,
        counts: totals.map(|suite| suite.counts),
        summary: totals.map(|suite| suite.to_string()),
        failing_tests: failed_tests(&text),
        excerpt,
    })
}

/// The full run with its failing groups and its comparison with `baseline`.
fn full_suite(run: TestRun, baseline: Option<&Path>) -> CliResult<FullSuite> {
    let mut by_group = BTreeMap::<String, usize>::new();
    for test in &run.failing_tests {
        let group = test
            .rsplit_once("::")
            .map_or_else(|| test.clone(), |(module, _)| module.to_owned());
        let count = by_group.entry(group).or_insert(0);
        *count = count.saturating_add(1);
    }
    let mut groups = by_group.into_iter().collect::<Vec<_>>();
    groups.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let delta = baseline
        .map(|path| -> CliResult<Delta> {
            let earlier: TestReport = report::load_json(path, "baseline report ")?;
            let prior = earlier
                .full
                .map(|suite| suite.run.failing_tests.into_iter().collect::<BTreeSet<_>>())
                .unwrap_or_default();
            let current = run.failing_tests.iter().cloned().collect::<BTreeSet<_>>();
            Ok(Delta {
                baseline: path.to_path_buf(),
                resolved: prior.difference(&current).count(),
                newly_failing: current.difference(&prior).count(),
            })
        })
        .transpose()?;
    Ok(FullSuite { run, groups, delta })
}

/// Render `report` as Markdown.
pub(crate) fn render_markdown(report: &TestReport) -> String {
    let mut text = String::from("# Generated Rust Test Report\n\n");
    push_fmt(&mut text, format_args!("- Cargo manifest: `{}`\n", report.cargo_manifest.display()));
    push_fmt(&mut text, format_args!("- Focused runs: `{}`\n", report.focus.len()));
    push_fmt(&mut text, format_args!("- Guard runs: `{}`\n", report.guards.len()));
    push_fmt(&mut text, format_args!("- Full suite executed: `{}`\n", report.full.is_some()));
    render_runs(&mut text, "Focused Runs", &report.focus);
    render_runs(&mut text, "Guard Runs", &report.guards);
    if let Some(full) = &report.full {
        render_full_suite(&mut text, full);
    }
    if let Some(diagnostics) = &report.diagnostics {
        text.push_str("\n## Compiler Diagnostics\n\n");
        text.push_str(&diagnostics.replacen("# Rust Diagnostics", "### Rust Diagnostics", 1));
    }
    text
}

/// The status word of a run.
const fn status(run: &TestRun) -> &'static str {
    if run.succeeded { "passed" } else { "failed" }
}

/// Render focused or guarded test outcomes with failure excerpts only when needed.
fn render_runs(text: &mut String, heading: &str, runs: &[TestRun]) {
    if runs.is_empty() {
        return;
    }
    push_fmt(text, format_args!("\n## {heading}\n\n"));
    for run in runs {
        push_fmt(
            text,
            format_args!(
                "- `{}`: `{}` - `{}`\n",
                run.label,
                status(run),
                run.summary.as_deref().unwrap_or("no test-result line")
            ),
        );
        if !run.succeeded {
            text.push_str("\n```text\n");
            for line in &run.excerpt {
                push_fmt(text, format_args!("{line}\n"));
            }
            text.push_str("```\n");
        }
    }
}

/// Render full-suite counts, high-frequency failing groups, and stable inventory.
fn render_full_suite(text: &mut String, full: &FullSuite) {
    let run = &full.run;
    text.push_str("\n## Full Suite\n\n");
    push_fmt(
        text,
        format_args!(
            "- Status: `{}`\n- Result: `{}`\n- Failing tests: `{}`\n",
            status(run),
            run.summary.as_deref().unwrap_or("no test-result line"),
            run.failing_tests.len()
        ),
    );
    text.push_str("\n### Largest Failing Groups\n\n| Failures | Test group |\n| ---: | --- |\n");
    for (group, count) in full.groups.iter().take(20) {
        push_fmt(text, format_args!("| {count} | `{group}` |\n"));
    }
    if let Some(delta) = &full.delta {
        text.push_str("\n### Delta From Baseline\n\n");
        push_fmt(
            text,
            format_args!(
                "- Baseline report: `{}`\n- Resolved tests: `{}`\n- Newly failing tests: `{}`\n",
                delta.baseline.display(),
                delta.resolved,
                delta.newly_failing
            ),
        );
    }
    text.push_str("\n<details>\n<summary>Failing test inventory</summary>\n\n");
    for test in &run.failing_tests {
        push_fmt(text, format_args!("- `{test}`\n"));
    }
    text.push_str("\n</details>\n");
}

/// The totals of one `cargo test` run.
///
/// Cargo prints one `test result:` line per test binary: the unit tests, each
/// integration test, then the doctests, so the LAST line is the doctests'
/// alone (`0 passed` for a generated library). The suite's totals are the sum
/// over every line, and only [`Self::of`] builds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SuiteTotals {
    /// Passed and failed, summed.
    counts: TestCounts,
    /// How many `test result:` lines were summed.
    binaries: usize,
}

impl SuiteTotals {
    /// The sum over every `test result:` line of `text`, or `None` when no
    /// binary ran (or a line cannot be read: a partial sum would be a lie).
    fn of(text: &str) -> Option<Self> {
        let mut totals = Self { counts: TestCounts { passed: 0, failed: 0 }, binaries: 0 };
        for line in text.lines().filter(|line| line.contains("test result:")) {
            let counts = parse_test_counts(line)?;
            totals.counts.passed = totals.counts.passed.saturating_add(counts.passed);
            totals.counts.failed = totals.counts.failed.saturating_add(counts.failed);
            totals.binaries = totals.binaries.saturating_add(1);
        }
        (totals.binaries > 0).then_some(totals)
    }
}

impl std::fmt::Display for SuiteTotals {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} passed; {} failed across {} test binar{}",
            self.counts.passed,
            self.counts.failed,
            self.binaries,
            if self.binaries == 1 { "y" } else { "ies" }
        )
    }
}

/// Extract `N passed; M failed` from Cargo's summary line.
pub(crate) fn parse_test_counts(summary: &str) -> Option<TestCounts> {
    let (before_passed, after_passed) = summary.split_once(" passed;")?;
    let passed = trailing_number(before_passed)?;
    let (before_failed, _) = after_passed.split_once(" failed")?;
    let failed = trailing_number(before_failed)?;
    Some(TestCounts { passed, failed })
}

/// Parse the last run of ASCII digits in `text`.
fn trailing_number(text: &str) -> Option<usize> {
    text.rsplit(|c: char| !c.is_ascii_digit())
        .find(|run| !run.is_empty())?
        .parse()
        .ok()
}

/// Extract failed Rust test paths from Cargo's failed-test listing.
fn failed_tests(text: &str) -> Vec<String> {
    let mut tests = text
        .lines()
        .filter_map(|line| {
            let candidate = line.strip_prefix("    ")?.trim();
            (candidate.contains("::") && !candidate.contains(' ')).then(|| candidate.to_owned())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    tests.sort();
    tests
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "a test fails fast on what it set up")]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_groups_failed_tests_from_cargo_output() {
        let output = "failures:\n    module_a::fails\n    module_a::also_fails\n    module_b::fails\n\ntest result: FAILED. 1 passed; 3 failed;\n";
        let tests = failed_tests(output);
        assert_eq!(tests.len(), 3, "all failed test paths should be retained");
        assert_eq!(
            SuiteTotals::of(output).map(|totals| totals.to_string()).as_deref(),
            Some("1 passed; 3 failed across 1 test binary"),
            "the Cargo summary should be retained"
        );
    }

    /// A library's doctest summary comes last and counts nothing; the totals
    /// are the sum over every binary, not the last line.
    #[test]
    fn totals_sum_every_test_binary() {
        let output = "test result: FAILED. 1768 passed; 21 failed; 0 ignored;\n\
                      test result: ok. 4 passed; 0 failed; 0 ignored;\n\
                      test result: ok. 0 passed; 0 failed; 0 ignored;\n";
        let totals = SuiteTotals::of(output).expect("three binaries");
        assert_eq!(totals.counts, TestCounts { passed: 1772, failed: 21 });
        assert_eq!(totals.binaries, 3);
        assert_eq!(SuiteTotals::of("compile error\n"), None);
    }

    /// A failing-tests fixture of one run, as a report would hold it.
    fn failing(tests: &[&str]) -> TestRun {
        TestRun {
            label: "full suite".to_owned(),
            succeeded: tests.is_empty(),
            summary: None,
            counts: None,
            failing_tests: tests.iter().map(|test| (*test).to_owned()).collect(),
            excerpt: Vec::new(),
        }
    }

    /// The delta reads an earlier JSON report, the form `--json-output` writes.
    #[test]
    fn a_baseline_report_is_read_as_json() {
        let earlier = TestReport {
            version: SchemaVersion::CURRENT,
            cargo_manifest: PathBuf::from("Cargo.toml"),
            focus: Vec::new(),
            guards: Vec::new(),
            full: Some(full_suite(failing(&["a::fixed", "a::stays"]), None).expect("no baseline")),
            diagnostics: None,
        };
        let directory = std::env::temp_dir().join(format!("smelt-test-report-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("scratch directory");
        let path = directory.join("before.json");
        std::fs::write(&path, serde_json::to_string(&earlier).expect("serializes")).expect("written");
        let suite = full_suite(failing(&["a::stays", "b::new"]), Some(&path)).expect("reads the baseline");
        std::fs::remove_dir_all(&directory).expect("scratch removed");
        let delta = suite.delta.expect("a baseline gives a delta");
        assert_eq!((delta.resolved, delta.newly_failing), (1, 1));
    }

    /// Counts come from Cargo's own line, and a run without one has none.
    #[test]
    fn counts_come_from_the_cargo_summary() {
        let counts = parse_test_counts("test result: FAILED. 1768 passed; 21 failed; 0 ignored;");
        assert_eq!(counts, Some(TestCounts { passed: 1768, failed: 21 }));
        assert_eq!(parse_test_counts("no summary here"), None);
    }
}
