//! Example binary that lowers source files and prints HIR output.
//!
//! Used for local debugging of frontend lowering behavior.

#![expect(
    clippy::cast_possible_truncation,
    reason = "example file IDs are small indexes from command-line arguments"
)]
#![expect(
    clippy::as_conversions,
    reason = "example file IDs are small indexes from command-line arguments"
)]
#![expect(
    clippy::use_debug,
    reason = "debug mode intentionally prints the raw HIR crate"
)]

use std::{
    env, fs,
    io::{self, Write},
    path::Path,
};

use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::{FileId, format_compact};

fn main() -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    let mut debug = false;
    let mut paths = Vec::new();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--debug" => debug = true,
            "--help" | "-h" => {
                writeln!(
                    stdout,
                    "usage: cargo run -p smelt-frontend-ts --example ts_print_hir -- [--debug] <file.ts> [...]"
                )
                .map_err(|error| error.to_string())?;
                return Ok(());
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(
            "usage: cargo run -p smelt-frontend-ts --example ts_print_hir -- [--debug] <file.ts> [...]"
                .to_owned(),
        );
    }

    let mut ctx = HirCtx::new();
    let mut loaded = Vec::new();

    for (idx, path) in paths.iter().enumerate() {
        let source =
            fs::read_to_string(path).map_err(|err| format!("failed to read {path}: {err}"))?;
        let module = to_hir(&source, FileId(idx as u32), &mut ctx)
            .map_err(|errors| format!("{}:\n{errors:#?}", Path::new(path).display()))?;
        loaded.push((path.clone(), module));
    }

    if debug {
        writeln!(stdout, "{:#?}", ctx.krate).map_err(|error| error.to_string())?;
    } else {
        write!(stdout, "{}", format_compact(&ctx.krate, &loaded))
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}
