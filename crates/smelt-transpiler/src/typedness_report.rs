//! `smelt typedness-report` — a positive typedness ratchet.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631),
//! `test/aot/typedness/run-typedness.ts` and `typedness.ts`: one record per
//! program saying how much of it is typed, kept in a checked-in baseline, and
//! a program may never become less typed than its record. Where Bun counts
//! "places" and "reads" that have a type that says something, Smelt counts —
//! on optimized MIR, through the Rust emitter's own type rendering (see
//! `smelt_codegen_rust::measure_typedness`) — values, property reads and calls,
//! each concrete, generic or erased.
//!
//! # Inputs
//!
//! Each path given on the command line is one of:
//! - a `smelt-typedness.json` sidecar (written by `smelt build` next to the
//!   generated `src/`), or a directory holding one (directly, or as the parent
//!   of a scanned `src/`): every file in it is a program;
//! - a `Smelt.toml` manifest: lowered in-process, every source file is a
//!   program (no Rust is generated or compiled);
//! - a `.ts`/`.py` source file: lowered in-process with its local imports, the
//!   whole crate is one program named by the path as given. This is how the
//!   examples corpus (`examples/typescript/end-to-end/*/input.ts`) is measured
//!   without a Rust build. A file that fails to lower is recorded as refused.
//!
//! # The ratchet
//!
//! A program is *no less typed* than its baseline record (Bun's
//! `isNoLessTyped`) when, for every kind, its erased count did not rise and its
//! typed share `(concrete + generic) / total` did not fall. The corpus total is
//! judged the same way. A program with no baseline record is not judged; a
//! program the baseline measured that is now refused is a regression. With
//! `--update-baseline`, the baseline is rewritten taking each program's new
//! record only when it is no less typed, so a regression can never be
//! snapshotted silently.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use smelt_codegen_rust::{KindCounts, TypednessCounts};

use crate::{
    CliResult, config_parser, lowering,
    manifest::{exclusion_base, manifest_relative_path},
    pipeline,
};

/// File name of the sidecar `smelt build` writes next to the generated `src/`.
pub(crate) const SIDECAR_FILE_NAME: &str = "smelt-typedness.json";

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypednessFormat {
    /// Human-readable Markdown.
    Markdown,
    /// JSON (also the sidecar and baseline format).
    Json,
}

/// One program's record: its counts, or `None` when it failed to lower.
pub(crate) type ProgramRecord = Option<TypednessCounts>;

/// A typedness report: the corpus total and every program's record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TypednessReport {
    /// Format version.
    pub(crate) version: u32,
    /// Sum over every measured program.
    pub(crate) total: TypednessCounts,
    /// Per-program records, by name; `null` marks a refused program.
    pub(crate) programs: BTreeMap<String, ProgramRecord>,
}

impl TypednessReport {
    /// Build a report from program records, computing the total.
    pub(crate) fn from_programs(programs: BTreeMap<String, ProgramRecord>) -> Self {
        let mut total = TypednessCounts::default();
        for counts in programs.values().flatten() {
            total.merge(*counts);
        }
        Self {
            version: 1,
            total,
            programs,
        }
    }
}

/// Options for one report run.
pub(crate) struct TypednessReportOptions<'a> {
    /// Inputs; see the module docs.
    pub(crate) paths: &'a [PathBuf],
    /// Output format.
    pub(crate) format: TypednessFormat,
    /// Baseline to compare against.
    pub(crate) baseline: Option<PathBuf>,
    /// Rewrite the baseline with every record that is no less typed.
    pub(crate) update_baseline: bool,
}

/// One thing that got less typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Worsening {
    /// Program name, or `<total>`.
    pub(crate) program: String,
    /// What got worse.
    pub(crate) detail: String,
}

/// The outcome of a run: rendered text and the regressions found.
pub(crate) struct TypednessOutcome {
    /// The report in the requested format.
    pub(crate) rendered: String,
    /// Everything that became less typed than the baseline.
    pub(crate) worsened: Vec<Worsening>,
}

