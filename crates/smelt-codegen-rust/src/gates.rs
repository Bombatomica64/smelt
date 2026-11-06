//! Checks a finished crate must pass before it is written.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

use crate::EmitError;
#[cfg(debug_assertions)]
use crate::runtime_symbols_check;

/// Fails the build, in debug builds only, when a crate's sources name a runtime
/// symbol nothing defines.
///
/// That is a missing edge in the `requires` graph of [`runtime_demand`], and
/// failing here names it where it was made rather than as an E0425 in the
/// generated crate.
#[cfg(debug_assertions)]
pub(crate) fn assert_runtime_symbols_defined<'a>(sources: impl IntoIterator<Item = &'a str>) -> Result<(), EmitError> {
    let undefined = runtime_symbols_check::undefined_runtime_symbols(sources);
    if undefined.is_empty() {
        return Ok(());
    }
    Err(EmitError::new(format!(
        "generated crate names runtime symbols with no definition: {}",
        undefined.join(", ")
    )))
}

/// Release builds trust the `requires` graph.
#[cfg(not(debug_assertions))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shares its signature with the debug build's check"
)]
pub(crate) fn assert_runtime_symbols_defined<'a>(_sources: impl IntoIterator<Item = &'a str>) -> Result<(), EmitError> {
    Ok(())
}
