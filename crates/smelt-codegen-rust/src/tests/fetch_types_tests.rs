//! Emission coverage for the WHATWG fetch types.
//!
//! The runtime tier (`tests/fetch_types_runtime.rs`) proves the semantics hold
//! when the crate runs; these tests are the cheap half: they pin the *shape* of
//! what is emitted, which is what a reviewer reads. Two properties matter here
//! and neither is visible at runtime:
//!
//! * a header operation emits a real method call on the concrete
//!   `SmeltHeaders` value — not a tagged-value field lookup, and not a
//!   `SmeltUnknown` anywhere in the operation;
//! * the fetch runtime is **pay-for-use**: a crate that never mentions
//!   `Headers` must not carry `SmeltHeaders` at all.

use super::*;

/// A header read emits a typed method call on the concrete runtime value.
#[test]
fn headers_read_emits_a_concrete_method_call() {
    let source = source_for(
        r#"
const headers = new Headers({ "Content-Type": "text/plain" });
const value = headers.get("content-type");
const present = headers.has("content-type");
"#,
    );
    assert!(
        source.contains("pub struct SmeltHeaders"),
        "the fetch prelude must be emitted:\n{source}"
    );
    assert!(
        source.contains(".get(&\"content-type\".to_owned())"),
        "`Headers.get` must be a method call on the value:\n{source}"
    );
    assert!(
        source.contains("let value: Option<String>"),
        "`Headers.get` must keep its `string | null` type:\n{source}"
    );
    assert!(
        source.contains("let present: bool"),
        "`Headers.has` must keep its boolean type:\n{source}"
    );
}

/// Each constructor initializer form emits its own conversion.
#[test]
fn headers_constructor_selects_the_conversion_by_initializer_type() {
    let record = source_for(r#"const headers = new Headers({ accept: "text/html" });"#);
    assert!(
        record.contains("SmeltHeaders::from_pairs("),
        "a record initializer builds pairs:\n{record}"
    );
    let empty = source_for(r"const headers = new Headers();");
    assert!(
        empty.contains("SmeltHeaders::new()"),
        "an empty constructor builds an empty list:\n{empty}"
    );
    let copied = source_for(
        r#"
const source = new Headers({ accept: "text/html" });
const copy = new Headers(source);
"#,
    );
    assert!(
        copied.contains("entries_sorted()"),
        "a `Headers` initializer copies the source pairs:\n{copied}"
    );
}

/// The mutating operations emit the matching runtime methods.
#[test]
fn headers_mutations_emit_their_runtime_methods() {
    let source = source_for(
        r#"
const headers = new Headers();
headers.set("accept", "text/html");
headers.append("accept", "application/json");
headers.delete("accept");
"#,
    );
    for expected in [".set(&", ".append(&", ".delete(&"] {
        assert!(
            source.contains(expected),
            "expected `{expected}` in the emitted mutations:\n{source}"
        );
    }
}

/// A crate that never mentions `Headers` carries none of the fetch runtime.
#[test]
fn fetch_runtime_is_pay_for_use() {
    let source = source_for(
        r#"
const scores = new Map<string, number>();
scores.set("a", 1);
const score = scores.get("a");
"#,
    );
    assert!(
        !source.contains("SmeltHeaders"),
        "a crate with no `Headers` must not carry the fetch runtime:\n{source}"
    );
}

/// No header operation routes a value through the tagged dynamic ABI.
#[test]
fn header_operations_carry_no_erasure() {
    let source = source_for(
        r#"
export function trace(headers: Headers): string | null {
  headers.set("x-trace", "abc");
  return headers.get("x-trace");
}
"#,
    );
    let body = emitted_function_body(&source, "fn trace(");
    assert!(
        !body.contains("SmeltUnknown"),
        "a fully typed header function must carry no erasure:\n{body}"
    );
    assert!(
        body.contains("SmeltHeaders"),
        "the parameter must be the concrete runtime type:\n{body}"
    );
}
