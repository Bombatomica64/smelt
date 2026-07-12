//! `smelt smelt-unknown-report` — measure and gate `SmeltUnknown` usage in
//! generated Rust output.
//!
//! # Why this exists
//!
//! Concrete unions, generics, and flow narrowing are expected to sharply reduce
//! how much generated Rust falls back to the erased `SmeltUnknown` tagged value.
//! That claim is only credible if it is *measured*. This module scans generated
//! `.rs` files, counts every `SmeltUnknown` occurrence, and classifies each one
//! into a [`Category`] so a reduction (or a regression) is a concrete number
//! rather than a projection.
//!
//! # What is counted
//!
//! The scan is textual and deterministic: it walks generated `.rs` files in
//! sorted order, and for each line records every `SmeltUnknown` token together
//! with the classification decided by [`classify_line`]. Textual counting is
//! deliberate — it needs no compiler, works on any generated crate (even ones
//! that do not yet type-check), and is stable enough to diff between runs and
//! store as a checked-in baseline.
//!
//! # Legitimate vs avoidable erasure
//!
//! The central distinction the issue asks for is *legitimate dynamic boundary*
//! versus *avoidable internal erasure*. The heuristics live in [`classify_line`]
//! and are documented there. In short:
//!
//! - **Runtime prelude** — the fixed `SmeltUnknown` enum definition, its trait
//!   impls, and the `smelt_*` helper functions emitted into every crate. This is
//!   shared scaffolding, not program-specific erasure, so it is reported
//!   separately and never counted as a regression signal.
//! - **Legitimate boundary** — genuine dynamic edges: erased closures/interop
//!   (`SmeltUnknown::Function`), promises (`SmeltUnknown::Promise`), explicit
//!   boundary adapters (`IntoSmeltUnknown`, `into_smelt_unknown`), and runtime
//!   narrowing guards.
//! - **Avoidable erasure** — everything else: `SmeltUnknown` in program value
//!   storage, function signatures, and collections where a concrete type, a
//!   generated union, or a scoped generic could have carried the static shape.
//!
//! The heuristics are intentionally conservative: when a line is ambiguous it is
//! classified as avoidable, so the metric never *hides* erasure — the worst case
//! is over-reporting, which is safe for a gate that must not silently regress.
//!
//! # Prelude boundary
//!
//! The emitter writes a stable sentinel comment
//! ([`smelt_codegen_rust::PRELUDE_END_MARKER`]) between the fixed runtime prelude
//! and the generated program. When the scanner finds it, every line up to and
//! including the marker is authoritatively the runtime prelude, which is exact
//! and needs no guessing. For older generated files (and the checked-in example
//! fixtures) that predate the marker, the scanner falls back to the per-line
//! prelude heuristics in [`update_prelude_helper_state`].

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::CliResult;

/// Output format for the `SmeltUnknown` report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnknownReportFormat {
    /// Human-readable Markdown.
    Markdown,
    /// Machine-readable JSON (also the checked-in baseline format).
    Json,
}

/// Options controlling one `SmeltUnknown` report run.
pub(crate) struct UnknownReportOptions<'a> {
    /// Roots to scan. Each may be a single `.rs` file or a directory that is
    /// walked recursively for `.rs` files.
    pub(crate) roots: &'a [PathBuf],
    /// Report serialization format.
    pub(crate) format: UnknownReportFormat,
    /// Optional prior JSON report used to compute per-category deltas. Comparing
    /// against a baseline is advisory by default: it surfaces regressions without
    /// failing the command, so intentional boundary additions are never blocked.
    pub(crate) baseline: Option<PathBuf>,
    /// When `true` and a `baseline` is provided, a rise in avoidable erasure above
    /// the baseline is reported as a [`Regression`] so the caller can exit
    /// nonzero. Defaults to `false`, preserving the historic advisory-only
    /// behavior.
    pub(crate) fail_on_regression: bool,
}

/// A detected avoidable-erasure regression versus a baseline.
///
/// Carried back to the CLI so it can print a message naming the delta and the
/// top offending files, then exit nonzero when `--fail-on-regression` is set.
pub(crate) struct Regression {
    /// Avoidable-erasure occurrences recorded in the baseline.
    pub(crate) baseline: usize,
    /// Avoidable-erasure occurrences in the current scan.
    pub(crate) current: usize,
    /// How far the current scan exceeds the baseline (`current - baseline`).
    pub(crate) delta: usize,
    /// The files contributing the most avoidable-erasure occurrences in the
    /// current scan, most first, capped to a readable count.
    pub(crate) top_files: Vec<(String, usize)>,
}

