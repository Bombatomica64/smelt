//! Codegen regression tests for the *fallible* and *reflective* stdlib seams.
//!
//! Each rule here is about a JavaScript operation whose observable behaviour is
//! not a value but a boundary:
//!
//! * `JSON.parse` can THROW. Emitting it as an infallible panicking expression
//!   left an enclosing `try` with no throwing edge, so MIR dropped the `catch`
//!   block for want of a predecessor and a catchable `SyntaxError` became a
//!   process abort.
//! * `Reflect.ownKeys` answers `(string | symbol)[]`. Aliasing it to
//!   `Object.keys` both filtered the symbol keys out and typed the result
//!   `List<String>`, which const-folded every `typeof key === 'string'` test in
//!   the consumer.

use super::*;

#[test]
fn a_caught_json_parse_keeps_its_catch_arm_and_does_not_panic() {
    // `JSON.parse` throws on malformed text; the source catches it. Lowering the
    // parse as a plain assignment gave the try region no unwind edge at all, so
    // the whole `catch` arm was unreachable and dropped, and the emitted parse
    // was `.expect("JSON parse failed")` -- an abort where JavaScript returns
    // `false`.
    let source = source_for(
        r"
export function isJson(value: string): boolean {
  try {
    JSON.parse(value);
    return true;
  } catch {
    return false;
  }
}
",
    );

    let body = emitted_function_body(&source, "fn is_json");
    assert!(
        !body.contains("JSON parse failed"),
        "a caught `JSON.parse` must not be an infallible panic:\n{body}"
    );
    assert!(
        body.contains("smelt_json_parse"),
        "`JSON.parse` must go through the fallible adapter:\n{body}"
    );
    assert!(
        body.contains("return false;"),
        "the `catch` arm must survive lowering:\n{body}"
    );
    assert!(
        source.contains("fn smelt_json_parse(text: &str) -> Result<SmeltUnknown,"),
        "the prelude must carry the fallible adapter:\n{source}"
    );
    // The thrown payload is the same record `new SyntaxError(m)` builds, so a
    // `catch` binding cannot tell the two apart.
    assert!(
        source.contains("SmeltUnknown::String(\"SyntaxError\".into())"),
        "a parse failure must throw a `SyntaxError` record:\n{source}"
    );
}

#[test]
fn an_uncaught_json_parse_makes_its_function_throw() {
    // With no handler in scope the parse still cannot panic: the throwing-
    // function propagation pass sees the fallible builtin and gives the
    // enclosing function the `Result` error channel.
    let source = source_for(
        r"
export function parseIt(value: string): unknown {
  return JSON.parse(value);
}
",
    );

    assert!(
        source.contains("fn parse_it(value: String) -> Result<"),
        "an uncaught fallible builtin must make its function throw:\n{source}"
    );
}

#[test]
fn reflect_own_keys_keeps_symbol_keys_and_a_dynamic_key_type() {
    // `Reflect.ownKeys` answers `(string | symbol)[]`. Aliased to `Object.keys`
    // it dropped the symbol keys and typed the list `String`, which folded the
    // consumer's `typeof key !== 'string'` guard to a constant.
    let source = source_for(
        r"
export function firstNonStringKey(value: object): boolean {
  for (const key of Reflect.ownKeys(value)) {
    if (typeof key !== 'string') {
      return true;
    }
  }
  return false;
}
",
    );

    let body = emitted_function_body(&source, "fn first_non_string_key");
    assert!(
        body.contains("smelt_own_keys"),
        "`Reflect.ownKeys` must use the own-key projection, not `Object.keys`:\n{body}"
    );
    assert!(
        !body.contains("if false"),
        "a `string | symbol` key must not const-fold its `typeof` test:\n{body}"
    );
    assert!(
        source.contains("fn smelt_own_keys"),
        "the prelude must carry the own-key projection:\n{source}"
    );
    assert!(
        source.contains("__smelt_symbol:"),
        "the own-key projection must map stored symbol keys back:\n{source}"
    );
}
