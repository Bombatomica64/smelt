//! TypeScript 7 type facts for Smelt, resolved by the tsrs checker.
//!
//! # Why this crate exists
//!
//! Smelt's TypeScript frontend parses with oxc and historically reconstructed
//! types itself from annotations (`lowering/ty`): generics, narrowing,
//! overloads and guards were each re-derived by hand, and whatever that
//! reconstruction could not evaluate (a conditional type whose arms differ, a
//! mapped type, an inferred call-site instantiation) degraded to
//! `Type::Unknown`. TypeScript already computes all of those facts.
//!
//! tsrs (`oxc-project/tsrs`) is a function-by-function Rust port of the
//! TypeScript 7 checker (pinned to TypeScript 7.1.0-beta, matching all of
//! upstream's error and `.types` conformance baselines). This crate embeds it
//! as a library, the way `smelt-py-types` embeds `ty` for Python, and exposes
//! what it resolves as plain owned data:
//!
//! - [`TsTypeFacts::type_at`]: the checker's type for the node at an exact
//!   oxc span (expressions, identifiers, declaration names), with narrowing,
//!   contextual typing and generic inference already applied;
//! - [`TsTypeFacts::diagnostics`]: the program's TypeScript diagnostics, so
//!   the frontend can enforce "valid TypeScript" in-process instead of
//!   shelling out to `tsc`.
//!
//! # Batch model
//!
//! tsrs types are arena handles bound to the checker thread, and checking
//! recurses deeply (tsrs's CLI runs the checker on a 512 MiB stack). [`analyze`]
//! therefore builds the program, checks it, walks every file and converts all
//! types on one dedicated thread, then returns the owned [`TsTypeFacts`]. A
//! batch compiler asks for every expression's type anyway, so the eager walk
//! costs no more than lazy queries would.

mod convert;
mod diagnostics;
mod project;
mod walk;

use std::collections::HashMap;
use std::fmt;

pub use project::SourceInput;
pub use smelt_ts_model::{
    FileTypes, SpanKey, TsDiagnostic, TsDiagnosticCategory, TsType, TsTypeFacts, TsTypeId, TsTypeKind, TsTypeTable,
    model,
};

use crate::convert::Converter;

/// Stack size of the checker thread; matches the tsrs command line.
const CHECKER_STACK_BYTES: usize = 512 << 20;

/// Failure to produce type facts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TsTypesError {
    /// The generated `tsconfig.json` was rejected.
    Config(String),
    /// An input file is missing from the built program.
    MissingFile(String),
    /// The checker thread could not start or panicked.
    Checker(String),
}

impl fmt::Display for TsTypesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(formatter, "tsrs rejected the generated tsconfig: {message}"),
            Self::MissingFile(path) => write!(formatter, "tsrs program has no source file `{path}`"),
            Self::Checker(message) => write!(formatter, "tsrs checker failed: {message}"),
        }
    }
}

impl std::error::Error for TsTypesError {}

/// Type-checks `inputs` as one TypeScript program and collects its type facts.
///
/// # Errors
///
/// Returns [`TsTypesError`] when the program cannot be configured, an input
/// is missing from the program, or the checker thread fails.
pub fn analyze(inputs: Vec<SourceInput>) -> Result<TsTypeFacts, TsTypesError> {
    std::thread::Builder::new()
        .name("smelt-tsrs".to_owned())
        .stack_size(CHECKER_STACK_BYTES)
        .spawn(move || analyze_on_checker_thread(&inputs))
        .map_err(|error| TsTypesError::Checker(error.to_string()))?
        .join()
        .map_err(|panic| TsTypesError::Checker(panic_message(panic.as_ref())))?
}

/// Builds, checks and walks the program; runs on the checker thread.
fn analyze_on_checker_thread(inputs: &[SourceInput]) -> Result<TsTypeFacts, TsTypesError> {
    let program = project::build_program(inputs)?;
    let ctx = tsrs_compiler::Context::default();
    // Diagnostics first: they check every file, and they take the checker lock
    // themselves, so they must finish before the walk holds the checker.
    let diagnostics = diagnostics::collect(program, &ctx);
    let mut checker = program.get_type_checker(&ctx);
    let mut converter = Converter::new(&mut checker);
    let mut by_span = HashMap::new();
    let mut type_nodes = HashMap::new();
    for input in inputs {
        let program_path = input.program_path();
        let file = program
            .get_source_file(&program_path)
            .ok_or_else(|| TsTypesError::MissingFile(program_path.clone()))?;
        let collected = walk::collect_file_types(&mut converter, file);
        by_span.insert(input.path.clone(), collected.values);
        type_nodes.insert(input.path.clone(), collected.type_nodes);
    }
    let types = converter.finish();
    drop(checker);
    Ok(TsTypeFacts::new(types, diagnostics, by_span, type_nodes))
}

/// Best-effort text of a thread panic payload.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "checker thread panicked".to_owned())
}
