//! Erasure reasons surfaced to users: severities, diagnostics and the
//! `smelt-erasure.json` sidecar.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631),
//! `src/sema/aot_rules.rs` (rules with `allow`/`warn`/`deny` severities, per
//! rule defaults, per-path scoped settings) and
//! `docs/bundler/aot-adoption.mdx` (how the rules surface to users).
//!
//! The frontends record every decision to erase a value to the tagged runtime
//! value in `smelt_hir::Crate::erasures`, and the Rust backend returns the
//! erasures it decides itself (`smelt_codegen_rust::emit_crate_with_modules_reporting`).
//! This module resolves each record to `file:line:col`, decides its severity
//! from the manifest's `[erasure]` table and the `--erasure` CLI flags, prints
//! `warning[erasure/<reason>]` / `error[erasure/<reason>]` diagnostics, and
//! writes the deterministic sidecar the `smelt-unknown-report` command reads.
//!
//! # Configuration
//!
//! ```toml
//! [erasure]
//! default = "allow"            # every reason (optional)
//! explicit-any = "warn"        # one reason
//!
//! [[erasure.scope]]
//! paths = ["src/core/**"]      # manifest-relative globs
//! missing-annotation = "deny"
//! ```
//!
//! On the CLI, `--erasure <reason>=<level>` (repeatable) sets a reason for
//! every file, `--erasure <reason>:<glob>=<level>` for matching files, and
//! `all` stands for every reason. Settings apply in order — reason defaults,
//! then the manifest's `default`, its per-reason levels, its scopes in order,
//! then the CLI flags in order — and the last matching setting wins, as in
//! Bun.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smelt_hir::{ErasureReason, ErasureRecord, ErasureSeverity};

use crate::{
    lowering::path_matches_glob,
    manifest::{exclusion_base, manifest_relative_path},
};

/// File name of the sidecar written next to the generated crate's `src/`.
pub(crate) const SIDECAR_FILE_NAME: &str = "smelt-erasure.json";

/// A severity as spelled in `Smelt.toml`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ErasureLevel {
    /// Record only.
    Allow,
    /// Print a warning.
    Warn,
    /// Print an error and fail `build`/`check`.
    Deny,
}

impl From<ErasureLevel> for ErasureSeverity {
    /// Map the manifest spelling onto the shared severity.
    fn from(level: ErasureLevel) -> Self {
        match level {
            ErasureLevel::Allow => Self::Allow,
            ErasureLevel::Warn => Self::Warn,
            ErasureLevel::Deny => Self::Deny,
        }
    }
}

/// The `[erasure]` table of `Smelt.toml`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct ErasureConfig {
    /// Severity for every reason, applied before the per-reason levels.
    #[serde(default)]
    default: Option<ErasureLevel>,
    /// Per-path settings, applied in order after the table's own levels.
    #[serde(default)]
    scope: Vec<ErasureScopeConfig>,
    /// Per-reason severities, keyed by reason name (`explicit-any`, ...).
    #[serde(flatten)]
    levels: BTreeMap<String, ErasureLevel>,
}

/// One `[[erasure.scope]]` entry.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ErasureScopeConfig {
    /// Manifest-relative globs (`*` within a segment, `**` across segments).
    paths: Vec<String>,
    /// Severity for every reason in matching files.
    #[serde(default)]
    default: Option<ErasureLevel>,
    /// Per-reason severities in matching files.
    #[serde(flatten)]
    levels: BTreeMap<String, ErasureLevel>,
}

/// One ordered setting: which reason (all when `None`), which files (all when
/// `None`), and the severity.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Setting {
    /// The reason set, or every reason.
    reason: Option<ErasureReason>,
    /// Manifest-relative globs the setting is limited to.
    scope: Option<Vec<String>>,
    /// The severity it sets.
    severity: ErasureSeverity,
}

/// Resolved erasure settings for one command run.
#[derive(Clone, Debug, Default)]
pub(crate) struct ErasureSettings {
    /// Ordered settings; the last matching one wins.
    settings: Vec<Setting>,
}

/// Name standing for every reason in `--erasure all=<level>` (Bun's `all`).
const EVERY_REASON: &str = "all";

/// Parse a reason name, accepting [`EVERY_REASON`].
fn parse_reason(name: &str) -> Result<Option<ErasureReason>, String> {
    if name == EVERY_REASON {
        return Ok(None);
    }
    ErasureReason::named(name).map(Some).ok_or_else(|| {
        let known = ErasureReason::all()
            .map(ErasureReason::name)
            .collect::<Vec<_>>()
            .join(", ");
        format!("unknown erasure reason `{name}`; known reasons: {EVERY_REASON}, {known}")
    })
}

