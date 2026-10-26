//! `smelt ts7-parity-report` — how much of TypeScript 7's type knowledge
//! survives Smelt's TypeScript lowering.
//!
//! The report lowers each input to HIR exactly as `smelt build` would, type
//! checks the same TypeScript sources with the embedded TypeScript 7 checker
//! (`smelt-ts-types`, tsrs), and compares every HIR expression's type with the
//! checker's type for the node at the same source span (see
//! [`crate::ts7_parity::classify`] for the shape rules).
//!
//! The headline number is **parity**, the share of statically typed
//! expressions whose type Smelt kept:
//! `agree / (agree + smelt-erased + wider + mismatch)`. `wider` rows are values Smelt types as an `Optional` or union around
//! the type TypeScript proved, usually a flow narrowing Smelt did not apply. `smelt-erased` rows are values TypeScript types concretely that Smelt
//! lowered to `Unknown`, grouped by the TypeScript shape and, where the
//! frontend recorded one, the erasure reason, which is the worklist for
//! closing the gap. `ts-dynamic` (`any`/`unknown`) and `ts-deferred` rows are
//! TypeScript's own boundaries and never count against Smelt.
//!
//! The checker sees only the TypeScript files the crate lowered, so an import
//! of an npm package that is not part of the crate resolves to `any` (TS2307)
//! and lands in `ts-dynamic`; the checker diagnostics section lists such
//! errors so a reader can tell when an input is under-resolved.
//!
//! # Inputs
//!
//! Each input is a `Smelt.toml` manifest (every source of the crate) or a
//! `.ts` file (lowered with its relative-import closure).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use smelt_hir::{Crate, FileId, Language};
use smelt_ts_types::{SourceInput, TsDiagnosticCategory, TsTypeFacts};

use crate::ts7_parity::{self, Verdict};
use crate::{CliResult, config_parser, lowering};

/// Rows kept per grouped table.
const TOP_GROUPS: usize = 30;

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParityFormat {
    /// Human-readable Markdown.
    Markdown,
    /// JSON.
    Json,
}

/// Command options.
#[derive(Debug)]
pub(crate) struct ParityReportOptions<'input> {
    /// Manifests or `.ts` files to measure.
    pub(crate) inputs: &'input [PathBuf],
    /// Output format.
    pub(crate) format: ParityFormat,
}

/// Verdict counts.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
struct Counts {
    /// Smelt kept the TypeScript shape.
    agree: usize,
    /// Smelt erased a concretely typed value.
    smelt_erased: usize,
    /// Smelt's type is a widening (unnarrowed `Optional`, wider union) of TypeScript's.
    wider: usize,
    /// Both concrete, shapes differ.
    mismatch: usize,
    /// TypeScript `any`/`unknown`/`object`/`symbol`.
    ts_dynamic: usize,
    /// TypeScript deferred generic operator.
    ts_deferred: usize,
    /// HIR expression whose span is not a checker node (synthesized by lowering).
    no_checker_node: usize,
}

impl Counts {
    /// Records one verdict.
    const fn add(&mut self, verdict: Verdict) {
        let slot = match verdict {
            Verdict::Agree => &mut self.agree,
            Verdict::SmeltErased => &mut self.smelt_erased,
            Verdict::Wider => &mut self.wider,
            Verdict::Mismatch => &mut self.mismatch,
            Verdict::TsDynamic => &mut self.ts_dynamic,
            Verdict::TsDeferred => &mut self.ts_deferred,
        };
        *slot = slot.saturating_add(1);
    }

    /// Statically typed expressions: the parity denominator.
    const fn typed(&self) -> usize {
        self.agree.saturating_add(self.smelt_erased).saturating_add(self.wider).saturating_add(self.mismatch)
    }

    /// Parity in percent, or `None` with nothing typed.
    fn parity_percent(&self) -> Option<f64> {
        let typed = self.typed();
        (typed > 0).then(|| ratio_percent(self.agree, typed))
    }
}

