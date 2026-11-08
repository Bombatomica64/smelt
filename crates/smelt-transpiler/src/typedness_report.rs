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
//! program the baseline measured that is now refused is a regression, and so
//! is a baseline program that was not measured at all: a program that vanished
//! from the inputs left the ratchet without anyone deciding it should. With
//! `--update-baseline`, the baseline is rewritten taking each program's new
//! record only when it is no less typed, so a record whose erased count rose
//! can never be snapshotted silently, and a vanished program keeps its record.
//! A program that is gone on purpose is named with `--retire-program`, which
//! stops judging it and, with `--update-baseline`, removes its record.
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
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use smelt_codegen_rust::{KindCounts, TypednessCounts};

use crate::{
    CliResult, config_parser, lowering,
    pipeline,
    report::{self, GatedOutcome, ReportFormat, Schema, SchemaVersion, push_fmt},
    source_files::{exclusion_base, manifest_relative_path},
};

/// File name of the sidecar `smelt build` writes next to the generated `src/`.
pub(crate) const SIDECAR_FILE_NAME: &str = "smelt-typedness.json";

/// One program's record: its counts, or `None` when it failed to lower.
pub(crate) type ProgramRecord = Option<TypednessCounts>;

/// A program's name in a report: a `/`-separated path with no `.` component,
/// and `..` folded where it can be, so `./x.ts`, `x.ts` and `a/../x.ts` are one
/// program, however the command line or a baseline spelled it.
///
/// Only [`Self::new`] builds one, and deserializing goes through it, so a
/// baseline written with another spelling still matches.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct ProgramKey(String);

impl ProgramKey {
    /// The key of the program at `path`.
    pub(crate) fn new(path: &str) -> Self {
        let absolute = path.starts_with('/');
        let mut parts: Vec<&str> = Vec::new();
        for part in path.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." if parts.last().is_some_and(|last| *last != "..") => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        let joined = parts.join("/");
        Self(if absolute { format!("/{joined}") } else { joined })
    }

    /// The key as text.
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for ProgramKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProgramKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Programs by key; two spellings of one program are an error, never an
/// overwrite.
pub(crate) type Programs = BTreeMap<ProgramKey, ProgramRecord>;

/// Add `added` to `programs`.
///
/// # Errors
///
/// When a program of `added` is already in `programs` (the same program given
/// twice, perhaps spelled two ways): one record would silently replace the other.
fn merge_programs(programs: &mut Programs, added: Programs) -> Result<(), String> {
    for (program, record) in added {
        if programs.contains_key(&program) {
            return Err(format!("program `{program}` is measured twice; each program may be given once"));
        }
        programs.insert(program, record);
    }
    Ok(())
}

/// Deserialize a report's programs, normalizing every key ([`ProgramKey::new`])
/// and refusing two spellings of one program.
fn unique_programs<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Programs, D::Error> {
    let spelled = BTreeMap::<String, ProgramRecord>::deserialize(deserializer)?;
    let mut programs = Programs::new();
    for (spelling, record) in spelled {
        merge_programs(&mut programs, BTreeMap::from([(ProgramKey::new(&spelling), record)]))
            .map_err(serde::de::Error::custom)?;
    }
    Ok(programs)
}

/// A typedness report: the corpus total and every program's record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TypednessReport {
    /// Format version; reading another is an error.
    pub(crate) version: SchemaVersion<Self>,
    /// Sum over every measured program.
    pub(crate) total: TypednessCounts,
    /// Per-program records, by name; `null` marks a refused program.
    #[serde(deserialize_with = "unique_programs")]
    pub(crate) programs: Programs,
}

impl Schema for TypednessReport {
    const NAME: &'static str = "typedness report";
    const VERSION: u32 = 1;
}

impl TypednessReport {
    /// Build a report from program records, computing the total.
    pub(crate) fn from_programs(programs: Programs) -> Self {
        let mut total = TypednessCounts::default();
        for counts in programs.values().flatten() {
            total.merge(*counts);
        }
        Self {
            version: SchemaVersion::CURRENT,
            total,
            programs,
        }
    }
}

