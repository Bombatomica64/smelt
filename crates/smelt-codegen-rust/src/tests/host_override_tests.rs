//! Codegen tests for host-global override-slot prelude emission.

use super::*;
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Lower TypeScript to Rust with the crate-level written-host-global set seeded,
/// mirroring what the transpiler pre-pass does before lowering.
fn source_for_with_host_globals(ts: &str, names: &[&str]) -> String {
    let mut ctx = HirCtx::new();
    for name in names {
        ctx.written_host_globals.insert((*name).to_owned());
    }
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering failed");
    smelt_mir::opt::optimize(&mut mir);
    emit_source(&mir).expect("Rust source")
}

/// A crate that reassigns a modeled host global emits the fixed override enum,
/// a per-written-name `thread_local!` slot, and the three fixed helpers, and
/// lowers the presence guard to a dynamic slot probe.
#[test]
fn slot_and_helpers_emitted_for_written_name() {
    let source = source_for_with_host_globals(
        "export function probe(): boolean { return typeof Blob !== 'undefined'; }\n\
         export function disable() { globalThis.Blob = undefined; }\n",
        &["Blob"],
    );
    assert!(
        source.contains("enum SmeltHostOverride"),
        "expected the override enum, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_HOST_OVERRIDE_BLOB"),
        "expected the per-name override slot, got:\n{source}"
    );
    assert!(
        source.contains("fn smelt_host_override_present"),
        "expected the presence helper, got:\n{source}"
    );
    assert!(
        source.contains("fn smelt_host_override_write"),
        "expected the write helper, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_HOST_OVERRIDE_BLOB.with(smelt_host_override_present)"),
        "presence guard must lower to a dynamic slot probe, got:\n{source}"
    );
}

/// A crate that never reassigns a host global emits none of the override
/// machinery (pay-for-use): the `typeof Blob` guard folds and native `new Blob`
/// construction is byte-identical to before.
#[test]
fn gating_off_when_no_writes() {
    let source = source_for(
        "export function probe(): boolean { return typeof Blob !== 'undefined'; }\n\
         export function make(): unknown { return new Blob(['x']); }\n",
    );
    assert!(
        !source.contains("SmeltHostOverride"),
        "no override machinery may be emitted without a write, got:\n{source}"
    );
    assert!(
        !source.contains("SMELT_HOST_OVERRIDE_"),
        "no override slot may be emitted without a write, got:\n{source}"
    );
}
