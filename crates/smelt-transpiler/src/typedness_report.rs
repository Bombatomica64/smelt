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
//! A program is *no less typed* than its baseline record when, for every kind
//! (values, reads, calls), its erased count did not rise. The corpus total is
//! judged the same way. A program with no baseline record is not judged; a
//! program the baseline measured that is now refused is a regression. With
//! `--update-baseline`, the baseline is rewritten taking each program's new
//! record only when it is no less typed, so a record whose erased count rose
//! can never be snapshotted silently.
//!
//! The typed share `(concrete + generic) / total` is still reported, but it no
//! longer judges. Old requirement (Bun's `isNoLessTyped`): erased may not rise
//! *and* the typed share may not fall. New requirement: erased may not rise.
//! Why: the counts are per MIR entry, so an optimization that removes redundant
//! concrete temporaries (e.g. `%t = rv; %x = move %t` becoming `%x = rv`)
//! lowers the share while the program's types are unchanged or better. The
//! north-star quantity is how many values, reads and calls end up erased in the
//! generated Rust; removing typed values that the emitted crate no longer
//! contains is not a typing regression, and judging the share would block it.
//! A value that loses its type moves to erased and is still caught.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use smelt_codegen_rust::{KindCounts, TypednessCounts};

use crate::{
    CliResult, config_parser, lowering,
    pipeline,
    report::{self, GatedOutcome, ReportFormat},
    source_files::{exclusion_base, manifest_relative_path},
};

/// File name of the sidecar `smelt build` writes next to the generated `src/`.
pub(crate) const SIDECAR_FILE_NAME: &str = "smelt-typedness.json";

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
    pub(crate) format: ReportFormat,
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

/// Run a report: measure, compare, optionally update the baseline, render.
pub(crate) fn typedness_report(options: &TypednessReportOptions<'_>) -> CliResult<GatedOutcome> {
    let mut programs = BTreeMap::new();
    for path in options.paths {
        programs.extend(measure_input(path)?);
    }
    let measured = TypednessReport::from_programs(programs);
    let baseline = match &options.baseline {
        Some(path) if path.is_file() => Some(report::load_json::<TypednessReport>(path, "")?),
        Some(path) if !options.update_baseline => {
            return Err(format!("baseline `{}` does not exist", path.display()).into());
        }
        _ => None,
    };
    let worsened = baseline
        .as_ref()
        .map(|baseline| compare(&measured, baseline))
        .unwrap_or_default();
    if options.update_baseline
        && let Some(path) = &options.baseline
    {
        let updated = updated_baseline(&measured, baseline.as_ref());
        write_report_if_changed(path, &updated)?;
    }
    let rendered = match options.format {
        ReportFormat::Json => render_json(&measured)?,
        ReportFormat::Markdown => render_markdown(&measured, baseline.as_ref(), &worsened),
    };
    Ok(GatedOutcome { rendered, regression: regression_message(&worsened) })
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
        return Ok(report::load_json::<TypednessReport>(&sidecar, "")?.programs);
    }
    let name = path.to_string_lossy().replace('\\', "/");
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return Ok(report::load_json::<TypednessReport>(path, "")?.programs);
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
    let config = config_parser::parse_path(manifest_path)?;
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

/// The gate's failure message, when any program became less typed.
fn regression_message(worsened: &[Worsening]) -> Option<String> {
    if worsened.is_empty() {
        return None;
    }
    let mut message = format!(
        "typedness regressed: {} program(s)/kind(s) became less typed than the baseline:",
        worsened.len()
    );
    for worsening in worsened {
        let _ = write!(message, "\n  {}: {}", worsening.program, worsening.detail);
    }
    message.push_str(
        "\nMake the program at least as typed again, or (only for a documented, \
         justified boundary) re-snapshot with --update-baseline. See the \
         `Typedness ratchet` policy in AGENTS.md.",
    );
    Some(message)
}

