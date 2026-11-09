//! Checks a finished crate must pass before it is written.
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible items are shared with the crate root and the emitter shards"
)]

use crate::EmitError;
use crate::runtime_symbols_check;

/// Fails the build when a crate's sources name a runtime symbol nothing
/// defines.
///
/// That is a missing edge in the `requires` graph of [`runtime_demand`], and
/// failing here names it where it was made rather than as an E0425 in the
/// generated crate. It runs in every build profile: the published binary and
/// the runtime-tier suite are release builds, and the check is one lexical
/// scan of text the emitter has just produced.
///
/// [`runtime_demand`]: crate::runtime_demand
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
