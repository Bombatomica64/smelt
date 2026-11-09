//! Generated Rust for Python strings and typed values.

use super::*;

#[test]
fn file_construction_is_concrete_and_answers_both_identities_statically() {
    let source = source_for(
        r#"
const file = new File(["content"], "file.txt", { type: "text/plain" });
const isFile = file instanceof File;
const isBlob = file instanceof Blob;
"#,
    );

    // Construction is the concrete `SmeltBlob`, and the string parts keep their
    // type through the typed constructor for `BlobPart`'s string arm.
    assert!(source.contains("SmeltBlob::from_string_parts("), "{source}");
    assert!(source.contains("pub struct SmeltBlob"), "{source}");
    // Both identities are then STATIC facts about a concretely-typed receiver:
    // a blob always is a `Blob`, and it is a `File` exactly when its optional
    // name is present. Neither needs the erased marker probe.
    // `is_file()` is the static answer for `instanceof File` on a concrete
    // receiver, and `true` for `instanceof Blob`.
    assert!(source.contains(".is_file()"), "{source}");
    // The user binding holds the static answer directly (no temporary).
    assert!(source.contains("let is_blob: bool = true;"), "{source}");
    // The erased record still carries both markers, for a blob that crosses a
    // dynamic boundary.
    assert!(source.contains("\"__smelt_file\".to_owned()"), "{source}");
    assert!(source.contains("\"__smelt_blob\".to_owned()"), "{source}");
}

#[test]
fn omits_blob_record_helper_without_blob_construction() {
    let source = source_for("const value: number = 1;");
    assert!(
        !source.contains("smelt_blob_record_from_parts"),
        "the Blob/File record helper must stay gated behind actual Blob/File construction: {source}"
    );
    let erased = source_for(
        r#"
const blob: unknown = new Blob(["a"]);
console.log(blob);
"#,
    );
    assert!(erased.contains("smelt_blob_record_from_parts"), "{erased}");
}

#[test]
fn emits_vitest_to_be_nan_using_same_value_semantics() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("NaN equality", () => {
  const value = NaN;
  expect(value).toBe(NaN);
  expect(Object.is(value, NaN)).toBe(true);
});
"#,
    );

    assert!(source.contains(".is_nan() &&"), "{source}");
    assert!(!source.contains("!= f64::NAN"), "{source}");
    assert!(!source.contains("== f64::NAN"), "{source}");
}

#[test]
fn emits_timezone_context_that_preserves_nan_values() {
    let source = source_for(
        r#"
import { tz } from "@date-fns/tz";
const context = tz("Asia/Singapore");
"#,
    );

    assert!(source.contains("if smelt_timestamp_ms.is_finite()"), "{source}");
    assert!(source.contains("else { f64::NAN }"), "{source}");
}

#[test]
fn list_slice_borrows_its_receiver_instead_of_cloning_it() {
    // A slice reads its receiver only through `&self` methods (`.len()`, `.iter()`),
    // so the receiver is borrowed, not cloned: a clone inside a loop over the same
    // list turns a linear algorithm quadratic. See `benchmarks/FINDINGS.md`.
    let source = source_for(
        r"
export function chunkNumbers(data: number[], size: number): number[][] {
  const chunks = Math.ceil(data.length / size);
  const result: number[][] = [];
  for (let index = 0; index < chunks; index++) {
    const start = index * size;
    result.push(data.slice(start, start + size));
  }
  return result;
}
",
    );

    // The slice still lowers to the same borrow-based iterator pipeline...
    assert!(source.contains("data.borrow().iter().skip("));
    assert!(source.contains("let smelt_len = data.len() as i64"));
    // ...and takes no copy of the receiver to get there.
    assert!(
        !source.contains("data.clone().iter()"),
        "slice receiver was cloned for an `.iter()` borrow:\n{source}"
    );
    assert!(
        !source.contains("data.clone().len()"),
        "slice receiver was cloned for a `.len()` borrow:\n{source}"
    );
}