/// Outcome of a report run: the rendered text plus any regression the caller may
/// want to act on.
pub(crate) struct UnknownReportOutcome {
    /// The report serialized in the requested format.
    pub(crate) rendered: String,
    /// Set only when `fail_on_regression` is requested, a baseline is present,
    /// and avoidable erasure rose above it.
    pub(crate) regression: Option<Regression>,
}

/// Classification of a single `SmeltUnknown` occurrence.
///
/// Ordering matters for deterministic report layout: variants are emitted in
/// declaration order, most "expected/benign" first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Category {
    /// The fixed runtime prelude: the `SmeltUnknown` enum definition, its trait
    /// impls, and the `smelt_*` helper functions emitted into every crate. Not
    /// program-specific erasure; excluded from the regression signal.
    RuntimePrelude,
    /// A genuine dynamic boundary: erased closures/interop, promises, JSON/plugin
    /// values, explicit boundary adapters, and runtime narrowing guards.
    LegitimateBoundary,
    /// Avoidable internal erasure: program value storage, function signatures, or
    /// collections that could carry a concrete type, union, or generic instead.
    AvoidableErasure,
}

impl Category {
    /// Stable, machine-readable identifier for this category.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimePrelude => "runtime-prelude",
            Self::LegitimateBoundary => "legitimate-boundary",
            Self::AvoidableErasure => "avoidable-erasure",
        }
    }

    /// Human-readable label for Markdown tables.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::RuntimePrelude => "Runtime prelude",
            Self::LegitimateBoundary => "Legitimate boundary",
            Self::AvoidableErasure => "Avoidable erasure",
        }
    }

    /// Every category, in report order.
    const fn all() -> [Self; 3] {
        [
            Self::RuntimePrelude,
            Self::LegitimateBoundary,
            Self::AvoidableErasure,
        ]
    }
}

/// One "occurrence shape" group: a category plus a normalized line pattern, with
/// counts. Groups let a report point at the *kind* of construct producing the
/// erasure without listing every raw line.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShapeGroup {
    /// Category the shape belongs to.
    category: Category,
    /// Normalized line pattern (identifiers/literals collapsed) so distinct call
    /// sites with the same structure share one group.
    shape: String,
    /// Total `SmeltUnknown` occurrences across all lines in this shape.
    occurrences: usize,
    /// Number of distinct files the shape appears in.
    files: usize,
    /// One `file:line` where the shape first appeared, for locating it.
    example: String,
}

/// Per-category totals plus the shape breakdown. Serialized as the report body
/// and as the checked-in baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UnknownReport {
    /// Number of generated `.rs` files scanned.
    files_scanned: usize,
    /// Total `SmeltUnknown` occurrences across every category.
    total: usize,
    /// Occurrence totals keyed by category identifier, in stable order.
    category_totals: BTreeMap<String, usize>,
    /// Distinct shape groups, most frequent first.
    shapes: Vec<ShapeGroup>,
}

impl UnknownReport {
    /// Occurrence total for one category (0 when absent).
    fn category_total(&self, category: Category) -> usize {
        self.category_totals
            .get(category.as_str())
            .copied()
            .unwrap_or(0)
    }
}

/// Number of top offending files named in a regression message.
const TOP_OFFENDER_LIMIT: usize = 10;

/// Run a `SmeltUnknown` scan and render the report in the requested format.
///
/// The returned [`UnknownReportOutcome`] carries the rendered text and, when
/// `fail_on_regression` is set and avoidable erasure rose above the baseline, a
/// [`Regression`] the caller can turn into a nonzero exit. The rendered report is
/// identical regardless of `fail_on_regression`; only the caller's exit behavior
/// changes.
pub(crate) fn unknown_report(
    options: &UnknownReportOptions<'_>,
) -> CliResult<UnknownReportOutcome> {
    let files = discover_rust_files(options.roots)?;
    let scan = scan_files(&files)?;
    let report = &scan.report;
    let baseline = match &options.baseline {
        Some(path) => Some(load_baseline(path)?),
        None => None,
    };
    let rendered = match options.format {
        UnknownReportFormat::Json => serde_json::to_string_pretty(report)?,
        UnknownReportFormat::Markdown => render_markdown(report, baseline.as_ref()),
    };
    let regression = if options.fail_on_regression {
        detect_regression(&scan, baseline.as_ref())
    } else {
        None
    };
    Ok(UnknownReportOutcome {
        rendered,
        regression,
    })
}