impl ErasureSettings {
    /// Build settings from the manifest table and the CLI `--erasure` flags.
    pub(crate) fn new(config: Option<&ErasureConfig>, cli: &[String]) -> Result<Self, String> {
        let mut settings = Vec::new();
        if let Some(table) = config {
            push_levels(&mut settings, table.default, &table.levels, None)?;
            for scope in &table.scope {
                push_levels(&mut settings, scope.default, &scope.levels, Some(&scope.paths))?;
            }
        }
        for flag in cli {
            settings.push(parse_cli_setting(flag)?);
        }
        Ok(Self { settings })
    }

    /// The severity of `reason` in the manifest-relative file `relative`.
    pub(crate) fn severity(&self, reason: ErasureReason, relative: &Path) -> ErasureSeverity {
        self.settings
            .iter()
            .rev()
            .filter(|setting| setting.reason.is_none_or(|set| set == reason))
            .find(|setting| {
                setting.scope.as_ref().is_none_or(|globs| {
                    globs.iter().any(|glob| path_matches_glob(relative, glob))
                })
            })
            .map_or_else(|| reason.default_severity(), |setting| setting.severity)
    }
}

/// Append the settings of one table (`default` first, then each reason).
fn push_levels(
    settings: &mut Vec<Setting>,
    default: Option<ErasureLevel>,
    levels: &BTreeMap<String, ErasureLevel>,
    scope: Option<&Vec<String>>,
) -> Result<(), String> {
    let scope = scope.cloned();
    if let Some(level) = default {
        settings.push(Setting {
            reason: None,
            scope: scope.clone(),
            severity: level.into(),
        });
    }
    for (name, level) in levels {
        let reason = parse_reason(name).map_err(|error| format!("[erasure]: {error}"))?;
        settings.push(Setting {
            reason,
            scope: scope.clone(),
            severity: (*level).into(),
        });
    }
    Ok(())
}

/// Parse one `--erasure <reason>[:<glob>]=<level>` flag.
fn parse_cli_setting(flag: &str) -> Result<Setting, String> {
    let (target, level) = flag
        .rsplit_once('=')
        .ok_or_else(|| format!("--erasure `{flag}`: expected `<reason>[:<glob>]=<level>`"))?;
    let severity = ErasureSeverity::named(level).ok_or_else(|| {
        format!("--erasure `{flag}`: unknown level `{level}`; use allow, warn or deny")
    })?;
    let (name, scope) = match target.split_once(':') {
        Some((name, glob)) if !glob.is_empty() => (name, Some(vec![glob.to_owned()])),
        Some((name, _)) => (name, None),
        None => (target, None),
    };
    let reason = parse_reason(name).map_err(|error| format!("--erasure `{flag}`: {error}"))?;
    Ok(Setting {
        reason,
        scope,
        severity,
    })
}

/// Where a record was decided.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ErasureOrigin {
    /// A frontend lowering site (`Crate::erasures`).
    Frontend,
    /// The Rust backend's type rendering.
    Backend,
}

/// One record resolved to a source location, as written to the sidecar.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ResolvedErasure {
    /// Manifest-relative source file (`/` separators).
    pub(crate) file: String,
    /// 1-based line.
    pub(crate) line: u32,
    /// 1-based column (UTF-8 bytes).
    pub(crate) column: u32,
    /// Reason name.
    pub(crate) reason: String,
    /// `legitimate` or `avoidable`.
    pub(crate) class: String,
    /// Frontend or backend.
    pub(crate) origin: ErasureOrigin,
    /// Short detail, may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) detail: String,
    /// Effective severity (`allow`, `warn`, `deny`).
    pub(crate) severity: String,
}

impl ResolvedErasure {
    /// Sort key: file, line, column, reason, detail, origin.
    fn key(&self) -> (&str, u32, u32, &str, &str, ErasureOrigin) {
        (
            self.file.as_str(),
            self.line,
            self.column,
            self.reason.as_str(),
            self.detail.as_str(),
            self.origin,
        )
    }
}

/// The `smelt-erasure.json` sidecar.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ErasureSidecar {
    /// Format version.
    pub(crate) version: u32,
    /// Record counts per reason name.
    pub(crate) totals: BTreeMap<String, usize>,
    /// Every record, sorted by file, line, column, reason.
    pub(crate) records: Vec<ResolvedErasure>,
}

