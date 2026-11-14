//! Shared shell of the report subcommands.
//!
//! Every report (`probe`, `smelt-unknown-report`, `typedness-report`,
//! `list-escape-report`, `ts7-parity-report`) renders to one [`ReportFormat`],
//! writes through [`write_report`], and a baseline-gated report answers with one
//! [`GatedOutcome`] that [`finish_gated`] turns into an exit status. The table
//! and counting helpers live here so each report keeps only what it measures.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{self, Write as _},
    path::Path,
};

use clap::ValueEnum;
use serde::de::DeserializeOwned;

use crate::CliResult;

/// Output format of the report subcommands (`--format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReportFormat {
    /// Human-readable Markdown.
    #[value(name = "md", alias = "markdown")]
    Markdown,
    /// Machine-readable JSON.
    Json,
}

/// What a baseline-gated report hands back to the CLI.
pub(crate) struct GatedOutcome {
    /// The report in the requested format.
    pub(crate) rendered: String,
    /// The gate's failure message, when the report regressed against its
    /// baseline. Advisory until `--fail-on-regression` asks [`finish_gated`] to
    /// enforce it.
    pub(crate) regression: Option<String>,
}

/// Write a rendered report to `output`, or to stdout when none is given.
pub(crate) fn write_report(output: Option<&str>, report: &str) -> CliResult<()> {
    if let Some(output_path) = output {
        std::fs::write(output_path, report)?;
    } else {
        write!(io::stdout().lock(), "{report}")?;
    }
    Ok(())
}

/// Write a gated report, then enforce its gate when `fail_on_regression` is set:
/// a regression prints its message verbatim and exits 1.
///
/// Returning `Err` instead would route through the default `Termination`
/// impl, which Debug-prints the string with escaped newlines — unreadable for
/// a CI gate.
#[expect(
    clippy::exit,
    reason = "a failing CI gate exits 1 with its message as written"
)]
pub(crate) fn finish_gated(
    outcome: &GatedOutcome,
    output: Option<&str>,
    fail_on_regression: bool,
) -> CliResult<()> {
    write_report(output, &outcome.rendered)?;
    if fail_on_regression && let Some(message) = &outcome.regression {
        writeln!(io::stderr().lock(), "{message}")?;
        std::process::exit(1);
    }
    Ok(())
}

/// Write `text` to `path` only when its bytes change, preserving the mtime of an
/// unchanged file.
pub(crate) fn write_if_changed(path: &Path, text: &str) -> CliResult<()> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == text) {
        return Ok(());
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// A versioned JSON format: a report, a baseline or a sidecar.
///
/// Its `version` field is a [`SchemaVersion`] of the format, so reading a file
/// of another version, or of none, fails instead of comparing against numbers
/// that mean something else.
pub(crate) trait Schema {
    /// The format's name in messages.
    const NAME: &'static str;
    /// The version this build reads and writes. Bump it with any change that
    /// alters what a field means.
    const VERSION: u32;
}

/// The `version` field of an `S` file.
///
/// It serializes as `S::VERSION`, and deserializing any other number fails, so
/// a value of a type with this field can only hold data of the current format,
/// however it was loaded. A missing field fails too (no `serde(default)`).
pub(crate) struct SchemaVersion<S>(std::marker::PhantomData<S>);

impl<S> SchemaVersion<S> {
    /// The current version.
    pub(crate) const CURRENT: Self = Self(std::marker::PhantomData);
}

impl<S> Clone for SchemaVersion<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for SchemaVersion<S> {}

impl<S> Default for SchemaVersion<S> {
    fn default() -> Self {
        Self::CURRENT
    }
}

impl<S> PartialEq for SchemaVersion<S> {
    /// Every value is the current version.
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<S> Eq for SchemaVersion<S> {}

impl<S: Schema> std::fmt::Debug for SchemaVersion<S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} v{}", S::NAME, S::VERSION)
    }
}

impl<S: Schema> serde::Serialize for SchemaVersion<S> {
    fn serialize<Ser: serde::Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        serializer.serialize_u32(S::VERSION)
    }
}

impl<'de, S: Schema> serde::Deserialize<'de> for SchemaVersion<S> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = u32::deserialize(deserializer)?;
        if version == S::VERSION {
            return Ok(Self::CURRENT);
        }
        Err(serde::de::Error::custom(format!(
            "{} version {version} is not the version this smelt reads ({}); regenerate the file",
            S::NAME,
            S::VERSION
        )))
    }
}

