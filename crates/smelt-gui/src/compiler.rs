//! Compiler bridge — runs the full Smelt pipeline on two in-memory source
//! files (TypeScript + Python) and reports a structured [`CompileReport`].
//!
//! TypeScript is lowered first (as `lib.ts`), then Python (as `main.py`), into a
//! single shared HIR crate so that cross-language imports like
//! `from lib import add` resolve exactly as in manifest-driven CLI builds. The
//! report captures the generated Rust source on success, a readable list of
//! diagnostics on failure, and a per-stage pass/fail/skip trace of the pipeline
//! derived from where the run stopped.

use std::time::{Duration, Instant};

use smelt_hir::FileId;

/// Virtual path for the TypeScript source file.
pub(super) const TS_PATH: &str = "src/lib.ts";

/// Virtual path for the Python source file.
pub(super) const PY_PATH: &str = "src/main.py";

/// One stage of the shared Smelt pipeline, in run order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// TypeScript source to HIR.
    TypeScriptLowering,
    /// Python source to HIR, into the same crate.
    PythonLowering,
    /// HIR to MIR.
    MirLowering,
    /// MIR optimization passes.
    MirOptimize,
    /// MIR validation.
    MirValidate,
    /// MIR to Rust source.
    RustCodegen,
}

impl Stage {
    /// Every stage in pipeline order; the pipeline view lists exactly these.
    pub(super) const ALL: [Self; 6] = [
        Self::TypeScriptLowering,
        Self::PythonLowering,
        Self::MirLowering,
        Self::MirOptimize,
        Self::MirValidate,
        Self::RustCodegen,
    ];

    /// Human-readable stage name.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::TypeScriptLowering => "TypeScript lowering",
            Self::PythonLowering => "Python lowering",
            Self::MirLowering => "MIR lowering",
            Self::MirOptimize => "MIR optimize",
            Self::MirValidate => "MIR validate",
            Self::RustCodegen => "Rust codegen",
        }
    }
}

/// Where a compilation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailurePoint {
    /// The input was rejected before any stage ran (both editors empty).
    Input,
    /// A pipeline stage failed.
    Stage(Stage),
}

impl FailurePoint {
    /// Label shown next to a diagnostic from this point.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Stage(stage) => stage.name(),
        }
    }
}

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

/// One readable diagnostic message, attributed to where it was produced.
#[derive(Debug, Clone)]
pub(super) struct DiagnosticMessage {
    /// Where the compilation stopped when this diagnostic was produced.
    pub(super) origin: FailurePoint,
    /// The error text.
    pub(super) message: String,
}

impl DiagnosticMessage {
    /// Label of the stage (or input check) that emitted this diagnostic.
    pub(super) const fn stage_label(&self) -> &'static str {
        self.origin.label()
    }
}

/// A failed compilation: where it stopped and why.
#[derive(Debug, Clone)]
pub(super) struct Failure {
    /// Where the compilation stopped.
    pub(super) at: FailurePoint,
    /// Readable diagnostics, one per underlying compiler error, all from `at`.
    pub(super) diagnostics: Vec<DiagnosticMessage>,
}

impl Failure {
    /// A failure at `at`, with one diagnostic per error formatted by `Debug`.
    ///
    /// Splitting the underlying error vector into separate messages keeps the
    /// diagnostics view readable instead of one large blob.
    fn debug<E: std::fmt::Debug>(at: Stage, errors: impl IntoIterator<Item = E>) -> Self {
        Self::messages(FailurePoint::Stage(at), errors.into_iter().map(|error| format!("{error:?}")))
    }

    /// A failure at `at` carrying the given messages.
    fn messages(at: FailurePoint, messages: impl IntoIterator<Item = String>) -> Self {
        Self {
            at,
            diagnostics: messages
                .into_iter()
                .map(|message| DiagnosticMessage { origin: at, message })
                .collect(),
        }
    }
}

/// Which editors had source when the compilation ran.
#[derive(Debug, Clone, Copy)]
struct Inputs {
    /// The TypeScript editor had non-blank source.
    ts: bool,
    /// The Python editor had non-blank source.
    py: bool,
}

/// Structured result of a single compilation.
#[derive(Debug, Clone)]
pub(super) struct CompileReport {
    /// Generated Rust source, or where and why the compilation failed.
    pub(super) outcome: Result<String, Failure>,
    /// Which editors had source; decides which frontend stages were skipped.
    inputs: Inputs,
    /// Wall-clock duration of the whole compilation.
    pub(super) duration: Duration,
}

impl CompileReport {
    /// Whether the compilation produced Rust output.
    pub(super) const fn is_ok(&self) -> bool {
        self.outcome.is_ok()
    }