/// Source text and display path per file id, read once per run.
struct SourceIndex {
    /// File id -> (manifest-relative display path, source text).
    files: HashMap<smelt_hir::FileId, (PathBuf, Option<String>)>,
}

impl SourceIndex {
    /// Index the crate's modules: their file ids, paths and source text.
    fn new(krate: &smelt_hir::Crate, manifest_dir: &Path) -> Self {
        let base = exclusion_base(manifest_dir);
        let files = krate
            .modules
            .iter()
            .map(|module| {
                let path = Path::new(&module.source.path);
                let relative = manifest_relative_path(path, &base);
                let text = std::fs::read_to_string(path).ok();
                (module.source.file, (relative, text))
            })
            .collect();
        Self { files }
    }
}

/// Resolve, classify and sort every record of a run.
pub(crate) fn resolve(
    krate: &smelt_hir::Crate,
    backend: &[ErasureRecord],
    manifest_dir: &Path,
    settings: &ErasureSettings,
) -> Vec<ResolvedErasure> {
    let index = SourceIndex::new(krate, manifest_dir);
    let mut frontend = krate.erasures.clone();
    smelt_hir::normalize_erasures(&mut frontend);
    let mut backend = backend.to_vec();
    smelt_hir::normalize_erasures(&mut backend);
    let mut resolved = frontend
        .iter()
        .map(|record| (record, ErasureOrigin::Frontend))
        .chain(backend.iter().map(|record| (record, ErasureOrigin::Backend)))
        .map(|(record, origin)| {
            let (relative, text) = index
                .files
                .get(&record.span.file)
                .map_or((PathBuf::from("<unknown>"), None), |(path, text)| {
                    (path.clone(), text.as_deref())
                });
            let (line, column) = text
                .and_then(|text| smelt_hir::line_col(text, record.span.start))
                .unwrap_or((1, 1));
            ResolvedErasure {
                file: display_relative(&relative),
                line,
                column,
                reason: record.reason.name().to_owned(),
                class: record.reason.class().as_str().to_owned(),
                origin,
                detail: record.detail.clone(),
                severity: settings
                    .severity(record.reason, &relative)
                    .as_str()
                    .to_owned(),
            }
        })
        .collect::<Vec<_>>();
    resolved.sort_by(|left, right| left.key().cmp(&right.key()));
    resolved.dedup_by(|left, right| left.key() == right.key());
    resolved
}

/// Render a relative path with `/` separators.
fn display_relative(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The one-line message of a record.
pub(crate) fn message(record: &ResolvedErasure) -> String {
    let help = ErasureReason::named(&record.reason).map_or("", |reason| reason.info().help);
    if record.detail.is_empty() {
        help.to_owned()
    } else {
        format!("{help} ({})", record.detail)
    }
}

/// Print `warn`/`deny` records to stderr and return how many were denied.
///
/// Format: `warning[erasure/<reason>]: <file>:<line>:<col>: <message>`.
pub(crate) fn print_diagnostics(records: &[ResolvedErasure]) -> usize {
    let mut stderr = std::io::stderr().lock();
    let mut denied = 0usize;
    for record in records {
        let label = match record.severity.as_str() {
            "warn" => "warning",
            "deny" => {
                denied = denied.saturating_add(1);
                "error"
            }
            _ => continue,
        };
        let _ignored = writeln!(
            stderr,
            "{label}[erasure/{}]: {}:{}:{}: {}",
            record.reason,
            record.file,
            record.line,
            record.column,
            message(record)
        );
    }
    denied
}

/// The error a run fails with when `denied` records were denied.
pub(crate) fn denied_error(denied: usize) -> Box<dyn std::error::Error> {
    format!(
        "{denied} erasure(s) denied by the `[erasure]` settings; see the `error[erasure/...]` \
         lines above"
    )
    .into()
}

/// Build the deterministic sidecar from resolved records.
pub(crate) fn sidecar(records: &[ResolvedErasure]) -> ErasureSidecar {
    let mut totals = BTreeMap::new();
    for record in records {
        let count: &mut usize = totals.entry(record.reason.clone()).or_default();
        *count = count.saturating_add(1);
    }
    ErasureSidecar {
        version: 1,
        totals,
        records: records.to_vec(),
    }
}

/// Write the sidecar into `output_dir`, only when its bytes change (keeps the
/// generated crate's mtimes stable, like the Rust emitter).
pub(crate) fn write_sidecar(
    output_dir: &Path,
    records: &[ResolvedErasure],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut text = serde_json::to_string_pretty(&sidecar(records))?;
    text.push('\n');
    let path = output_dir.join(SIDECAR_FILE_NAME);
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == text) {
        return Ok(());
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// Load a sidecar from `path`.
pub(crate) fn load_sidecar(path: &Path) -> Result<ErasureSidecar, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse `{}`: {error}", path.display()))
}

/// Avoidable source lines listed in a report, most erased first.
const REPORT_LINE_LIMIT: usize = 200;

/// The per-reason / per-source-line breakdown the `smelt-unknown-report`
/// command adds when a sidecar is available.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ErasureBreakdown {
    /// Records read from the sidecars.
    pub(crate) records: usize,
    /// Records whose reason is avoidable.
    pub(crate) avoidable: usize,
    /// One row per reason, most records first.
    pub(crate) by_reason: Vec<ReasonRow>,
    /// Distinct avoidable `(file, line, reason)` groups.
    pub(crate) avoidable_lines_total: usize,
    /// The avoidable groups with the most records, capped at
    /// [`REPORT_LINE_LIMIT`]; the sidecar keeps every record.
    pub(crate) avoidable_lines: Vec<LineRow>,
}