#[test]
fn omits_encode_uri_helper_without_usage() {
    let source = source_for("const value: number = 1;");
    assert!(
        !source.contains("smelt_encode_uri"),
        "the encodeURI helper must stay gated behind actual encodeURI usage: {source}"
    );
    let used = source_for(r#"console.log(encodeURI("a b"));"#);
    assert!(used.contains("fn smelt_encode_uri("), "{used}");
}

#[test]
fn structured_clone_of_concrete_value_stays_pass_through() {
    let source = source_for(
        r"
interface Point { x: number; y: number; }
export function copy(value: Point): Point {
  return structuredClone(value);
}
",
    );

    // A concretely typed argument keeps its static shape: HIR values are
    // immutable, so no runtime deep-clone helper call is emitted for it (the
    // erased deep-clone helper is reserved for genuinely dynamic `unknown`
    // values, per the SmeltUnknown boundary rules).
    assert!(
        !source.contains("smelt_structured_clone("),
        "concretely typed structuredClone must not route through the erased deep-clone helper: {source}"
    );
}

#[test]
fn emits_timer_prelude_for_set_timeout_value_form() {
    let source = source_for(
        r"
const scheduled = globalThis.setTimeout;
const id = scheduled(() => {}, 4);
",
    );

    // The timer op lives inside the synthesized first-class closure body, so
    // the prelude gate must scan closure rvalues too.
    assert!(source.contains("fn smelt_set_timeout("), "{source}");
}


/// A module-level `const` holding a modeled host value, read from a FUNCTION,
/// reaches a module-global slot rather than the declared type's default.
#[test]
fn module_const_host_value_reaches_a_slot_when_read_from_a_function() {
    let source = source_for(
        r#"
const encoder = new TextEncoder();
const headers = new Headers({ "content-type": "text/plain" });

export function encoded(text: string): number {
  return encoder.encode(text).length;
}

export function contentType(): string {
  return headers.get("content-type") ?? "none";
}
"#,
    );

    // One `thread_local` slot per binding, lazily initialized by CALLING the
    // nullary function the frontend synthesized from the initializer.
    assert!(
        source.contains("static SMELT_GLOBAL_ENCODER_0: ::std::cell::RefCell<SmeltTextEncoder>"),
        "{source}"
    );
    assert!(
        source.contains("static SMELT_GLOBAL_HEADERS_1: ::std::cell::RefCell<SmeltHeaders>"),
        "{source}"
    );
    // Each initializer is a real function item, and its name carries the
    // global's ITEM INDEX rather than the module's absolute path — a path in a
    // symbol both leaks the build machine's filesystem and makes any golden
    // containing it unreproducible elsewhere.
    assert!(
        source.contains("fn smelt_global_init__encoder__0()"),
        "{source}"
    );
    assert!(
        source.contains("fn smelt_global_init__headers__1()"),
        "{source}"
    );
    assert!(!source.contains("__module_"), "{source}");
    // `contentType` reads THROUGH the slot, so it sees the one object.
    assert_eq!(
        source
            .matches("SMELT_GLOBAL_HEADERS_1.with(|smelt_value| smelt_value.borrow().clone())")
            .count(),
        1,
        "{source}"
    );
    // The module body touches the slot at the binding's own declaration, so
    // the lazy cell's single evaluation happens in source order
    // (`force_module_slot_init`); the unconsumed read neither clones nor binds
    // the value.
    assert!(
        source.contains("    SMELT_GLOBAL_HEADERS_1.with(|_| ());\n"),
        "{source}"
    );
}

/// A slot whose initializer can THROW gets a `Result` from its synthesized
/// function, which a `thread_local!` initializer cannot propagate. A throw
/// during module evaluation is always uncaught, so the slot reports it as
/// `main`'s `Result` return would and exits 1; an infallible initializer keeps
/// the bare call and the crate gets no reporter at all.
#[test]
fn module_slot_with_a_throwing_initializer_reports_like_main() {
    let source = source_for(
        r#"
function load(text: string): number {
  if (text === "") {
    throw new Error("empty");
  }
  return text.length;
}
const size: number = load("abc");
const encoder = new TextEncoder();

export function report(): number {
  return size * 2 + encoder.encode("x").length;
}
"#,
    );

    // The slot's container (`Cell` for a `Copy` scalar, `RefCell` otherwise)
    // is not what this test is about; the fallible call inside it is.
    assert!(
        source.contains(
            "::new(smelt_global_init__size__0().unwrap_or_else(|error| smelt_module_init_failed(error)))"
        ),
        "{source}"
    );
    assert!(
        source.contains("fn smelt_module_init_failed(error: Box<dyn std::error::Error>) -> !"),
        "{source}"
    );
    assert!(source.contains("eprintln!(\"Error: {error:?}\");"), "{source}");
    // The infallible initializer is untouched.
    assert!(
        source.contains("::std::cell::RefCell::new(smelt_global_init__encoder__1());"),
        "{source}"
    );

    let infallible = source_for(
        r"
const encoder = new TextEncoder();

export function encoded(text: string): number {
  return encoder.encode(text).length;
}
",
    );
    assert!(!infallible.contains("smelt_module_init_failed"), "{infallible}");
}

/// A module-level binding used ONLY in the module body keeps its ordinary
/// module-body local; slots are for bindings a function reads.
#[test]
fn module_const_host_value_used_only_in_the_module_body_stays_a_local() {
    let source = source_for(
        r#"
const headers = new Headers({ "content-type": "text/plain" });
console.log(headers.get("content-type") ?? "none");
"#,
    );

    assert!(!source.contains("SMELT_GLOBAL_HEADERS"), "{source}");
    assert!(source.contains("let headers: SmeltHeaders"), "{source}");
}
/// `x instanceof <concrete host class>` narrows an ERASED `x` in the true
/// branch, so the member read that follows is a concrete typed read rather
/// than an erased field probe that answers `undefined`, the same checked cast
/// a user type predicate (`x is Headers`) narrows through.
#[test]
fn instanceof_a_host_class_narrows_an_erased_local() {
    let source = source_for(
        r#"
const direct: unknown = new Headers({ a: "b" });
if (direct instanceof Headers) {
  console.log(direct.get("a") ?? "null");
}
"#,
    );

    // The guard is the marker probe, and the narrowed body materializes the
    // class through its checked adapter.
    assert!(
        source.contains("value.contains_key(\"__smelt_headers\")"),
        "{source}"
    );
    assert!(
        source.contains("<SmeltHeaders as SmeltFromUnknown>::smelt_from_unknown(direct"),
        "{source}"
    );
    // The narrowed read is typed, so no erased field probe is emitted.
    assert!(
        !source.contains("smelt_get_unknown_field(&direct.clone(), \"get\")"),
        "the erased member path must not be reached after narrowing\n{source}"
    );
}


/// A modeled class whose state is not a record erases through its own adapter,
/// carrying a registry identity marker, so `instanceof` on the erased value is
/// answered from that marker and not the generic struct path.
#[test]
fn host_value_erasure_stamps_the_registry_marker() {
    let source = source_for(
        r"
const enc: unknown = new TextEncoder();
console.log(enc instanceof TextEncoder);
",
    );

    // The adapter, not the declared-field record builder.
    assert!(
        source.contains("impl IntoSmeltUnknown for SmeltTextEncoder"),
        "{source}"
    );
    assert!(
        source.contains("(\"__smelt_textencoder\".to_owned(), SmeltUnknown::Bool(true))"),
        "{source}"
    );
    // The erased record keeps the value's OWN object id, so erasing one value
    // twice yields two `===`-equal objects.
    assert!(
        source.contains("SmeltObject::with_id(smelt_id,"),
        "{source}"
    );
    // And the live value is retained, so narrowing hands back the same object.
    assert!(
        source.contains("let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id,"),
        "{source}"
    );
    // The check is the shared marker probe.
    assert!(
        source.contains("value.contains_key(\"__smelt_textencoder\")"),
        "{source}"
    );
    assert!(
        !source.contains("(\"__smelt_class\".to_owned(), SmeltUnknown::String(\"TextEncoder\""),
        "the generic struct erasure must not claim a host runtime type\n{source}"
    );
}

/// A `node:http` `Server` erases but does NOT recover.
///
/// A server is a live listening socket with a handler closure and a tokio
/// shutdown sender; there is no empty server to fall back to, so rather than
/// fabricate one that is not listening the emitter writes no `SmeltFromUnknown`
/// and `StdlibClass::narrows_from_erased` excludes it. Its `instanceof` is
/// still answered, because identity is knowable where reconstruction is not.
#[test]
fn a_server_erases_but_does_not_recover() {
    let source = source_for(
        r#"
import { createServer } from "node:http";

const server = createServer((_req, res) => {
  res.end("ok");
});
const erased: unknown = server;
console.log(erased instanceof Server);
"#,
    );

    assert!(
        source.contains("impl IntoSmeltUnknown for SmeltHttpServer"),
        "{source}"
    );
    assert!(
        !source.contains("impl SmeltFromUnknown for SmeltHttpServer"),
        "a server has no honest empty value to recover to\n{source}"
    );
    assert!(
        source.contains("value.contains_key(\"__smelt_httpserver\")"),
        "{source}"
    );
}

/// The host-origin helpers are pay-for-use.
///
/// Only the classes whose state is not a record retain their live value (in the
/// erased record's origin slot), so a program that erases none of them must not
/// carry the helpers.
#[test]
fn the_host_origin_registry_is_pay_for_use() {
    let without = source_for(
        r"
const value: unknown = { a: 1 };
console.log(value);
",
    );
    assert!(!without.contains("fn smelt_with_host_origin"), "{without}");

    let with = source_for(
        r"
const enc: unknown = new TextEncoder();
console.log(enc);
",
    );
    assert!(with.contains("fn smelt_with_host_origin"), "{with}");
}

/// A class backed by a generated runtime type is not reflectively constructible
/// as a marker record.
///
/// Reflected construction has only a class NAME to work from, so for a host
/// class it builds a record carrying that class's marker. For a runtime-typed
/// class that record would answer `instanceof` correctly and then silently fail
/// every method called on it. `Blob` is the deliberate exception: its reflected
/// constructor shares one record definition with its erasure.
#[test]
fn runtime_typed_host_classes_are_not_reflectively_constructible() {
    let source = source_for(
        r"
const value: unknown = new TextEncoder();
console.log(value);
",
    );

    let table = source
        .lines()
        .find(|line| line.contains("fn smelt_builtin_construct_kind"))
        .unwrap_or_default();
    for excluded in [
        "TextEncoder",
        "TextDecoder",
        "EventEmitter",
        "IncomingMessage",
        "ServerResponse",
    ] {
        assert!(
            !table.contains(&format!("(\"{excluded}\"")),
            "{excluded} must not be reflectively constructible as a marker record\n{table}"
        );
    }
    assert!(table.contains("(\"Blob\", \"blob\")"), "{table}");
}

/// Every host-object marker lives in the internal `__smelt` key namespace.
///
/// The erased field store counts its keys in that namespace
/// (`SmeltFieldStore::has_internal_keys`), and every marker probe —
/// `smelt_object_has_host_marker`, `smelt_host_buffer_marker`, the
/// regexp/error/arguments checks of `for...in` — answers `false` without
/// scanning when the count is zero. A marker outside the namespace would make
/// that fast path report a host object as a plain record.
#[test]
fn every_host_marker_is_in_the_internal_key_namespace() {
    for marker in smelt_stdlib::host_object_markers() {
        assert!(marker.starts_with("__smelt"), "host marker {marker} is outside the internal namespace");
    }
    let source = source_for(
        r"
const enc: unknown = new TextEncoder();
console.log(JSON.stringify(enc), JSON.stringify({ a: 1 }));
",
    );
    assert!(
        source.contains("fn smelt_is_internal_key(&self) -> bool { self.starts_with(\"__smelt\") }"),
        "{source}"
    );
    assert!(
        source.contains("object.store.borrow().has_internal_keys() && "),
        "{source}"
    );
}
