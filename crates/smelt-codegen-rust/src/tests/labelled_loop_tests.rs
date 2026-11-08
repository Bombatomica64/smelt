//! Function-body loops left by more than one level, and the structural bound
//! that refuses a cycle the emitter cannot structure.
//!
//! A source `continue outer` makes the inner loop's body jump to the outer
//! loop's latch. The function emitter used to refuse to structure the inner
//! loop, emit its header as an `if`, and close the inner cycle with a bare
//! `continue` of the OUTER loop. The inner initialisation then re-ran forever
//! while the body kept pushing (the `labeled_statements` node-parity program
//! allocated without bound). These tests pin the labelled rendering and that an
//! unstructurable cycle is an error rather than code that loops or drops a
//! body.

use super::*;
use smelt_mir::{BlockId, Terminator};

/// `continue outer` runs the outer update and continues the OUTER loop by
/// label; the inner loop is a real loop of its own.
#[test]
fn continue_outer_through_the_latch_is_a_labelled_continue() {
    let source = source_for(
        r"
export function pairs(n: number): string {
  const out: string[] = [];
  outer: for (let a = 0; a < n; a++) {
    for (let b = 0; b < n; b++) {
      if (b > a) continue outer;
      out.push(`${a}${b}`);
    }
  }
  return out.join(',');
}
",
    );
    let body = emitted_function_body(&source, "fn pairs(");
    let label = body
        .lines()
        .find_map(|line| line.trim().strip_suffix(": loop {").map(str::to_owned))
        .unwrap_or_else(|| panic!("the outer loop carries a label:\n{body}"));
    let continue_outer = format!("continue {label};");
    assert!(body.contains(&continue_outer), "{body}");
    // The jump runs the outer update first: `a + 1.0` precedes it.
    let jump = body.find(&continue_outer).unwrap_or_default();
    assert!(body.get(..jump).is_some_and(|before| before.contains("a + 1.0")), "{body}");
    // The inner loop is a loop, so its own back edge has its own header.
    assert!(body.matches("loop {").count() + body.matches("while ").count() >= 2, "{body}");
}

/// `break outer` from two loops deep leaves the outer loop, and a loop no
/// edge names by label carries no label (no `unused_labels` warning).
#[test]
fn break_outer_leaves_the_outer_loop_and_unused_labels_are_not_written() {
    let source = source_for(
        r"
export function first(rows: number[][], target: number): number {
  let found = -1;
  search: while (found < 0) {
    for (const row of rows) {
      for (const x of row) {
        if (x === target) { found = x; break search; }
      }
    }
    break;
  }
  return found;
}

export function plain(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < i; j++) { total += j; }
  }
  return total;
}
",
    );
    let plain = emitted_function_body(&source, "fn plain(");
    assert!(!plain.contains("'smelt_loop_"), "{plain}");
    let first = emitted_function_body(&source, "fn first(");
    if let Some(label) = first
        .lines()
        .find_map(|line| line.trim().split_once(": loop {").map(|(label, _)| label.to_owned()))
    {
        assert!(first.contains(&format!("break {label};")) || first.contains(&format!("continue {label};")), "{first}");
    }
}

/// A cycle with two entries (irreducible: no block dominates it) cannot be
/// written as structured Rust. Emission answers an error naming the function;
/// it neither recurses until the stack is exhausted nor writes a default
/// `return` in place of the body.
#[test]
fn an_irreducible_cycle_is_an_emit_error() {
    let mut ctx = HirCtx::new();
    assert!(
        to_hir(
            "export function f(c: boolean): number { if (c) { return 1; } else { return 2; } }",
            FileId(0),
            &mut ctx,
        )
        .is_ok(),
        "HIR"
    );
    let Ok(mut mir) = smelt_mir::lower_hir(&ctx.krate) else {
        panic!("MIR lowering failed");
    };
    smelt_mir::opt::optimize(&mut mir);
    let function = mir
        .functions
        .iter_mut()
        .find(|function| {
            matches!(
                function.blocks.first().and_then(|block| block.terminator.as_ref()),
                Some(Terminator::Switch { .. })
            )
        })
        .unwrap_or_else(|| panic!("`f` branches in its entry block"));
    let Some(Terminator::Switch { then_block, else_block, .. }) =
        function.blocks.first().and_then(|block| block.terminator.clone())
    else {
        panic!("entry switch");
    };
    let retarget = |function: &mut smelt_mir::MirFunction, from: BlockId, to: BlockId| {
        if let Some(block) = function.blocks.iter_mut().find(|block| block.id == from) {
            block.terminator = Some(Terminator::Goto(to));
        }
    };
    retarget(function, then_block, else_block);
    retarget(function, else_block, then_block);
    let error = match emit_source(&mir) {
        Ok(source) => panic!("an irreducible cycle emitted:\n{}", emitted_function_body(&source, "fn f(")),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("could not be structured"), "{error}");
}