/// `part / whole` as a percentage.
fn ratio_percent(part: usize, whole: usize) -> f64 {
    let numerator = u32::try_from(part).map_or(f64::from(u32::MAX), f64::from);
    let denominator = u32::try_from(whole).map_or(f64::from(u32::MAX), f64::from);
    numerator * 100.0_f64 / denominator
}

/// One grouped family of gaps.
#[derive(Debug, Clone, Serialize)]
struct Group {
    /// `HIR shape ← TypeScript shape`.
    key: String,
    /// Occurrences.
    count: usize,
    /// First occurrence: `path:line:col`, checker spelling and erasure reason.
    example: String,
}

/// The whole report.
#[derive(Debug, Default, Serialize)]
struct ParityReport {
    /// TypeScript version the embedded checker ports.
    typescript: &'static str,
    /// Inputs as given.
    inputs: Vec<String>,
    /// Corpus totals.
    totals: Counts,
    /// Per source file.
    files: BTreeMap<String, Counts>,
    /// Smelt-erased families.
    erased: Vec<Group>,
    /// Wider-type families.
    wider: Vec<Group>,
    /// Mismatch families.
    mismatches: Vec<Group>,
    /// Checker errors by TS code: count and first message.
    checker_errors: BTreeMap<String, (usize, String)>,
    /// Inputs Smelt could not lower, with the first lowering error.
    refused: Vec<(String, String)>,
}

/// A HIR source span: file, start and end byte offsets.
type SpanKey = (FileId, u32, u32);

/// Grouping accumulator: key → (count, first example).
type GroupMap = HashMap<String, (usize, String)>;

/// Group accumulators for the verdicts that are gaps.
#[derive(Default)]
struct Groups {
    /// Smelt-erased families.
    erased: GroupMap,
    /// Wider-type families.
    wider: GroupMap,
    /// Mismatch families.
    mismatches: GroupMap,
}

/// Runs the report and returns the rendered text.
///
/// # Errors
///
/// Returns an error when an input cannot be lowered or the checker fails.
pub(crate) fn ts7_parity_report(options: &ParityReportOptions<'_>) -> CliResult<String> {
    let mut report = ParityReport { typescript: "7.1.0-beta", ..ParityReport::default() };
    let mut groups = Groups::default();
    for input in options.inputs {
        report.inputs.push(input.display().to_string());
        // An input Smelt cannot lower has no HIR to compare: record it (that is
        // itself the parity answer for it) and keep measuring the others.
        let krate = match lower_input(input) {
            Ok(krate) => krate,
            Err(error) => {
                let first_line: String = error.to_string().lines().take(12).collect::<Vec<_>>().join(" ");
                report.refused.push((input.display().to_string(), first_line));
                continue;
            }
        };
        let sources = typescript_sources(&krate, input);
        let facts = smelt_ts_types::analyze(
            sources
                .values()
                .map(|source| {
                    let input = SourceInput::new(source.path.clone(), source.text.clone());
                    match std::path::absolute(&source.path) {
                        Ok(location) => input.located_at(location.display().to_string()),
                        Err(_) => input,
                    }
                })
                .collect(),
        )?;
        record_checker_errors(&facts, &mut report.checker_errors);
        let mut measure = Measure { krate: &krate, facts: &facts, sources: &sources, report: &mut report };
        measure.walk(&mut groups);
    }
    report.erased = top_groups(groups.erased);
    report.wider = top_groups(groups.wider);
    report.mismatches = top_groups(groups.mismatches);
    match options.format {
        ParityFormat::Json => Ok(serde_json::to_string_pretty(&report)?),
        ParityFormat::Markdown => Ok(render_markdown(&report)),
    }
}