/// Compute an avoidable-erasure [`Regression`] versus `baseline`, if any.
///
/// Returns `None` when there is no baseline, or when avoidable erasure did not
/// rise above it — a flat or falling count is never a regression. The top
/// offending files are ranked by their avoidable-erasure occurrence count in the
/// current scan, so the message points reviewers at where to look first.
fn detect_regression(scan: &ScanResult, baseline: Option<&UnknownReport>) -> Option<Regression> {
    let baseline_report = baseline?;
    let baseline_avoidable = baseline_report.category_total(Category::AvoidableErasure);
    let current_avoidable = scan.report.category_total(Category::AvoidableErasure);
    let delta = current_avoidable.checked_sub(baseline_avoidable)?;
    if delta == 0 {
        return None;
    }
    let mut top_files: Vec<(String, usize)> = scan
        .avoidable_by_file
        .iter()
        .map(|(file, count)| (file.clone(), *count))
        .collect();
    // Most avoidable occurrences first; ties broken by path for stability.
    top_files.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    top_files.truncate(TOP_OFFENDER_LIMIT);
    Some(Regression {
        baseline: baseline_avoidable,
        current: current_avoidable,
        delta,
        top_files,
    })
}

/// Load a prior JSON report to diff against, mapping IO/parse errors into the
/// CLI error type with the offending path.
fn load_baseline(path: &Path) -> CliResult<UnknownReport> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read baseline `{}`: {error}", path.display()))?;
    let report = serde_json::from_str::<UnknownReport>(&text)
        .map_err(|error| format!("failed to parse baseline `{}`: {error}", path.display()))?;
    Ok(report)
}

/// Expand the requested roots into a sorted, de-duplicated list of `.rs` files.
///
/// Directories are walked recursively. Sorting makes the first-seen example for
/// each shape deterministic across runs and platforms.
fn discover_rust_files(roots: &[PathBuf]) -> CliResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for root in roots {
        collect_rust_files(root, &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Recursively collect `.rs` files under `root` (or `root` itself when it is a
/// single `.rs` file) into `out`.
fn collect_rust_files(root: &Path, out: &mut Vec<PathBuf>) -> CliResult<()> {
    let metadata = std::fs::metadata(root)
        .map_err(|error| format!("failed to stat `{}`: {error}", root.display()))?;
    if metadata.is_file() {
        if root.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(root.to_path_buf());
        }
        return Ok(());
    }
    // Read directory entries in a stable order so the walk is deterministic.
    let mut entries = std::fs::read_dir(root)
        .map_err(|error| format!("failed to read directory `{}`: {error}", root.display()))?
        .map(|dir_entry| dir_entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for entry in entries {
        // Skip Cargo build output; it duplicates generated sources and would
        // double-count while making the scan non-deterministic across machines.
        if entry.file_name().and_then(|name| name.to_str()) == Some("target") {
            continue;
        }
        collect_rust_files(&entry, out)?;
    }
    Ok(())
}

/// Format a file path for report examples, relative to `base` when possible.
///
/// A relative display keeps committed baselines portable: the same corpus scanned
/// from the same working directory yields byte-identical `file:line` examples on
/// any machine. Falls back to the full path when the file is not under `base`.
fn display_path(file: &Path, base: Option<&Path>) -> String {
    if let Some(base_path) = base
        && let Ok(relative) = file.strip_prefix(base_path)
    {
        return relative.display().to_string();
    }
    file.display().to_string()
}

/// Identity of one shape group: its category and normalized line pattern.
type ShapeKey = (Category, String);

/// Result of scanning a corpus: the serializable report plus a per-file
/// avoidable-erasure tally used to name top offenders in a regression message.
///
/// The per-file tally is deliberately kept out of [`UnknownReport`] so the
/// checked-in baseline JSON stays small and stable across large corpora (a
/// per-file map would add one entry per generated file).
struct ScanResult {
    /// The serializable, checked-in report.
    report: UnknownReport,
    /// Avoidable-erasure occurrences keyed by displayed file path.
    avoidable_by_file: BTreeMap<String, usize>,
}

/// Scan every file, tallying `SmeltUnknown` occurrences per shape and category.
fn scan_files(files: &[PathBuf]) -> CliResult<ScanResult> {
    let mut occurrences: BTreeMap<ShapeKey, usize> = BTreeMap::new();
    let mut file_sets: BTreeMap<ShapeKey, usize> = BTreeMap::new();
    let mut examples: BTreeMap<ShapeKey, String> = BTreeMap::new();
    let mut category_totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut avoidable_by_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;

    // Resolve the current directory once so `file:line` examples are recorded
    // relative to it, keeping committed baselines portable across machines.
    let cwd = std::env::current_dir().ok();
    for file in files {
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("failed to read `{}`: {error}", file.display()))?;
        let display = display_path(file, cwd.as_deref());
        // Track which shapes appeared in this file to count distinct files.
        let mut seen_here: std::collections::BTreeSet<ShapeKey> = std::collections::BTreeSet::new();
        let mut in_prelude_helper = false;
        // Authoritative prelude boundary: the 0-based index of the sentinel line,
        // if the emitter wrote one. Every line at or before it is prelude.
        let marker_line = text
            .lines()
            .position(|line| line.trim() == smelt_codegen_rust::PRELUDE_END_MARKER);
        for (index, line) in text.lines().enumerate() {
            update_prelude_helper_state(line, &mut in_prelude_helper);
            let count = count_occurrences(line, "SmeltUnknown");
            if count == 0 {
                continue;
            }
            // Prefer the exact marker boundary; fall back to the per-line
            // heuristic when the file predates the sentinel.
            let before_marker = marker_line.is_some_and(|marker| index <= marker);
            let category = if before_marker {
                Category::RuntimePrelude
            } else {
                classify_line(line, in_prelude_helper && marker_line.is_none())
            };
            let shape = normalize_line(line);
            let key = (category, shape.clone());
            bump(&mut occurrences, key.clone(), count);
            bump(&mut category_totals, category.as_str().to_owned(), count);
            if category == Category::AvoidableErasure {
                bump(&mut avoidable_by_file, display.clone(), count);
            }
            total = total.saturating_add(count);
            examples
                .entry(key.clone())
                .or_insert_with(|| format!("{display}:{}", index.saturating_add(1)));
            seen_here.insert(key);
        }
        for key in seen_here {
            bump(&mut file_sets, key, 1);
        }
    }

    let mut shapes: Vec<ShapeGroup> = occurrences
        .into_iter()
        .map(|((category, shape), count)| {
            let key = (category, shape.clone());
            ShapeGroup {
                category,
                shape,
                occurrences: count,
                files: file_sets.get(&key).copied().unwrap_or(0),
                example: examples.get(&key).cloned().unwrap_or_default(),
            }
        })
        .collect();
    // Most frequent first; ties broken by category then shape for stability.
    shapes.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then(left.category.cmp(&right.category))
            .then(left.shape.cmp(&right.shape))
    });

    Ok(ScanResult {
        report: UnknownReport {
            files_scanned: files.len(),
            total,
            category_totals,
            shapes,
        },
        avoidable_by_file,
    })
}

