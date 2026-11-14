//! The `smelt` binary: a thin entry over [`smelt_transpiler::cli::run`].

use std::io::Write as _;
use std::process::ExitCode;

/// Stack of the thread the CLI runs on.
///
/// Frontend lowering and MIR lowering recurse over expression trees, and a
/// type-level library's tests nest calls deeply enough (`match(x).with(..)`
/// chains of dozens of links, each with closure arguments) to overflow the
/// 8 MiB main-thread stack of a debug build. The checker thread already runs
/// on 512 MiB (`smelt-ts-types`); untouched stack pages cost no memory.
const CLI_STACK_BYTES: usize = 1 << 30;

/// Runs the CLI on a large-stack thread and reports an error the way a `main`
/// returning `Result` does: `Error: {error:?}` on stderr and exit status 1.
fn main() -> ExitCode {
    let outcome = std::thread::Builder::new()
        .name("smelt".to_owned())
        .stack_size(CLI_STACK_BYTES)
        .spawn(|| smelt_transpiler::cli::run().map_err(|error| format!("{error:?}")))
        .map_err(|error| format!("{error:?}"))
        .and_then(|thread| {
            thread
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(std::io::stderr().lock(), "Error: {error}");
            ExitCode::FAILURE
        }
    }
}
