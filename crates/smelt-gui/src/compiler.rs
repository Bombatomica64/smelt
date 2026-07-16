//! Compiler bridge — runs the full Smelt pipeline on two in-memory source
//! files (TypeScript + Python) and reports a structured [`CompileReport`].
//!
//! TypeScript is lowered first (as `lib.ts`), then Python (as `main.py`), into a
//! single shared HIR crate so that cross-language imports like
//! `from lib import add` resolve exactly as in manifest-driven CLI builds. The
//! report captures the generated Rust source on success, a readable list of
//! diagnostics on failure, and a per-stage pass/fail/skip trace of the pipeline.

use std::time::{Duration, Instant};

use smelt_hir::FileId;

/// Virtual path for the TypeScript source file.
pub(super) const TS_PATH: &str = "src/lib.ts";

/// Virtual path for the Python source file.
pub(super) const PY_PATH: &str = "src/main.py";

/// Ordered list of pipeline stage names, used to pad skipped stages on failure.
const STAGE_NAMES: [&str; 6] = [
    "TypeScript lowering",
    "Python lowering",
    "MIR lowering",
    "MIR optimize",
    "MIR validate",
    "Rust codegen",
];

/// Outcome of a single pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StageStatus {
    /// Stage ran and succeeded.
    Passed,
    /// Stage ran and failed (the cause is in [`CompileReport::diagnostics`]).
    Failed,
    /// Stage was not reached (an earlier stage failed) or had no input.
    Skipped,
}

/// A single named pipeline stage and its outcome.
#[derive(Debug, Clone)]
pub(super) struct PipelineStageReport {
    /// Human-readable stage name.
    pub(super) name: &'static str,
    /// Whether the stage passed, failed, or was skipped.
    pub(super) status: StageStatus,
}

/// One readable diagnostic message, attributed to the stage that produced it.
#[derive(Debug, Clone)]
pub(super) struct DiagnosticMessage {
    /// Pipeline stage that emitted this diagnostic.
    pub(super) stage: &'static str,
    /// The error text.
    pub(super) message: String,
}

/// Structured result of a single compilation.
#[derive(Debug, Clone)]
pub(super) struct CompileReport {
    /// Generated Rust source, present only on success.
    pub(super) rust_source: Option<String>,
    /// Readable diagnostics, one per underlying compiler error.
    pub(super) diagnostics: Vec<DiagnosticMessage>,
    /// Per-stage pass/fail/skip trace.
    pub(super) stages: Vec<PipelineStageReport>,
    /// Wall-clock duration of the whole compilation.
    pub(super) duration: Duration,
    /// Whether the compilation produced Rust output.
    pub(super) ok: bool,
}

impl CompileReport {
    /// Returns the generated Rust source, or an empty string if compilation
    /// failed. Convenient for populating the read-only output editor.
    pub(super) fn rust_or_empty(&self) -> String {
        self.rust_source.clone().unwrap_or_default()
    }

    /// Returns the name of the first failing stage, if any.
    pub(super) fn failing_stage(&self) -> Option<&'static str> {
        self.stages
            .iter()
            .find(|s| s.status == StageStatus::Failed)
            .map(|s| s.name)
    }
}

