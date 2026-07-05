//! Markdown reporting for diagnostics emitted by generated Rust crates.
//!
//! The report command runs `cargo check --message-format=json`, groups compiler
//! diagnostics by code and message text, and emits a sorted Markdown summary
//! that is stable enough to hand to another LLM or use as a blocker checklist.

use std::{
    collections::HashMap,
    fmt::Write as _,
    path::Path,
    process::{Command, Stdio},
};

use serde::Deserialize;

use crate::CliResult;

/// Run `cargo check` for `cargo_manifest` and render grouped diagnostics.
pub(crate) fn rust_diagnostics_markdown(cargo_manifest: &Path) -> CliResult<String> {
    let output = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(cargo_manifest)
        .arg("--message-format=json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    Ok(render_rust_diagnostics_report(
        cargo_manifest,
        &stdout,
        &stderr,
        output.status.success(),
    ))
}

/// Render a Markdown report from cargo JSON output and stderr text.
fn render_rust_diagnostics_report(
    cargo_manifest: &Path,
    stdout: &str,
    stderr: &str,
    success: bool,
) -> String {
    let mut groups = DiagnosticGroups::default();
    for line in stdout.lines() {
        let Ok(cargo_message) = serde_json::from_str::<CargoMessage>(line) else {
            continue;
        };
        let CargoMessage::CompilerMessage { message } = cargo_message else {
            continue;
        };
        if message.level != "warning" && message.level != "error" {
            continue;
        }
        groups.add(message);
    }

    let mut report = String::new();
    report.push_str("# Rust Diagnostics\n\n");
    push_report_fmt(
        &mut report,
        format_args!("- Cargo manifest: `{}`\n", cargo_manifest.display()),
    );
    push_report_fmt(
        &mut report,
        format_args!(
            "- Cargo check: `{}`\n",
            if success { "passed" } else { "failed" }
        ),
    );
    push_report_fmt(
        &mut report,
        format_args!("- Errors: `{}`\n", groups.error_count),
    );
    push_report_fmt(
        &mut report,
        format_args!("- Warnings: `{}`\n\n", groups.warning_count),
    );

    report.push_str("## Summary By Code\n\n");
    if groups.by_code.is_empty() {
        report.push_str("No compiler diagnostics were emitted.\n\n");
    } else {
        let mut by_code = groups.by_code.into_iter().collect::<Vec<_>>();
        by_code.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then_with(|| left.0.level.cmp(&right.0.level))
                .then_with(|| left.0.code.cmp(&right.0.code))
        });
        for (index, (key, count)) in by_code.iter().enumerate() {
            push_report_fmt(
                &mut report,
                format_args!(
                    "{}. **{}** `{}` - {} diagnostic{}\n",
                    index.saturating_add(1),
                    key.level,
                    key.code.as_deref().unwrap_or("no-code"),
                    count,
                    if *count == 1 { "" } else { "s" }
                ),
            );
        }
        report.push('\n');
    }

    report.push_str("## Groups\n\n");
    if groups.entries.is_empty() {
        report.push_str("No compiler diagnostics were emitted.\n");
    } else {
        let mut entries = groups.entries.into_values().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.level.cmp(&right.level))
                .then_with(|| left.code.cmp(&right.code))
                .then_with(|| left.message.cmp(&right.message))
        });
        for (index, entry) in entries.iter().enumerate() {
            push_report_fmt(
                &mut report,
                format_args!(
                    "{}. **{}** `{}` - {} occurrence{}\n",
                    index.saturating_add(1),
                    entry.level,
                    entry.code.as_deref().unwrap_or("no-code"),
                    entry.count,
                    if entry.count == 1 { "" } else { "s" }
                ),
            );
            push_report_fmt(
                &mut report,
                format_args!("   - Message: {}\n", entry.message),
            );
            if !entry.examples.is_empty() {
                report.push_str("   - Examples:\n");
                for example in &entry.examples {
                    push_report_fmt(&mut report, format_args!("     - `{example}`\n"));
                }
            }
        }
    }

    if !stderr.trim().is_empty() {
        report.push_str("\n## Cargo Stderr\n\n```text\n");
        report.push_str(stderr.trim());
        report.push_str("\n```\n");
    }
    report
}

/// Append formatted text to a Markdown report buffer.
fn push_report_fmt(report: &mut String, args: std::fmt::Arguments<'_>) {
    if report.write_fmt(args).is_err() {
        // Formatting into a `String` is infallible in practice; keep the helper
        // total so report rendering can stay allocation-light without panics.
    }
}

/// Accumulator for diagnostic grouping.
#[derive(Default)]
struct DiagnosticGroups {
    /// Groups keyed by diagnostic level, code, and summary text.
    entries: HashMap<DiagnosticKey, DiagnosticEntry>,
    /// Counts keyed by diagnostic level and rustc code.
    by_code: HashMap<DiagnosticCodeKey, usize>,
    /// Total number of error diagnostics.
    error_count: usize,
    /// Total number of warning diagnostics.
    warning_count: usize,
}

