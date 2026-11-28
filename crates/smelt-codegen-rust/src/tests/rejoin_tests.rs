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
//!
//! The same requirement holds for loops whose body leaves them other than
//! through their exit (`loop_exit_join`): `n` sequential `for...of` loops over
//! generators (each a `try`/`finally` whose `next()` call's handler jumps past
//! the loop to the clause), in a function, a closure, or the body of an
//! enclosing loop, and `n` sequential `try { while (..) { .. } .. } finally`.
//! Each early exit re-emitted the rest of the body (`4^n` copies in a function,
//! `3^n` in a closure): 6 loops took ~90 s to transpile.
//!
//! And for every other branch whose arms meet only transitively
//! (`branch_rejoin_programs`): a `switch` whose `case` calls a function,
//! throws or returns, a `try` in a loop body, an `if`/`else` holding a call
//! inside a `try`; in a function, a loop body, a closure and a forward `if`
//! arm. `n` sequential three-way `switch` statements with a call in a `case`
//! rendered the tail `3^n` times.

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

/// A generator and the function `run` whose body is `n` copies of `statement`
/// (inside `wrap`, which holds one `{}` for them), then one marker statement.
fn generator_loops(n: usize, statement: &str, wrap: &str) -> String {
    let body = wrap.replace("{}", &statement.repeat(n));
    format!(
        r"
function* digits(limit: number): Generator<number> {{
  for (let digit = 0; digit < limit; digit++) {{
    yield digit;
  }}
}}

function risky(value: number): number {{
  if (value > 1000) {{
    throw new Error('too big');
  }}
  return value;
}}

export function run(seed: number): number {{
  let total = seed;
  let i = 0;
{body}
  total = total * 7919;
  return total;
}}
"
    )
}

/// The loop shapes whose early exits rejoin past the loop: a `for...of` over
/// a generator in a function, in a closure, and nested in an outer loop, and
/// a `while` (plain, with a compound condition, with a call as condition)
/// with code after it inside `try`/`finally`.
fn loop_exit_programs(n: usize) -> Vec<(&'static str, String)> {
    let for_of = "  for (const digit of digits(3)) { total += digit; }\n";
    vec![
        ("function for-of", generator_loops(n, for_of, "{}")),
        (
            "closure for-of",
            generator_loops(
                n,
                for_of,
                "  const step = (): number => {\n{}    return total;\n  };\n  total += step();",
            ),
        ),
        (
            "for-of in an outer loop",
            generator_loops(
                n,
                for_of,
                "  for (let round = 0; round < 2; round++) {\n{}    total += round;\n  }",
            ),
        ),
        (
            "while in try/finally",
            generator_loops(
                n,
                "  try { while (i < 3) { i++; total += risky(i); } total += 1; } finally { total += 2; }\n",
                "{}",
            ),
        ),
        (
            "compound-condition while in try/finally",
            generator_loops(
                n,
                "  try { while (i < 3 && total < 100000) { i++; total += risky(i); } total += 1; } finally { total += 2; }\n",
                "{}",
            ),
        ),
        (
            // Before the exit join this shape was refused ("closes a cycle
            // outside the loop being emitted"), and overflowed the stack on
            // the way there: the `catch` leaves the compound `while` for the
            // outer loop's update.
            "compound-condition while in try/catch in an outer loop",
            generator_loops(
                n,
                "    try { while (i < 3 && total < 100000) { i++; total += risky(i); } } catch (error) { total -= 1; }\n",
                "  for (let round = 0; round < 2; round++) {\n{}    i = round;\n  }",
            ),
        ),
        (
            "call-condition while in try/finally",
            generator_loops(
                n,
                "  try { while (risky(i) < 3) { i++; total += risky(i); } total += 1; } finally { total += 2; }\n",
                "{}",
            ),
        ),
    ]
}

