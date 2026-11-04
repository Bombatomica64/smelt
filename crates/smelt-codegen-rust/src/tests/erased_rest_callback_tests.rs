//! Calls through a callback parameter typed with an erased rest list.
//!
//! `(...args: any[]) => number` (and Python's `Callable[..., int]`) is the
//! erased-rest shape. Passed as a parameter it is a borrowed
//! `&dyn Fn(SmeltList<SmeltUnknown>) -> f64` handle that returns its declared
//! type; only the owned `SmeltErasedFunction::call` ABI yields `SmeltUnknown`.
//! Re-extracting a number from the handle's `f64` result was E0308.

use super::{emitted_function_body, source_for};

/// A callback parameter of the erased-rest shape with a concrete return type.
const APPLY: &str = r"
export function apply(f: (...args: any[]) => number, x: number): number {
  return f(x, 1);
}
";

#[test]
fn borrowed_erased_rest_callback_returns_its_declared_type() {
    let source = source_for(APPLY);
    let body = emitted_function_body(&source, "fn apply(");
    assert!(
        source.contains("f: &dyn Fn(SmeltList<SmeltUnknown>) -> f64"),
        "the callback parameter should be a borrowed erased-rest handle: {source}"
    );
    assert!(
        !body.contains("SmeltUnknown::Number(value) => value"),
        "the f64 result was re-extracted from SmeltUnknown: {body}"
    );
}
