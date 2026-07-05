//! Markdown reports for focused and full generated Rust test investigations.
//!
//! This command surface keeps repetitive generated-crate execution out of an
//! agent's reasoning context while preserving the facts needed to choose and
//! validate general transpiler fixes.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{CliResult, diagnostics};

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
    /// Earlier Markdown report used for test-inventory comparison.
    pub(crate) baseline_report: Option<PathBuf>,
    /// Whether to append grouped Rust compiler diagnostics.
    pub(crate) include_diagnostics: bool,
    /// Whether generated test Cargo calls should suppress warnings.
    pub(crate) suppress_warnings: bool,
}

/// Run requested generated Rust tests and render a compact Markdown report.
pub(crate) fn rust_test_report_markdown(options: &RustTestReportOptions<'_>) -> CliResult<String> {
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
    let full_run = options.full.then(|| run_test(options, None)).transpose()?;

    let mut report = String::new();
    push_fmt(
        &mut report,
        format_args!("# Generated Rust Test Report\n\n"),
    );
    push_fmt(
        &mut report,
        format_args!("- Cargo manifest: `{}`\n", options.cargo_manifest.display()),
    );
    push_fmt(
        &mut report,
        format_args!("- Focused runs: `{}`\n", focus.len()),
    );
    push_fmt(
        &mut report,
        format_args!("- Guard runs: `{}`\n", guards.len()),
    );
    push_fmt(
        &mut report,
        format_args!("- Full suite executed: `{}`\n", options.full),
    );

    render_runs(&mut report, "Focused Runs", &focus);
    render_runs(&mut report, "Regression Guards", &guards);

    if let Some(run) = &full_run {
        render_full_suite(&mut report, run, options.baseline_report.as_deref())?;
    }
    if options.include_diagnostics {
        report.push_str("\n## Compiler Diagnostics\n\n");
        let diagnostics = diagnostics::rust_diagnostics_markdown(options.cargo_manifest)?;
        report.push_str(&diagnostics.replacen("# Rust Diagnostics", "### Rust Diagnostics", 1));
    }
    Ok(report)
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
    let mut text = String::from_utf8(output.stdout)?;
    text.push_str(&String::from_utf8(output.stderr)?);
    Ok(TestRun {
        label: filter.map_or_else(|| "full suite".to_owned(), ToOwned::to_owned),
        passed: output.status.success(),
        summary: final_test_summary(&text),
        failed_tests: failed_tests(&text),
        text,
    })
}

/// Render focused or guarded test outcomes with failure excerpts only when needed.
fn render_runs(report: &mut String, heading: &str, runs: &[TestRun]) {
    if runs.is_empty() {
        return;
    }
    push_fmt(report, format_args!("\n## {heading}\n\n"));
    for run in runs {
        push_fmt(
            report,
            format_args!(
                "- `{}`: `{}` - `{}`\n",
                run.label,
                if run.passed { "passed" } else { "failed" },
                run.summary.as_deref().unwrap_or("no test-result line")
            ),
        );
        if !run.passed {
            report.push_str("\n```text\n");
            for line in run
                .text
                .lines()
                .rev()
                .take(30)
                .collect::<Vec<_>>()
                .iter()
                .rev()
            {
                push_fmt(report, format_args!("{line}\n"));
            }
            report.push_str("```\n");
        }
    }
}

