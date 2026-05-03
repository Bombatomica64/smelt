#![expect(clippy::print_stdout, reason = "example binary")]
//! Parse a Python file with ruff and inspect the raw AST.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-py --example parser -- [--summary] <file.py>
//! ```
//! By default the full `{:#?}` AST is printed.
//! Pass `--summary` to print only the top-level statement count.

use std::{fs, path::Path};

use pico_args::Arguments;
use smelt_frontend_py::parse_module;
use smelt_hir::FileId;

fn main() -> Result<(), String> {
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
        println!(
            "{}: {} top-level statements",
            path.display(),
            module.body.len()
        );
    } else {
        println!("{module:#?}");
    }

    Ok(())
}
