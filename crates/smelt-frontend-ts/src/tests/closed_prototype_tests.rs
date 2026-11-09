//! A call of a method no model claims, on a receiver whose prototype is closed
//! (a string, an array, a number, a boolean), is reported at transpile time.
//!
//! The erased call ABI the call used to take answers only the methods the
//! runtime's erased prototype tables list, so the generated program compiled
//! and then always threw `TypeError: value is not a function` where JavaScript
//! runs the method. Erased, generic and record receivers keep the erased call:
//! their value can hold the member at run time. In a test-tier file the call
//! throws a named `TypeError` instead, so one unmodelled method blocks only
//! the test that reaches it.

use super::*;

#[test]
fn an_unmodelled_string_method_is_a_named_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
const s: string = "abc";
export const html = s.anchor("x");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "`String.prototype.anchor` is not modelled")
}

#[test]
fn an_unmodelled_number_method_is_a_named_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
const n: number = 5;
export const text = n.toLocaleString("de-DE");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "`Number.prototype.toLocaleString` is not modelled")
}

#[test]
fn a_modelled_method_and_an_erased_receiver_still_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const s: string = "abc";
export const upper = s.toUpperCase();
export function call(value: any): unknown {
  return value.anchor("x");
}
"#),
        &mut ctx,
    )?;
    ensure_signature(&ctx, "call", "call(value: Unknown) -> Unknown")?;
    ensure_exprs!(&ctx, StringCase, BindThis);
    ensure!(crate_has_field(&ctx, "anchor"), "no read of field `anchor`");
    Ok(())
}

#[test]
fn a_test_file_lowers_and_blocks_only_at_the_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
const s: string = "abc";
export const html = s.anchor("x");
"#),
        "tests/page.test.ts",
        &mut ctx,
    )?;
    Ok(())
}
