//! The MIR stage as one call: lower HIR, optimize, validate.

use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};

use crate::opt::OptimizeError;
use crate::{LowerError, Mir, ValidationError, lower_hir, opt, validate};

/// Why [`compile`] produced no MIR.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompileError {
    /// HIR could not be lowered.
    Lower(Vec<LowerError>),
    /// Optimization did not converge, or (in debug and test builds) a pass
    /// left invalid MIR.
    Optimize(OptimizeError),
    /// The optimized MIR failed validation.
    Validation(Vec<ValidationError>),
}

impl fmt::Display for CompileError {
    /// Writes the failing stage and its errors.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // One line per error, its message and source byte range: an
            // internal limit reached by valid source reads as a diagnostic,
            // not as a dump of the error structure.
            Self::Lower(errors) => {
                write!(formatter, "MIR lowering failed:")?;
                for error in errors {
                    write!(formatter, "\n  {}", error.message)?;
                    if let Some(span) = error.span {
                        write!(formatter, " (bytes {}..{})", span.start, span.end)?;
                    }
                }
                Ok(())
            }
            Self::Optimize(error) => write!(formatter, "{error}"),
            Self::Validation(errors) => write!(formatter, "MIR validation failed:\n{errors:#?}"),
        }
    }
}

impl Error for CompileError {}

/// Lowers HIR to MIR, optimizes it, and validates the result.
///
/// # Errors
///
/// Returns [`CompileError`] when lowering, optimization or validation fails.
pub fn compile(krate: &smelt_hir::Crate) -> Result<Mir, CompileError> {
    compile_observed(krate, |_, _| {})
}

/// [`compile`], reporting each stage's label and wall-clock time to `observe`
/// as it finishes (`mir.lower_hir`, `mir.optimize`, `mir.validate`).
///
/// # Errors
///
/// Returns [`CompileError`] when lowering, optimization or validation fails.
pub fn compile_observed(
    krate: &smelt_hir::Crate,
    mut observe: impl FnMut(&'static str, Duration),
) -> Result<Mir, CompileError> {
    let start = Instant::now();
    let lowered = lower_hir(krate);
    observe("mir.lower_hir", start.elapsed());
    let mut mir = lowered.map_err(CompileError::Lower)?;

    let start = Instant::now();
    let optimized = opt::try_optimize(&mut mir);
    observe("mir.optimize", start.elapsed());
    optimized.map_err(CompileError::Optimize)?;

    let start = Instant::now();
    let errors = validate(&mir);
    observe("mir.validate", start.elapsed());
    if errors.is_empty() {
        Ok(mir)
    } else {
        Err(CompileError::Validation(errors))
    }
}