/// Runs the full pipeline and returns a structured [`CompileReport`].
///
/// Stages are recorded as they run so the UI can show exactly where a failing
/// compilation stopped. The two source languages are merged into one HIR crate
/// before MIR lowering, so a single Rust program is emitted with one entrypoint.
#[expect(
    clippy::too_many_lines,
    reason = "linear stage-by-stage pipeline driver"
)]
pub(super) fn compile_report(ts_source: &str, py_source: &str) -> CompileReport {
    let start = Instant::now();
    let mut stages: Vec<PipelineStageReport> = Vec::with_capacity(STAGE_NAMES.len());
    let mut diagnostics: Vec<DiagnosticMessage> = Vec::new();

    let ts_has = !ts_source.trim().is_empty();
    let py_has = !py_source.trim().is_empty();

    if !ts_has && !py_has {
        diagnostics.push(DiagnosticMessage {
            stage: "input",
            message: "Both editors are empty — add TypeScript or Python source to compile."
                .to_owned(),
        });
        return bail(stages, diagnostics, 0, start);
    }

    // ── Stage 1: TypeScript lowering ────────────────────────────────
    let mut ts_ctx = smelt_frontend_ts::HirCtx::new();
    if ts_has {
        match smelt_frontend_ts::to_hir_with_path(ts_source, FileId(0), TS_PATH, &mut ts_ctx) {
            Ok(module) => {
                // Rename the TS module body away from `main` so the Python
                // entrypoint owns `fn main` when both languages are present.
                if py_has {
                    rename_module(&mut ts_ctx.krate, module, "lib");
                }
                stages.push(passed(0));
            }
            Err(errors) => {
                push_debug_diags(&mut diagnostics, STAGE_NAMES[0], errors);
                stages.push(failed(0));
                return bail(stages, diagnostics, 1, start);
            }
        }
    } else {
        stages.push(skipped(0));
    }

    // ── Stage 2: Python lowering ────────────────────────────────────
    // The Python frontend lives behind the `python` feature (see Cargo.toml):
    // a TS-only build reports Python input as a stage failure rather than
    // lowering it.
    let krate = if py_has {
        #[cfg(feature = "python")]
        {
            let mut py_ctx = smelt_frontend_py::HirCtx {
                krate: ts_ctx.krate,
                module_namespaces: std::collections::HashMap::new(),
                enum_members: std::collections::HashMap::new(),
            };
            match smelt_frontend_py::to_hir_with_path(py_source, FileId(1), PY_PATH, &mut py_ctx) {
                Ok(module) => {
                    rename_module(&mut py_ctx.krate, module, "main");
                    stages.push(passed(1));
                    py_ctx.krate
                }
                Err(errors) => {
                    push_debug_diags(&mut diagnostics, STAGE_NAMES[1], errors);
                    stages.push(failed(1));
                    return bail(stages, diagnostics, 2, start);
                }
            }
        }
        #[cfg(not(feature = "python"))]
        {
            diagnostics.push(DiagnosticMessage {
                stage: STAGE_NAMES[1],
                message: "Python support is not compiled into this build of smelt. \
                          Rebuild from a source checkout with `--features python` to \
                          compile Python source."
                    .to_owned(),
            });
            stages.push(failed(1));
            return bail(stages, diagnostics, 2, start);
        }
    } else {
        stages.push(skipped(1));
        ts_ctx.krate
    };

    // ── Stage 3: MIR lowering ───────────────────────────────────────
    let mut mir = match smelt_mir::lower_hir(&krate) {
        Ok(mir) => {
            stages.push(passed(2));
            mir
        }
        Err(errors) => {
            push_debug_diags(&mut diagnostics, STAGE_NAMES[2], errors);
            stages.push(failed(2));
            return bail(stages, diagnostics, 3, start);
        }
    };

    // ── Stage 4: MIR optimize ───────────────────────────────────────
    smelt_mir::opt::optimize(&mut mir);
    stages.push(passed(3));

    // ── Stage 5: MIR validate ───────────────────────────────────────
    let validation_errors = smelt_mir::validate(&mir);
    if validation_errors.is_empty() {
        stages.push(passed(4));
    } else {
        push_debug_diags(&mut diagnostics, STAGE_NAMES[4], validation_errors);
        stages.push(failed(4));
        return bail(stages, diagnostics, 5, start);
    }

    // ── Stage 6: Rust codegen ───────────────────────────────────────
    match smelt_codegen_rust::emit_source(&mir) {
        Ok(rust) => {
            stages.push(passed(5));
            CompileReport {
                rust_source: Some(rust),
                diagnostics,
                stages,
                duration: start.elapsed(),
                ok: true,
            }
        }
        Err(e) => {
            diagnostics.push(DiagnosticMessage {
                stage: STAGE_NAMES[5],
                message: format!("{e}"),
            });
            stages.push(failed(5));
            bail(stages, diagnostics, 6, start)
        }
    }
}

