//! Compiler bridge — wraps the Smelt pipeline for dual-source transpilation.
//!
//! Lowers TypeScript first (as `lib.ts`), then Python (as `main.py`), into a
//! single shared HIR crate so that cross-language imports like
//! `from lib import add` work exactly as in manifest-driven CLI builds.

use smelt_hir::FileId;

/// Virtual path for the TypeScript source file.
pub(super) const TS_PATH: &str = "src/lib.ts";

/// Virtual path for the Python source file.
pub(super) const PY_PATH: &str = "src/main.py";

/// Compilation result.
#[derive(Debug, Clone)]
pub(super) enum CompileResult {
    /// Generated Rust source code.
    Ok(String),
    /// One or more errors.
    Err(String),
}

/// Runs the full Smelt pipeline on two in-memory source files (TS + Python),
/// lowering them into a single shared HIR crate so exports are visible across
/// languages.
pub(super) fn compile(ts_source: &str, py_source: &str) -> CompileResult {
    let ts_has_content = !ts_source.trim().is_empty();
    let py_has_content = !py_source.trim().is_empty();

    if !ts_has_content && !py_has_content {
        return CompileResult::Err("Both editors are empty.".to_owned());
    }

    let krate = match lower_combined(ts_source, ts_has_content, py_source, py_has_content) {
        Ok(k) => k,
        Err(msg) => return CompileResult::Err(msg),
    };

    let mut mir = match smelt_mir::lower_hir(&krate) {
        Ok(mir) => mir,
        Err(errors) => {
            let msg = errors
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join("\n");
            return CompileResult::Err(format!("MIR lowering failed:\n{msg}"));
        }
    };

    smelt_mir::opt::optimize(&mut mir);

    let validation_errors = smelt_mir::validate(&mir);
    if !validation_errors.is_empty() {
        let msg = validation_errors
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        return CompileResult::Err(format!("MIR validation failed:\n{msg}"));
    }

    match smelt_codegen_rust::emit_source(&mir) {
        Ok(rust) => CompileResult::Ok(rust),
        Err(e) => CompileResult::Err(format!("Codegen failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_only_compiles() {
        let ts = "let x: number = 42;\nconsole.log(x);\n";
        let result = compile(ts, "");
        match &result {
            CompileResult::Ok(rust) => assert!(
                rust.contains("fn main"),
                "expected fn main in output: {rust}"
            ),
            CompileResult::Err(e) => panic!("TS-only compile failed: {e}"),
        }
    }

    #[test]
    fn cross_import_compiles() {
        let ts = "export function add(a: number, b: number): number {\n    return a + b;\n}\n";
        let py = "from lib import add\n\nresult: float = add(2.0, 3.0)\nprint(result)\n";
        let result = compile(ts, py);
        match &result {
            CompileResult::Ok(rust) => {
                assert!(rust.contains("fn main"), "expected fn main: {rust}");
                assert!(rust.contains("println!"), "expected println: {rust}");
            }
            CompileResult::Err(e) => panic!("Cross-import compile failed: {e}"),
        }
    }
}

/// Lower TS then Python into one shared HIR crate, mirroring the CLI's
/// `lower_ordered_manifest_sources` flow. Passes the path so that
/// `from lib import X` resolves the TS module by its file stem.
fn lower_combined(
    ts_source: &str,
    ts_has_content: bool,
    py_source: &str,
    py_has_content: bool,
) -> Result<smelt_hir::Crate, String> {
    let mut ts_ctx = smelt_frontend_ts::HirCtx::new();

    if ts_has_content {
        smelt_frontend_ts::to_hir_with_path(ts_source, FileId(0), TS_PATH, &mut ts_ctx).map_err(
            |errors| {
                errors
                    .iter()
                    .map(|e| format!("{e:?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        )?;
    }

    if py_has_content {
        let mut py_ctx = smelt_frontend_py::HirCtx {
            krate: ts_ctx.krate,
            module_namespaces: std::collections::HashMap::new(),
            enum_members: std::collections::HashMap::new(),
        };
        smelt_frontend_py::to_hir_with_path(py_source, FileId(1), PY_PATH, &mut py_ctx).map_err(
            |errors| {
                errors
                    .iter()
                    .map(|e| format!("{e:?}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        )?;
        Ok(py_ctx.krate)
    } else {
        Ok(ts_ctx.krate)
    }
}