#[test]
fn loop_exits_emit_their_shared_tail_once() {
    for (shape, program) in loop_exit_programs(4) {
        let source = source_for(&program);
        let emitted = program_body(&source);
        assert_eq!(
            emitted.matches("* 7919.0").count(),
            1,
            "{shape}: the statement after four sequential loops must be emitted once:\n{emitted}"
        );
    }
}

#[test]
fn loop_exit_emission_grows_linearly() {
    // Twelve loops against four: linear emission is ~3x the text; per-exit
    // emission was `4^8` times larger (and never finished).
    for ((shape, four), (_, twelve)) in loop_exit_programs(4)
        .into_iter()
        .zip(loop_exit_programs(12))
    {
        let small = program_body(&source_for(&four)).len();
        let large = program_body(&source_for(&twelve)).len();
        assert!(
            large < small * 4,
            "{shape}: {small} bytes for 4 loops, {large} for 12"
        );
    }
}

/// `n` copies of `statement` (`{i}` is the statement's index) inside `wrap`
/// (whose one `{}` holds them), in a function `run` that ends with one marker
/// statement, after the helper `risky` that may throw.
fn branch_statements(n: usize, statement: &str, wrap: &str) -> String {
    let statements = (0..n)
        .map(|index| statement.replace("{i}", &index.to_string()))
        .collect::<String>();
    let body = wrap.replace("{}", &statements);
    format!(
        r"
function risky(value: number): number {{
  if (value > 1000) {{
    throw new Error('too big');
  }}
  return value + 1;
}}

export function run(seed: number): number {{
  let total = seed;
{body}
  total = total * 7919;
  return total;
}}
"
    )
}

/// The statement shapes whose arms rejoin only transitively, each in the
/// contexts with distinct emitters: a function body, a loop body, a closure
/// body, and a forward `if` arm. Every one of them re-emitted the code after
/// it once per path before its rejoin rule (`3^n` for a three-way `switch`
/// with a call in a `case`, `4^n` for an `if`/`else` holding a call inside a
/// `try`), so `n` sequential statements multiplied the rest of the body.
///
/// * a `switch` whose `case` calls a function (`emit_match`'s post-dominator
///   join; `emit_loop_match`, `emit_closure_match`, the join-region `match`);
/// * a `switch` whose `case` throws on one path, or returns (a `case` that
///   leaves the function constrains no join: `branch_rejoin_post_dominator`);
/// * a `try`/`catch`, `try`/`finally` in a loop body (`throwing_loop_join`);
/// * an `if`/`else` whose arm calls a function inside a `try`
///   (`emit_switch_at_post_dominator`: the `if` rejoins after the `try`);
/// * an `if`/`else` whose arm may throw or return (`2^n`: a `throw`, or a
///   `return` inside one arm, ended the paths a post-dominator must meet).
/// * a `switch` inside a `try` whose two calling `case`s throw into one
///   `catch` that returns or rethrows (`3^n`: a `return`/`throw` more than one
///   arm reaches is no arm's own exit: `shared_exit_join_candidates`).
fn branch_rejoin_programs(n: usize) -> Vec<(String, String)> {
    let shapes = [
        (
            "switch with a call in a case",
            "  switch (total % 3) { case 0: total = risky(total); break; case 1: total += {i}; break; default: total -= 1; }\n",
        ),
        (
            "switch with a throwing case",
            "  switch (total % 3) { case 0: if (total > 1e9) { throw new Error('x'); } total += 2; break; case 1: total += {i}; break; default: total -= 1; }\n",
        ),
        (
            "switch with a returning case",
            "  switch (total % 3) { case 0: total = risky(total); break; case 7: return -{i}; default: total -= 1; }\n",
        ),
        (
            "switch inside try",
            "  try { switch (total % 3) { case 0: total = risky(total); break; default: total -= 1; } } catch (error) { total -= {i}; }\n",
        ),
        (
            "switch inside try whose catch returns",
            "  try { switch (total % 3) { case 0: total = risky(total); break; case 1: total = risky(total + {i}); break; default: total -= 1; } } catch (error) { return -{i}; }\n",
        ),
        (
            "switch inside try whose catch rethrows",
            "  try { switch (total % 3) { case 0: total = risky(total); break; case 1: total = risky(total + {i}); break; default: total -= 1; } } catch (error) { throw new Error('again'); }\n",
        ),
        (
            "try/catch",
            "  try { total = risky(total); } catch (error) { total -= {i}; }\n",
        ),
        (
            "try/finally",
            "  try { total = risky(total); } finally { total += {i}; }\n",
        ),
        (
            "if/else whose arm may throw",
            "  if (total % 2 === 0) { if (total > 1e9) { throw new Error('x'); } total += {i}; } else { total -= 1; }\n",
        ),
        (
            "if/else whose arm may return",
            "  if (total % 2 === 0) { if (total > 1e9) { return -{i}; } total += {i}; } else { total -= 1; }\n",
        ),
        (
            "if/else chain with calls inside try",
            "  try { if (total % 3 === 0) { total = risky(total); } else if (total % 3 === 1) { total = risky(total + 1); } else { total += {i}; } } catch (error) { total -= 1; }\n",
        ),
    ];
    let contexts = [
        ("function", "{}"),
        (
            "loop body",
            "  for (let round = 0; round < 2; round++) {\n{}    total += round;\n  }",
        ),
        (
            "closure",
            "  const step = (): number => {\n{}    return total;\n  };\n  total += step();",
        ),
        ("if arm", "  if (seed > 0) {\n{}    total += 1;\n  }"),
    ];
    let mut programs = Vec::new();
    for (shape, statement) in shapes {
        for (context, wrap) in contexts {
            programs.push((
                format!("{shape} in a {context}"),
                branch_statements(n, statement, wrap),
            ));
        }
    }
    programs
}