/// Records of one reason.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ReasonRow {
    /// Reason name.
    pub(crate) reason: String,
    /// `legitimate` or `avoidable`.
    pub(crate) class: String,
    /// Records with this reason.
    pub(crate) count: usize,
}

/// Avoidable records of one reason on one source line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LineRow {
    /// Manifest-relative source file.
    pub(crate) file: String,
    /// 1-based line.
    pub(crate) line: u32,
    /// Reason name.
    pub(crate) reason: String,
    /// Records on this line with this reason.
    pub(crate) count: usize,
    /// The first record's detail, may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) detail: String,
}

/// Aggregate sidecar records into the report breakdown.
///
/// The class comes from the reason table, not from the sidecar's own `class`
/// field, so a stale sidecar cannot reclassify a reason.
pub(crate) fn breakdown(sidecars: &[ErasureSidecar]) -> ErasureBreakdown {
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut lines: BTreeMap<(String, u32, String), (usize, String)> = BTreeMap::new();
    let mut records = 0usize;
    let mut avoidable = 0usize;
    for record in sidecars.iter().flat_map(|sidecar| &sidecar.records) {
        records = records.saturating_add(1);
        let count: &mut usize = reasons.entry(record.reason.clone()).or_default();
        *count = count.saturating_add(1);
        if class_of(&record.reason) != "avoidable" {
            continue;
        }
        avoidable = avoidable.saturating_add(1);
        let entry = lines
            .entry((record.file.clone(), record.line, record.reason.clone()))
            .or_insert_with(|| (0, record.detail.clone()));
        entry.0 = entry.0.saturating_add(1);
    }
    let mut by_reason = reasons
        .into_iter()
        .map(|(reason, count)| ReasonRow {
            class: class_of(&reason).to_owned(),
            reason,
            count,
        })
        .collect::<Vec<_>>();
    by_reason.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.reason.cmp(&right.reason))
    });
    let avoidable_lines_total = lines.len();
    let mut avoidable_lines = lines
        .into_iter()
        .map(|((file, line, reason), (count, detail))| LineRow {
            file,
            line,
            reason,
            count,
            detail,
        })
        .collect::<Vec<_>>();
    avoidable_lines.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.file.cmp(&right.file))
            .then(left.line.cmp(&right.line))
            .then(left.reason.cmp(&right.reason))
    });
    avoidable_lines.truncate(REPORT_LINE_LIMIT);
    ErasureBreakdown {
        records,
        avoidable,
        by_reason,
        avoidable_lines_total,
        avoidable_lines,
    }
}

/// The class of a reason name: `legitimate`, `avoidable`, or `avoidable` for a
/// name this build does not know (conservative: never hide erasure).
fn class_of(reason: &str) -> &'static str {
    ErasureReason::named(reason).map_or("avoidable", |reason| reason.class().as_str())
}

