//! Crate integration tests for Rust codegen.

use super::*;
use smelt_frontend_py as py_frontend;
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;
use smelt_stdlib::BackendDependency;

/// Converts TypeScript source to generated Rust source.
fn source_for(ts: &str) -> String {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    }
}

/// Converts Python source to generated Rust source.
fn source_for_py(py: &str) -> String {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(py_frontend::to_hir(py, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    }
}

/// Converts Python source at `path` to generated Rust source.
fn source_for_py_path(py: &str, path: &str) -> String {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
        py_frontend::to_hir_with_path(py, FileId(0), path, &mut ctx).is_ok(),
        "HIR"
    );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    }
}

mod part_1_tests;
mod part_2_tests;
mod part_3_tests;
mod part_4_tests;
mod part_5_tests;
mod part_6_tests;
mod part_7_tests;
mod generics_tests;
mod reference_class_tests;
mod module_globals_tests;
mod snapshot_tests;
mod snapshot_tests_part_2;
mod host_override_tests;
mod tail_r3_tests;
mod tail_r7_tests;