/// Add `amount` to the counter stored under `key`, inserting it if absent.
///
/// Saturating so a pathological corpus can never overflow the tally.
fn bump<K: Ord>(counts: &mut BTreeMap<K, usize>, key: K, amount: usize) {
    counts
        .entry(key)
        .and_modify(|count| *count = count.saturating_add(amount))
        .or_insert(amount);
}

/// Count non-overlapping occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let mut rest = haystack;
    while let Some(index) = rest.find(needle) {
        count = count.saturating_add(1);
        rest = &rest[index.saturating_add(needle.len())..];
    }
    count
}

/// Update whether we are inside a `smelt_*` runtime prelude helper definition.
///
/// The prelude helpers are emitted as free functions named `smelt_...` (or `fn
/// smelt_...`), and the `SmeltUnknown` enum/impls are prelude too. Because Rust
/// item bodies are brace-balanced we cannot cheaply track nesting line-by-line
/// without a real parser, so this uses a coarse, documented heuristic: a helper
/// definition line *starts* a prelude region, and a line that is exactly a
/// closing brace at column zero *ends* it. This is sufficient because generated
/// helpers are top-level items whose closing brace sits at the left margin.
fn update_prelude_helper_state(line: &str, in_prelude_helper: &mut bool) {
    let trimmed = line.trim_start();
    if is_prelude_definition_start(trimmed) {
        *in_prelude_helper = true;
        // A one-line helper (`fn smelt_x(...) -> ... { ... }`) both opens and
        // closes on the same line; treat a line that ends with `}` as closed.
        if line.trim_end().ends_with('}') && !opens_more_braces_than_it_closes(line) {
            *in_prelude_helper = false;
        }
        return;
    }
    if *in_prelude_helper && line == "}" {
        *in_prelude_helper = false;
    }
}

/// Whether a trimmed line begins a runtime-prelude item definition.
fn is_prelude_definition_start(trimmed: &str) -> bool {
    trimmed.starts_with("fn smelt_")
        || trimmed.starts_with("pub fn smelt_")
        || trimmed.starts_with("async fn smelt_")
        || trimmed.starts_with("pub async fn smelt_")
        || trimmed.starts_with("pub enum SmeltUnknown")
        || trimmed.starts_with("enum SmeltUnknown")
        || trimmed.starts_with("impl SmeltUnknown")
        || trimmed.starts_with("impl Clone for SmeltUnknown")
        || trimmed.starts_with("impl PartialEq for SmeltUnknown")
        || trimmed.starts_with("impl Default for SmeltUnknown")
        || trimmed.starts_with("impl ::std::fmt::Debug for SmeltUnknown")
}

