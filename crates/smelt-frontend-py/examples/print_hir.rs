//! Print the HIR for one or more Python source files.
//!
//! Usage:
//! ```bash
//! cargo run -p smelt-frontend-py --example py_print_hir -- [--debug] <file.py> [...]
//! ```

use std::{
    env,
    fmt::{self, Debug, Display},
    fs,
    io::{self, Write},
    path::Path,
};

use smelt_frontend_py::{HirCtx, to_hir};
use smelt_hir::{FileId, format_compact};

fn main() -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    let mut debug = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--debug" => debug = true,
            "--help" | "-h" => {
                writeln!(
                    stdout,
                    "usage: cargo run -p smelt-frontend-py --example py_print_hir -- [--debug] <file.py> [...]"
                )
                .map_err(|error| error.to_string())?;
                return Ok(());
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(
            "usage: cargo run -p smelt-frontend-py --example py_print_hir -- [--debug] <file.py> [...]"
                .to_owned(),
        );
    }

    let mut ctx = HirCtx::new();
    let mut loaded = Vec::new();

    for (idx, path) in paths.iter().enumerate() {
        let source =
            fs::read_to_string(path).map_err(|err| format!("failed to read {path}: {err}"))?;
        let file_id = u32::try_from(idx)
            .map(FileId)
            .map_err(|error| format!("file index does not fit in u32: {error}"))?;
        let module = to_hir(&source, file_id, &mut ctx)
            .map_err(|errors| format!("{}:\n{errors:#?}", Path::new(path).display()))?;
        loaded.push((path.clone(), module));
    }

    if debug {
        writeln!(stdout, "{}", DebugOutput(&ctx.krate)).map_err(|error| error.to_string())?;
    } else {
        write!(stdout, "{}", format_compact(&ctx.krate, &loaded))
            .map_err(|error| error.to_string())?;
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
