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

#[test]
fn throwing_an_error_constructor_emits_the_error_record() {
    // The payload ABI was in place, but the *throw statement* never handed it an
    // error: the frontend narrowed `new Error(m)` down to `m` before the operand
    // ever reached MIR, so every throw in a generated crate entered the channel
    // as `smelt_throw(SmeltUnknown::String(..))`. Downstream, `error instanceof
    // Error` was false, `error.message` was `undefined`, and `error.name` was
    // unreadable — even though the identical construction used as a *value* built
    // the full record. This pins the throw site to the record.
    let source = source_for(
        r#"
function boom(): void {
  throw new RangeError("out of range");
}

boom();
"#,
    );

    assert!(
        source.contains("smelt_throw("),
        "throw should enter the payload-preserving error channel:\n{source}"
    );
    assert!(
        !source.contains(r#"smelt_throw(SmeltUnknown::String("out of range".to_owned()))"#),
        "throw must not collapse an Error to its message string:\n{source}"
    );
    assert!(
        source.contains(r#"("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".to_owned()))"#),
        "the thrown record must carry the spelled error class:\n{source}"
    );
    assert!(
        source.contains(r#"("message".to_owned(), SmeltUnknown::String("out of range".to_owned()))"#),
        "the thrown record must carry the message:\n{source}"
    );
}

#[test]
fn throwing_an_error_from_a_callback_emits_the_error_record() {
    // A `throw` inside an arrow lowers through the reduced callback expression
    // language, which carried its own copy of the narrowing: `new Error(m)`
    // became `m`, and any other construction became the empty string. Fixing the
    // statement path alone would have left this shape — the one
    // `attempt(() => { throw ... })` uses — still throwing a bare string.
    let source = source_for(
        r#"
function apply(f: () => number): number {
  return f();
}

function run(): number {
  return apply(() => {
    throw new Error("callback boom");
  });
}

run();
"#,
    );

    assert!(
        !source.contains(r#"smelt_throw(SmeltUnknown::String("callback boom".to_owned()))"#),
        "a callback throw must not collapse an Error to its message string:\n{source}"
    );
    assert!(
        source.contains(r#"("__smelt_error".to_owned(), SmeltUnknown::String("Error".to_owned()))"#),
        "a callback-thrown Error must carry the class marker:\n{source}"
    );
    assert!(
        source.contains(r#"("message".to_owned(), SmeltUnknown::String("callback boom".to_owned()))"#),
        "a callback-thrown Error must carry the message:\n{source}"
    );
}

#[test]
fn erased_error_stringifies_through_error_prototype_to_string() {
    // Now that a thrown `Error` survives as an object, the JavaScript `ToString`
    // of that object must be `Error.prototype.toString` (`"name: message"`), not
    // the generic `[object Object]` placeholder. While the payload was a bare
    // string, `String(err)` happened to read the message; without this rule,
    // preserving the object would have replaced that with useless text.
    //
    // The rule keys off the `__smelt_error` marker, exactly as the sibling
    // `__smelt_regexp` arm keys off its own, so it holds for every error value
    // rather than for one spelling.
    let source = source_for(
        r#"
function boom(): void {
  throw new Error("kaboom");
}

function text(): string {
  try {
    boom();
    return "no throw";
  } catch (error) {
    return String(error);
  }
}

const rendered = text();
"#,
    );

    assert!(
        source.contains(r#"SmeltUnknown::Object(value) if value.contains_key("__smelt_error")"#),
        "the erased ToString must have an Error.prototype.toString arm:\n{source}"
    );
    assert!(
        source.contains(r#"format!("{smelt_error_name}: {smelt_error_message}")"#),
        "the Error arm must render `name: message`:\n{source}"
    );
}

/// An unrelated closure elsewhere in the crate must not suppress the fold.
///
/// The fold refuses to touch a local that a closure captures, because such a
/// local is read through the closure environment rather than through an operand
/// the read tally can see. That guard was first written against the crate-wide
/// closure table -- but `MirClosure::captures` records a `source_local`, and a
/// `LocalId` is meaningful only inside its owning body. A function that
/// happened to reuse a local number some *other* function's closure captured
/// was therefore treated as capturing it too. In a two-function fixture nothing
/// collided and the fold looked correct; across a real library almost every
/// low-numbered local collided with something, and 29 of 30 throw sites
/// silently kept their staged temporaries. Hence the many closures below: the
/// point of the fixture is to occupy a spread of local numbers.
#[test]
fn a_foreign_closure_capture_does_not_suppress_the_fold() {
    let source = source_for(
        r#"
export function manyClosures(values: number[]): number[] {
  const a = 1;
  const b = 2;
  const c = 3;
  const d = 4;
  const e = 5;
  const f = 6;
  const g = 7;
  const h = 8;
  return values
    .map(value => value * a + b)
    .map(value => value * c + d)
    .map(value => value * e + f)
    .map(value => value * g + h);
}

export function thrower(size: number): number {
  if (!Number.isInteger(size) || size <= 0) {
    throw new Error('Size must be an integer greater than zero.');
  }
  return size;
}
"#,
    );

    assert!(
        source.contains("smelt_throw(SmeltUnknown::Object("),
        "the payload must still be built at the throw site when the crate \
         contains unrelated capturing closures:\n{source}"
    );
    assert!(
        !source.contains("smelt_throw(_smelt_tmp"),
        "no staged temporary may survive into the throw:\n{source}"
    );
}

/// A thrown `Error` must be built as one expression at the throw site.
///
/// Making `throw` value-preserving gave `throw new Error(m)` a
/// `{__smelt_error, message}` record payload. MIR is three-address, so the
/// record and its erasure to `SmeltUnknown` each landed in their own temporary,
/// and one source statement emitted five lines of Rust: two `let` declarations,
/// two assignments, and the `return Err(..)`. Two of those lines were bare
/// `SmeltUnknown` bindings that read as erasure in their own right even though
/// they were only the interior of the exception-payload boundary.
///
/// A team writing this Rust by hand would construct the payload inside
/// `smelt_throw(..)`. The emitter therefore folds the temporaries that only
/// stage a throw payload into the throw expression.
#[test]
fn thrown_error_payload_is_built_at_the_throw_site() {
    let source = source_for(
        r#"
export function chunk(size: number): number {
  if (size <= 0) {
    throw new Error('Size must be an integer greater than zero.');
  }
  return size;
}
"#,
    );

    assert!(
        source.contains("smelt_throw(SmeltUnknown::Object("),
        "the thrown payload should be constructed inside smelt_throw:\n{source}"
    );
    assert!(
        source.contains("Size must be an integer greater than zero."),
        "the thrown message should survive into the throw expression:\n{source}"
    );
    assert!(
        !source.contains(": SmeltUnknown;"),
        "a thrown payload must not spill an erased SmeltUnknown temporary:\n{source}"
    );
    assert!(
        !source.contains("SmeltRecord<String, SmeltUnknown>;"),
        "a thrown payload must not spill a staged record temporary:\n{source}"
    );
    for line in source.lines() {
        assert!(
            !(line.contains("_smelt_tmp") && line.contains("SmeltUnknown::Object(")),
            "the payload should not be assigned to a temporary first:\n{source}"
        );
    }
}

/// Folding the throw payload must not disturb a throw of a plain value.
///
/// `throw` is value-preserving for every operand, so the fold is keyed on the
/// shape of the MIR (a write-once, read-once temporary staged immediately
/// before the throw), never on the operand being an `Error`. A thrown string
/// literal is already a constant operand with nothing to fold, and a thrown
/// object literal folds its record construction the same way an `Error` does —
/// both must still reach `smelt_throw` carrying their own value.
#[test]
fn throwing_a_plain_value_keeps_its_own_payload() {
    let source = source_for(
        r#"
export function keep(value: unknown): unknown {
  return value;
}

export function reject(flag: boolean): number {
  if (flag) {
    throw 'negative';
  }
  throw { code: 1 };
}
"#,
    );

    assert!(
        source.contains("smelt_throw(SmeltUnknown::String(\"negative\".to_owned()))"),
        "a thrown string must stay a string payload:\n{source}"
    );
    assert!(
        source.contains("\"code\".to_owned()"),
        "a thrown object literal must keep its own fields:\n{source}"
    );
    for line in source.lines() {
        assert!(
            !(line.contains("_smelt_tmp") && line.contains("\"code\".to_owned()")),
            "the object payload should be built at the throw site:\n{source}"
        );
    }
}
