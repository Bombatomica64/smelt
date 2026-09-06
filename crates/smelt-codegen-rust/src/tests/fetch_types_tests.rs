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

/// A parameter read emits a typed method call on the concrete runtime value.
#[test]
fn url_search_params_read_emits_a_concrete_method_call() {
    let source = source_for(
        r#"
const params = new URLSearchParams("a=1");
const first = params.get("a");
const all = params.getAll("a");
const text = params.toString();
const count = params.size;
"#,
    );
    assert!(
        source.contains("pub struct SmeltUrlSearchParams"),
        "the params runtime type must be emitted:\n{source}"
    );
    assert!(
        source.contains("SmeltUrlSearchParams::from_query("),
        "a string initializer parses a query:\n{source}"
    );
    assert!(
        source.contains("let first: Option<String>"),
        "`get` must keep its `string | null` type:\n{source}"
    );
    assert!(
        source.contains(".to_text()"),
        "`toString` must be the urlencoded serialization:\n{source}"
    );
    assert!(
        source.contains(".size()"),
        "`size` must read the pair count:\n{source}"
    );
}

/// The params runtime is emitted only when a program uses it, and pulls in `url`.
#[test]
fn url_search_params_runtime_is_pay_for_use_and_declares_its_dependency() {
    let plain = source_for(r"const total = 1 + 1;");
    assert!(
        !plain.contains("SmeltUrlSearchParams"),
        "a crate with no params value must not carry the runtime:\n{plain}"
    );
    let mut ctx = HirCtx::new();
    assert!(
        to_hir(r#"const params = new URLSearchParams("a=1");"#, FileId(0), &mut ctx).is_ok(),
        "HIR"
    );
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    assert!(
        crate::stdlib::backend_dependencies(&mir).contains(&BackendDependency::Url),
        "a params value serializes through `url::form_urlencoded`, so the crate needs `url`"
    );
}

/// A `Response` member emits a typed method call on the concrete runtime value.
#[test]
fn response_member_emits_a_concrete_method_call() {
    let source = source_for(
        r#"
export function statusOf(response: Response): number {
  return response.status;
}
"#,
    );
    assert!(source.contains(".status()"), "{source}");
    assert!(
        source.contains("response: SmeltResponse"),
        "a `Response` parameter must be the concrete runtime type: {source}"
    );
    assert!(
        !source.contains("SmeltUnknown"),
        "a typed status read must not route through the erased carrier: {source}"
    );
}

/// A `Request` member emits a typed method call, and `url` is a real `String`.
#[test]
fn request_member_emits_a_concrete_method_call() {
    let source = source_for(
        r#"
export function schemeEnd(request: Request): number {
  return request.url.indexOf(":");
}
"#,
    );
    assert!(source.contains(".url()"), "{source}");
    assert!(
        source.contains("request: SmeltRequest"),
        "a `Request` parameter must be the concrete runtime type: {source}"
    );
}

/// The `Response`/`Request` runtimes are pay-for-use.
///
/// A crate that never mentions either must carry neither them nor the
/// `SmeltBody` they hold — the body has no source spelling of its own, so its
/// gate is exactly "a type that has a body is present".
#[test]
fn response_and_request_runtimes_are_pay_for_use() {
    let plain = source_for(r"const total = 1 + 1;");
    assert!(!plain.contains("SmeltResponse"), "{plain}");
    assert!(!plain.contains("SmeltRequest"), "{plain}");
    assert!(!plain.contains("SmeltBody"), "{plain}");

    let response_only = source_for(r#"const made = new Response("hi");"#);
    assert!(response_only.contains("struct SmeltResponse"), "{response_only}");
    assert!(
        response_only.contains("struct SmeltBody"),
        "a response holds a body, so the body comes with it: {response_only}"
    );
    assert!(
        response_only.contains("struct SmeltHeaders"),
        "a response holds a header list, so headers come with it: {response_only}"
    );
    assert!(
        !response_only.contains("struct SmeltRequest"),
        "a response must not drag in the request type: {response_only}"
    );
}

/// The `Request` host-identity marker is stamped by the erasure adapter.
///
/// es-toolkit's `isPlainObject` spec constructs `new Request('http://localhost')`
/// only to probe host identity, and the probe reads `__smelt_request`.
/// Construction is typed now, so the marker is stamped where the value crosses
/// into an `unknown` position — which is exactly where the probe reads it. The
/// frontend half of this gate
/// (`estk_transpile_gate_tests::request_construction_carries_no_marker_record`)
/// asserts construction no longer builds a record.
#[test]
fn request_erasure_stamps_the_host_identity_marker() {
    let source = source_for(
        r#"
export function make(): unknown {
  return new Request("http://localhost");
}
"#,
    );
    assert!(
        source.contains("impl IntoSmeltUnknown for SmeltRequest"),
        "the request must carry its own boundary adapter: {source}"
    );
    assert!(
        source.contains("__smelt_request"),
        "the adapter must stamp the host identity marker: {source}"
    );
}

/// A `Response` reaching an erased position stamps its own marker too.
#[test]
fn response_erasure_stamps_the_host_identity_marker() {
    let source = source_for(
        r#"
export function make(): unknown {
  return new Response("hi");
}
"#,
    );
    assert!(
        source.contains("impl IntoSmeltUnknown for SmeltResponse"),
        "the response must carry its own boundary adapter: {source}"
    );
    assert!(source.contains("__smelt_response"), "{source}");
}
