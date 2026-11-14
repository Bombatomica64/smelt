//! The committed baseline: detecting a regression against it and writing an
//! updated snapshot, which can never take a rise in avoidable erasure.

use super::{Category, ScanResult, TOP_OFFENDER_LIMIT, UnknownReport};
use crate::report::push_fmt;

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

/// The gate's failure message: the delta and the files that contribute the
/// most, so reviewers know where to look first.
pub(super) fn regression_message(regression: &Regression) -> String {
    let mut message = format!(
        "avoidable erasure regressed: {} > {} (baseline), +{} occurrence(s)",
        regression.current, regression.baseline, regression.delta,
    );
    if !regression.top_files.is_empty() {
        message.push_str("\ntop offending files:");
        for (file, count) in &regression.top_files {
            push_fmt(&mut message, format_args!("\n  {count:>6}  {file}"));
        }
    }
    message.push_str(
        "\nReclassify a genuine boundary (with a code comment + regression test) \
         and re-snapshot the baseline, or reduce the erasure. See the \
         `SmeltUnknown enforcement` policy in AGENTS.md.",
    );
    message
}

/// The report `--update-baseline` writes: the scan, unless avoidable erasure
/// rose above `baseline`, which then stands, so a regression can never be
/// snapshotted. The report has no per-file records to keep selectively, so the
/// guard is on the corpus total the gate itself judges.
pub(super) fn updated_baseline<'report>(
    scanned: &'report UnknownReport,
    baseline: Option<&'report UnknownReport>,
) -> &'report UnknownReport {
    let avoidable = |report: &UnknownReport| report.category_total(Category::AvoidableErasure);
    baseline
        .filter(|prior| avoidable(scanned) > avoidable(prior))
        .unwrap_or(scanned)
}

/// The largest share of the baseline's files a scan may lose, in percent. A
/// few files come and go with ordinary work; losing more means the scan no
/// longer covers the corpus the baseline measured, and its counts prove nothing.
const MAX_SCAN_DROP_PERCENT: usize = 10;

/// Why the scan cannot judge anything: it saw no file at all, or lost more than
/// [`MAX_SCAN_DROP_PERCENT`] of the files `baseline` scanned. Such a scan also
/// reports a lower erasure total, so without this check it passes the gate.
pub(super) fn coverage_loss(
    scanned: &UnknownReport,
    baseline: Option<&UnknownReport>,
) -> Option<String> {
    if scanned.files_scanned == 0 {
        return Some("the scan found no generated Rust file, so it judged nothing".to_owned());
    }
    let prior = baseline?.files_scanned;
    let floor = prior.saturating_mul(100_usize.saturating_sub(MAX_SCAN_DROP_PERCENT));
    (scanned.files_scanned.saturating_mul(100) < floor).then(|| {
        format!(
            "the scan covered {} file(s) where the baseline covered {prior}, a drop of more than \
             {MAX_SCAN_DROP_PERCENT}%. If the corpus shrank on purpose, delete the baseline and \
             write it again in the same commit.",
            scanned.files_scanned,
        )
    })
}

/// Compute an avoidable-erasure [`Regression`] versus `baseline`, if any.
///
/// Returns `None` when avoidable erasure did not rise above the baseline — a
/// flat or falling count is never a regression. The top
/// offending files are ranked by their avoidable-erasure occurrence count in the
/// current scan, so the message points reviewers at where to look first.
pub(super) fn detect_regression(scan: &ScanResult, baseline: &UnknownReport) -> Option<Regression> {
    let baseline_avoidable = baseline.category_total(Category::AvoidableErasure);
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
