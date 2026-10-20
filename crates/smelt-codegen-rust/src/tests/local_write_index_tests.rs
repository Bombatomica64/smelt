//! The precomputed per-function write index (`LocalWriteIndex`) must answer
//! exactly what the per-local, per-block scans it replaced answered.
//!
//! The index exists for compile-time scaling (see `compile_scaling.rs`); a
//! difference from the reference would silently change which bindings are
//! emitted `mut`, so the CFG half is checked block by block here and the
//! emitted text of the whole example corpus is checked by its goldens.

use super::*;
use crate::emitter::{EmitContext, FunctionEmitter};

/// Programs with every repeating shape: plain loops, nested loops, a loop
/// exited by `return`/`break`/`throw`, `try` inside a loop, `continue` in
/// a nested loop, a do-while, and straight-line code after a loop.
const SHAPES: &str = r"
export function shapes(items: number[], limit: number): number {
  let total = 0;
  for (const item of items) {
    if (item > limit) {
      break;
    }
    total += item;
  }
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      if (j === i) {
        continue;
      }
      total += j;
    }
  }
  let k = 0;
  do {
    k += 1;
    try {
      if (k === 2) {
        throw new Error('two');
      }
    } catch (error) {
      total -= 1;
    }
  } while (k < 4);
  while (true) {
    if (total > 10) {
      return total;
    }
    total += 5;
  }
}
export function straight(a: number): number {
  const b = a + 1;
  const c = b * 2;
  return c - a;
}
";

#[test]
fn repeat_reachable_matches_per_block_reference() {
    let mut ctx = HirCtx::new();
    if let Err(error) = to_hir(SHAPES, FileId(0), &mut ctx) {
        panic!("HIR: {error:?}");
    }
    let Ok(mut mir) = smelt_mir::lower_hir(&ctx.krate) else {
        panic!("MIR lowering failed");
    };
    smelt_mir::opt::optimize(&mut mir);
    let Ok(context) = EmitContext::new(&mir) else {
        panic!("emit context");
    };
    let mut repeating_functions = 0_usize;
    for function in &mir.functions {
        let Ok(emitter) = FunctionEmitter::new(&mir, &context, function) else {
            panic!("function emitter");
        };
        let (indexed, reference) = emitter.repeat_reachable_against_reference();
        assert_eq!(indexed, reference, "function {:?}", function.id);
        if !reference.is_empty() {
            repeating_functions += 1_usize;
        }
    }
    // The fixture must actually exercise cycles, or the comparison is vacuous.
    assert!(repeating_functions > 0_usize, "no function had a repeating region");
}
