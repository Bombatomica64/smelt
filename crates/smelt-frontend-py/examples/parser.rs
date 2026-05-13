#![expect(
    clippy::str_to_string,
    reason = "example code keeps simple string conversion close to existing style"
)]
#![expect(
    clippy::use_debug,
    reason = "example intentionally prints the raw Ruff AST without summary mode"
)]
//! Parse a Python file with ruff and inspect the raw AST.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-py --example py_parser -- [--summary] <file.py>
//! ```
//! By default the full `{:#?}` AST is printed.
//! Pass `--summary` to print only the top-level statement count.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use pico_args::Arguments;
use smelt_frontend_py::parse_module;
use smelt_hir::FileId;

fn main() -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    let mut args = Arguments::from_env();

    let summary_only = args.contains("--summary");
    let name: String = args
        .free_from_str()
        .unwrap_or_else(|_| "test.py".to_string());

    let path = Path::new(&name);
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read '{}': {err}", path.display()))?;

    let module =
        parse_module(&source, FileId(0)).map_err(|errors| format!("parse errors:\n{errors:#?}"))?;

    if summary_only {
        writeln!(
            stdout,
            "{}: {} top-level statements",
            path.display(),
            module.body.len()
        )
        .map_err(|error| error.to_string())?;
    } else {
        writeln!(stdout, "{module:#?}").map_err(|error| error.to_string())?;
    }

    Ok(())
}
