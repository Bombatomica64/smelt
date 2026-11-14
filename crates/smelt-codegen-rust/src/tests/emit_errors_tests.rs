//! Generated Rust for throw and catch.
//!
//! Where a `catch` runs and what it receives is in the `throw_and_catch_forms`
//! Node-parity fixture; these tests pin the emitted function shapes.

use super::*;

/// A top-level `try` that catches everything leaves `main` infallible.
#[test]
fn emits_caught_throw_without_result_signature() {
    let source = source_for(
        "try {
  throw \"boom\";
} catch (err: string) {
  console.log(err);
}
",
    );

    assert_eq!(
        rust_fn(&source, "main"),
        r#"fn main() {
let err: JsString;
err = JsString::from("boom");
let _ = { println!("{}", err); };
return;
}"#
    );
}

/// A throwing function adapted to a callback propagates through the adapter's
/// `Result`, rather than aborting where the signature can carry the error.
#[test]
fn a_throwing_call_in_a_fallible_closure_body_propagates_with_question_mark() {
    let source = source_for(
        r"
function thrower(x: number): string {
  if (x > 0) { throw new Error('boom'); }
  return 'ok';
}
export function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return 'caught';
  }
}
export function useIt(): string {
  return guard(thrower, 1);
}
",
    );

    assert_eq!(
        rust_fn(&source, "__smelt_fn_typed_0_0"),
        r"fn __smelt_fn_typed_0_0() -> SmeltFnTyped0_0 {
thread_local! { static SMELT_FN_TYPED: ::std::cell::OnceCell<SmeltFnTyped0_0> = ::std::cell::OnceCell::new(); }
SMELT_FN_TYPED.with(|smelt_cell| smelt_cell.get_or_init(|| { let smelt_function: SmeltFnTyped0_0 = ::std::rc::Rc::new(|closure_arg_0: f64| -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_1: JsString = thrower(closure_arg_0)?;
Ok::<JsString, Box<dyn std::error::Error>>(_smelt_tmp_1.clone())
}); smelt_function }).clone())
}"
    );
}
