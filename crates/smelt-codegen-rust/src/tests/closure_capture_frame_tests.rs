//! Capture frames of nested closures, asserted at the MIR level.
//!
//! A closure nested two levels deep may read a local of its GRANDPARENT frame.
//! Two rules make that representable, and both are asserted here because the
//! generated Rust cannot express a violation of either:
//!
//! 1. **Threading.** Every intermediate closure captures the grandparent local
//!    too, so the innermost capture has something to name in the frame that
//!    immediately encloses it.
//! 2. **Frame locality.** A capture's `source_local` always names a local of
//!    that immediately enclosing frame — a parameter, a body local, or that
//!    frame's own capture — never a local id resolved further out.
//!
//! Before rule 1, a name read only inside a nested `function` DECLARATION was
//! invisible to the capture scan of the frame containing the declaration (the
//! scan walked function and arrow EXPRESSIONS but not declarations). The
//! declaration's own capture then resolved in a further-out frame, and two
//! different values — a grandparent parameter and a parent parameter, both
//! local `%0` of their own frames — collided on one source local id. Codegen
//! could not tell them apart, which is why this is asserted on the capture
//! lists rather than on emitted text.

use super::*;
use smelt_mir::{Mir, MirClosure};

/// Lower TypeScript to MIR the way the pipeline does, without optimization.
///
/// The capture lists are what is under test, so the optimizer is deliberately
/// not run: this asserts what lowering produced.
fn mir_for(ts: &str) -> Mir {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(err) => panic!("MIR lowering failed: {err:?}"),
    }
}

/// Return the names of a closure's captured locals, in capture order.
fn capture_names(mir: &Mir, closure: &MirClosure) -> Vec<String> {
    closure
        .captures
        .iter()
        .map(|capture| {
            mir.symbols
                .get(capture.symbol)
                .unwrap_or("<unnamed>")
                .to_owned()
        })
        .collect()
}

/// Assert that every capture names a local of the frame that encloses it.
///
/// The enclosing frame of `ClosureId(n)` is whichever body holds the
/// `Rvalue::Closure` that builds it; rather than rediscover that here, the
/// weaker but sufficient invariant is checked: within ONE closure, no two
/// captures of different values share a source local id.
fn capture_sources_are_distinct(closure: &MirClosure) -> bool {
    let mut seen = std::collections::HashMap::new();
    closure.captures.iter().all(|capture| {
        seen.insert(capture.source_local, capture.symbol)
            .is_none_or(|previous| previous == capture.symbol)
    })
}

#[test]
fn nested_function_declaration_threads_a_grandparent_capture() {
    let mir = mir_for(
        r"
const makeWalker = (steps: string[]) => (label: string): string => {
  return walk(0)
  function walk(i: number): string {
    if (i >= steps.length) {
      return label
    }
    return steps[i] + '>' + walk(i + 1)
  }
}
console.log(makeWalker(['a'])('end'))
",
    );

    // The outer arrow builds the middle arrow; the middle arrow builds `walk`.
    let outer = mir
        .closures
        .iter()
        .find(|closure| capture_names(&mir, closure).is_empty())
        .expect("the outermost arrow captures nothing");
    assert!(outer.captures.is_empty());

    let middle = mir
        .closures
        .iter()
        .find(|closure| capture_names(&mir, closure) == ["steps"])
        .unwrap_or_else(|| {
            panic!(
                "the intermediate closure must capture `steps` for its nested \
                 `walk` to have anything to name; captures were {:?}",
                mir.closures
                    .iter()
                    .map(|closure| capture_names(&mir, closure))
                    .collect::<Vec<_>>()
            )
        });
    assert!(capture_sources_are_distinct(middle));

    let inner = mir
        .closures
        .iter()
        .find(|closure| {
            let names = capture_names(&mir, closure);
            names.contains(&"steps".to_owned()) && names.contains(&"label".to_owned())
        })
        .unwrap_or_else(|| {
            panic!(
                "`walk` must capture both the grandparent's `steps` and the \
                 parent's `label`; captures were {:?}",
                mir.closures
                    .iter()
                    .map(|closure| capture_names(&mir, closure))
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        capture_sources_are_distinct(inner),
        "two captured values shared one source local id: {:?}",
        inner
            .captures
            .iter()
            .map(|capture| (capture.source_local, capture.symbol))
            .collect::<Vec<_>>()
    );
    // `walk` is self-recursive, so it also captures its own binding.
    assert!(capture_names(&mir, inner).contains(&"walk".to_owned()));
}

#[test]
fn every_closure_capture_has_a_distinct_source_local() {
    let mir = mir_for(
        r"
const makeJoiner = (parts: string[]) => (glue: string): string => {
  return join()
  function join(): string {
    let out = ''
    for (const part of parts) {
      out = out === '' ? part : out + glue + part
    }
    return out
  }
}
console.log(makeJoiner(['x'])('-'))
",
    );

    for closure in &mir.closures {
        assert!(
            capture_sources_are_distinct(closure),
            "closure {:?} records two values under one source local: {:?}",
            closure.id,
            closure
                .captures
                .iter()
                .map(|capture| (capture.source_local, capture.symbol))
                .collect::<Vec<_>>()
        );
    }

    let inner = mir
        .closures
        .iter()
        .find(|closure| {
            let names = capture_names(&mir, closure);
            names.contains(&"parts".to_owned()) && names.contains(&"glue".to_owned())
        })
        .expect("the non-recursive nested declaration captures both frames' locals");
    assert!(capture_sources_are_distinct(inner));
}

#[test]
fn three_level_arrow_nesting_keeps_each_frames_own_local() {
    let mir = mir_for(
        r"
const tag = (prefix: string) => (suffix: string) => (body: string) =>
  prefix + body + suffix
console.log(tag('<')('>')('mid'))
",
    );

    let inner = mir
        .closures
        .iter()
        .find(|closure| {
            let names = capture_names(&mir, closure);
            names.contains(&"prefix".to_owned()) && names.contains(&"suffix".to_owned())
        })
        .expect("the innermost arrow captures both enclosing parameters");
    assert!(
        capture_sources_are_distinct(inner),
        "`prefix` and `suffix` must not collide on one source local"
    );

    let middle = mir
        .closures
        .iter()
        .find(|closure| capture_names(&mir, closure) == ["prefix"])
        .expect("the intermediate arrow threads `prefix` through");
    assert!(capture_sources_are_distinct(middle));
}

#[test]
fn distinct_captures_get_distinct_generated_bindings() {
    // The emitted names are the observable half of the same rule: every closure
    // parameter is emitted as `closure_arg_{index}`, so a three-deep closure
    // captures its parent's `closure_arg_0` AND its grandparent's — distinct
    // values under one identifier unless the capture prelude disambiguates.
    let source = source_for(
        r"
const tag = (prefix: string) => (suffix: string) => (body: string) =>
  prefix + body + suffix
console.log(tag('<')('>')('mid'))
",
    );
    let bindings = source
        .lines()
        .filter(|line| line.trim_start().starts_with("let smelt_captured_"))
        .map(|line| line.trim().to_owned())
        .collect::<Vec<_>>();
    let names = bindings
        .iter()
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
        .collect::<Vec<_>>();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        names.len(),
        unique.len(),
        "capture preludes reused one binding name: {bindings:?}"
    );
}