/// Builds a passed stage report for the stage at `index`.
const fn passed(index: usize) -> PipelineStageReport {
    PipelineStageReport {
        name: STAGE_NAMES[index],
        status: StageStatus::Passed,
    }
}

/// Builds a failed stage report for the stage at `index`.
const fn failed(index: usize) -> PipelineStageReport {
    PipelineStageReport {
        name: STAGE_NAMES[index],
        status: StageStatus::Failed,
    }
}

/// Builds a skipped stage report for the stage at `index`.
const fn skipped(index: usize) -> PipelineStageReport {
    PipelineStageReport {
        name: STAGE_NAMES[index],
        status: StageStatus::Skipped,
    }
}

/// Finalizes a failed compilation, padding any not-yet-reached stages (from
/// `next_index` onward) as skipped so the pipeline view stays complete.
fn bail(
    mut stages: Vec<PipelineStageReport>,
    diagnostics: Vec<DiagnosticMessage>,
    next_index: usize,
    start: Instant,
) -> CompileReport {
    for name in STAGE_NAMES.iter().skip(next_index) {
        stages.push(PipelineStageReport {
            name,
            status: StageStatus::Skipped,
        });
    }
    CompileReport {
        rust_source: None,
        diagnostics,
        stages,
        duration: start.elapsed(),
        ok: false,
    }
}

/// Appends one diagnostic per error, formatted via `Debug`, attributed to
/// `stage`. Splitting the underlying error vector into separate messages keeps
/// the diagnostics view readable instead of one large blob.
fn push_debug_diags<E: std::fmt::Debug>(
    diagnostics: &mut Vec<DiagnosticMessage>,
    stage: &'static str,
    errors: impl IntoIterator<Item = E>,
) {
    for error in errors {
        diagnostics.push(DiagnosticMessage {
            stage,
            message: format!("{error:?}"),
        });
    }
}

/// Rename a lowered module body before MIR lowering chooses Rust function names.
fn rename_module(krate: &mut smelt_hir::Crate, module_id: smelt_hir::ModuleId, name: &str) {
    if let Ok(index) = usize::try_from(module_id.0)
        && let Some(module) = krate.modules.get_mut(index)
    {
        name.clone_into(&mut module.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::examples;

    #[test]
    fn ts_only_compiles() {
        let report = compile_report("let x: number = 42;\nconsole.log(x);\n", "");
        assert!(
            report.ok,
            "TS-only compile failed: {:?}",
            report.diagnostics
        );
        let rust = report.rust_source.expect("rust output");
        assert!(rust.contains("fn main"), "expected fn main: {rust}");
    }

    #[test]
    fn cross_import_compiles_with_one_main() {
        let ts = "export function add(a: number, b: number): number {\n    return a + b;\n}\n";
        let py = "from lib import add\n\nresult: float = add(2.0, 3.0)\nprint(result)\n";
        let report = compile_report(ts, py);
        assert!(report.ok, "cross-import failed: {:?}", report.diagnostics);
        let rust = report.rust_source.expect("rust output");
        assert_eq!(
            rust.matches("fn main(").count(),
            1,
            "expected exactly one entrypoint: {rust}"
        );
    }

    #[test]
    fn empty_input_reports_readable_diagnostic() {
        let report = compile_report("", "   \n");
        assert!(!report.ok);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.message.contains("empty")),
            "expected an empty-input diagnostic: {:?}",
            report.diagnostics
        );
    }

    #[test]
    fn failure_marks_a_stage() {
        // Deliberately broken TypeScript should fail at the first stage.
        let report = compile_report("function (((", "");
        assert!(!report.ok);
        assert!(
            report.failing_stage().is_some(),
            "expected a failing stage to be marked: {:?}",
            report.stages
        );
    }

    #[test]
    fn every_showcase_example_compiles() {
        for example in examples::EXAMPLES {
            let report = compile_report(example.ts_source, example.py_source);
            assert!(
                report.ok,
                "showcase example `{}` failed to compile: {:?}",
                example.id, report.diagnostics
            );
        }
    }
}
