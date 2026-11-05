//! The `smelt` binary: a thin entry over [`smelt_transpiler::cli::run`].

fn main() -> smelt_transpiler::CliResult<()> {
    smelt_transpiler::cli::run()
}