/// Run a report: measure, compare, optionally update the baseline, render.
pub(crate) fn typedness_report(options: &TypednessReportOptions<'_>) -> CliResult<TypednessOutcome> {
    let mut programs = BTreeMap::new();
    for path in options.paths {
        programs.extend(measure_input(path)?);
    }
    let report = TypednessReport::from_programs(programs);
    let baseline = match &options.baseline {
        Some(path) if path.is_file() => Some(load_report(path)?),
        Some(path) if !options.update_baseline => {
            return Err(format!("baseline `{}` does not exist", path.display()).into());
        }
        _ => None,
    };
    let worsened = baseline
        .as_ref()
        .map(|baseline| compare(&report, baseline))
        .unwrap_or_default();
    if options.update_baseline
        && let Some(path) = &options.baseline
    {
        let updated = updated_baseline(&report, baseline.as_ref());
        write_report_if_changed(path, &updated)?;
    }
    let rendered = match options.format {
        TypednessFormat::Json => render_json(&report)?,
        TypednessFormat::Markdown => render_markdown(&report, baseline.as_ref(), &worsened),
    };
    Ok(TypednessOutcome { rendered, worsened })
}

/// Measure one input path into program records.
fn measure_input(path: &Path) -> CliResult<BTreeMap<String, ProgramRecord>> {
    if path.is_dir() {
        let sidecar = std::iter::once(path.join(SIDECAR_FILE_NAME))
            .chain(path.parent().map(|parent| parent.join(SIDECAR_FILE_NAME)))
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                format!("`{}` holds no `{SIDECAR_FILE_NAME}`", path.display())
            })?;
        return Ok(load_report(&sidecar)?.programs);
    }
    let name = path.to_string_lossy().replace('\\', "/");
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Ok(load_report(path)?.programs);
    }
    if path.file_name().is_some_and(|file| file == "Smelt.toml") {
        return measure_manifest(path);
    }
    // A single source program: the whole lowered crate is its record.
    let record = lowering::lower_single_file(&name)
        .ok()
        .and_then(|(krate, _)| pipeline::lower_to_optimized_mir(&krate).ok())
        .and_then(|mir| smelt_codegen_rust::measure_typedness(&mir).ok())
        .map(|measurement| measurement.total());
    Ok(BTreeMap::from([(name, record)]))
}

/// Lower a manifest in-process and measure every source file.
fn measure_manifest(manifest_path: &Path) -> CliResult<BTreeMap<String, ProgramRecord>> {
    let config = config_parser::parse(
        manifest_path
            .to_str()
            .ok_or("manifest path contains invalid UTF-8")?,
    )?;
    let (krate, _) = lowering::lower_manifest_entries(&config, manifest_path)?;
    let mir = pipeline::lower_to_optimized_mir(&krate)?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(programs_of(&smelt_codegen_rust::measure_typedness(&mir)?, manifest_dir))
}

/// Per-file records of a measurement, keyed by manifest-relative path.
pub(crate) fn programs_of(
    measurement: &smelt_codegen_rust::TypednessMeasurement,
    manifest_dir: &Path,
) -> BTreeMap<String, ProgramRecord> {
    let base = exclusion_base(manifest_dir);
    let mut programs: BTreeMap<String, ProgramRecord> = BTreeMap::new();
    for (file, counts) in &measurement.files {
        let relative = manifest_relative_path(Path::new(file), &base);
        let key = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let entry = programs.entry(key).or_insert(Some(TypednessCounts::default()));
        if let Some(entry) = entry {
            entry.merge(*counts);
        }
    }
    programs
}

/// Load a report (sidecar or baseline).
fn load_report(path: &Path) -> CliResult<TypednessReport> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse `{}`: {error}", path.display()).into())
}

/// Serialize a report deterministically (pretty JSON, trailing newline).
fn render_json(report: &TypednessReport) -> CliResult<String> {
    let mut text = serde_json::to_string_pretty(report)?;
    text.push('\n');
    Ok(text)
}