impl DiagnosticGroups {
    /// Add one compiler diagnostic to the report groups.
    fn add(&mut self, message: RustDiagnostic) {
        match message.level.as_str() {
            "error" => self.error_count = self.error_count.saturating_add(1),
            "warning" => self.warning_count = self.warning_count.saturating_add(1),
            _ => {}
        }
        let summary = first_line(&message.message);
        let code = message.code.as_ref().map(|code| code.code.clone());
        let code_key = DiagnosticCodeKey {
            level: message.level.clone(),
            code: code.clone(),
        };
        let code_count = self.by_code.entry(code_key).or_insert(0);
        *code_count = code_count.saturating_add(1);
        let key = DiagnosticKey {
            level: message.level.clone(),
            code: code.clone(),
            message: summary.clone(),
        };
        let entry = self.entries.entry(key).or_insert_with(|| DiagnosticEntry {
            level: message.level,
            code,
            message: summary,
            count: 0,
            examples: Vec::new(),
        });
        entry.count = entry.count.saturating_add(1);
        if entry.examples.len() < 5
            && let Some(example) = message.spans.iter().find_map(|span| {
                span.file_name
                    .as_ref()
                    .map(|file| format!("{file}:{}", span.line_start.unwrap_or(0)))
            })
        {
            entry.examples.push(example);
        }
    }
}

/// Stable key for one diagnostic group.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DiagnosticKey {
    /// Diagnostic level, usually `warning` or `error`.
    level: String,
    /// Optional rustc diagnostic code or lint name.
    code: Option<String>,
    /// First line of the diagnostic message.
    message: String,
}

/// Stable key for one diagnostic code summary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DiagnosticCodeKey {
    /// Diagnostic level, usually `warning` or `error`.
    level: String,
    /// Optional rustc diagnostic code or lint name.
    code: Option<String>,
}

/// Rendered diagnostic group.
struct DiagnosticEntry {
    /// Diagnostic level, usually `warning` or `error`.
    level: String,
    /// Optional rustc diagnostic code or lint name.
    code: Option<String>,
    /// First line of the diagnostic message.
    message: String,
    /// Number of diagnostics in this group.
    count: usize,
    /// Representative source locations for this group.
    examples: Vec<String>,
}

/// Top-level cargo JSON message variants used by this report.
#[derive(Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum CargoMessage {
    /// A compiler diagnostic emitted by rustc.
    CompilerMessage {
        /// The rustc diagnostic payload.
        message: RustDiagnostic,
    },
    /// Any other cargo JSON line.
    #[serde(other)]
    Other,
}

/// rustc diagnostic shape used by cargo JSON output.
#[derive(Deserialize)]
struct RustDiagnostic {
    /// Diagnostic level, usually `warning` or `error`.
    level: String,
    /// Human-readable diagnostic message.
    message: String,
    /// Optional rustc diagnostic code.
    code: Option<RustDiagnosticCode>,
    /// Source spans attached to this diagnostic.
    #[serde(default)]
    spans: Vec<RustDiagnosticSpan>,
}

/// rustc diagnostic code wrapper.
#[derive(Deserialize)]
struct RustDiagnosticCode {
    /// Diagnostic code such as `E0308` or a lint name.
    code: String,
}

/// rustc span location used for report examples.
#[derive(Deserialize)]
struct RustDiagnosticSpan {
    /// Source file path.
    file_name: Option<String>,
    /// 1-based line start.
    line_start: Option<usize>,
}

/// Return the first line of a diagnostic message.
fn first_line(message: &str) -> String {
    message.lines().next().unwrap_or(message).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_warning_messages_by_code_and_text() {
        let stdout = r#"{"reason":"compiler-message","message":{"level":"warning","message":"variable does not need to be mutable","code":{"code":"unused_mut"},"spans":[{"file_name":"src/main.rs","line_start":10}]}}
{"reason":"compiler-message","message":{"level":"warning","message":"variable does not need to be mutable","code":{"code":"unused_mut"},"spans":[{"file_name":"src/lib.rs","line_start":12}]}}
{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","code":{"code":"E0308"},"spans":[{"file_name":"src/main.rs","line_start":20}]}}"#;
        let report = render_rust_diagnostics_report(Path::new("Cargo.toml"), stdout, "", false);

        assert!(report.contains("- Errors: `1`"));
        assert!(report.contains("- Warnings: `2`"));
        assert!(report.contains("## Summary By Code"));
        assert!(report.contains("**warning** `unused_mut` - 2 diagnostics"));
        assert!(report.contains("**warning** `unused_mut` - 2 occurrences"));
        assert!(report.contains("**error** `E0308` - 1 occurrence"));
        assert!(report.contains("`src/main.rs:10`"));
    }
}
