//! Regression tests for the exception-payload ABI (see `crate::thrown`).
//!
//! Every `throw`, every `reject`, and every `catch` binding in generated Rust
//! shares one error channel, `Result<T, Box<dyn std::error::Error>>`. These tests
//! pin two properties of that channel:
//!
//! 1. A thrown value enters it whole, rather than being replaced by
//!    `format!("{}", value)` (which rendered every erased JavaScript `Error` as
//!    the literal text `[object Object]`).
//! 2. An erased `catch` binding leaves it by recovering that same value, rather
//!    than by rebuilding a synthetic `{__smelt_error, message}` record out of the
//!    error's `Display` text.

use super::*;

/// Text of the old, payload-destroying throw form.
const STRINGIFIED_THROW: &str = "std::io::Error::new(std::io::ErrorKind::Other, format!(";

#[test]
fn throw_of_an_error_object_carries_its_payload() {
    let source = source_for(
        r#"
function boom(value: unknown): void {
  throw value;
}

boom(new TypeError("bad"));
"#,
    );

    assert!(
        source.contains("smelt_throw("),
        "throw should enter the payload-preserving error channel:\n{source}"
    );
    assert!(
        !source.contains(STRINGIFIED_THROW),
        "throw must not stringify its payload into a std::io::Error:\n{source}"
    );
}

#[test]
fn erased_catch_binding_recovers_the_thrown_payload() {
    let source = source_for(
        r#"
function boom(): void {
  throw new TypeError("bad");
}

function run(): unknown {
  try {
    boom();
    return null;
  } catch (error) {
    return error;
  }
}

const caught = run();
"#,
    );

    // The pre-ABI form bound the catch parameter to a record rebuilt from the
    // error's `Display` text. Recovering the payload replaces that whole
    // expression, so the binding is now exactly the recovery call.
    assert!(
        source.contains("let error = smelt_thrown_value(&*__smelt_error);"),
        "an erased catch binding should recover the payload:\n{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::String(__smelt_error.to_string())"),
        "catch must not rebuild the error from Display text:\n{source}"
    );
}

#[test]
fn awaited_rejection_recovers_the_thrown_payload() {
    let source = source_for(
        r#"
async function boom(): Promise<number> {
  throw new TypeError("bad");
}

async function run(): Promise<unknown> {
  try {
    return await boom();
  } catch (error) {
    return error;
  }
}
"#,
    );

    assert!(
        source.contains("smelt_thrown_value(&*__smelt_error)"),
        "a catch after `await` should recover the rejection payload:\n{source}"
    );
}

#[test]
fn promise_rejection_enters_the_payload_channel() {
    let source = source_for(
        r#"
function boom(): Promise<number> {
  return new Promise<number>((_resolve, reject) => {
    reject(new TypeError("bad"));
  });
}
"#,
    );

    assert!(
        source.contains("Some(Err(smelt_throw(error)))"),
        "`reject(value)` should enter the payload channel:\n{source}"
    );
    assert!(
        !source.contains("smelt_reject_message"),
        "the reject bridge must not pre-flatten the reason to a message:\n{source}"
    );
}

#[test]
fn payload_channel_declares_its_recovery_adapters() {
    let source = source_for(
        r#"
function boom(value: unknown): void {
  throw value;
}

boom(new TypeError("bad"));
"#,
    );

    assert!(
        source.contains("struct SmeltThrown { value: SmeltUnknown }"),
        "the payload carrier should be emitted:\n{source}"
    );
    assert!(
        source.contains("impl ::std::error::Error for SmeltThrown {}"),
        "the payload carrier must be usable as the channel's error type:\n{source}"
    );
    // `Display` projects `message` so a string-typed `catch` and substring
    // `toThrow("...")` matchers observe the message rather than "[object Object]".
    assert!(
        source.contains("fn smelt_thrown_message(value: &SmeltUnknown) -> String"),
        "the message projection should be emitted:\n{source}"
    );
    // A foreign error that reached the channel through `?` has no payload, so the
    // adapter must still present it as an erased Error record.
    assert!(
        source.contains("if let Some(thrown) = error.downcast_ref::<SmeltThrown>()"),
        "payload recovery should be a downcast, keeping the channel open to \
         foreign errors:\n{source}"
    );
}

/// The load-bearing justification for the payload being a `SmeltUnknown`.
///
/// `CLAUDE.md` requires that any `SmeltUnknown` boundary be accompanied by proof
/// that concrete types, generated unions, or scoped generics cannot represent it.
/// Here one function's single error channel carries two structurally unrelated
/// payloads — a field-bearing object and a bare string — chosen at run time, and
/// a `catch` in a *different* function observes both:
///
/// * No concrete Rust type spans a two-field record and a string.
/// * No generated union arm can be selected, because the arm is picked by a
///   run-time branch and the `catch` is not in the throwing function's signature
///   (JavaScript has no throws-clause, so nothing propagates the choice).
/// * No function-scoped type parameter works either: the channel type is fixed by
///   `Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn Error>>>`,
///   shared by every erased callback in the crate, so it cannot mention a
///   parameter belonging to one caller.
#[test]
fn one_channel_carries_structurally_unrelated_payloads() {
    let source = source_for(
        r#"
declare function pickAtRuntime(): boolean;

function boom(): void {
  if (pickAtRuntime()) {
    throw { code: 42, tag: "record" };
  }
  throw "a bare string";
}

function run(): unknown {
  try {
    boom();
    return null;
  } catch (error) {
    return error;
  }
}

const caught = run();
"#,
    );

    // Both arms enter the one channel, and neither is widened to the other.
    assert_eq!(
        source
            .matches("Err::<_, Box<dyn std::error::Error>>(smelt_throw(")
            .count(),
        2,
        "both throw arms should enter the payload channel:\n{source}"
    );
    assert!(
        source.contains(r#"smelt_throw(SmeltUnknown::String("a bare string".to_owned()))"#),
        "a thrown string must keep its string identity:\n{source}"
    );
    assert!(
        source.contains("smelt_throw(SmeltUnknown::Object("),
        "a thrown record must keep its object identity:\n{source}"
    );
    assert!(
        source.contains(r#"("code".to_owned(), SmeltUnknown::Number(42.0 as f64))"#)
            && source.contains(r#"("tag".to_owned(), SmeltUnknown::String("record".to_owned()))"#),
        "a thrown record must keep its fields:\n{source}"
    );
    // And the single catch recovers whichever one arrived, without a discriminant
    // supplied by the signature.
    assert!(
        source.contains("let error = smelt_thrown_value(&*__smelt_error);"),
        "the shared catch should recover either payload:\n{source}"
    );
}