/// Lowers one input to HIR.
fn lower_input(input: &Path) -> CliResult<Crate> {
    if input.file_name().is_some_and(|name| name == "Smelt.toml") {
        let config = config_parser::parse(input.to_str().ok_or("manifest path contains invalid UTF-8")?)?;
        return Ok(lowering::lower_manifest_entries(&config, input)?.0);
    }
    let file = input.to_str().ok_or("input path contains invalid UTF-8")?;
    Ok(lowering::lower_single_file(file)?.0)
}

/// One TypeScript source of the lowered crate.
struct Source {
    /// Path as the module recorded it.
    path: String,
    /// Source text.
    text: String,
}

/// The crate's TypeScript sources keyed by `FileId`, read back from disk.
///
/// Module paths are taken as recorded; a relative path that does not exist
/// from the working directory is retried against the input's directory.
fn typescript_sources(krate: &Crate, input: &Path) -> HashMap<FileId, Source> {
    let base = input.parent().unwrap_or_else(|| Path::new("."));
    let mut sources = HashMap::new();
    let mut seen = HashSet::new();
    for module in &krate.modules {
        if module.source.language != Language::TypeScript || !seen.insert(module.source.path.clone()) {
            continue;
        }
        let path = &module.source.path;
        let read = fs::read_to_string(path).or_else(|_| fs::read_to_string(base.join(path)));
        if let Ok(text) = read {
            sources.insert(module.source.file, Source { path: path.clone(), text });
        }
    }
    sources
}

/// Adds the checker's errors to the per-code tally.
fn record_checker_errors(facts: &TsTypeFacts, errors: &mut BTreeMap<String, (usize, String)>) {
    for diagnostic in facts.diagnostics.iter().filter(|diagnostic| diagnostic.category == TsDiagnosticCategory::Error) {
        let entry = errors.entry(format!("TS{}", diagnostic.code)).or_insert_with(|| (0, diagnostic.message.clone()));
        entry.0 = entry.0.saturating_add(1);
    }
}

/// Per-input measurement state.
struct Measure<'input> {
    /// Lowered crate.
    krate: &'input Crate,
    /// Checker facts for its TypeScript sources.
    facts: &'input TsTypeFacts,
    /// Sources by file id.
    sources: &'input HashMap<FileId, Source>,
    /// Report being filled.
    report: &'input mut ParityReport,
}