/// Write a report to `path` only when its bytes change.
pub(crate) fn write_report_if_changed(path: &Path, report: &TypednessReport) -> CliResult<()> {
    let text = render_json(report)?;
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == text) {
        return Ok(());
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// The three kinds, by name.
fn kinds(counts: &TypednessCounts) -> [(&'static str, KindCounts); 3] {
    [
        ("values", counts.values),
        ("reads", counts.reads),
        ("calls", counts.calls),
    ]
}

/// Why `now` is less typed than `then`, one line per kind that got worse;
/// empty when `now` is no less typed (Bun's `isNoLessTyped`).
pub(crate) fn worsening(now: &TypednessCounts, then: &TypednessCounts) -> Vec<String> {
    kinds(now)
        .into_iter()
        .zip(kinds(then))
        .filter_map(|((kind, now), (_, then))| {
            let mut reasons = Vec::new();
            if now.erased > then.erased {
                reasons.push(format!("erased {} -> {}", then.erased, now.erased));
            }
            // typed(now)/total(now) >= typed(then)/total(then), cross-multiplied.
            let share_now = u128::from(now.typed() as u64) * u128::from(then.total() as u64);
            let share_then = u128::from(then.typed() as u64) * u128::from(now.total() as u64);
            if share_now < share_then {
                reasons.push(format!(
                    "typed share {} -> {}",
                    share(then),
                    share(now)
                ));
            }
            (!reasons.is_empty()).then(|| format!("{kind}: {}", reasons.join(", ")))
        })
        .collect()
}

/// Compare a report with its baseline: every judged program and the total.
pub(crate) fn compare(report: &TypednessReport, baseline: &TypednessReport) -> Vec<Worsening> {
    let mut worsened = Vec::new();
    for (program, record) in &report.programs {
        let Some(then) = baseline.programs.get(program) else {
            continue;
        };
        match (record, then) {
            (Some(now), Some(then)) => {
                for detail in worsening(now, then) {
                    worsened.push(Worsening {
                        program: program.clone(),
                        detail,
                    });
                }
            }
            (None, Some(_)) => worsened.push(Worsening {
                program: program.clone(),
                detail: "was measured, now refused (fails to lower)".to_owned(),
            }),
            (_, None) => {}
        }
    }
    for detail in worsening(&report.total, &baseline.total) {
        worsened.push(Worsening {
            program: "<total>".to_owned(),
            detail,
        });
    }
    worsened
}

/// The baseline rewritten with every record that is no less typed (Bun's
/// `AOT_PROGRAMS_UPDATE=1`): a worsened program keeps its old record, a new
/// program gets one, a program no longer measured is dropped.
pub(crate) fn updated_baseline(
    report: &TypednessReport,
    baseline: Option<&TypednessReport>,
) -> TypednessReport {
    let programs = report
        .programs
        .iter()
        .map(|(program, record)| {
            let kept = baseline
                .and_then(|baseline| baseline.programs.get(program))
                .filter(|then| match (record, then) {
                    (Some(now), Some(then)) => !worsening(now, then).is_empty(),
                    (None, Some(_)) => true,
                    _ => false,
                });
            (program.clone(), kept.copied().unwrap_or(*record))
        })
        .collect();
    TypednessReport::from_programs(programs)
}

/// Typed share as a percentage with one decimal, `-` for an empty kind.
fn share(counts: KindCounts) -> String {
    if counts.total() == 0 {
        return "-".to_owned();
    }
    let permille = counts.typed().saturating_mul(1000) / counts.total();
    format!("{}.{}%", permille / 10, permille % 10)
}

/// Render the Markdown report.
fn render_markdown(
    report: &TypednessReport,
    baseline: Option<&TypednessReport>,
    worsened: &[Worsening],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Typedness report");
    let _ = writeln!(out);
    let measured = report.programs.values().filter(|record| record.is_some()).count();
    let _ = writeln!(
        out,
        "- Programs: {} ({measured} measured, {} refused)",
        report.programs.len(),
        report.programs.len().saturating_sub(measured)
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "| Kind | Concrete | Generic | Erased | Typed share |");
    let _ = writeln!(out, "| --- | ---: | ---: | ---: | ---: |");
    for (kind, counts) in kinds(&report.total) {
        let _ = writeln!(
            out,
            "| {kind} | {} | {} | {} | {} |",
            counts.concrete,
            counts.generic,
            counts.erased,
            share(counts)
        );
    }
    let _ = writeln!(out);
    if let Some(baseline) = baseline {
        let _ = writeln!(out, "## Change vs baseline");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Kind | Baseline share | Current share | Erased delta |");
        let _ = writeln!(out, "| --- | ---: | ---: | ---: |");
        for ((kind, now), (_, then)) in kinds(&report.total).into_iter().zip(kinds(&baseline.total)) {
            let delta = i128::from(now.erased as u64) - i128::from(then.erased as u64);
            let _ = writeln!(out, "| {kind} | {} | {} | {delta:+} |", share(then), share(now));
        }
        let _ = writeln!(out);
        if worsened.is_empty() {
            let _ = writeln!(out, "No program became less typed than its baseline record.");
        } else {
            let _ = writeln!(out, "**Less typed than the baseline:**");
            let _ = writeln!(out);
            for worsening in worsened {
                let _ = writeln!(out, "- `{}`: {}", worsening.program, worsening.detail);
            }
        }
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts with the given (concrete, generic, erased) for every kind.
    fn counts(concrete: usize, generic: usize, erased: usize) -> TypednessCounts {
        let kind = KindCounts::new(concrete, generic, erased);
        let mut counts = TypednessCounts::default();
        counts.values = kind;
        counts.reads = kind;
        counts.calls = kind;
        counts
    }

    /// More typed or equal passes; more erased or a lower share fails.
    #[test]
    fn no_less_typed_follows_bun() {
        assert!(worsening(&counts(5, 1, 2), &counts(5, 1, 2)).is_empty());
        assert!(worsening(&counts(7, 1, 1), &counts(5, 1, 2)).is_empty());
        // More values overall but the same erased count and a higher share.
        assert!(worsening(&counts(10, 0, 2), &counts(5, 0, 2)).is_empty());
        // Erased rose.
        assert_eq!(worsening(&counts(9, 0, 3), &counts(5, 0, 2)).len(), 3);
        // Erased flat but the share fell (typed values disappeared).
        let fell = worsening(&counts(1, 0, 2), &counts(5, 0, 2));
        assert_eq!(fell.len(), 3);
        assert!(fell[0].contains("typed share"), "{fell:?}");
    }

    /// A generic value counts as typed, exactly like a concrete one.
    #[test]
    fn generic_counts_as_typed() {
        assert!(worsening(&counts(4, 2, 1), &counts(6, 0, 1)).is_empty());
    }

    /// Programs are judged individually; new programs are not judged; a refused
    /// program that used to be measured is a regression.
    #[test]
    fn compare_judges_each_program_and_the_total() {
        let baseline = TypednessReport::from_programs(BTreeMap::from([
            ("a.ts".to_owned(), Some(counts(5, 0, 1))),
            ("b.ts".to_owned(), Some(counts(5, 0, 1))),
        ]));
        let report = TypednessReport::from_programs(BTreeMap::from([
            ("a.ts".to_owned(), Some(counts(5, 0, 2))),
            ("b.ts".to_owned(), None),
            ("new.ts".to_owned(), Some(counts(0, 0, 9))),
        ]));
        let worsened = compare(&report, &baseline);
        let programs = worsened.iter().map(|w| w.program.as_str()).collect::<Vec<_>>();
        assert!(programs.contains(&"a.ts"));
        assert!(programs.contains(&"b.ts"));
        assert!(!programs.contains(&"new.ts"));
    }

    /// The update keeps a worsened program's old record and takes the rest.
    #[test]
    fn update_never_snapshots_a_regression() {
        let baseline = TypednessReport::from_programs(BTreeMap::from([
            ("worse.ts".to_owned(), Some(counts(5, 0, 1))),
            ("better.ts".to_owned(), Some(counts(5, 0, 1))),
            ("gone.ts".to_owned(), Some(counts(1, 0, 0))),
        ]));
        let report = TypednessReport::from_programs(BTreeMap::from([
            ("worse.ts".to_owned(), Some(counts(5, 0, 3))),
            ("better.ts".to_owned(), Some(counts(6, 0, 0))),
            ("new.ts".to_owned(), Some(counts(2, 0, 0))),
        ]));
        let updated = updated_baseline(&report, Some(&baseline));
        assert_eq!(updated.programs["worse.ts"], Some(counts(5, 0, 1)));
        assert_eq!(updated.programs["better.ts"], Some(counts(6, 0, 0)));
        assert_eq!(updated.programs["new.ts"], Some(counts(2, 0, 0)));
        assert!(!updated.programs.contains_key("gone.ts"));
    }
}