/// Whether a line opens more braces than it closes (i.e. leaves a block open).
fn opens_more_braces_than_it_closes(line: &str) -> bool {
    let opens = count_occurrences(line, "{");
    let closes = count_occurrences(line, "}");
    opens > closes
}

/// Classify one line's `SmeltUnknown` occurrences into a [`Category`].
///
/// # Heuristics (documented, ordered)
///
/// The order matters: the first matching rule wins.
///
/// 1. **Runtime prelude** — the line defines the `SmeltUnknown` enum/impls or
///    sits inside a `smelt_*` helper body (`in_prelude_helper`). This is the
///    fixed scaffolding shared by every generated crate.
/// 2. **Legitimate boundary** — the line names a genuine dynamic edge:
///    - `SmeltUnknown::Function` / `SmeltUnknown::Promise` — erased callables and
///      promises are irreducibly dynamic (JS closures/interop, async values).
///    - `IntoSmeltUnknown` / `into_smelt_unknown` / `to_smelt_unknown` — explicit
///      boundary adapters, exactly the sanctioned way to cross into erasure.
///    - runtime narrowing helpers (`js_typeof`, `tag_check`, `smelt_unknown_is_`,
///      `as_smelt_`, `.expect_`) — inspecting an already-dynamic value.
/// 3. **Avoidable erasure** — everything else. This deliberately captures
///    `SmeltUnknown` in program function signatures, `Vec<SmeltUnknown>` storage,
///    and concrete-tag construction (`SmeltUnknown::Number/String/Bool/...`) used
///    as ordinary data flow, because those are the cases concrete types, unions,
///    or generics are expected to remove. Ambiguous lines fall here on purpose:
///    over-reporting avoidable erasure is safe for a non-blocking gate.
fn classify_line(line: &str, in_prelude_helper: bool) -> Category {
    if in_prelude_helper || is_prelude_definition_start(line.trim_start()) {
        return Category::RuntimePrelude;
    }
    if is_legitimate_boundary_line(line) {
        return Category::LegitimateBoundary;
    }
    Category::AvoidableErasure
}

/// Whether a line's `SmeltUnknown` usage sits at a genuine dynamic boundary.
///
/// See [`classify_line`] rule 2 for the rationale behind each marker.
fn is_legitimate_boundary_line(line: &str) -> bool {
    const BOUNDARY_MARKERS: [&str; 8] = [
        "SmeltUnknown::Function",
        "SmeltUnknown::Promise",
        "IntoSmeltUnknown",
        "into_smelt_unknown",
        "to_smelt_unknown",
        "js_typeof",
        "tag_check",
        "smelt_unknown_is_",
    ];
    BOUNDARY_MARKERS.iter().any(|marker| line.contains(marker))
}

/// Normalize a line into a stable shape so structurally identical call sites
/// collapse onto one group.
///
/// Erased per-site specifics:
/// - string/char literals become `"S"` / `'C'`;
/// - runs of ASCII digits become `N`;
/// - leading indentation is stripped and internal whitespace runs collapse to a
///   single space, so formatting differences never split a group.
///
/// Identifiers are intentionally *kept*: the surrounding identifiers (e.g.
/// `Vec<SmeltUnknown>`, `SmeltUnknown::Number`) are the informative part of the
/// shape, so unlike the probe normalizer this does not blank them out.
fn normalize_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.trim().chars().peekable();
    let mut last_was_space = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                out.push_str("\"S\"");
                // Skip the string body, honoring escapes.
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
                last_was_space = false;
            }
            '\'' => {
                out.push_str("'C'");
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            chars.next();
                        }
                        '\'' => break,
                        _ => {}
                    }
                }
                last_was_space = false;
            }
            digit if digit.is_ascii_digit() => {
                out.push('N');
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
                last_was_space = false;
            }
            space if space.is_whitespace() => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            other => {
                out.push(other);
                last_was_space = false;
            }
        }
    }
    out.trim().to_owned()
}