impl Measure<'_> {
    /// Classifies every source span that HIR expressions carry.
    ///
    /// Lowering can give several HIR expressions one source span (a bound
    /// method and the call through it both span `obj.method(arg)`), so the
    /// verdict is per span: the closest verdict among that span's HIR types
    /// (see [`Verdict`]'s order). The callee of a direct `Call` is skipped
    /// first: HIR types an item reference in callee position by the call's
    /// result, so it is not a function value to compare.
    fn walk(&mut self, groups: &mut Groups) {
        let erasure_reasons = self.erasure_reasons();
        let mut by_span: HashMap<SpanKey, Vec<smelt_hir::TypeId>> = HashMap::new();
        for body in &self.krate.bodies {
            let callees = direct_callees(body);
            for (index, expr) in body.exprs.iter().enumerate() {
                let span = expr.span;
                if !callees.contains(&index) && span.start < span.end && self.sources.contains_key(&span.file) {
                    by_span.entry((span.file, span.start, span.end)).or_default().push(expr.ty);
                }
            }
        }
        // Sorted so the first example of each group is deterministic.
        let mut spans: Vec<(SpanKey, Vec<smelt_hir::TypeId>)> = by_span.into_iter().collect();
        spans.sort_by_key(|((file, start, end), _)| (file.0, *start, *end));
        for (key, types) in &spans {
            self.classify_span(*key, types, &erasure_reasons, groups);
        }
    }

    /// Classifies one source span given every HIR type lowered at it.
    fn classify_span(
        &mut self,
        key: SpanKey,
        types: &[smelt_hir::TypeId],
        erasure_reasons: &HashMap<SpanKey, &'static str>,
        groups: &mut Groups,
    ) {
        let (file, start, end) = key;
        let Some(source) = self.sources.get(&file) else {
            return;
        };
        let file_counts = self.report.files.entry(source.path.clone()).or_default();
        let Some(ts) = self.facts.type_id_at(&source.path, start, end) else {
            file_counts.no_checker_node = file_counts.no_checker_node.saturating_add(1);
            self.report.totals.no_checker_node = self.report.totals.no_checker_node.saturating_add(1);
            return;
        };
        let Some((verdict, hir)) = types
            .iter()
            .map(|&hir| (ts7_parity::classify(self.krate, hir, self.facts, ts), hir))
            .min_by_key(|(verdict, _)| *verdict)
        else {
            return;
        };
        file_counts.add(verdict);
        self.report.totals.add(verdict);
        let target = match verdict {
            Verdict::SmeltErased => &mut groups.erased,
            Verdict::Wider => &mut groups.wider,
            Verdict::Mismatch => &mut groups.mismatches,
            Verdict::Agree | Verdict::TsDynamic | Verdict::TsDeferred => return,
        };
        let group = format!("{} ← {}", ts7_parity::hir_shape(self.krate, hir), ts7_parity::ts_shape(self.facts, ts));
        let example = self.example(source, start, ts, erasure_reasons.get(&key).copied());
        let entry = target.entry(group).or_insert_with(|| (0, example));
        entry.0 = entry.0.saturating_add(1);
    }

    /// Frontend erasure reasons keyed by exact span.
    fn erasure_reasons(&self) -> HashMap<SpanKey, &'static str> {
        self.krate
            .erasures
            .iter()
            .map(|record| ((record.span.file, record.span.start, record.span.end), record.reason.name()))
            .collect()
    }

    /// `path:line:col` plus the checker spelling and erasure reason.
    fn example(&self, source: &Source, start: u32, ts: smelt_ts_types::TsTypeId, reason: Option<&str>) -> String {
        let (line, column) = line_column(&source.text, start);
        let spelling: String =
            self.facts.types.get(ts).map_or("?", |ty| ty.display.as_str()).chars().take(80).collect();
        let erasure = reason.map_or_else(String::new, |name| format!(" (erasure: {name})"));
        format!("{}:{line}:{column} `{spelling}`{erasure}", source.path)
    }
}

/// Indices of the expressions that are the callee of a direct `Call` in `body`.
fn direct_callees(body: &smelt_hir::Body) -> HashSet<usize> {
    body.exprs
        .iter()
        .filter_map(|expr| {
            if let smelt_hir::ExprKind::Call { callee, .. } = &expr.kind {
                usize::try_from(callee.0).ok()
            } else {
                None
            }
        })
        .collect()
}

/// 1-based line and column of byte offset `offset`.
fn line_column(text: &str, offset: u32) -> (usize, usize) {
    let byte = usize::try_from(offset).unwrap_or(usize::MAX).min(text.len());
    let before = text.get(..byte).unwrap_or(text);
    let line = before.matches('\n').count().saturating_add(1);
    let column = before.rsplit('\n').next().map_or(0, |tail| tail.chars().count()).saturating_add(1);
    (line, column)
}

/// The largest groups, by count then key.
fn top_groups(groups: GroupMap) -> Vec<Group> {
    let mut rows: Vec<Group> =
        groups.into_iter().map(|(key, (count, example))| Group { key, count, example }).collect();
    rows.sort_by(|left, right| right.count.cmp(&left.count).then_with(|| left.key.cmp(&right.key)));
    rows.truncate(TOP_GROUPS);
    rows
}