/// Render the breakdown as the report's Markdown section.
pub(crate) fn render_breakdown_markdown(out: &mut String, breakdown: &ErasureBreakdown) {
    let _ = writeln!(out, "## Avoidable erasure by reason and source line");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "From the `{SIDECAR_FILE_NAME}` sidecar: {} erasure decision(s) recorded where lowering \
         chose the tagged runtime value, {} of them avoidable.",
        breakdown.records, breakdown.avoidable
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "| Reason | Class | Decisions |");
    let _ = writeln!(out, "| --- | --- | ---: |");
    for row in &breakdown.by_reason {
        let _ = writeln!(out, "| `{}` | {} | {} |", row.reason, row.class, row.count);
    }
    let _ = writeln!(out);
    if breakdown.avoidable_lines.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "Avoidable source lines (top {} of {}):",
        breakdown.avoidable_lines.len(),
        breakdown.avoidable_lines_total
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "| Source line | Reason | Decisions | Detail |");
    let _ = writeln!(out, "| --- | --- | ---: | --- |");
    for row in &breakdown.avoidable_lines {
        let _ = writeln!(
            out,
            "| `{}:{}` | `{}` | {} | {} |",
            row.file,
            row.line,
            row.reason,
            row.count,
            row.detail.replace('|', "\\|")
        );
    }
    let _ = writeln!(out);
}

/// A short human summary of records per reason, for messages.
#[cfg(test)]
fn summary(records: &[ResolvedErasure]) -> String {
    let mut out = String::new();
    for (reason, count) in sidecar(records).totals {
        let _ = writeln!(out, "{reason}: {count}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a manifest `[erasure]` table from TOML text.
    fn config(text: &str) -> ErasureConfig {
        toml::from_str(text).expect("erasure table parses")
    }

    /// Defaults: every reason allows except `unclassified`, which warns.
    #[test]
    fn defaults_follow_the_reason_table() {
        let settings = ErasureSettings::new(None, &[]).expect("settings");
        let file = Path::new("src/a.ts");
        assert_eq!(
            settings.severity(ErasureReason::ExplicitAny, file),
            ErasureSeverity::Allow
        );
        assert_eq!(
            settings.severity(ErasureReason::Unclassified, file),
            ErasureSeverity::Warn
        );
    }

    /// The manifest's default, per-reason levels and scopes apply in order,
    /// and CLI flags override them.
    #[test]
    fn manifest_scopes_and_cli_flags_apply_in_order() {
        let table = config(
            r#"
            default = "warn"
            explicit-any = "deny"
            [[scope]]
            paths = ["vendor/**"]
            default = "allow"
            "#,
        );
        let settings = ErasureSettings::new(Some(&table), &[]).expect("settings");
        let src = Path::new("src/a.ts");
        let vendor = Path::new("vendor/lib/b.ts");
        assert_eq!(settings.severity(ErasureReason::ExplicitAny, src), ErasureSeverity::Deny);
        assert_eq!(settings.severity(ErasureReason::TypeLevel, src), ErasureSeverity::Warn);
        assert_eq!(settings.severity(ErasureReason::ExplicitAny, vendor), ErasureSeverity::Allow);

        let cli = vec!["explicit-any=allow".to_owned(), "all:vendor/**=deny".to_owned()];
        let settings = ErasureSettings::new(Some(&table), &cli).expect("settings");
        assert_eq!(settings.severity(ErasureReason::ExplicitAny, src), ErasureSeverity::Allow);
        assert_eq!(settings.severity(ErasureReason::TypeLevel, vendor), ErasureSeverity::Deny);
    }

    /// Unknown reasons and levels are rejected with the list of known names.
    #[test]
    fn unknown_names_are_rejected() {
        let error = ErasureSettings::new(None, &["no-such=warn".to_owned()]).unwrap_err();
        assert!(error.contains("unknown erasure reason `no-such`"), "{error}");
        assert!(error.contains("explicit-any"), "{error}");
        let error = ErasureSettings::new(None, &["explicit-any=loud".to_owned()]).unwrap_err();
        assert!(error.contains("unknown level `loud`"), "{error}");
        let table = config("typo-reason = \"warn\"");
        assert!(ErasureSettings::new(Some(&table), &[]).is_err());
    }

    /// The sidecar totals count records per reason.
    #[test]
    fn sidecar_totals_count_records() {
        let record = |reason: &str, line| ResolvedErasure {
            file: "a.ts".to_owned(),
            line,
            column: 1,
            reason: reason.to_owned(),
            class: "avoidable".to_owned(),
            origin: ErasureOrigin::Frontend,
            detail: String::new(),
            severity: "allow".to_owned(),
        };
        let records = vec![record("explicit-any", 1), record("explicit-any", 2), record("type-level", 3)];
        assert_eq!(summary(&records), "explicit-any: 2\ntype-level: 1\n");
    }
}