/// Render the report as Markdown, optionally with a baseline delta section.
fn render_markdown(report: &UnknownReport, baseline: Option<&UnknownReport>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# SmeltUnknown usage report");
    let _ = writeln!(out);
    let _ = writeln!(out, "- Files scanned: {}", report.files_scanned);
    let _ = writeln!(out, "- Total `SmeltUnknown` occurrences: {}", report.total);
    let _ = writeln!(
        out,
        "- Avoidable erasure: **{}** (the number this metric is designed to drive down)",
        report.category_total(Category::AvoidableErasure)
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## Occurrences by category");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Category | Occurrences |");
    let _ = writeln!(out, "| --- | ---: |");
    for category in Category::all() {
        let _ = writeln!(
            out,
            "| {} | {} |",
            category.label(),
            report.category_total(category)
        );
    }
    let _ = writeln!(out);

    if let Some(baseline_report) = baseline {
        render_baseline_delta(&mut out, report, baseline_report);
    }

    render_shape_table(&mut out, report);
    out
}

/// Render the advisory baseline-delta section.
///
/// Deltas are informational: a positive avoidable-erasure delta is flagged as a
/// regression to review, but the command still exits successfully so intentional
/// boundary additions (which raise the legitimate-boundary count) never block a
/// build or CI step.
fn render_baseline_delta(out: &mut String, report: &UnknownReport, baseline: &UnknownReport) {
    let _ = writeln!(out, "## Change vs baseline");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Category | Baseline | Current | Delta |");
    let _ = writeln!(out, "| --- | ---: | ---: | ---: |");
    for category in Category::all() {
        let before = baseline.category_total(category);
        let after = report.category_total(category);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            category.label(),
            before,
            after,
            format_delta(before, after),
        );
    }
    let _ = writeln!(out);
    let avoidable_delta = report
        .category_total(Category::AvoidableErasure)
        .checked_sub(baseline.category_total(Category::AvoidableErasure));
    match avoidable_delta {
        Some(0) | None => {
            let _ = writeln!(
                out,
                "Avoidable erasure did not increase versus the baseline.",
            );
        }
        Some(increase) => {
            let _ = writeln!(
                out,
                "> **Regression to review:** avoidable erasure rose by {increase}. \
                 Confirm each new `SmeltUnknown` marks a genuine dynamic boundary \
                 (see the `SmeltUnknown enforcement` policy in `AGENTS.md`).",
            );
        }
    }
    let _ = writeln!(out);
}

/// Format a signed delta between two counts for a Markdown table cell.
fn format_delta(before: usize, after: usize) -> String {
    if after >= before {
        format!("+{}", after.saturating_sub(before))
    } else {
        format!("-{}", before.saturating_sub(after))
    }
}

/// Render the per-shape breakdown table.
fn render_shape_table(out: &mut String, report: &UnknownReport) {
    let _ = writeln!(out, "## Occurrence shapes");
    let _ = writeln!(out);
    if report.shapes.is_empty() {
        let _ = writeln!(out, "No `SmeltUnknown` occurrences were found.");
        return;
    }
    let _ = writeln!(out, "| Occurrences | Files | Category | Shape | Example |");
    let _ = writeln!(out, "| ---: | ---: | --- | --- | --- |");
    for shape in &report.shapes {
        let _ = writeln!(
            out,
            "| {} | {} | {} | `{}` | `{}` |",
            shape.occurrences,
            shape.files,
            shape.category.label(),
            escape_cell(&shape.shape),
            escape_cell(&shape.example),
        );
    }
    let _ = writeln!(out);
}