/// Render full-suite counts, high-frequency failing groups, and stable inventory.
fn render_full_suite(
    report: &mut String,
    run: &TestRun,
    baseline_report: Option<&Path>,
) -> CliResult<()> {
    report.push_str("\n## Full Suite\n\n");
    push_fmt(
        report,
        format_args!(
            "- Status: `{}`\n- Result: `{}`\n- Failing tests: `{}`\n",
            if run.passed { "passed" } else { "failed" },
            run.summary.as_deref().unwrap_or("no test-result line"),
            run.failed_tests.len()
        ),
    );

    let mut by_group = BTreeMap::<String, usize>::new();
    for test in &run.failed_tests {
        let group = test
            .rsplit_once("::")
            .map_or_else(|| test.clone(), |(module, _)| module.to_owned());
        let count = by_group.entry(group).or_insert(0);
        *count = count.saturating_add(1);
    }
    let mut groups = by_group.into_iter().collect::<Vec<_>>();
    groups.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    report.push_str("\n### Largest Failing Groups\n\n| Failures | Test group |\n| ---: | --- |\n");
    for (group, count) in groups.iter().take(20) {
        push_fmt(report, format_args!("| {count} | `{group}` |\n"));
    }

    if let Some(path) = baseline_report {
        let baseline = std::fs::read_to_string(path)?;
        let prior = inventory_from_report(&baseline);
        let current = run.failed_tests.iter().cloned().collect::<BTreeSet<_>>();
        report.push_str("\n### Delta From Baseline\n\n");
        push_fmt(
            report,
            format_args!(
                "- Baseline report: `{}`\n- Resolved tests: `{}`\n- Newly failing tests: `{}`\n",
                path.display(),
                prior.difference(&current).count(),
                current.difference(&prior).count()
            ),
        );
    }

    report.push_str("\n<details>\n<summary>Failing test inventory</summary>\n\n");
    for test in &run.failed_tests {
        push_fmt(report, format_args!("- `{test}`\n"));
    }
    report.push_str("\n</details>\n");
    Ok(())
}

/// Extract Cargo's final summary line from one test execution.
fn final_test_summary(text: &str) -> Option<String> {
    text.lines()
        .rfind(|line| line.contains("test result:"))
        .map(str::trim)
        .map(ToOwned::to_owned)
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

/// Extract the detailed test inventory from a previously emitted report.
fn inventory_from_report(report: &str) -> BTreeSet<String> {
    let mut inside_inventory = false;
    let mut tests = BTreeSet::new();
    for line in report.lines() {
        if line == "<summary>Failing test inventory</summary>" {
            inside_inventory = true;
            continue;
        }
        if inside_inventory && line == "</details>" {
            break;
        }
        if inside_inventory
            && let Some(test) = line
                .strip_prefix("- `")
                .and_then(|inventory_line| inventory_line.strip_suffix('`'))
        {
            tests.insert(test.to_owned());
        }
    }
    tests
}

/// Append formatted content to the Markdown report buffer.
fn push_fmt(report: &mut String, args: std::fmt::Arguments<'_>) {
    if report.write_fmt(args).is_err() {
        // Formatting into String does not fail; retaining a total helper avoids
        // panics in the report path.
    }
}

/// Captured result of one generated Cargo test invocation.
struct TestRun {
    /// User-supplied test filter or `full suite`.
    label: String,
    /// Whether Cargo returned successful test status.
    passed: bool,
    /// Last Cargo test result line, when tests executed.
    summary: Option<String>,
    /// Fully-qualified tests listed as failed by Cargo.
    failed_tests: Vec<String>,
    /// Captured output retained for focused failure excerpts.
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_groups_failed_tests_from_cargo_output() {
        let output = "failures:\n    module_a::fails\n    module_a::also_fails\n    module_b::fails\n\ntest result: FAILED. 1 passed; 3 failed;\n";
        let tests = failed_tests(output);
        assert_eq!(tests.len(), 3, "all failed test paths should be retained");
        assert_eq!(
            final_test_summary(output).as_deref(),
            Some("test result: FAILED. 1 passed; 3 failed;"),
            "the terminal Cargo summary should be retained"
        );
    }

    #[test]
    fn reads_previous_report_inventory_for_delta_comparisons() {
        let report = "<details>\n<summary>Failing test inventory</summary>\n\n- `a::fails`\n- `b::fails`\n\n</details>\n";
        let tests = inventory_from_report(report);
        assert!(
            tests.contains("a::fails"),
            "first prior failure should parse"
        );
        assert!(
            tests.contains("b::fails"),
            "second prior failure should parse"
        );
    }
}