    /// The generated Rust source, present only on success.
    pub(super) fn rust_source(&self) -> Option<&str> {
        self.outcome.as_deref().ok()
    }

    /// Returns the generated Rust source, or an empty string if compilation
    /// failed. Convenient for populating the read-only output editor.
    pub(super) fn rust_or_empty(&self) -> String {
        self.rust_source().unwrap_or_default().to_owned()
    }

    /// Readable diagnostics; empty on success.
    pub(super) fn diagnostics(&self) -> &[DiagnosticMessage] {
        self.outcome.as_ref().map_or_else(|failure| failure.diagnostics.as_slice(), |_| &[])
    }

    /// Returns the name of the failing stage, if a stage failed.
    pub(super) fn failing_stage(&self) -> Option<&'static str> {
        match &self.outcome {
            Err(Failure { at: FailurePoint::Stage(stage), .. }) => Some(stage.name()),
            Ok(_) | Err(Failure { at: FailurePoint::Input, .. }) => None,
        }
    }

    /// Per-stage pass/fail/skip trace, derived from the outcome.
    ///
    /// Stages before the failing one passed (or were skipped for lack of
    /// input), the failing stage failed, and every later stage was skipped. A
    /// failure at the input check skips every stage.
    pub(super) fn stages(&self) -> Vec<PipelineStageReport> {
        let failed_at = match &self.outcome {
            Ok(_) => None,
            Err(failure) => Some(failure.at),
        };
        let mut reached_failure = matches!(failed_at, Some(FailurePoint::Input));
        Stage::ALL
            .into_iter()
            .map(|stage| {
                let status = if reached_failure {
                    StageStatus::Skipped
                } else if failed_at == Some(FailurePoint::Stage(stage)) {
                    reached_failure = true;
                    StageStatus::Failed
                } else if self.stage_lacks_input(stage) {
                    StageStatus::Skipped
                } else {
                    StageStatus::Passed
                };
                PipelineStageReport { name: stage.name(), status }
            })
            .collect()
    }

    /// Whether `stage` is a frontend stage whose editor was empty.
    const fn stage_lacks_input(&self, stage: Stage) -> bool {
        match stage {
            Stage::TypeScriptLowering => !self.inputs.ts,
            Stage::PythonLowering => !self.inputs.py,
            Stage::MirLowering | Stage::MirOptimize | Stage::MirValidate | Stage::RustCodegen => {
                false
            }
        }
    }
}

/// Runs the full pipeline and returns a structured [`CompileReport`].
///
/// The two source languages are merged into one HIR crate before MIR lowering,
/// so a single Rust program is emitted with one entrypoint. The stage trace is
/// derived from where the run stopped (see [`CompileReport::stages`]).
pub(super) fn compile_report(ts_source: &str, py_source: &str) -> CompileReport {
    let start = Instant::now();
    let inputs = Inputs { ts: !ts_source.trim().is_empty(), py: !py_source.trim().is_empty() };
    let outcome = run_pipeline(ts_source, py_source, inputs);
    CompileReport { outcome, inputs, duration: start.elapsed() }
}

/// Runs every stage in order, stopping at the first failure.
fn run_pipeline(ts_source: &str, py_source: &str, inputs: Inputs) -> Result<String, Failure> {
    if !inputs.ts && !inputs.py {
        return Err(Failure::messages(
            FailurePoint::Input,
            ["Both editors are empty — add TypeScript or Python source to compile.".to_owned()],
        ));
    }

    // ── TypeScript lowering ─────────────────────────────────────────
    let mut ts_ctx = smelt_frontend_ts::HirCtx::new();
    if inputs.ts {
        let module = smelt_frontend_ts::to_hir_with_path(ts_source, FileId(0), TS_PATH, &mut ts_ctx)
            .map_err(|errors| Failure::debug(Stage::TypeScriptLowering, errors))?;
        // Rename the TS module body away from `main` so the Python entrypoint
        // owns `fn main` when both languages are present.
        if inputs.py {
            rename_module(&mut ts_ctx.krate, module, "lib");
        }
    }

    // ── Python lowering ─────────────────────────────────────────────
    let krate = if inputs.py {
        lower_python(ts_ctx.krate, py_source)?
    } else {
        ts_ctx.krate
    };

    // ── MIR lowering, optimize, validate ────────────────────────────
    let mir = smelt_mir::compile(&krate).map_err(|error| match error {
        smelt_mir::CompileError::Lower(errors) => Failure::debug(Stage::MirLowering, errors),
        smelt_mir::CompileError::Validation(errors) => Failure::debug(Stage::MirValidate, errors),
        other => Failure::messages(FailurePoint::Stage(Stage::MirLowering), [format!("{other}")]),
    })?;

    // ── Rust codegen ────────────────────────────────────────────────
    smelt_codegen_rust::emit_source(&mir).map_err(|error| {
        Failure::messages(FailurePoint::Stage(Stage::RustCodegen), [format!("{error}")])
    })
}

