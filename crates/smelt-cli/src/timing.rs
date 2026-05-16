//! Optional wall-clock timing helpers for CLI profiling.

use std::{io::Write as _, time::Instant};

/// Measures one CLI phase when `SMELT_TIMINGS` is enabled.
pub(crate) fn measure<T, E>(label: &str, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    if std::env::var_os("SMELT_TIMINGS").is_none() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    let _ignored = writeln!(
        std::io::stderr().lock(),
        "smelt timing: {label}: {:.3}s",
        start.elapsed().as_secs_f64()
    );
    result
}
