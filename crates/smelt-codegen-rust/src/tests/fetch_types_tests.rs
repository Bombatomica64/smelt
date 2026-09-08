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

/// A union initializer dispatches on the arm, with no erasure in between.
///
/// `HeadersInit` is a union in WHATWG's IDL, so source that keeps it as one
/// hands the constructor a value whose arm the runtime picks and whose
/// conversion the compiler picks. A generated union is a tagged enum, so both
/// halves stay static: the emitted code matches the enum and runs each arm's
/// own conversion. The negative assertion is the point of the test — erasing
/// the union to `SmeltUnknown` to re-inspect its tag would answer the same
/// question with the type thrown away, and `SmeltHeaders` has no erased
/// constructor to recover it with.
#[test]
fn headers_constructor_dispatches_on_a_union_initializer() {
    let source = source_for(
        r#"
type HeadersInitUnion = [string, string][] | Record<string, string> | Headers;
function contentTypeOf(init: HeadersInitUnion): string {
  return new Headers(init).get("content-type") ?? "none";
}
const pairs: [string, string][] = [["content-type", "text/html"]];
console.log(contentTypeOf(pairs));
"#,
    );
    assert!(
        source.contains("::M0(smelt_headers_init)")
            && source.contains("::M1(smelt_headers_init)")
            && source.contains("::M2(smelt_headers_init)"),
        "each union arm must get its own conversion:\n{source}"
    );
    assert!(
        source.contains("smelt_headers_init.entries_sorted()"),
        "the `Headers` arm must copy the source pairs:\n{source}"
    );
    assert!(
        !source.contains("SmeltHeaders::from_smelt_unknown"),
        "a union initializer must not route through the erased boundary:\n{source}"
    );
}

/// An absent initializer is the empty header list, not a blocker.
///
/// WHATWG's constructor takes `HeadersInit?` and `new Headers(undefined)` is
/// the empty list — the same answer the no-argument spelling gives — so an
/// optional init key needs no narrowing proof to be lowered.
#[test]
fn headers_constructor_accepts_an_absent_initializer() {
    let source = source_for(
        r#"
interface InitLike { headers?: Record<string, string> }
function firstHeader(init: InitLike): string {
  return new Headers(init.headers).get("a") ?? "none";
}
console.log(firstHeader({ headers: { a: "1" } }));
"#,
    );
    assert!(
        source.contains("None => SmeltHeaders::new()"),
        "an absent initializer must build an empty header list:\n{source}"
    );
    assert!(
        source.contains("Some(smelt_headers_init) => SmeltHeaders::from_pairs("),
        "a present initializer must keep its own conversion:\n{source}"
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
        r"
export function statusOf(response: Response): number {
  return response.status;
}
",
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

/// A bare nested literal is built at the arm's element type, not erased.
///
/// TypeScript infers `string[][]` for `[["a", "b"]]` while the union's arm is a
/// tuple list, so the literal is not an exact member of the union. Erasing it
/// into a `SmeltUnknown::Array` for the union to reconstruct at runtime is two
/// conversions and a lost type for a value whose arm the compiler knows — and
/// it is the shape Hono writes inline (`{ headers: [['a','b']] }`).
#[test]
fn a_nested_array_literal_injects_into_the_tuple_arm() {
    let source = source_for(
        r#"
type HeadersInitUnion = [string, string][] | Record<string, string> | Headers;
function contentTypeOf(init: HeadersInitUnion): string {
  return new Headers(init).get("content-type") ?? "none";
}
console.log(contentTypeOf([["content-type", "text/html"]]));
"#,
    );
    assert!(
        source.contains("Vec<(String, String)>"),
        "the literal must be built at the arm's tuple element type:\n{source}"
    );
    assert!(
        !source.contains("from_smelt_unknown(SmeltUnknown::Array"),
        "the literal must not round-trip through the erased boundary:\n{source}"
    );
}

/// An erased `BodyInit` body dispatches at run time instead of being refused.
///
/// `BodyInit` is a union whose unmodeled arms are host classes, so a
/// `body?: BodyInit | null | undefined` parameter — Hono's own
/// `createResponseInstance` signature — erases, and the whole constructor call
/// used to be a build-time blocker including for the string every real caller
/// passes. The arms are distinguishable at run time by tag and only there, so
/// the conversion dispatches on the tag: a string is text, a nullish value is
/// the empty body, and an unmodeled arm throws naming itself rather than
/// putting wrong bytes in the body.
#[test]
fn an_erased_body_init_dispatches_on_the_runtime_tag() {
    let source = source_for(
        r#"
const make = (body?: BodyInit | null | undefined): Response => new Response(body);
const filled = make("hi");
const empty = make();
"#,
    );

    assert!(
        source.contains("SmeltUnknown::String(value) => SmeltBody::from_text("),
        "a string arm must become a text body:\n{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltBody::empty()"),
        "a nullish arm must become the empty body:\n{source}"
    );
    assert!(
        source.contains("body arm is not modeled yet"),
        "an unmodeled arm must name itself at run time:\n{source}"
    );
}

/// An ambient init dictionary is emitted as a struct with typed optional fields.
///
/// The negative assertion is the point: an erased record was what made
/// `init.headers` a `SmeltUnknown`, and what made the checked cast recover an
/// EMPTY header list from a record literal.
#[test]
fn an_ambient_response_init_is_a_typed_struct() {
    let source = source_for(
        r#"
const make = (init?: globalThis.ResponseInit): Response => new Response("b", init);
const made = make({ status: 201, headers: { "x-a": "1" } });
const status = made.status;
"#,
    );

    assert!(
        source.contains("struct globalThis_ResponseInit"),
        "the ambient init must be emitted as a struct:\n{source}"
    );
    assert!(
        source.contains("status: Option<f64>") && source.contains("status_text: Option<String>"),
        "the init's scalar keys must be typed options:\n{source}"
    );
    assert!(
        !source.contains("smelt_get_unknown_field(&init"),
        "an init key must not be read through the erased boundary:\n{source}"
    );
}