#[test]
fn rejoined_branches_emit_their_shared_tail_once() {
    for (shape, program) in branch_rejoin_programs(5) {
        let source = source_for(&program);
        let emitted = program_body(&source);
        assert_eq!(
            emitted.matches("* 7919.0").count(),
            1,
            "{shape}: the statement after five sequential statements must be emitted once:\n{emitted}"
        );
    }
}

#[test]
fn rejoined_branch_emission_grows_linearly() {
    // Ten statements against four: linear emission is ~2.5x the text; per-path
    // emission was `3^6` (switch) to `4^6` (if/else in try) times larger.
    for ((shape, four), (_, ten)) in branch_rejoin_programs(4)
        .into_iter()
        .zip(branch_rejoin_programs(10))
    {
        let small = program_body(&source_for(&four)).len();
        let large = program_body(&source_for(&ten)).len();
        assert!(
            large < small * 4,
            "{shape}: {small} bytes for 4 statements, {large} for 10"
        );
    }
}

/// A `switch` whose cases all return, two of whose labels share a case body
/// holding a loop (es-toolkit's `compat/array/some`). Every path from it ends
/// in an arm's own `return`, so with those neutral no block reaches the
/// function's exit and there is no join: the post-dominator sets stay full,
/// and a full set also holds the exit node, so the old "can it reach the
/// exit" test passed and the per-branch table named a block inside the loop
/// (`edge from block 9 back to block 6 closes a cycle no loop was opened
/// for`). The arms keep their own emission.
#[test]
fn switch_whose_cases_all_return_keeps_its_loops() {
    let source = source_for(
        r"
export function some(source: number[], predicate: string | number): boolean {
  switch (typeof predicate) {
    case 'number':
    case 'string': {
      for (let i = 0; i < source.length; i++) {
        if (source[i] > 2) {
          return true;
        }
      }
      return false;
    }
  }
  return true;
}
",
    );
    let emitted = program_body(&source);
    assert_eq!(
        emitted.matches("loop {").count(),
        2,
        "each arm sharing the case body keeps its loop:\n{emitted}"
    );
}