/// Load a JSON report, naming the offending path on failure.
///
/// `kind` prefixes the path in the message: `"baseline "` for a file that can
/// only be a baseline, `""` for one that can be a sidecar too. Only a
/// versioned format loads ([`Schema`]); its version is checked as it is read.
pub(crate) fn load_json<T: DeserializeOwned + Schema>(
    path: &Path,
    kind: &str,
) -> Result<T, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {kind}`{}`: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse {kind}`{}`: {error}", path.display()))
}

/// Escape Markdown table-cell metacharacters so multi-line or pipe-containing
/// text never breaks the surrounding table row.
pub(crate) fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Append formatted text to a `String` buffer.
pub(crate) fn push_fmt(out: &mut String, args: std::fmt::Arguments<'_>) {
    // Writing into a `String` cannot fail; ignoring the result keeps report
    // rendering total.
    let _ignored = out.write_fmt(args);
}

/// Add `amount` to the counter stored under `key`, inserting it if absent.
///
/// Saturating so a pathological corpus can never overflow the tally.
pub(crate) fn bump<K: Ord>(counts: &mut BTreeMap<K, usize>, key: K, amount: usize) {
    counts
        .entry(key)
        .and_modify(|count| *count = count.saturating_add(amount))
        .or_insert(amount);
}

/// `part / whole` as a percentage.
#[cfg(feature = "ts7")]
pub(crate) fn ratio_percent(part: usize, whole: usize) -> f64 {
    let numerator = u32::try_from(part).map_or(f64::from(u32::MAX), f64::from);
    let denominator = u32::try_from(whole).map_or(f64::from(u32::MAX), f64::from);
    numerator * 100.0_f64 / denominator
}

/// `count` as `n (p.p%)` against `total`, tolerating a zero total.
pub(crate) fn count_share(count: usize, total: usize) -> String {
    if total == 0 {
        return format!("{count} (—)");
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts here are report percentages, far below f64's exact-integer range"
    )]
    let percent = (count as f64) * 100.0_f64 / (total as f64);
    format!("{count} ({percent:.1}%)")
}

/// `part / total` as a percentage with one decimal, truncated rather than
/// rounded so a committed baseline's share never reads higher than it is; `-`
/// for an empty total.
pub(crate) fn truncated_share(part: usize, total: usize) -> String {
    if total == 0 {
        return "-".to_owned();
    }
    let permille = part.saturating_mul(1000) / total;
    format!("{}.{}%", permille / 10, permille % 10)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "a test fails fast on what it set up")]
mod tests {
    //! The output formats each report depends on staying byte-stable.

    use super::*;

    #[test]
    fn shares_keep_their_formats() {
        assert_eq!(count_share(1, 3), "1 (33.3%)");
        assert_eq!(count_share(2, 3), "2 (66.7%)");
        assert_eq!(count_share(1, 0), "1 (—)");
        assert_eq!(truncated_share(2, 3), "66.6%");
        assert_eq!(truncated_share(0, 0), "-");
    }

    #[cfg(feature = "ts7")]
    #[test]
    fn parity_ratio_keeps_its_format() {
        assert_eq!(format!("{:.1}%", ratio_percent(2, 3)), "66.7%");
    }

    #[test]
    fn cells_cannot_break_their_row() {
        assert_eq!(escape_cell("a|b\nc\rd"), "a\\|b c d");
    }

    /// A one-field versioned format.
    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    struct Probe {
        /// Its version.
        version: SchemaVersion<Self>,
    }

    impl Schema for Probe {
        const NAME: &'static str = "probe";
        const VERSION: u32 = 3;
    }

    #[test]
    fn load_json_names_the_file_it_could_not_read() {
        let missing = Path::new("does/not/exist.json");
        let error = load_json::<Probe>(missing, "baseline ").expect_err("missing file");
        assert!(
            error.starts_with("failed to read baseline `does/not/exist.json`: "),
            "{error}"
        );
        let unnamed = load_json::<Probe>(missing, "").expect_err("missing file");
        assert!(
            unnamed.starts_with("failed to read `does/not/exist.json`: "),
            "{unnamed}"
        );
    }

    /// The current version round-trips; another version, or none, is refused.
    #[test]
    fn a_schema_version_reads_only_the_current_version() {
        let written = serde_json::to_string(&Probe {
            version: SchemaVersion::CURRENT,
        })
        .expect("serialize");
        assert_eq!(written, r#"{"version":3}"#);
        assert!(serde_json::from_str::<Probe>(&written).is_ok());
        let refused = serde_json::from_str::<Probe>(r#"{"version":2}"#)
            .expect_err("old version")
            .to_string();
        assert!(
            refused.contains("probe version 2") && refused.contains("(3)"),
            "{refused}"
        );
        assert!(
            serde_json::from_str::<Probe>("{}").is_err(),
            "a missing version is refused"
        );
    }
}