/// Options for one report run.
pub(crate) struct TypednessReportOptions<'a> {
    /// Inputs; see the module docs.
    pub(crate) paths: &'a [PathBuf],
    /// Command-line defines, over a manifest's.
    pub(crate) defines: &'a [(String, String)],
    /// Output format.
    pub(crate) format: ReportFormat,
    /// Baseline to compare against.
    pub(crate) baseline: Option<PathBuf>,
    /// Rewrite the baseline with every record that is no less typed.
    pub(crate) update_baseline: bool,
    /// Baseline programs that are gone on purpose.
    pub(crate) retired: &'a [String],
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
    let mut programs = Programs::new();
    for path in options.paths {
        merge_programs(&mut programs, measure_input(path, options.defines)?)?;
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
        .map(|baseline| compare(&measured, baseline, options.retired))
        .unwrap_or_default();
    if options.update_baseline
        && let Some(path) = &options.baseline
    {
        let updated = updated_baseline(&measured, baseline.as_ref(), options.retired);
        write_report_if_changed(path, &updated)?;
    }
    let rendered = match options.format {
        ReportFormat::Json => render_json(&measured)?,
        ReportFormat::Markdown => render_markdown(&measured, baseline.as_ref(), &worsened),
    };
    Ok(GatedOutcome { rendered, regression: regression_message(&worsened) })
}

/// Measure one input path into program records.
fn measure_input(path: &Path, defines: &[(String, String)]) -> CliResult<Programs> {
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
        return measure_manifest(path, defines);
    }
    // A single source program: the whole lowered crate is its record.
    let record = lowering::ManifestLowering::for_file(&name)
        .and_then(|lowering| lowering.check()?.into_measured().lower())
        .ok()
        .and_then(|lowered| pipeline::lower_to_optimized_mir(&lowered.krate).ok())
        .and_then(|mir| smelt_codegen_rust::measure_typedness(&mir).ok())
        .map(|measurement| measurement.total());
    Ok(BTreeMap::from([(ProgramKey::new(&name), record)]))
}

/// Lower a manifest in-process and measure every source file.
fn measure_manifest(
    manifest_path: &Path,
    defines: &[(String, String)],
) -> CliResult<Programs> {
    let mut config = config_parser::parse_path(manifest_path)?;
    config.extend_defines(defines.iter().cloned());
    let krate = lowering::ManifestLowering::for_manifest(&config, manifest_path)?.check()?.into_measured().lower()?.krate;
    let mir = pipeline::lower_to_optimized_mir(&krate)?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(programs_of(&smelt_codegen_rust::measure_typedness(&mir)?, manifest_dir))
}