/// Escape Markdown table-cell metacharacters so a cell never breaks its row.
fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counting is non-overlapping and handles repeats on one line.
    #[test]
    fn counts_occurrences_per_line() {
        assert_eq!(count_occurrences("SmeltUnknown SmeltUnknown", "SmeltUnknown"), 2);
        assert_eq!(count_occurrences("nothing here", "SmeltUnknown"), 0);
        assert_eq!(count_occurrences("aaa", "aa"), 1);
    }

    /// Prelude definition starts and single-line helpers toggle state correctly.
    #[test]
    fn tracks_prelude_helper_regions() {
        let mut in_prelude = false;
        update_prelude_helper_state("pub enum SmeltUnknown {", &mut in_prelude);
        assert!(in_prelude, "enum def opens a prelude region");
        update_prelude_helper_state("    Number(f64),", &mut in_prelude);
        assert!(in_prelude, "field lines stay inside the region");
        update_prelude_helper_state("}", &mut in_prelude);
        assert!(!in_prelude, "left-margin close ends the region");

        // A one-line helper opens and closes on the same line.
        update_prelude_helper_state(
            "fn smelt_missing() -> SmeltUnknown { SmeltUnknown::Undefined }",
            &mut in_prelude,
        );
        assert!(!in_prelude, "one-line helper does not leak state");
    }

    /// The enum definition and helper bodies classify as runtime prelude.
    #[test]
    fn classifies_runtime_prelude() {
        assert_eq!(
            classify_line("pub enum SmeltUnknown {", false),
            Category::RuntimePrelude
        );
        assert_eq!(
            classify_line("    SmeltUnknown::Number(*value)", true),
            Category::RuntimePrelude,
            "inside a helper body everything is prelude"
        );
    }

    /// Erased callables, promises, and adapters are legitimate boundaries.
    #[test]
    fn classifies_legitimate_boundaries() {
        assert_eq!(
            classify_line("let f = SmeltUnknown::Function(rc);", false),
            Category::LegitimateBoundary
        );
        assert_eq!(
            classify_line("SmeltUnknown::Promise(promise)", false),
            Category::LegitimateBoundary
        );
        assert_eq!(
            classify_line("value.into_smelt_unknown()", false),
            Category::LegitimateBoundary
        );
    }

    /// Program storage and signatures classify as avoidable erasure.
    #[test]
    fn classifies_avoidable_erasure() {
        assert_eq!(
            classify_line("fn my_program_fn(x: SmeltUnknown) -> SmeltUnknown {", false),
            Category::AvoidableErasure
        );
        assert_eq!(
            classify_line("let values: Vec<SmeltUnknown> = Vec::new();", false),
            Category::AvoidableErasure
        );
    }

    /// Normalization collapses literals and digits but keeps identifiers.
    #[test]
    fn normalize_collapses_literals_keeps_identifiers() {
        assert_eq!(
            normalize_line("    let x: Vec<SmeltUnknown> = vec![42, 7];"),
            "let x: Vec<SmeltUnknown> = vec![N, N];"
        );
        assert_eq!(
            normalize_line("SmeltUnknown::String(\"hello\".to_owned())"),
            "SmeltUnknown::String(\"S\".to_owned())"
        );
    }

    /// A full scan groups by shape, tallies categories, and stays deterministic.
    #[test]
    fn scan_groups_and_categorizes() {
        let dir = std::env::temp_dir().join(format!(
            "smelt-unknown-report-test-{}",
            std::process::id()
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gen.rs");
        std::fs::write(
            &file,
            concat!(
                "// @generated by smelt.\n",
                "pub enum SmeltUnknown {\n",
                "    Function(Rc<dyn Fn(Vec<SmeltUnknown>)>),\n",
                "}\n",
                "fn program(x: SmeltUnknown) -> SmeltUnknown {\n",
                "    let cb = SmeltUnknown::Function(rc);\n",
                "    x\n",
                "}\n",
            ),
        )
        .unwrap();

        let report = scan_files(&[file]).unwrap().report;
        assert_eq!(report.files_scanned, 1);
        // enum line (1) + Function field (1) inside enum = 2 prelude;
        // signature line has 2; Function construction has 1 legitimate.
        assert!(report.category_total(Category::RuntimePrelude) >= 2);
        assert_eq!(report.category_total(Category::LegitimateBoundary), 1);
        assert_eq!(report.category_total(Category::AvoidableErasure), 2);

        // Re-scanning yields identical output (determinism).
        let json_a = serde_json::to_string(&report).unwrap();
        let report_b = scan_files(&[dir.join("gen.rs")]).unwrap().report;
        let json_b = serde_json::to_string(&report_b).unwrap();
        assert_eq!(json_a, json_b);

        drop(std::fs::remove_dir_all(&dir));
    }

    /// When the emitter's prelude sentinel is present, everything above it is
    /// classified as prelude regardless of the per-line heuristics — so a program
    /// signature accidentally sitting above the marker would still be prelude,
    /// and a boundary-looking line below it is judged on its own merits.
    #[test]
    fn marker_defines_authoritative_prelude_boundary() {
        let dir = std::env::temp_dir().join(format!(
            "smelt-unknown-report-marker-{}",
            std::process::id()
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gen.rs");
        // The pre-marker `fn helper` signature looks like avoidable program
        // erasure, but because it sits above the sentinel the boundary makes it
        // prelude; the post-marker `fn program` signature stays avoidable.
        std::fs::write(
            &file,
            format!(
                concat!(
                    "// @generated by smelt.\n",
                    "fn helper(x: SmeltUnknown) -> SmeltUnknown {{ x }}\n",
                    "{marker}\n",
                    "fn program(x: SmeltUnknown) -> SmeltUnknown {{ x }}\n",
                ),
                marker = smelt_codegen_rust::PRELUDE_END_MARKER,
            ),
        )
        .unwrap();

        let report = scan_files(&[file]).unwrap().report;
        // The pre-marker signature (2 occurrences) is prelude; the post-marker
        // program signature (2 occurrences) is avoidable erasure.
        assert_eq!(report.category_total(Category::RuntimePrelude), 2);
        assert_eq!(report.category_total(Category::AvoidableErasure), 2);
        assert_eq!(report.category_total(Category::LegitimateBoundary), 0);

        drop(std::fs::remove_dir_all(&dir));
    }

    /// Markdown rendering emits the category table and flags regressions.
    #[test]
    fn markdown_flags_avoidable_regression() {
        let baseline = UnknownReport {
            files_scanned: 1,
            total: 2,
            category_totals: BTreeMap::from([(
                Category::AvoidableErasure.as_str().to_owned(),
                2usize,
            )]),
            shapes: Vec::new(),
        };
        let current = UnknownReport {
            files_scanned: 1,
            total: 5,
            category_totals: BTreeMap::from([(
                Category::AvoidableErasure.as_str().to_owned(),
                5usize,
            )]),
            shapes: Vec::new(),
        };
        let markdown = render_markdown(&current, Some(&baseline));
        assert!(markdown.contains("## Occurrences by category"));
        assert!(markdown.contains("Regression to review"));
        assert!(markdown.contains("rose by 3"));
    }

    /// No regression note when avoidable erasure holds steady or drops.
    #[test]
    fn markdown_reports_no_regression_when_flat() {
        let baseline = UnknownReport {
            files_scanned: 1,
            total: 4,
            category_totals: BTreeMap::from([(
                Category::AvoidableErasure.as_str().to_owned(),
                4usize,
            )]),
            shapes: Vec::new(),
        };
        let current = baseline.clone();
        let markdown = render_markdown(&current, Some(&baseline));
        assert!(markdown.contains("did not increase"));
    }

    /// Build a [`ScanResult`] fixture with the given avoidable total and per-file
    /// tally, for exercising [`detect_regression`] without touching disk.
    fn fixture_scan(avoidable: usize, avoidable_by_file: &[(&str, usize)]) -> ScanResult {
        ScanResult {
            report: UnknownReport {
                files_scanned: avoidable_by_file.len(),
                total: avoidable,
                category_totals: BTreeMap::from([(
                    Category::AvoidableErasure.as_str().to_owned(),
                    avoidable,
                )]),
                shapes: Vec::new(),
            },
            avoidable_by_file: avoidable_by_file
                .iter()
                .map(|(file, count)| ((*file).to_owned(), *count))
                .collect(),
        }
    }

    /// A fabricated baseline report carrying just an avoidable total.
    fn fixture_baseline(avoidable: usize) -> UnknownReport {
        UnknownReport {
            files_scanned: 1,
            total: avoidable,
            category_totals: BTreeMap::from([(
                Category::AvoidableErasure.as_str().to_owned(),
                avoidable,
            )]),
            shapes: Vec::new(),
        }
    }

    /// A rise above the baseline is a regression naming the delta and the top
    /// offending files, ranked by avoidable count.
    #[test]
    fn detect_regression_flags_increase_with_top_files() {
        let scan = fixture_scan(10, &[("a.rs", 2), ("b.rs", 7), ("c.rs", 1)]);
        let baseline = fixture_baseline(4);
        let regression = detect_regression(&scan, Some(&baseline)).expect("increase is a regression");
        assert_eq!(regression.baseline, 4);
        assert_eq!(regression.current, 10);
        assert_eq!(regression.delta, 6);
        // Ranked by count, most first.
        assert_eq!(regression.top_files[0], ("b.rs".to_owned(), 7));
        assert_eq!(regression.top_files[1], ("a.rs".to_owned(), 2));
        assert_eq!(regression.top_files[2], ("c.rs".to_owned(), 1));
    }

    /// A flat or falling avoidable count is never a regression.
    #[test]
    fn detect_regression_ignores_flat_and_decrease() {
        let flat = fixture_scan(4, &[("a.rs", 4)]);
        assert!(detect_regression(&flat, Some(&fixture_baseline(4))).is_none());
        let lower = fixture_scan(2, &[("a.rs", 2)]);
        assert!(detect_regression(&lower, Some(&fixture_baseline(9))).is_none());
    }

    /// Without a baseline there is nothing to regress against.
    #[test]
    fn detect_regression_requires_baseline() {
        let scan = fixture_scan(99, &[("a.rs", 99)]);
        assert!(detect_regression(&scan, None).is_none());
    }

    /// The top-offenders list is capped so a huge corpus yields a readable message.
    #[test]
    fn detect_regression_caps_top_files() {
        let files: Vec<(String, usize)> = (0..25)
            .map(|index| (format!("file{index:02}.rs"), index + 1))
            .collect();
        let refs: Vec<(&str, usize)> = files
            .iter()
            .map(|(name, count)| (name.as_str(), *count))
            .collect();
        let scan = fixture_scan(1000, &refs);
        let regression =
            detect_regression(&scan, Some(&fixture_baseline(0))).expect("increase is a regression");
        assert_eq!(regression.top_files.len(), TOP_OFFENDER_LIMIT);
        // The single largest contributor leads.
        assert_eq!(regression.top_files[0].1, 25);
    }
}
