#![expect(clippy::print_stdout, reason = "example binary")]
#![expect(
    clippy::cast_possible_truncation,
    reason = "example file IDs are small indexes from command-line arguments"
)]
//! Print the HIR for one or more Python source files.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-py --example print_hir -- [--debug] <file.py> [...]
//! ```

use std::{env, fs, path::Path};

use smelt_frontend_py::{HirCtx, to_hir};
use smelt_hir::{FileId, format_compact};

fn main() -> Result<(), String> {
    let mut debug = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--debug" => debug = true,
            "--help" | "-h" => {
                println!(
                    "usage: cargo run -p smelt-frontend-py --example print_hir -- [--debug] <file.py> [...]"
                );
                return Ok(());
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(
            "usage: cargo run -p smelt-frontend-py --example print_hir -- [--debug] <file.py> [...]"
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
        println!("{:#?}", ctx.krate);
    } else {
        print!("{}", format_compact(&ctx.krate, &loaded));
    }

    Ok(())
}