/// Per-file records of a measurement, keyed by manifest-relative path.
pub(crate) fn programs_of(
    measurement: &smelt_codegen_rust::TypednessMeasurement,
    manifest_dir: &Path,
) -> Programs {
    let base = exclusion_base(manifest_dir);
    let mut programs = Programs::new();
    for (file, counts) in &measurement.files {
        let relative = manifest_relative_path(Path::new(file), &base);
        let key = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let entry = programs.entry(ProgramKey::new(&key)).or_insert(Some(TypednessCounts::default()));
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
        push_fmt(&mut message, format_args!("\n  {}: {}", worsening.program, worsening.detail));
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

/// Baseline programs absent from `report` and not named in `retired` (in any
/// spelling of their path).
fn vanished<'baseline>(
    report: &TypednessReport,
    baseline: &'baseline TypednessReport,
    retired: &[String],
) -> impl Iterator<Item = (&'baseline ProgramKey, &'baseline ProgramRecord)> {
    let measured = report.programs.keys().collect::<BTreeSet<_>>();
    let retired_keys = retired.iter().map(|program| ProgramKey::new(program)).collect::<BTreeSet<_>>();
    baseline
        .programs
        .iter()
        .filter(move |(program, _)| !measured.contains(program) && !retired_keys.contains(*program))
}

/// Compare a report with its baseline: every judged program, every baseline
/// program that vanished without being retired, and the total.
pub(crate) fn compare(
    report: &TypednessReport,
    baseline: &TypednessReport,
    retired: &[String],
) -> Vec<Worsening> {
    let mut worsened = Vec::new();
    for (program, _) in vanished(report, baseline, retired) {
        worsened.push(Worsening {
            program: program.to_string(),
            detail: "in the baseline but no longer measured (retire it with --retire-program if \
                     it is gone on purpose)"
                .to_owned(),
        });
    }
    for (program, record) in &report.programs {
        let Some(then) = baseline.programs.get(program) else {
            continue;
        };
        match (record, then) {
            (Some(now), Some(then)) => {
                for detail in worsening(now, then) {
                    worsened.push(Worsening {
                        program: program.to_string(),
                        detail,
                    });
                }
            }
            (None, Some(_)) => worsened.push(Worsening {
                program: program.to_string(),
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
/// program gets one, a program no longer measured keeps its record until it is
/// named in `retired`, which alone removes one.
pub(crate) fn updated_baseline(
    report: &TypednessReport,
    baseline: Option<&TypednessReport>,
    retired: &[String],
) -> TypednessReport {
    let mut programs: Programs = report
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
    if let Some(baseline) = baseline {
        programs.extend(
            vanished(report, baseline, retired).map(|(program, record)| (program.clone(), *record)),
        );
    }
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
    push_fmt(&mut out, format_args!("# Typedness report\n"));
    out.push('\n');
    let measured = report.programs.values().filter(|record| record.is_some()).count();
    push_fmt(
        &mut out,
        format_args!(
            "- Programs: {} ({measured} measured, {} refused)\n",
            report.programs.len(),
            report.programs.len().saturating_sub(measured)
        ),
    );
    out.push('\n');
    push_fmt(&mut out, format_args!("| Kind | Concrete | Generic | Erased | Typed share |\n"));
    push_fmt(&mut out, format_args!("| --- | ---: | ---: | ---: | ---: |\n"));
    for (kind, counts) in kinds(&report.total) {
        push_fmt(
            &mut out,
            format_args!(
                "| {kind} | {} | {} | {} | {} |\n",
                counts.concrete,
                counts.generic,
                counts.erased,
                share(counts)
            ),
        );
    }
    out.push('\n');
    if let Some(baseline) = baseline {
        push_fmt(&mut out, format_args!("## Change vs baseline\n"));
        out.push('\n');
        push_fmt(
            &mut out,
            format_args!(
                "| Kind | Baseline share | Current share | Erased delta |\n"
            ),
        );
        push_fmt(&mut out, format_args!("| --- | ---: | ---: | ---: |\n"));
        for ((kind, now), (_, then)) in kinds(&report.total).into_iter().zip(kinds(&baseline.total)) {
            let delta = i128::from(now.erased as u64) - i128::from(then.erased as u64);
            push_fmt(
                &mut out,
                format_args!(
                    "| {kind} | {} | {} | {delta:+} |\n", share(then), share(now)
                ),
            );
        }
        out.push('\n');
        if worsened.is_empty() {
            push_fmt(
                &mut out,
                format_args!(
                    "No program became less typed than its baseline record.\n"
                ),
            );
        } else {
            push_fmt(&mut out, format_args!("**Less typed than the baseline:**\n"));
            out.push('\n');
            for worsening in worsened {
                push_fmt(
                    &mut out,
                    format_args!(
                        "- `{}`: {}\n", worsening.program, worsening.detail
                    ),
                );
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "a test fails fast on what it set up")]
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
            ProgramKey::new("p.ts"),
            Some(counts(5, 0, 2)),
        )]));
        let report = TypednessReport::from_programs(BTreeMap::from([(
            ProgramKey::new("p.ts"),
            Some(counts(1, 0, 2)),
        )]));
        assert_eq!(compare(&report, &baseline, &[]), Vec::new());
        assert_eq!(updated_baseline(&report, Some(&baseline), &[]).programs["p.ts"], Some(counts(1, 0, 2)));
    }

    /// A baseline program that is no longer measured fails the gate, and keeps
    /// its record through `--update-baseline`, until it is retired by name.
    #[test]
    fn a_vanished_program_is_judged_and_never_erased_silently() {
        let baseline = TypednessReport::from_programs(BTreeMap::from([
            (ProgramKey::new("kept.ts"), Some(counts(5, 0, 1))),
            (ProgramKey::new("gone.ts"), Some(counts(5, 0, 1))),
        ]));
        let report = TypednessReport::from_programs(BTreeMap::from([(
            ProgramKey::new("kept.ts"),
            Some(counts(5, 0, 1)),
        )]));

        let worsened = compare(&report, &baseline, &[]);
        assert_eq!(worsened.len(), 1, "{worsened:?}");
        assert_eq!(worsened[0].program, "gone.ts");
        let updated = updated_baseline(&report, Some(&baseline), &[]);
        assert_eq!(updated.programs["gone.ts"], Some(counts(5, 0, 1)));
        assert_eq!(updated.total, baseline.total, "a kept record still counts in the total");

        let retired = ["gone.ts".to_owned()];
        assert_eq!(compare(&report, &baseline, &retired), Vec::new());
        let updated = updated_baseline(&report, Some(&baseline), &retired);
        assert!(!updated.programs.contains_key("gone.ts"), "{updated:?}");
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
            (ProgramKey::new("a.ts"), Some(counts(5, 0, 1))),
            (ProgramKey::new("b.ts"), Some(counts(5, 0, 1))),
        ]));
        let report = TypednessReport::from_programs(BTreeMap::from([
            (ProgramKey::new("a.ts"), Some(counts(5, 0, 2))),
            (ProgramKey::new("b.ts"), None),
            (ProgramKey::new("new.ts"), Some(counts(0, 0, 9))),
        ]));
        let worsened = compare(&report, &baseline, &[]);
        let programs = worsened.iter().map(|w| w.program.as_str()).collect::<Vec<_>>();
        assert!(programs.contains(&"a.ts"));
        assert!(programs.contains(&"b.ts"));
        assert!(!programs.contains(&"new.ts"));
    }

    /// The update keeps the old record of a worsened or vanished program and
    /// takes the rest.
    #[test]
    fn update_never_snapshots_a_regression() {
        let baseline = TypednessReport::from_programs(BTreeMap::from([
            (ProgramKey::new("worse.ts"), Some(counts(5, 0, 1))),
            (ProgramKey::new("better.ts"), Some(counts(5, 0, 1))),
            (ProgramKey::new("gone.ts"), Some(counts(1, 0, 0))),
        ]));
        let report = TypednessReport::from_programs(BTreeMap::from([
            (ProgramKey::new("worse.ts"), Some(counts(5, 0, 3))),
            (ProgramKey::new("better.ts"), Some(counts(6, 0, 0))),
            (ProgramKey::new("new.ts"), Some(counts(2, 0, 0))),
        ]));
        let updated = updated_baseline(&report, Some(&baseline), &[]);
        assert_eq!(updated.programs["worse.ts"], Some(counts(5, 0, 1)));
        assert_eq!(updated.programs["better.ts"], Some(counts(6, 0, 0)));
        assert_eq!(updated.programs["new.ts"], Some(counts(2, 0, 0)));
        assert_eq!(updated.programs["gone.ts"], Some(counts(1, 0, 0)));
    }

    /// Every spelling of one path is one program.
    #[test]
    fn program_keys_do_not_depend_on_spelling() {
        for spelling in ["x/input.ts", "./x/input.ts", "x//input.ts", "x/./input.ts", "y/../x/input.ts", "x\\input.ts"] {
            assert_eq!(ProgramKey::new(spelling).as_str(), "x/input.ts", "{spelling}");
        }
        assert_eq!(ProgramKey::new("../a.ts").as_str(), "../a.ts");
        assert_eq!(ProgramKey::new("/abs/./a.ts").as_str(), "/abs/a.ts");
    }

    /// Two spellings of one program, given twice or written into one baseline,
    /// are an error instead of one record silently replacing the other.
    #[test]
    fn a_program_measured_twice_is_an_error() {
        let mut programs = Programs::new();
        merge_programs(&mut programs, BTreeMap::from([(ProgramKey::new("x.ts"), None)])).expect("first");
        let error = merge_programs(&mut programs, BTreeMap::from([(ProgramKey::new("./x.ts"), None)]))
            .expect_err("a duplicate is refused");
        assert!(error.contains("x.ts"), "{error}");
        let text = r#"{"version":1,"total":{"values":{"concrete":0,"generic":0,"erased":0},"reads":{"concrete":0,"generic":0,"erased":0},"calls":{"concrete":0,"generic":0,"erased":0}},"programs":{"./x.ts":null,"x.ts":null}}"#;
        let parsed = serde_json::from_str::<TypednessReport>(text).expect_err("duplicate spellings").to_string();
        assert!(parsed.contains("measured twice"), "{parsed}");
    }
}