/// Renders the Markdown report.
fn render_markdown(report: &ParityReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# TypeScript {} type parity\n", report.typescript);
    let _ = writeln!(out, "Inputs: {}\n", report.inputs.join(", "));
    let totals = &report.totals;
    let parity = totals.parity_percent().map_or_else(|| "n/a".to_owned(), |percent| format!("{percent:.1}%"));
    let _ = writeln!(out, "**Parity: {parity}** ({} of {} statically typed expressions agree)\n", totals.agree, totals.typed());
    let _ = writeln!(out, "| verdict | count |\n| --- | ---: |");
    for (name, count) in [
        ("agree", totals.agree),
        ("smelt-erased", totals.smelt_erased),
        ("wider", totals.wider),
        ("mismatch", totals.mismatch),
        ("ts-dynamic", totals.ts_dynamic),
        ("ts-deferred", totals.ts_deferred),
        ("no-checker-node", totals.no_checker_node),
    ] {
        let _ = writeln!(out, "| {name} | {count} |");
    }
    render_groups(&mut out, "Smelt-erased families (HIR ← TypeScript)", &report.erased);
    render_groups(&mut out, "Wider-type families (HIR ← TypeScript)", &report.wider);
    render_groups(&mut out, "Mismatch families (HIR ← TypeScript)", &report.mismatches);
    render_files(&mut out, &report.files);
    render_refused(&mut out, &report.refused);
    render_checker_errors(&mut out, &report.checker_errors);
    out
}

/// Renders one grouped table.
fn render_groups(out: &mut String, title: &str, groups: &[Group]) {
    let _ = writeln!(out, "\n## {title}\n");
    if groups.is_empty() {
        let _ = writeln!(out, "None.");
        return;
    }
    let _ = writeln!(out, "| count | shape | first example |\n| ---: | --- | --- |");
    for group in groups {
        let _ = writeln!(out, "| {} | `{}` | {} |", group.count, group.key.replace('|', "\\|"), group.example.replace('|', "\\|"));
    }
}

/// Renders the per-file table, lowest parity first.
fn render_files(out: &mut String, files: &BTreeMap<String, Counts>) {
    let _ = writeln!(out, "\n## Files (lowest parity first)\n");
    let _ = writeln!(out, "| file | parity | agree | erased | wider | mismatch | dynamic |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    let mut rows: Vec<(&String, &Counts)> = files.iter().filter(|(_, counts)| counts.typed() > 0).collect();
    rows.sort_by(|left, right| {
        let left_parity = left.1.parity_percent().unwrap_or(100.0_f64);
        let right_parity = right.1.parity_percent().unwrap_or(100.0_f64);
        left_parity.total_cmp(&right_parity).then_with(|| left.0.cmp(right.0))
    });
    for (file, counts) in rows.into_iter().take(TOP_GROUPS) {
        let parity = counts.parity_percent().unwrap_or(100.0_f64);
        let _ = writeln!(
            out,
            "| {file} | {parity:.1}% | {} | {} | {} | {} | {} |",
            counts.agree, counts.smelt_erased, counts.wider, counts.mismatch, counts.ts_dynamic
        );
    }
}

/// Renders the inputs Smelt could not lower.
fn render_refused(out: &mut String, refused: &[(String, String)]) {
    if refused.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n## Refused inputs (Smelt could not lower them)\n");
    for (input, error) in refused {
        let short: String = error.chars().take(400).collect();
        let _ = writeln!(out, "- `{input}`: {}", short.replace('|', "\\|"));
    }
}

/// Renders the checker error tally.
fn render_checker_errors(out: &mut String, errors: &BTreeMap<String, (usize, String)>) {
    let _ = writeln!(out, "\n## TypeScript checker errors\n");
    if errors.is_empty() {
        let _ = writeln!(out, "None: every input type-checks under TypeScript 7.");
        return;
    }
    let _ = writeln!(out, "| code | count | first message |\n| --- | ---: | --- |");
    for (code, (count, message)) in errors {
        let first_line = message.lines().next().unwrap_or_default().replace('|', "\\|");
        let _ = writeln!(out, "| {code} | {count} | {first_line} |");
    }
}