/// Lowers the Python source into the crate the TypeScript stage produced.
///
/// The Python frontend lives behind the `python` feature (see Cargo.toml): a
/// TS-only build reports Python input as a stage failure rather than lowering it.
#[cfg(feature = "python")]
fn lower_python(krate: smelt_hir::Crate, py_source: &str) -> Result<smelt_hir::Crate, Failure> {
    let mut py_ctx = smelt_frontend_py::HirCtx {
        krate,
        module_namespaces: std::collections::HashMap::new(),
        enum_members: std::collections::HashMap::new(),
    };
    let module = smelt_frontend_py::to_hir_with_path(py_source, FileId(1), PY_PATH, &mut py_ctx)
        .map_err(|errors| Failure::debug(Stage::PythonLowering, errors))?;
    rename_module(&mut py_ctx.krate, module, "main");
    Ok(py_ctx.krate)
}

/// Reports Python input as unsupported in a build without the `python` feature.
#[cfg(not(feature = "python"))]
fn lower_python(_krate: smelt_hir::Crate, _py_source: &str) -> Result<smelt_hir::Crate, Failure> {
    Err(Failure::messages(
        FailurePoint::Stage(Stage::PythonLowering),
        ["Python support is not compiled into this build of smelt. \
          Rebuild from a source checkout with `--features python` to \
          compile Python source."
            .to_owned()],
    ))
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
            report.is_ok(),
            "TS-only compile failed: {:?}",
            report.diagnostics()
        );
        let rust = report.rust_source().expect("rust output");
        assert!(rust.contains("fn main"), "expected fn main: {rust}");
    }

    #[test]
    fn cross_import_compiles_with_one_main() {
        let ts = "export function add(a: number, b: number): number {\n    return a + b;\n}\n";
        let py = "from lib import add\n\nresult: float = add(2.0, 3.0)\nprint(result)\n";
        let report = compile_report(ts, py);
        assert!(report.is_ok(), "cross-import failed: {:?}", report.diagnostics());
        let rust = report.rust_source().expect("rust output");
        assert_eq!(
            rust.matches("fn main(").count(),
            1,
            "expected exactly one entrypoint: {rust}"
        );
    }

    #[test]
    fn ts_only_success_skips_the_python_stage() {
        let report = compile_report("let x: number = 1;\nconsole.log(x);\n", "");
        let statuses = report.stages().iter().map(|stage| stage.status).collect::<Vec<_>>();
        assert_eq!(
            statuses,
            [
                StageStatus::Passed,
                StageStatus::Skipped,
                StageStatus::Passed,
                StageStatus::Passed,
                StageStatus::Passed,
                StageStatus::Passed,
            ]
        );
        assert!(report.diagnostics().is_empty());
    }

    #[test]
    fn empty_input_reports_readable_diagnostic() {
        let report = compile_report("", "   \n");
        assert!(!report.is_ok());
        assert!(
            report
                .diagnostics()
                .iter()
                .any(|d| d.message.contains("empty")),
            "expected an empty-input diagnostic: {:?}",
            report.diagnostics()
        );
        // Nothing ran, so every stage is skipped and none is blamed.
        assert!(report.stages().iter().all(|stage| stage.status == StageStatus::Skipped));
        assert_eq!(report.failing_stage(), None);
        assert_eq!(report.diagnostics()[0].stage_label(), "input");
    }

    #[test]
    fn failure_marks_a_stage() {
        // Deliberately broken TypeScript should fail at the first stage.
        let report = compile_report("function (((", "");
        assert!(!report.is_ok());
        assert_eq!(report.failing_stage(), Some(Stage::TypeScriptLowering.name()));
        let statuses = report.stages().iter().map(|stage| stage.status).collect::<Vec<_>>();
        assert_eq!(
            statuses,
            [
                StageStatus::Failed,
                StageStatus::Skipped,
                StageStatus::Skipped,
                StageStatus::Skipped,
                StageStatus::Skipped,
                StageStatus::Skipped,
            ]
        );
        assert!(
            report
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.stage_label() == "TypeScript lowering")
        );
    }

    #[test]
    fn every_showcase_example_compiles() {
        for example in examples::EXAMPLES {
            let report = compile_report(example.ts_source, example.py_source);
            assert!(
                report.is_ok(),
                "showcase example `{}` failed to compile: {:?}",
                example.id, report.diagnostics()
            );
        }
    }
}
