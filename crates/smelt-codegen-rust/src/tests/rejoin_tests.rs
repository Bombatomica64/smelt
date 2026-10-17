//! Branches and `try`s rejoin: their shared continuation is emitted once.
//!
//! Requirement (behaviour is unchanged, emission is structured): for `n`
//! sequential two-way branches in a closure body, or `n` sequential
//! `try`/`catch` statements in a function body, the code after them appears
//! ONCE in the generated Rust, and the emitted size grows linearly in `n`.
//! Before the rejoin rules each branch re-emitted the whole rest of the body
//! per path (`2^n` copies; `3^n` for a `try`, whose panic-routed arm repeats
//! the `catch` continuation) — Hono's `utils/cookie.test.ts` produced a 616 MB
//! module and v5's `compose` hit the closure emitter's unstructured-flow panic.

use super::*;

/// The program text after the runtime prelude.
fn program_body(source: &str) -> &str {
    source
        .split_once("// @smelt:prelude-end")
        .map_or(source, |(_, program)| program)
}

/// `n` sequential `if`/`else` statements inside a closure that captures a
/// local, followed by one marker statement.
fn closure_with_sequential_branches(n: usize) -> String {
    let mut body = String::new();
    for index in 0..n {
        body.push_str(&format!(
            "    if (a > {index}) {{ total += {index}; }} else {{ total -= {index}; }}\n"
        ));
    }
    format!(
        r"
export function run(seed: number): number {{
  let total = seed;
  const step = (a: number): number => {{
{body}    total = total * 7919;
    return total;
  }};
  return step(seed);
}}
"
    )
}

/// `n` sequential `try`/`catch` statements in one function, followed by one
/// marker statement.
fn function_with_sequential_trys(n: usize) -> String {
    let mut body = String::new();
    for index in 0..n {
        body.push_str(&format!(
            "  try {{ total += risky(total, {index}); }} catch (error) {{ total -= {index}; }}\n"
        ));
    }
    format!(
        r"
function risky(value: number, index: number): number {{
  if (value > index * 3) {{
    throw new Error('too big');
  }}
  return index;
}}

export function run(seed: number): number {{
  let total = seed;
{body}  total = total * 7919;
  return total;
}}
"
    )
}

#[test]
fn closure_branches_emit_their_shared_tail_once() {
    let source = source_for(&closure_with_sequential_branches(6));
    let program = program_body(&source);
    assert_eq!(
        program.matches("* 7919.0").count(),
        1,
        "the statement after six sequential branches must be emitted once:\n{program}"
    );
    assert!(
        !program.contains("recursive closure control flow is not structured yet"),
        "{program}"
    );
}

#[test]
fn function_trys_emit_their_shared_tail_once() {
    let source = source_for(&function_with_sequential_trys(5));
    let program = program_body(&source);
    assert_eq!(
        program.matches("* 7919.0").count(),
        1,
        "the statement after five sequential try/catch statements must be emitted once:\n{program}"
    );
}

#[test]
fn rejoined_emission_grows_linearly() {
    // Doubling the branch count must not square (or worse) the output: with
    // per-path emission, 8 branches are 16x the text of 4, not ~2x.
    let closure_small = program_body(&source_for(&closure_with_sequential_branches(4))).len();
    let closure_large = program_body(&source_for(&closure_with_sequential_branches(8))).len();
    assert!(
        closure_large * 10 < closure_small * 25,
        "closure body: {closure_small} bytes for 4 branches, {closure_large} for 8"
    );
    let trys_small = program_body(&source_for(&function_with_sequential_trys(4))).len();
    let trys_large = program_body(&source_for(&function_with_sequential_trys(8))).len();
    assert!(
        trys_large * 10 < trys_small * 25,
        "function body: {trys_small} bytes for 4 try statements, {trys_large} for 8"
    );
}
