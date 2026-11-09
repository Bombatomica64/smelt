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
//! and needs no guessing. For generated files without the marker, the scanner
//! falls back to the per-line prelude heuristics in
//! [`update_prelude_helper_state`].

mod baseline;
mod classify;
mod render;
mod scan;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use baseline::{coverage_loss, detect_regression, regression_message, updated_baseline};
use render::render_markdown;
use scan::{ScanResult, scan_files};

use crate::{
    CliResult,
    report::{self, GatedOutcome, ReportFormat, Schema, SchemaVersion},
    source_files::discover_rust_files,
};

/// Options controlling one `SmeltUnknown` report run.
pub(crate) struct UnknownReportOptions<'a> {
    /// Roots to scan. Each may be a single `.rs` file or a directory that is
    /// walked recursively for `.rs` files.
    pub(crate) roots: &'a [PathBuf],
    /// Report serialization format.
    pub(crate) format: ReportFormat,
    /// Optional prior JSON report used to compute per-category deltas. Comparing
    /// against a baseline is advisory by default: it surfaces regressions without
    /// failing the command, so intentional boundary additions are never blocked.
    pub(crate) baseline: Option<PathBuf>,
    /// Rewrite `baseline` with this scan, unless avoidable erasure rose.
    pub(crate) update_baseline: bool,
    /// Explicit `smelt-erasure.json` sidecars to read (`--erasure`). Besides
    /// these, a sidecar beside a scanned directory (`<root>/../smelt-erasure.json`,
    /// the layout `smelt build` writes next to `src/`) or inside it is read
    /// automatically.
    pub(crate) erasure_sidecars: &'a [PathBuf],
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
    /// Normalized line pattern (string and char literals and digit runs collapsed, identifiers kept) so distinct call
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
    /// Format version; reading another is an error.
    version: SchemaVersion<Self>,
    /// Number of generated `.rs` files scanned.
    files_scanned: usize,
    /// Total `SmeltUnknown` occurrences across every category.
    total: usize,
    /// Occurrence totals keyed by category identifier, in stable order.
    category_totals: BTreeMap<String, usize>,
    /// Distinct shape groups, most frequent first.
    shapes: Vec<ShapeGroup>,
    /// Per-reason / per-source-line breakdown from `smelt-erasure.json`
    /// sidecars, when any were found. Optional so baselines written before the
    /// sidecar existed (and corpora without one) keep parsing unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    erasure: Option<crate::erasure::ErasureBreakdown>,
}

impl Schema for UnknownReport {
    const NAME: &'static str = "SmeltUnknown report";
    const VERSION: u32 = 1;
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
/// The returned [`GatedOutcome`] carries the rendered text and, when the scan
/// cannot judge the baseline's corpus ([`coverage_loss`]) or avoidable erasure
/// rose above the baseline, the message saying so; only `--fail-on-regression`
/// turns that into a nonzero exit. A scan with a coverage loss never rewrites
/// the baseline.
pub(crate) fn unknown_report(
    options: &UnknownReportOptions<'_>,
) -> CliResult<GatedOutcome> {
    let files = discover_rust_files(options.roots)?;
    let mut scan = scan_files(&files)?;
    let sidecars = discover_erasure_sidecars(options.roots, options.erasure_sidecars)?;
    if !sidecars.is_empty() {
        scan.report.erasure = Some(crate::erasure::breakdown(&sidecars));
    }
    let scanned = &scan.report;
    let baseline = match &options.baseline {
        Some(path) if options.update_baseline && !path.is_file() => None,
        Some(path) => Some(report::load_json::<UnknownReport>(path, "baseline ")?),
        None => None,
    };
    let lost_coverage = options.baseline.as_ref().and_then(|_| coverage_loss(scanned, baseline.as_ref()));
    if options.update_baseline
        && lost_coverage.is_none()
        && let Some(path) = &options.baseline
    {
        let updated = updated_baseline(scanned, baseline.as_ref());
        report::write_if_changed(path, &serde_json::to_string_pretty(updated)?)?;
    }
    let rendered = match options.format {
        ReportFormat::Json => serde_json::to_string_pretty(scanned)?,
        ReportFormat::Markdown => render_markdown(scanned, baseline.as_ref()),
    };
    let regression = baseline.as_ref().and_then(|prior| detect_regression(&scan, prior));
    Ok(GatedOutcome {
        rendered,
        regression: lost_coverage.or_else(|| regression.as_ref().map(regression_message)),
    })
}

/// Load the explicit sidecars plus any found beside or inside a scanned
/// directory root, each path once, in a stable order.
fn discover_erasure_sidecars(
    roots: &[PathBuf],
    explicit: &[PathBuf],
) -> CliResult<Vec<crate::erasure::ErasureSidecar>> {
    let mut paths = explicit.to_vec();
    for root in roots.iter().filter(|root| root.is_dir()) {
        let inside = root.join(crate::erasure::SIDECAR_FILE_NAME);
        let beside = root
            .parent()
            .map(|parent| parent.join(crate::erasure::SIDECAR_FILE_NAME));
        paths.extend(
            [Some(inside), beside]
                .into_iter()
                .flatten()
                .filter(|path| path.is_file()),
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    paths.retain(|path| seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())));
    paths
        .iter()
        .map(|path| crate::erasure::load_sidecar(path).map_err(Into::into))
        .collect()
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

#[cfg(test)]
mod tests;
