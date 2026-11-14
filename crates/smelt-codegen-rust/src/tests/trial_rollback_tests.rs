//! A discarded generic body-cleanliness trial must leave nothing behind.
//!
//! `EmitContext::populate_generic_functions` trial-renders every
//! signature-safe generic free function, possibly over several rounds, and
//! throws the rendered text away. Everything a render mints into the shared
//! context — function-item accessors, hoisted record helpers, the
//! `util.inspect` flag, backend erasure records — used to survive the trial
//! (see `emitter::ContextCheckpoint`).

use super::*;

/// Lower TypeScript to optimized MIR.
fn mir_for(ts: &str) -> Mir {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let Ok(mut mir) = smelt_mir::lower_hir(&ctx.krate) else {
        panic!("MIR lowering failed");
    };
    smelt_mir::opt::optimize(&mut mir);
    mir
}

/// A generic function that forwards to another generic function is erased in
/// the FIRST round of the generic fixed point (its callee is not generic yet),
/// and generic in the final answer. The first round's trial render used to
/// leave an `erased-generic T` record for `outer` in the backend erasure
/// report although the emitted `outer<T>` erases nothing — a phantom avoidable
/// erasure in `smelt-erasure.json`.
#[test]
fn discarded_generic_trial_records_no_backend_erasure() {
    let mir = mir_for(
        r"
export function inner<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return xs.filter(cb);
}
export function outer<T>(xs: T[], cb: (v: T) => boolean): T[] {
  return inner(xs, cb);
}
export function use1(ns: number[]): number[] {
  return outer(ns, (v: number) => v > 1);
}
",
    );
    let Ok(root) = crate::crate_builder::emit_root(
        &mir,
        GeneratedAllocator::System,
        Vec::new(),
        |_function, _context| crate::crate_builder::FreeFunctionHome::Root,
    ) else {
        panic!("emission failed");
    };
    let source = root.source();
    let erasures = root.erasures;
    // Both functions really are emitted generic …
    assert!(source.contains("fn inner<T: "), "{source}");
    assert!(source.contains("fn outer<T: "), "{source}");
    // … so no type parameter of theirs was erased by the emission.
    let erased_generics: Vec<_> = erasures
        .iter()
        .filter(|record| record.reason == smelt_hir::ErasureReason::ErasedGeneric)
        .collect();
    assert!(
        erased_generics.is_empty(),
        "backend erasures recorded by a discarded trial: {erased_generics:?}"
    );
}
