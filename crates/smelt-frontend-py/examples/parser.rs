//! Parse a Python file with ruff and inspect the raw AST.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-py --example py_parser -- [--summary] <file.py>
//! ```
//! By default the full `{:#?}` AST is printed.
//! Pass `--summary` to print only the top-level statement count.

use std::{
    fmt::{self, Debug, Display},
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
        .unwrap_or_else(|_| "test.py".to_owned());

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
        writeln!(stdout, "{}", DebugOutput(&module)).map_err(|error| error.to_string())?;
    }

    Ok(())
}

/// Displays a value through its debug formatter without using debug format strings.
struct DebugOutput<'a, T>(&'a T);

impl<T: Debug> Display for DebugOutput<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(self.0, formatter)
    }
}