/// Serialize a report deterministically (pretty JSON, trailing newline).
fn render_json(report: &TypednessReport) -> CliResult<String> {
    let mut text = serde_json::to_string_pretty(report)?;
    text.push('\n');
    Ok(text)
}

/// Write a report to `path` only when its bytes change.
pub(crate) fn write_report_if_changed(path: &Path, report: &TypednessReport) -> CliResult<()> {
    report::write_if_changed(path, &render_json(report)?)
}

/// The three kinds, by name.
fn kinds(counts: &TypednessCounts) -> [(&'static str, KindCounts); 3] {
    [
        ("values", counts.values),
        ("reads", counts.reads),
        ("calls", counts.calls),
    ]
}

/// Why `now` is less typed than `then`, one line per kind whose erased count
/// rose; empty when `now` is no less typed. The typed share is deliberately
/// not judged (see the module docs, "The ratchet"): a share fall with erased
/// counts flat only means typed entries were removed.
pub(crate) fn worsening(now: &TypednessCounts, then: &TypednessCounts) -> Vec<String> {
    kinds(now)
        .into_iter()
        .zip(kinds(then))
        .filter(|((_, now), (_, then))| now.erased > then.erased)
        .map(|((kind, now), (_, then))| format!("{kind}: erased {} -> {}", then.erased, now.erased))
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
    report::truncated_share(counts.typed(), counts.total())
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

    /// Equal or fewer erased passes; any erased rise fails, per kind.
    #[test]
    fn erased_rise_is_less_typed() {
        assert_eq!(worsening(&counts(5, 1, 2), &counts(5, 1, 2)), Vec::<String>::new());
        assert_eq!(worsening(&counts(7, 1, 1), &counts(5, 1, 2)), Vec::<String>::new());
        // More values overall, same erased count.
        assert_eq!(worsening(&counts(10, 0, 2), &counts(5, 0, 2)), Vec::<String>::new());
        // Erased rose, even though the share rose too.
        let rose = worsening(&counts(90, 0, 3), &counts(5, 0, 2));
        assert_eq!(rose.len(), 3);
        assert_eq!(rose[0], "values: erased 2 -> 3");
        // Only one kind's erased count rose: only that kind is reported.
        let mut one = counts(5, 0, 2);
        one.reads = KindCounts::new(5, 0, 3);
        assert_eq!(worsening(&one, &counts(5, 0, 2)), vec!["reads: erased 2 -> 3".to_owned()]);
    }

    /// A typed-share fall with erased counts not rising is not less typed:
    /// removing redundant concrete temporaries (let-temp elision) must pass.
    /// Under the old rule (share may not fall) this case failed.
    #[test]
    fn share_fall_without_erased_rise_passes() {
        // 5 typed of 7 -> 1 typed of 3: share 71% -> 33%, erased flat.
        assert_eq!(worsening(&counts(1, 0, 2), &counts(5, 0, 2)), Vec::<String>::new());
        // Share fell and erased fell.
        assert_eq!(worsening(&counts(1, 0, 1), &counts(5, 0, 2)), Vec::<String>::new());
        // The update takes such a record.
        let baseline = TypednessReport::from_programs(BTreeMap::from([(
            "p.ts".to_owned(),
            Some(counts(5, 0, 2)),
        )]));
        let report = TypednessReport::from_programs(BTreeMap::from([(
            "p.ts".to_owned(),
            Some(counts(1, 0, 2)),
        )]));
        assert_eq!(compare(&report, &baseline), Vec::new());
        assert_eq!(updated_baseline(&report, Some(&baseline)).programs["p.ts"], Some(counts(1, 0, 2)));
    }

    /// A generic value counts as typed, exactly like a concrete one.
    #[test]
    fn generic_counts_as_typed() {
        assert_eq!(share(KindCounts::new(4, 2, 1)), share(KindCounts::new(6, 0, 1)));
        assert_eq!(share(KindCounts::new(4, 2, 1)), "85.7%");
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
