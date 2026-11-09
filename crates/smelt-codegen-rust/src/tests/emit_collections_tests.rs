//! Generated Rust for lists, maps, records and iteration.

use super::*;

/// A concrete numeric list cannot share storage with a list of tagged values.
#[test]
fn concrete_list_erasure_still_converts_elements() {
    let source = source_for(include_str!("../../tests/fixtures/erased_generic_list.ts"));
    let body = emitted_function_body(&source, "fn concrete_array");
    assert!(
        body.contains("SmeltUnknown::Number("),
        "concrete numeric elements still need conversion into tagged values: {body}"
    );
    assert!(
        body.contains("collect::<Vec<_>>()"),
        "different element representations require a new buffer: {body}"
    );
    assert!(
        !body.contains(ERASED_GENERIC_ARRAY_ALIAS_FN),
        "a concrete list is not already erased, so it must not take the alias boundary: {body}"
    );
}

/// A source `unknown[]` erased into `unknown` keeps its direct wrap: only an
/// erased generic list leaving the erased-generic ABI takes the named alias
/// helper the SmeltUnknown report counts as a boundary. A program with no
/// generic list carries no helper.
#[test]
fn source_unknown_list_erasure_keeps_its_direct_wrap() {
    let source = source_for(
        r"
export function eraseUnknown(data: unknown[]): unknown {
  return data
}
",
    );
    let body = emitted_function_body(&source, "fn erase_unknown");
    assert!(
        body.contains("SmeltUnknown::Array(") && body.contains(".into())"),
        "an `unknown[]` already has the erased representation: {body}"
    );
    assert!(
        !body.contains("collect::<Vec<_>>()"),
        "an `unknown[]` must share its storage, not rebuild it: {body}"
    );
    assert!(!source.contains(ERASED_GENERIC_LIST_ALIAS_FN), "{source}");
    assert!(!source.contains(ERASED_GENERIC_ARRAY_ALIAS_FN), "{source}");
}

/// A dotted write on a record widened by a symbol key must insert through the
/// dictionary API; a `.get(..)` read expression is never an assignable place.
#[test]
fn dotted_write_to_unknown_key_record_inserts_string_key() {
    let source = source_for(
        r"
export function cycle(): unknown {
  const symbolKey = Symbol('key')
  const complex = { loop: null, [symbolKey]: 'symbol' }
  complex.loop = complex
  return complex
}
",
    );

    // The record is a property-key record (its keys were the erased carrier
    // before symbols were a type), so the dotted member is inserted under the
    // property key `"loop"`.
    assert!(
        source.contains(".insert(SmeltToPropKey::smelt_to_prop_key(&(JsString::from(\"loop\"))),"),
        "{source}"
    );
    assert!(!source.contains("expect(\"missing field\") ="), "{source}");
}

#[test]
fn injects_url_dependency_for_url_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Url)],
        GeneratedAllocator::System,
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
    );

    assert!(manifest.contains("url = \"2\""));
}

#[test]
fn hides_internal_metadata_from_erased_object_projection() {
    let source = source_for(
        r"
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const dateCount = keyCount(new Date(0));
const regexpCount = keyCount(/abc/u);
",
    );

    assert!(
        source.contains("smelt_is_for_in_record_key"),
        "erased object projections must filter internal metadata keys: {source}"
    );
    assert!(
        source.contains("\"__smelt_regexp\".to_owned()"),
        "RegExp erasure must keep a marker so source/flags stay non-enumerable: {source}"
    );
    assert!(
        source.contains("SmeltRegExp::new(JsString::from(\"abc\"), JsString::from(\"u\"))"),
        "RegExp literal arguments should lower to RegExp values, not strings: {source}"
    );
}

#[test]
fn emits_total_unknown_primitive_and_object_extraction() {
    let source = source_for(
        r"
declare const value: unknown;
const text: string = value as string;
const count: number = value as number;
const flag: boolean = value as boolean;
const bag: Record<string, unknown> = value as Record<string, unknown>;
",
    );

    assert!(
        source.contains("SmeltUnknown::Null => String::new()"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false"),
        "{source}"
    );
    // The numeric extraction of an erased string is the shared
    // `StringToNumber` helper rather than a bare `parse::<f64>()`.
    assert!(
        source.contains("SmeltUnknown::String(value) => smelt_js_string_to_number(&value)"),
        "{source}"
    );
    assert!(source.contains("_ => SmeltRecord::new()"), "{source}");
}

#[test]
fn casts_unknown_arrays_to_string_keyed_records() {
    let source = source_for(
        r#"
declare const value: unknown;
const bag: Record<string, unknown> = value as Record<string, unknown>;
const first = bag["0"];
"#,
    );

    // An array's own-key view is its element indices followed by the named
    // properties in its side table, which is exactly `own_entries`.
    assert!(
        source.contains("SmeltUnknown::Array(smelt_values) => smelt_values.own_entries().into_iter().map(|(smelt_key, smelt_value)| (JsString::from(smelt_key), smelt_value)).collect()"),
        "{source}"
    );
}

#[test]
fn emits_javascript_array_property_key_coercion() {
    let source = source_for(
        r#"
const record: Record<string, number> = { short: 5 };
const key = ["short"];
const value = record[key];
"#,
    );

    assert!(
        source.contains(".collect::<Vec<_>>().join(\",\")"),
        "{source}"
    );
    assert!(
        !source.contains("values.get(&\"[object Object]\""),
        "{source}"
    );
}

#[test]
fn preserves_order_when_casting_unknown_objects_to_records() {
    let source = source_for(
        r"
declare const value: unknown;
const bag: Record<string, unknown> = value as Record<string, unknown>;
const entries = Object.entries(bag);
",
    );

    // Recovering an erased object at its record type hands back a second
    // handle on the object's OWN field store, which carries the key order (and
    // the reference identity) rather than re-deriving either.
    assert!(
        source.contains(
            "fn smelt_shared_record(&self) -> SmeltRecord<JsString, SmeltUnknown> { SmeltRecord { id: self.id, store: ::std::rc::Rc::clone(&self.store) } }"
        ),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Object(smelt_value) => smelt_value.smelt_shared_record()"),
        "{source}"
    );
}

/// Erasing a record to a JavaScript object must carry the record's key order
/// across, not re-derive it.
///
/// `SmeltObject` has always CARRIED an `order` vector — `iter`/`keys`/`values`
/// and the serde impl all read it — but its constructors used to take an
/// unordered `HashMap` and recover an order by SORTING the keys. Every erasure
/// site fed them `record.iter().collect()`, so the ordered entry stream the
/// record had just produced was dropped into a hash map and then alphabetised:
/// `{ foo: 1, bar: 2, baz: 3 }` erased to an object whose `Object.keys` read
/// `["bar", "baz", "foo"]`. The constructors now take the ordered entry
/// sequence, which is the only form that can express a JavaScript object's key
/// order at all.
#[test]
fn erasing_a_record_to_an_object_keeps_the_source_key_order() {
    let source = source_for(
        r"
const plain = { foo: 1, bar: 2, baz: 3 };
const erased: unknown = plain;
",
    );

    assert!(
        source.contains("fn new<K: Into<JsString>>(entries: Vec<(K, SmeltUnknown)>) -> Self"),
        "{source}"
    );
    assert!(
        source.contains("fn with_id<K: Into<JsString>>(id: usize, entries: Vec<(K, SmeltUnknown)>) -> Self"),
        "{source}"
    );
    // No constructor may guess an order back out of an unordered map.
    assert!(
        !source.contains("let mut order = values.keys().cloned().collect::<Vec<_>>(); order.sort();"),
        "{source}"
    );
}

/// JavaScript own-key order is not plain insertion order: array-index keys come
/// first in ascending numeric order, then the remaining string keys in insertion
/// order (`OrdinaryOwnPropertyKeys`). Both erased containers must therefore place
/// a newly inserted key rather than push it, so `{ b: 1, 2: "x", a: 3, 1: "y" }`
/// enumerates as `1, 2, b, a`. The ordering is maintained at insert time so
/// `keys()` stays a plain read of one ordered structure.
#[test]
fn object_and_record_inserts_follow_javascript_own_key_order() {
    let source = source_for(
        r#"
const mixed = { b: 1, 2: "x", a: 3, 1: "y" };
const erased: unknown = mixed;
"#,
    );

    assert!(
        source.contains("fn smelt_canonical_array_index(key: &str) -> Option<u32>"),
        "{source}"
    );
    assert!(
        source.contains(
            "fn smelt_js_key_order_position<K: SmeltPropertyKey, V>(entries: &[SmeltFieldEntry<K, V>], key: &K) -> usize"
        ),
        "{source}"
    );
    // Both containers share ONE ordered store, so the placement happens once,
    // in `SmeltFieldStore::insert`; neither appends unconditionally.
    assert_eq!(
        source
            .matches("let position = smelt_js_key_order_position(&self.entries, &key);")
            .count(),
        1,
        "{source}"
    );
    assert!(
        source.contains("self.entries.insert(position, SmeltFieldEntry { fingerprint, key, value });"),
        "{source}"
    );
}

#[test]
fn emits_array_iterator_next_as_typed_result() {
    let source = source_for(
        r#"
export function firstEntry(values: string[]): string | undefined {
  const iterator = values.entries();
  const result = iterator.next();
  if ("done" in result && result.done) {
    return undefined;
  }
  return result.value[1];
}
"#,
    );

    assert!(
        source.contains(
            "if iterator.is_empty() { SmeltGeneratorResult::Complete(()) } else { SmeltGeneratorResult::Yielded(iterator.borrow_mut().remove(0)) }"
        ),
        "iterator next should consume into a typed result: {source}"
    );
    assert!(
        emitted_function_body(&source, "fn first_entry(")
            .contains("matches!(result.clone(), SmeltGeneratorResult::Complete(_))"),
        "iterator done should inspect the typed result: {source}"
    );
    assert!(
        !emitted_function_body(&source, "fn first_entry(").contains("SmeltUnknown"),
        "the iterator protocol stays typed: {source}"
    );
}

#[test]
fn copies_erased_object_rest_destructuring_properties() {
    let source = source_for(
        r"
declare function getValue(): unknown;
declare function makeCall(): (...args: unknown[]) => void;

const source = getValue() as { call: (...args: unknown[]) => void; cancel: () => void; flush: () => void };
const { call, ...rest } = source;
const callable = makeCall();
const merged = Object.assign(callable, rest);
merged.cancel();
merged.flush();
",
    );

    assert!(
        source.contains("SmeltRecord::with_id_from_entries(smelt_map.id, smelt_map.into_iter())")
            || source.contains("SmeltRecord::with_id_from_entries(smelt_values.id"),
        "{source}"
    );
    assert!(source.contains(".remove(&JsString::from(\"call\"))"), "{source}");
    assert!(source.contains(".extend("), "{source}");
}

#[test]
fn dynamic_object_destructuring_defaults_do_not_require_missing_fields() {
    let source = source_for(
        r"
export function pick(options: Record<string, unknown>): unknown {
  const { leading = false, trailing = true, maxWait } = options;
  return [leading, trailing, maxWait];
}
",
    );

    // A missing field falls back instead of panicking: a destructuring read,
    // like any property read, goes through `smelt_record_property`, which
    // consults the prototype entries an erased object carries and answers
    // `undefined` on a miss; the requirement here is only that it does not
    // `expect` the field.
    for field in ["leading", "trailing", "maxWait"] {
        assert!(
            source.contains(&format!("smelt_record_property(&options, &JsString::from(\"{field}\"))")),
            "{field}: {source}"
        );
    }
    assert!(!source.contains("expect(\"missing field\")"), "{source}");
}

#[test]
fn emits_typed_array_from_element_literal_through_the_element_codec() {
    // `new Uint8Array([1, 2, 3])` hands its element list to the family's
    // element constructor, which encodes each element at the view's own width;
    // an indexed read decodes it back at that same width. It used to emit a
    // bare `vec![10.0, 20.0, 30.0]` numeric list, which is why the view had no
    // identity, no `.buffer`, and a byte count for its `length`.
    let source = source_for(
        r"
export function first(): number {
  const values = new Uint8Array([10, 20, 30]);
  return values[0];
}
",
    );

    assert!(source.contains("vec![10.0, 20.0, 30.0]"), "{source}");
    assert!(
        source.contains("SmeltTypedArray::with_elements(SmeltTypedArrayKind::Uint8"),
        "{source}"
    );
    assert!(source.contains("values.get("), "{source}");
    // The codec is the kind's, in one place, so every width and signedness
    // decodes the way the spec says.
    assert!(
        source.contains("pub fn decode(self, bytes: &[u8], at: usize) -> f64"),
        "{source}"
    );
}

#[test]
fn inserts_unknown_iterable_values_into_typed_sets() {
    let source = source_for(
        r"
declare function values(): unknown;

export function collect(): Set<number> {
  const results = new Set<number>();
  for (const value of values() as Iterable<unknown>) {
    results.add(value as number);
  }
  return results;
}
",
    );

    assert!(
        source.contains("match value")
            && (source.contains("results.push(") || source.contains("results.insert(")),
        "{source}"
    );
    assert!(!source.contains("Default::default();\n"), "{source}");
}

#[test]
fn emits_object_literal_as_destination_interface_struct() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

function format(options: Options): string {
  return options.width ?? "full";
}

export function run(): string {
  return format({ width: "short" });
}
"#,
    );

    // The literal IS the struct: `Options { width: Some(..) }`, with no
    // `SmeltRecord<String, SmeltUnknown>` materialized and reconstructed. The
    // reconstruction this used to assert read three keys per field and
    // funnelled every tag through `to_string()`, which is a wrong value for any
    // non-string optional field.
    assert!(source.contains("Options { width: Some("), "{source}");
    assert!(!source.contains("smelt_record_map"), "{source}");
    assert!(
        !source.contains("format(::std::collections::HashMap::from"),
        "{source}"
    );
}

#[test]
fn matches_object_literal_keys_to_interface_fields_by_source_spelling() {
    // A JavaScript property key is case-sensitive and never case-folded; an
    // interface FIELD has two spellings, the source name and the Rust-safe
    // rendering the struct field carries. Matching the literal's keys against
    // the RENDERED spelling silently dropped every field whose source name is
    // not already a valid Rust name: the field looked absent, took the optional
    // default, and the program printed `undefined` where Node prints the value.
    let source = source_for(
        r#"
interface Shape {
  plain?: number;
  camelCase?: string;
  snake_case?: string;
}

export function build(): Shape {
  return { plain: 1, camelCase: "a", snake_case: "b" };
}
"#,
    );

    assert!(
        source.contains(
            "Shape { plain: Some(1.0), camel_case: Some(JsString::from(\"a\")), snake_case: Some(JsString::from(\"b\")) }"
        ),
        "{source}"
    );
    // The specific defect: a camelCase key must not fall through to the
    // optional default.
    assert!(!source.contains("camel_case: None"), "{source}");
}

#[test]
fn builds_a_nested_optional_interface_literal_as_a_struct() {
    // The nested literal receives the field's type as its own hint, so both
    // levels are built directly. Without it the inner literal lowered as a
    // `Dict`, which made the OUTER literal unbuildable as a struct too, and
    // both ends went through the erased `SmeltRecord` reconstruction.
    let source = source_for(
        r#"
interface Inner {
  camelCase?: string;
  count?: number;
}

interface Outer {
  innerShape?: Inner;
}

export function build(): Outer {
  return { innerShape: { camelCase: "deep", count: 2 } };
}
"#,
    );

    assert!(
        source.contains("Inner { camel_case: Some(JsString::from(\"deep\")), count: Some(2.0) }"),
        "{source}"
    );
    assert!(source.contains("Outer { inner_shape: Some("), "{source}");
    assert!(!source.contains("smelt_record_map"), "{source}");
}

#[test]
fn adapts_object_literal_for_interface_callback_field_argument() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

interface FormatLong {
  date: (options: Options) => string;
}

export function run(formatLong: FormatLong): string {
  return formatLong.date({ width: "short" });
}
"#,
    );

    assert!(
        source.contains("(format_long.date.clone())({ let smelt_record_map ="),
        "{source}"
    );
    assert!(
        source.contains(
            "Options { width: smelt_record_map.get_text(\"width\").cloned().map(|smelt_value| smelt_value) }"
        ),
        "{source}"
    );
}

#[test]
fn emits_empty_object_literal_as_optional_interface_struct_defaults() {
    let source = source_for(
        r"
interface Duration {
  years?: number;
  months?: number;
}

export function make(): Duration {
  return {};
}
",
    );

    // An empty literal for an all-optional interface is the struct with every
    // field absent -- what a hand-writing Rust team would spell. It used to
    // build an erased record and then project each field back out of it,
    // consulting `__smelt_proto:`/`__smelt_method:` slots for keys the
    // interface declares and the literal simply does not set.
    // `None::<f64>` rather than a bare `None`: the field's own type annotation
    // is what makes the absent arm inferable at the construction site.
    assert!(
        source.contains("Duration { years: None::<f64>, months: None::<f64> }"),
        "{source}"
    );
    assert!(!source.contains("smelt_record_map"), "{source}");
}

#[test]
fn preserves_optional_record_arguments_and_defaults_missing_fields() {
    let source = source_for(
        r"
function throttle(
  callback: () => void,
  wait = 0,
  { leading = true, trailing = true }: { leading?: boolean; trailing?: boolean } = {},
): boolean {
  callback();
  return leading && trailing && wait >= 0;
}

const explicit = throttle(() => {}, 1, { leading: false });
const defaulted = throttle(() => {});
",
    );

    assert!(
        source.contains(".get(&JsString::from(\"leading\")).flatten()")
            && source.contains(".unwrap_or(true)"),
        "{source}"
    );
    assert!(
        source.contains("SmeltRecord::from([(JsString::from(\"leading\"), Some(false))])"),
        "{source}"
    );
    assert!(
        source.contains(", 1.0, _smelt_tmp_3)"),
        "explicit object argument should remain in the call: {source}"
    );
}

#[test]
fn coerces_function_typed_dict_literal_values_to_trait_objects() {
    let source = source_for(
        r#"
export function callbacks(): Record<string, (value: number) => string> {
  const suffix = "!";
  return {
    a: (value: number) => value.toString(),
    b: (value: number) => value.toString() + suffix,
  };
}
"#,
    );

    assert!(
        source
            .matches("let smelt_fn: ::std::rc::Rc<dyn Fn(f64) -> JsString>")
            .count()
            >= 2,
        "{source}"
    );
}

#[test]
fn unwraps_throwing_array_map_callbacks_before_collecting() {
    let source = source_for(
        r#"
function run(values: string[]): string[] {
  return values.map((value) => {
    if (value === "x") {
      throw new Error(value);
    }
    return value;
  });
}
"#,
    );

    assert!(
        source.contains(".map(|(smelt_index, smelt_item)| { ((smelt_callback)(")
            && source.contains(
                ")).unwrap_or_else(|smelt_error: Box<dyn std::error::Error>| smelt_panic_throw(smelt_error))"
            ),
        "{source}"
    );
}

#[test]
fn projects_fields_from_optional_interface_records() {
    let source = source_for(
        r"
interface Options {
  comparison?: number;
}

function read(options?: Options): number {
  return options?.comparison ?? 0;
}

function positive(options?: Options): boolean {
  return options.comparison > 0;
}
",
    );

    assert!(
        source.contains(
            "options.clone().as_ref().and_then(|_smelt_value| _smelt_value.comparison.clone())"
        ),
        "{source}"
    );
    assert!(source.contains(".unwrap_or(0.0) > 0.0"), "{source}");
}

#[test]
fn emits_escaping_closure_spread_calls_with_owned_callback_state() {
    let source = source_for(
        r"
export function purryOn(
  isArg: (x: unknown) => boolean,
  implementation: (data: unknown, first: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return isArg(args[0])
    ? (data: unknown) => implementation(data, ...args)
    : implementation(args[0], args[1], args.slice(2));
}
",
    );

    assert!(
        source.contains("implementation: ::std::rc::Rc<dyn Fn"),
        "{source}"
    );
    assert!(source.contains("(implementation)("), "{source}");
    assert!(source.contains("args.get("), "{source}");
    // What this test is really about: the ESCAPING closure must own its captured
    // `args`, because it outlives the frame. That capture still clones.
    assert!(source.contains("let args = args.clone();"), "{source}");
    // The immediately-evaluated `args.slice(2)` in the other branch is a different
    // expression, and it reads `args` only through `&self` methods, so it borrows.
    // It used to emit `args.clone().iter().skip(` — a whole-`Vec` copy for a read.
    assert!(
        source.contains("args.borrow().iter().skip(")
            || source.contains("SmeltUnknown::Array(args.clone().into())"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Array(args.clone()).get"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Array(args.clone()).clone().into_iter()"),
        "{source}"
    );
}

#[test]
fn adapts_rest_callback_shape_to_trailing_list_parameter() {
    let source = source_for(
        r"
function purryOn(
  isArg: (x: unknown) => boolean,
  implementation: (data: unknown, first: unknown, rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return isArg(args[0])
    ? (data: unknown) => implementation(data, ...args)
    : implementation(args[0], args[1], args.slice(2));
}

function implementation(data: unknown, ...cases: Array<unknown>): unknown {
  return cases;
}

export function conditional(args: Array<unknown>): unknown {
  return purryOn((x) => true, implementation, args);
}
",
    );

    assert!(
        source.contains("smelt_forwarded_args.push(smelt_arg1"),
        "{source}"
    );
    assert!(
        source.contains("smelt_forwarded_args.extend(smelt_arg2.clone().into_iter()"),
        "{source}"
    );
    assert!(
        !source.contains("if let SmeltUnknown::Array(values) = arg1.clone()"),
        "{source}"
    );
}

#[test]
fn spreads_trailing_rest_vectors_across_erased_function_boundaries() {
    let source = source_for(
        r"
function callConcrete(
  callback: (data: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return callback(args[0], ...args.slice(1));
}

export function useConcrete(): unknown {
  return callConcrete((data, ...rest) => rest, [1, 2, 3]);
}

export function wrapConcrete(): unknown {
  const callback: unknown = (data: unknown, ...rest: Array<unknown>) => rest;
  return callback;
}

",
    );

    assert!(
        source.contains("smelt_args.iter().skip(1).cloned().collect::<SmeltList<_>>()"),
        "{source}"
    );
    assert!(!source.contains("SmeltUnknown::Array(arg1)"), "{source}");
}

#[test]
fn does_not_spread_array_parameters_across_erased_function_boundaries() {
    let source = source_for(
        r"
export function wrapArrayParam(): unknown {
  const callback: unknown = (data: unknown, values: Array<unknown>) => values;
  return callback;
}
",
    );

    assert!(
        source.contains("smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Null)"),
        "{source}"
    );
    assert!(
        !source.contains("smelt_args.iter().skip(1).cloned().collect::<Vec<_>>()"),
        "{source}"
    );
}

#[test]
fn passes_a_mutated_structural_record_as_a_shared_handle() {
    // A record whose field is written after construction is a JavaScript
    // reference value, so it lowers to the reference-record handle newtype (see
    // `classify::reference_classes`) rather than being threaded through Rust's
    // `&mut` ABI. The callee's write goes through the shared cell, so the caller
    // observes it while the value is still passed by value.
    let source = source_for(
        r"
interface Flags {
  era?: number;
}

function setEra(flags: Flags, value: number): void {
  flags.era = value;
}

function readEra(): number {
  const flags: Flags = {};
  setEra(flags, 1);
  return flags.era!;
}
",
    );

    assert!(
        source.contains("struct Flags(::std::rc::Rc<::std::cell::RefCell<FlagsInner>>);"),
        "{source}"
    );
    assert!(source.contains("fn set_era(mut flags: Flags, value: f64)"), "{source}");
    assert!(source.contains("flags.0.borrow_mut().era ="), "{source}");
}

#[test]
fn emits_mutable_function_field_parameters_for_structural_objects() {
    let source = source_for(
        r"
interface Flags {
  era?: number;
}

interface Setter {
  set: (flags: Flags) => number | [number, Flags];
}

function run(setter: Setter): number {
  const flags: Flags = {};
  setter.set(flags);
  return flags.era!;
}
",
    );

    assert!(
        source.contains("set: ::std::rc::Rc<dyn Fn(&mut Flags) -> SmeltUnion"),
        "{source}"
    );
    assert!(
        source.contains("(setter.set.clone())(&mut flags)"),
        "{source}"
    );
}

#[test]
fn emits_delete_on_erased_object_surfaces() {
    let source = source_for(
        r"
function removeKey(value: unknown, key: string): boolean {
  return delete value[key];
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Object(smelt_map) => smelt_map.remove"),
        "{source}"
    );
    assert!(source.contains("_ => true"), "{source}");
}

#[test]
fn emits_unknown_record_literals_with_tagged_values() {
    let source = source_for(
        r#"
const value: Record<string, unknown> = { done: false, name: "skip" };
"#,
    );

    assert!(source.contains("::std::collections::HashMap<String, SmeltUnknown>"));
    assert!(source.contains("SmeltUnknown::Bool(false)"));
    assert!(source.contains("SmeltUnknown::String(\"skip\".into())"));
}

#[test]
fn emits_erased_dict_index_assignment_with_key_coercion() {
    let source = source_for(
        r"
function assign(out: Record<string, unknown>, key: unknown, value: unknown): Record<string, unknown> {
  out[key as string] = value;
  return out;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value)"),
        "{source}"
    );
    assert!(
        source.contains("out.insert(_smelt_tmp_3.clone(), value.clone());"),
        "{source}"
    );
}

#[test]
fn emits_list_operand_item_coercion() {
    let source = source_for(
        r"
function widen(values: string[]): unknown[] {
  return values;
}
",
    );

    assert!(
        source.contains(
            "smelt_l.into_iter().map(|smelt_value| SmeltUnknown::String(smelt_value.into())).collect::<Vec<_>>()"
        ),
        "{source}"
    );
}

#[test]
fn emits_string_chars_into_unknown_list_destination() {
    let source = source_for(
        r"
function chars(value: string): unknown[] {
  return [...value];
}
",
    );

    assert!(
        source.contains(".map(|smelt_value| SmeltUnknown::String(smelt_value.into())).collect::<Vec<_>>()"),
        "{source}"
    );
}

#[test]
fn emits_tuple_into_unknown_runtime_conversion() {
    let source = source_for(
        r"
function pair(value: unknown): unknown {
  return [value, value] as [unknown, unknown];
}
",
    );

    assert!(
        source
            .contains("impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B)"),
        "{source}"
    );
}

#[test]
fn ambient_binding_reads_the_global_object_member() {
    // An AMBIENT binding has no initializer: the HOST provides it, as a member
    // of the global object. Its read is that member (a dynamic boundary), not
    // a fabricated default record of the declared type — which is what this
    // test used to pin. (A binding WITH an initializer, `let defaultOptions:
    // DefaultOptions = {}`, is a module slot holding its one evaluated value
    // instead — see `module_bound_intersection_alias_reads_its_slot`.)
    let source = source_for(
        r"
type A = { locale?: unknown };
type B = { weekStartsOn?: number };
type DefaultOptions = A & B;

declare const defaultOptions: DefaultOptions;

export function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
",
    );

    // The member read off the ONE global object, with no default record
    // fabricated in between.
    assert!(
        source.contains("let _smelt_tmp_0: SmeltUnknown = smelt_global_object();")
            && source.contains(
                "let _smelt_tmp_1: SmeltUnknown = smelt_get_unknown_field(&_smelt_tmp_0.clone(), \"defaultOptions\").clone();"
            ),
        "{source}"
    );
}

#[test]
fn omits_absent_optional_fields_when_erasing_structural_objects() {
    let source = source_for(
        r"
interface Flags {
  era?: number;
}

declare const flags: Flags;
const erased: unknown = flags;
",
    );

    assert!(
        source.contains("if let Some(smelt_value) = smelt_object_value.era.clone()"),
        "{source}"
    );
    assert!(
        !source.contains("(\"era\".to_owned(), smelt_object_value.era.clone().map_or"),
        "{source}"
    );
}

#[test]
fn emits_record_field_assignment_as_insert() {
    let source = source_for(
        "let user: Record<string, string> = { name: \"Ada\" };
user.name = \"Grace\";
console.log(user.name);
",
    );

    assert!(
        source.contains("let mut user: SmeltRecord<JsString, JsString>"),
        "{source}"
    );
    assert!(
        source.contains("user.insert(JsString::from(\"name\"), JsString::from(\"Grace\"));"),
        "{source}"
    );
    assert!(
        source.contains("user.get(&JsString::from(\"name\")).expect(\"missing field\")"),
        "{source}"
    );
}

#[test]
fn reinserts_dynamic_record_list_alias_after_push() {
    let source = source_for(
        r"
function group(values: string[]): Record<string, string[]> {
  const output: Record<string, string[]> = {};
  for (const value of values) {
    const key = value[0];
    const items = output[key];
    if (items === undefined) {
      output[key] = [value];
    } else {
      items.push(value);
    }
  }
  return output;
}
",
    );

    // `items` is both assigned a keyed read and presence-tested, so it holds
    // `string[] | undefined` (see `presence_tested_locals`): the read is the
    // optional lookup itself, and `items === undefined` tests it. The pushed
    // list is the record's own shared list, so no re-insertion is needed.
    assert!(
        source.contains("let mut items: Option<SmeltList<JsString>>;"),
        "{source}"
    );
    assert!(source.contains("items.clone().is_none()"), "{source}");
    assert!(
        !source.contains(".cloned().unwrap_or(SmeltList::new(Vec::<JsString>::new()))"),
        "a missing key must read as absent, not as a fresh empty list: {source}"
    );
}

#[test]
fn preserves_computed_symbol_object_literal_properties() {
    let source = source_for(
        r#"
function read(): unknown {
  const SymbolKey = Symbol("kind");
  const item = { [SymbolKey]: "cat", [Symbol("inline")]: "dog", 2: 123 };
  return item[SymbolKey];
}
"#,
    );

    // Both symbol keys are typed symbols (they were erased
    // `SmeltUnknown::Symbol` literals before symbols were a type), each a new
    // symbol per evaluation, and the literal they key is a property-key
    // record holding all three properties.
    assert!(
        source.contains("SmeltSymbol::unique(SmeltSymbol::from_spelling(\"Symbol(kind)"),
        "{source}"
    );
    assert!(
        source.contains("SmeltSymbol::unique(SmeltSymbol::from_spelling(\"Symbol(inline)"),
        "{source}"
    );
    // Old requirement: the record was a `SmeltJsMap<SmeltPropKey, _>`. New:
    // a property-key record is an object, `SmeltPropKeyRecord<_>` over the
    // object field store, so its erased form shares that store (remeda's
    // `clone`); the key is still the sealed `SmeltPropKey`.
    assert!(source.contains("let item: SmeltPropKeyRecord<SmeltUnknown>"), "{source}");
    assert!(
        source.contains("SmeltUnknown::String(\"cat\".into())"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::String(\"dog\".into())"),
        "{source}"
    );
}

#[test]
fn preserves_object_has_own_length_on_erased_arrays() {
    let source = source_for(
        r#"
function hasLength(value: unknown): boolean {
  return Object.hasOwn(value, "length");
}
"#,
    );

    assert!(
        source.contains("smelt_has_own_property("),
        "`Object.hasOwn` must ask the own-property authority:\n{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Array(values) => key == \"length\" || values.named_keys().contains(&key.to_owned())"
        ),
        "an array's `length` and named keys are own properties:\n{source}"
    );
    // The `hasOwn` lowering must inspect the erased value directly, not cast it
    // into a typed record. Scope the check to the source from the emitted
    // `fn has_length` onward: `SmeltRecord::with_id_from_entries` also
    // legitimately appears in the runtime prelude's `SmeltFromUnknown for
    // SmeltRecord` impl, which is unrelated to this program's behavior.
    let function_body = source
        .split("fn has_length")
        .nth(1)
        .expect("generated source defines has_length");
    assert!(
        !function_body.contains("SmeltRecord::with_id_from_entries"),
        "{source}"
    );
}

#[test]
fn projects_string_indices_when_casting_unknown_to_record() {
    let source = source_for(
        r"
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::String(smelt_value) => smelt_value.units().iter().enumerate()"),
        "{source}"
    );
}

#[test]
fn emits_unknown_field_assignment_as_object_insert() {
    let source = source_for(
        r#"
function assign(value: unknown): unknown {
  value.name = "Grace";
  return value;
}
"#,
    );

    assert!(source.contains("match &mut value"), "{source}");
    assert!(
        source.contains("SmeltUnknown::Object(smelt_map) => { smelt_map.insert(\"name\".to_owned(), SmeltUnknown::String(\"Grace\".into())); }"),
        "{source}"
    );
    assert!(
        source.contains("*other = SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"name\".to_owned(), SmeltUnknown::String(\"Grace\".into()))])));"),
        "{source}"
    );
}

#[test]
fn tuple_to_tuple_coercion_materializes_nontrivial_source_once() {
    // Regression: element-wise tuple coercion references the source once per
    // field. When the source is a non-trivial expression (an inlined call), it
    // must be materialized into a single temporary so by-value arguments are
    // not moved once per element (E0382).
    let source = source_for(
        r"
function pair(values: number[]): [number[], number[]] {
  return [values, values];
}
function widen(values: number[]): [unknown[], unknown[]] {
  return pair(values);
}
",
    );

    // The coercion must never emit the source call more than once: one
    // definition plus one call site. Duplicating it would re-move `values`
    // (E0382). This holds whether the call is bound to a temp (trivial source,
    // inlined coercion) or inlined into the coercion (non-trivial source,
    // materialized into `smelt_tuple_src`).
    assert_eq!(source.matches("pair(").count(), 2, "{source}");
}

#[test]
fn self_referential_list_push_evaluates_item_before_borrow() {
    // Regression: `array.push(array)` must evaluate the pushed item into a
    // temporary BEFORE taking the list's mutable borrow, or the item's read of
    // the list conflicts with the `&mut` push (E0502).
    let source = source_for(
        r"
function grow(): number {
  const array: unknown[] = [];
  return array.push(array);
}
",
    );

    assert!(source.contains("let smelt_push_item ="), "{source}");
    assert!(source.contains(".push(smelt_push_item)"), "{source}");
}

#[test]
fn adapts_nested_object_literal_option_fields_to_structural_records() {
    let source = source_for(
        r#"
interface Locale {
  code: string;
}

interface FormatOptions {
  locale?: Locale;
}

function format(options?: FormatOptions): string {
  return options?.locale?.code ?? "";
}

const customLocale = { code: "fr" };
const result = format({ locale: customLocale });
"#,
    );

    assert!(
        source.contains("FormatOptions { locale: smelt_record_map.get_text(\"locale\").map(|smelt_value|"),
        "{source}"
    );
    assert!(source.contains("Locale { code:"), "{source}");
    assert!(!source.contains("FormatOptions { locale: None"), "{source}");
}

#[test]
fn adapts_nested_required_records_when_retyping_mixed_option_bags() {
    let source = source_for(
        r#"
interface InputFormatLong {
  date: (value: string) => string;
}

interface InputLocale {
  formatLong: InputFormatLong;
}

interface OutputFormatLong {
  date: (value: string) => string;
}

interface OutputLocale {
  formatLong: OutputFormatLong;
}

interface FormatOptions {
  locale?: OutputLocale;
  weekStartsOn?: number;
}

function format(options?: FormatOptions): string {
  return options?.locale?.formatLong.date("P") ?? "";
}

function forward(locale: InputLocale): string {
  return format({ locale, weekStartsOn: 0 });
}
"#,
    );

    assert!(source.contains("OutputLocale {"), "{source}");
    assert!(source.contains("OutputFormatLong {"), "{source}");
    assert!(
        !source.contains(
            "locale: smelt_record_map.get(\"locale\").cloned().map(|value| Default::default())"
        ),
        "{source}"
    );
}

#[test]
fn bounds_recursive_locale_callback_option_record_adaptation() {
    let source = source_for(
        r#"
interface CallbackOptions {
  locale?: Locale;
}

interface Locale {
  formatRelative: (token: string, options?: CallbackOptions) => string;
}

function forward(value: unknown): string {
  const locale = value as Locale;
  return locale.formatRelative("today", { locale });
}
"#,
    );

    assert!(source.contains("Locale {"), "{source}");
    assert!(source.contains("format_relative:"), "{source}");
    assert!(
        !source.contains(
            "locale: smelt_record_map.get(\"locale\").cloned().map(|value| Default::default())"
        ),
        "{source}"
    );
}

#[test]
fn adapts_shorter_object_literal_callbacks_to_interface_field_signatures() {
    let source = source_for(
        r#"
interface Locale {
  localize: Localize;
}

interface Localize {
  month: (value: number, options?: string) => string;
}

interface FormatOptions {
  locale?: Locale;
}

function useLocale(options?: FormatOptions): string {
  return options?.locale?.localize.month(0) ?? "";
}

const customLocale = { localize: { month: () => "works" } };
const result = useLocale({ locale: customLocale });
"#,
    );

    assert!(source.contains("let smelt_adapted:"), "{source}");
    assert!(source.contains("move |smelt_arg0: f64"), "{source}");
    assert!(source.contains("(smelt_callback)()"), "{source}");
}

#[test]
fn adapts_shorter_callbacks_returning_records_to_optional_interface_results() {
    let source = source_for(
        r#"
interface MatchResult {
  value: number;
  rest: string;
}

interface Match {
  era: (text: string, options?: unknown) => MatchResult | null;
}

interface Locale {
  match: Match;
}

interface ParseOptions {
  locale?: Locale;
}

function useOptions(options?: ParseOptions): MatchResult | null {
  return options?.locale?.match.era("BC") ?? null;
}

const customLocale = {
  match: {
    era: () => ({ value: 0, rest: " works" }),
  },
};

const result = useOptions({ locale: customLocale });
"#,
    );

    assert!(source.contains("let smelt_adapted:"), "{source}");
    assert!(source.contains("(smelt_callback)()"), "{source}");
    assert!(
        source.contains("Option<MatchResult>> = ::std::rc::Rc::new(move |smelt_arg0: JsString")
            && source.contains("Some({ let smelt_record_map = (smelt_callback)().clone();"),
        "the provided shorter callback must be adapted to the optional result field: {source}"
    );
}

#[test]
fn emits_record_index_assignment_as_insert() {
    let source = source_for(
        "let user: Record<string, string> = { name: \"Ada\" };
let key = \"name\";
user[key] = \"Grace\";
console.log(user[key]);
",
    );

    assert!(
        source.contains("user.insert(key.clone(), JsString::from(\"Grace\"));"),
        "{source}"
    );
    assert!(
        source.contains("user.get(&key.clone()).unwrap_or(JsString::new())"),
        "{source}"
    );
}

#[test]
fn emits_array_from_length_mapper() {
    let source = source_for(
        r"
function range(start: number, length: number, step: number): number[] {
  return Array.from({ length }, (_, i) => (i === 0 ? start : start + i * step));
}
",
    );

    assert!(source.contains("array_from_length"), "{source}");
    assert!(source.contains("(0..smelt_array_from_length).map"), "{source}");
    assert!(
        source.contains("(smelt_callback)(&(SmeltUnknown::Null), smelt_index as f64)"),
        "{source}"
    );
}

#[test]
fn emits_erased_string_key_index_reads_for_arrays_and_strings() {
    let source = source_for(
        r"
export function read(value: unknown, key: string): unknown {
  return value[key as keyof typeof value];
}
",
    );

    assert!(source.contains("SmeltUnknown::Array(values)"), "{source}");
    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("smelt_key == \"length\""), "{source}");
    assert!(source.contains("parse::<usize>()"), "{source}");
    assert!(!source.contains("unknown is not object"), "{source}");
}

#[test]
fn lowers_an_array_hole_to_undefined_not_null() {
    // A hole in an array literal reads as `undefined` in JavaScript, never as
    // `null` — the index is absent, and an absent property read answers
    // `undefined`. Lowering it to `null` made `[1, , 2]` indistinguishable from
    // `[1, null, 2]`.
    let source = source_for(
        r"
export function holes(): unknown[] {
  return [1, , 2];
}
",
    );

    let body = source.split_once("fn holes(").expect("generated function").1;
    let body = body.split_once("\nfn ").map_or(body, |(head, _)| head);
    assert!(
        body.contains("SmeltUnknown::Undefined"),
        "the hole must be `undefined`: {body}"
    );
    assert!(
        !body.contains("SmeltUnknown::Null"),
        "the hole must not become `null`: {body}"
    );
}

#[test]
fn emits_object_symbol_iterator_generator_as_erased_iterable() {
    let source = source_for(
        r"
function count(items: Iterable<unknown>): number {
  const erased = items as unknown;
  return [...(erased as Iterable<unknown>)].length;
}

const result = count({
  *[Symbol.iterator]() {
    yield 0;
    yield 1;
  },
});
",
    );

    assert!(source.contains("\"__smelt_symbol_iterator\""), "{source}");
    assert!(
        source.contains("iterator(vec![]).unwrap_or(SmeltUnknown::Null)"),
        "{source}"
    );
    assert!(
        !source.contains("else { panic!(\"unknown is not array\") })"),
        "{source}"
    );
}

#[test]
fn adapts_rest_callback_without_flattening_list_arguments() {
    let source = source_for(
        r"
function purry<T>(
  callback: (...args: unknown[]) => unknown,
): (data: readonly T[], compare: (left: T, right: T) => number) => unknown {
  const n = 2;
  return (data, compare) => callback(data, compare, n);
}

function wrap<T>(
  func: (data: readonly T[], compare: (left: T, right: T) => number, n: number) => unknown,
): unknown {
  const n = 2;
  return purry((...args) => func(...args, n));
}
",
    );

    // Both fixed parameters are read out of the packed rest vector by index
    // (`__smelt_spread.get({..index..})`). The callable-typed parameter's read
    // is hoisted into a `let smelt_source_value = closure_arg_0.get(..)` binding
    // by the callable-object narrowing, so assert on the shared `.get(` read.
    //
    // `data: readonly T[]` is extracted out of the packed rest vector, and `T`
    // is not a Rust generic in this emission scope, so its elements are already
    // `SmeltUnknown`: the extraction re-wraps the SAME array rather than
    // rebuilding its element vector (see `erased_to_list_expr`). That is what
    // keeps a `purry`-style runtime dispatcher from paying an O(n) copy on every
    // crossing, and it is what makes the array a callback receives BE the array
    // being iterated.
    assert!(
        source.contains("__smelt_spread.borrow().get(")
            && source
                .contains("SmeltUnknown::Array(smelt_values) => SmeltList::with_storage(smelt_values.id,"),
        "fixed callback spread calls should read the first fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.matches("__smelt_spread.borrow().get(").count() >= 2,
        "fixed callback spread calls should read the second fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.contains("}, n)"),
        "fixed callback spread calls should keep trailing scalar arguments after spread expansion: {source}"
    );
}

#[test]
fn emits_tuple_element_push_as_concrete_tuple_value() {
    // Pushing a bare `[value, key]` literal into an `Array<[number, string]>`
    // emits a real `(f64, String)` tuple value rather than widening the pushed
    // item to a `SmeltUnknown` list that could never re-type into the tuple.
    let source = source_for(
        r"
export function collect(value: number, key: string): Array<[number, string]> {
  const result: Array<[number, string]> = [];
  result.push([value, key]);
  return result;
}
",
    );

    assert!(
        source.contains("SmeltList<(f64, JsString)>"),
        "tuple-element array should keep its concrete tuple element type: {source}"
    );
    assert!(
        source.contains("(value, key.clone())"),
        "pushed literal should be a concrete tuple value: {source}"
    );
    assert!(
        source.contains("result.borrow_mut().push("),
        "the tuple value should be pushed onto the array: {source}"
    );
    assert!(
        !source.contains("SmeltList<SmeltUnknown>"),
        "tuple-element push must not widen the array to SmeltUnknown: {source}"
    );
}

/// `String.prototype.match` emits the typed `Option<SmeltMatch>` value and an
/// array method on it reads the match's array view, with no erasure.
#[test]
fn emits_string_match_as_typed_match_with_array_view() {
    let source = source_for(
        r"
function marker(path: string): number {
  const m = path.match(/^(?:a()|b())$/);
  if (!m) return -1;
  return m.indexOf('', 1);
}
",
    );

    assert!(source.contains("Option<SmeltMatch> = "), "{source}");
    assert!(source.contains(".match_string(&"), "{source}");
    assert!(!source.contains(".map(SmeltList::from)"), "{source}");
    assert!(source.contains(".to_array_view()"), "{source}");
    assert!(!source.contains(".into_smelt_unknown()"), "{source}");
}

/// A match passed where `string[]` is expected is converted through its array
/// view (an unmatched group reads `""` there), never through `SmeltUnknown`.
#[test]
fn coerces_match_to_string_list_through_array_view() {
    let source = source_for(
        r"
function takes(xs: string[]): number { return xs.length; }
function count(text: string): number {
  const m = text.match(/(a)(b)?/);
  return m ? takes(m) : 0;
}
",
    );

    assert!(source.contains(".to_array_view()"), "{source}");
    assert!(!source.contains(".into_smelt_unknown()"), "{source}");
}

#[test]
fn coerces_rendered_list_values_to_tuple_destinations() {
    let source = source_for(
        r"
function invoke(
  values: unknown[],
  callback: (pair: [unknown, unknown]) => unknown,
): unknown {
  return callback(values);
}
",
    );

    assert!(
        source.contains("let smelt_tuple_values = values.clone()"),
        "{source}"
    );
    assert!(source.contains("smelt_tuple_values.get(0)"), "{source}");
    assert!(source.contains("smelt_tuple_values.get(1)"), "{source}");
}

#[test]
fn erased_array_indexed_assignment_sets_element_not_object() {
    // `result[i] = value` where `result` holds a runtime `SmeltUnknown::Array`
    // must set the array element (extending with `undefined`), not convert the
    // array into an object. The regression turned cloned arrays into objects
    // keyed by "0","1",..., breaking round-trips back to a typed list.
    let source = source_for(
        r"
export function fill(): unknown {
  const result: any = [];
  for (let i = 0; i < 3; i++) {
    result[i] = i;
  }
  return result;
}
",
    );

    // The call site routes through the prelude helper, which sets an array
    // element (via `set_index`) instead of converting the array to an object.
    assert!(
        source.contains("smelt_index_assign(&mut result,"),
        "erased indexed assignment must route through the prelude helper: {source}"
    );
    assert!(
        source.contains("array.set_index(index, value)"),
        "the prelude helper must set an array element at a numeric index: {source}"
    );
}

#[test]
fn flattens_python_dict_get_optional_value_type() {
    let source = source_for_py(
        r#"
from typing import Optional

def read(values: dict[str, Optional[int]]) -> Optional[int]:
    return values.get("x")
"#,
    );

    assert!(
        source.contains("values: SmeltRecord<String, Option<i64>>"),
        "{source}"
    );
    assert!(!source.contains("Option<Option<i64>>"), "{source}");
}

#[test]
fn flattens_optional_list_index_over_optional_items() {
    let source = source_for(
        r"
function read(values: Array<number | undefined>, index: number): number | undefined {
  return values?.[index];
}
",
    );

    assert!(source.contains(".cloned().flatten()"), "{source}");
    assert!(!source.contains("Option<Option<f64>>"), "{source}");
}

#[test]
fn emits_unknown_index_assignment_as_object_mutation() {
    let source = source_for(
        r"
function build(key: unknown, value: unknown): unknown {
  const result: unknown = {};
  // @ts-expect-error dynamic index writes are accepted at erased object boundaries.
  result[key] = value;
  return result;
}
",
    );

    // The dynamic index write routes through the prelude helper, which inserts
    // an object property (and handles arrays/other values) in one place.
    assert!(source.contains("smelt_index_assign(&mut result,"), "{source}");
    assert!(source.contains("map.insert(key, value)"), "{source}");
    assert!(!source.contains("unknown is not null"), "{source}");
}

#[test]
fn emits_array_destructuring_assignment_as_indexed_writes() {
    let source = source_for(
        r"
function swap(data: unknown[], i: number, j: number): void {
  [data[i], data[j]] = [data[j], data[i]];
}
",
    );

    assert!(source.contains("let __smelt_destructure"), "{source}");
    assert!(source.contains("__smelt_destructure.borrow().get"), "{source}");
    assert!(source.contains("normalized = 1.0"), "{source}");
    assert_eq!(
        source
            .matches("data.borrow_mut()[smelt_assign_index] = smelt_assign_value")
            .count(),
        2,
        "{source}"
    );
    assert!(
        !source.contains("data[smelt_assign_index] = SmeltUnknown::Array"),
        "{source}"
    );
}

#[test]
fn emits_array_for_each_with_function_callback_parameter() {
    let source = source_for(
        r"
function visit(values: number[], callback: (value: number, index: number, data: number[]) => void): void {
  values.forEach(callback);
}
",
    );

    assert!(source.contains(".iter().enumerate().for_each"), "{source}");
    assert!(source.contains("(smelt_callback)"), "{source}");
    // The subject is that the function-typed callback parameter is forwarded
    // directly, with no synthesized default standing in for it. Asserted against
    // that emitter's own marker rather than a bare `Default::default()`, which
    // the runtime prelude also contains for unrelated reasons (the field map's
    // hasher) and which would make this pass or fail on prelude text.
    assert!(!source.contains("smelt_default_callback"), "{source}");
}

#[test]
fn emits_array_flat_map_with_function_callback_parameter() {
    let source = source_for(
        r"
function expand(values: number[], callback: (value: number, index: number, data: number[]) => number[]): number[] {
  return values.flatMap(callback);
}
",
    );

    assert!(source.contains(".iter().enumerate().flat_map"), "{source}");
    assert!(source.contains("(smelt_callback)"), "{source}");
    // The subject is that the function-typed callback parameter is forwarded
    // directly, with no synthesized default standing in for it. Asserted against
    // that emitter's own marker rather than a bare `Default::default()`, which
    // the runtime prelude also contains for unrelated reasons (the field map's
    // hasher) and which would make this pass or fail on prelude text.
    assert!(!source.contains("smelt_default_callback"), "{source}");
}

#[test]
fn buffer_from_emits_marker_record() {
    // `Buffer.from([...])` erases to a marker-bearing record carrying the
    // `__smelt_buffer` identity key so downstream `instanceof`/`isBuffer` resolve.
    let generated =
        source_for("export function f() { const b = Buffer.from([1, 2, 3]); return b; }");
    assert!(
        generated.contains("\"__smelt_buffer\""),
        "expected `Buffer.from` to emit a `__smelt_buffer` marker record:\n{generated}"
    );
}

/// The private host-marker array builder emits every registry marker plus the
/// runtime-owned abort/namespace markers, so the frontend construction path, the
/// `instanceof` codegen path, and this runtime filter share one source of truth.
#[test]
fn host_marker_registry_array_covers_registry_and_runtime_markers() {
    let text = smelt_runtime::source::item_source("smelt_object_has_host_marker")
        .expect("the host-marker helper is a runtime item");
    let array = text
        .split("has_internal_keys() && [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the helper spells the marker array");
    for marker in smelt_stdlib::host_object_markers() {
        assert!(
            array.contains(&format!("\"{marker}\"")),
            "host-marker array missing registry marker `{marker}`: {array}"
        );
    }
    for marker in [
        "__smelt_abortcontroller",
        "__smelt_abortsignal",
        "__smelt_builtin_namespace",
        "__smelt_global_object",
    ] {
        assert!(
            array.contains(&format!("\"{marker}\"")),
            "host-marker array missing runtime marker `{marker}`: {array}"
        );
    }
}

#[test]
fn writes_back_list_pushes_through_tuple_indexes() {
    let source = source_for(
        r"
function partitionLike(values: number[]): [number[], number[]] {
  const result: [number[], number[]] = [[], []];
  result[0].push(values[0]);
  result[1].push(values[1]);
  return result;
}
",
    );

    assert!(source.contains("result.0 ="), "{source}");
    assert!(source.contains("result.1 ="), "{source}");
}

#[test]
fn reuses_a_stable_identity_when_erasing_a_source_list_local() {
    // Erasing the SAME list local twice must reuse one erased-array id so the
    // two erasures compare `===` equal (arrays compare by id). The typed list
    // (`SmeltList`) carries its own reference id, and a `.clone()` shares it, so
    // each erasure reuses the list's `id()` rather than an address-keyed sidecar.
    let source = source_for(
        r"
const data: number[] = [1, 2, 3];
const same = (data as unknown) === (data as unknown);
",
    );

    let body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .expect("emitted source has a main function");

    assert!(
        body.contains("let smelt_l = data.clone();"),
        "erasing a list local should bind it to read its reference id\n{source}"
    );
    assert!(
        body.contains("let smelt_id = smelt_l.id();"),
        "erasing a list local should read the list's own reference id\n{source}"
    );
    assert!(
        body.contains("SmeltArray::with_id(smelt_id,"),
        "erasing a list local should build the array with the list's own identity\n{source}"
    );
}

#[test]
fn keeps_a_fresh_identity_when_erasing_a_list_literal() {
    // A fresh list expression (here a literal) must keep `SmeltArray::new` (via
    // `.into()`) so distinct array literals are never `===`, matching JS.
    let source = source_for(
        r"
const other = ([1, 2, 3] as unknown) === ([1, 2, 3] as unknown);
",
    );

    let body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .expect("emitted source has a main function");

    assert!(
        body.contains("SmeltUnknown::Array(vec![") && body.contains("].into())"),
        "a list literal should erase through the fresh-id `.into()` path\n{source}"
    );
    assert!(
        !body.contains("smelt_list_identity("),
        "a list literal must not be keyed to a reused identity\n{source}"
    );
    assert!(
        !body.contains("SmeltArray::with_id("),
        "a list literal must use a fresh `SmeltArray::new` identity\n{source}"
    );
}

#[test]
fn lowers_imported_function_value_as_map_callback() {
    // The same support extends to value-returning array methods like `map`: a
    // bare imported function passed as the callback lowers to a mapping closure
    // rather than being rejected for not being an inline arrow.
    let source = source_for(
        r#"
import { deburr } from "./deburr";
function run(): unknown {
  const arr = ["a", "b"];
  return arr.map(deburr);
}
"#,
    );

    assert!(
        source.contains("smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)|"),
        "imported map callback should lower to a mapping closure\n{source}"
    );
    assert!(
        source.contains("(smelt_callback)(smelt_item.clone(), smelt_index as i64,"),
        "the closure body should call the resolved callback value\n{source}"
    );
}

#[test]
fn lowers_array_at_inside_callback_body() {
    // `data.at(i)` called inside a `.map` callback body must lower through the
    // same optional-index path used in statement position (es-toolkit
    // `src/array/at.spec.ts`), rather than failing closure-body lowering.
    let source = source_for(
        r"
const data = [10, 20, 30];
const indices = [0, 1, 2];
const out = indices.map(i => data.at(i));
",
    );

    assert!(
        source.contains(".get(") || source.contains("optional"),
        "array .at inside a callback should lower to a checked optional index\n{source}"
    );
}

#[test]
fn lowers_array_join_inside_callback_body() {
    // `parts.join('-')` inside a callback body lowers to the same `StringJoin`
    // the direct path emits.
    let source = source_for(
        r#"
const rows: string[][] = [["a", "b"], ["c", "d"]];
const joined = rows.map(parts => parts.join("-"));
"#,
    );

    assert!(
        source.contains("JsString::join(") && source.contains(", &JsString::from(\"-\"))"),
        "array .join inside a callback should lower to a Rust join\n{source}"
    );
}

#[test]
fn lowers_array_join_default_separator_inside_callback_body() {
    // A bare `.join()` inside a callback body defaults to the `","` separator,
    // matching the statement-position lowering.
    let source = source_for(
        r"
const rows: number[][] = [[1, 2], [3, 4]];
const joined = rows.map(parts => parts.join());
",
    );

    assert!(
        source.contains("JsString::join(") && source.contains(", &JsString::from(\",\"))"),
        "array .join() inside a callback should default to a comma separator\n{source}"
    );
}

#[test]
fn lowers_map_has_inside_callback_body() {
    // A typed `Map` receiver's `.has(key)` inside a callback body lowers to the
    // same `contains_key` check the direct `map_has_call` path emits (es-toolkit
    // `src/map/every.spec.ts`).
    let source = source_for(
        r"
export function every<K, V>(
  map: Map<K, V>,
  doesMatch: (value: V, key: K, map: Map<K, V>) => boolean
): boolean {
  for (const [key, value] of map) {
    if (!doesMatch(value, key, map)) {
      return false;
    }
  }
  return true;
}

const m = new Map<string, number>();
const r = every(m, (value, key, originalMap) => originalMap.has(key) && value > 0);
",
    );

    assert!(
        source.contains("contains_key"),
        "Map .has inside a callback should lower to a contains_key check\n{source}"
    );
}

#[test]
fn lowers_array_concat_with_multiple_arguments() {
    // `Array.prototype.concat` accepts any number of arguments; each array
    // argument is spread and each scalar is appended, chained left to right.
    let source = source_for(
        r"
export function joinAll(a: number[], b: number[], c: number[]): number[] {
  return a.concat(b, c);
}
export function appendScalars(a: number[], x: number, y: number): number[] {
  return a.concat(x, y);
}
export function copy(a: number[]): number[] {
  return a.concat();
}
",
    );
    // Two right-arguments produce chained list concatenations (`.chain(...)`),
    // and the no-argument form is a shallow copy.
    assert!(source.matches(".chain(").count() >= 3, "{source}");
    assert!(source.contains("a.clone()"), "{source}");
}

#[test]
fn lowers_typeof_in_array_element_position() {
    // `typeof x` appearing as an array literal element (which routes through the
    // generic `unary_expression` path rather than the top-level expression
    // dispatcher) must lower instead of rejecting the file.
    let source = source_for(
        r#"
export function tags(x: unknown): string[] {
  return [typeof x, "tail"];
}
"#,
    );
    assert!(source.contains("vec!"), "{source}");
}

#[test]
fn lowers_array_push_with_multiple_arguments() {
    // `Array.prototype.push` accepts any number of arguments, appending each in
    // order and returning the new length; spread arguments extend the list.
    let source = source_for(
        r"
export function appendThree(a: number[]): number {
  return a.push(1, 2, 3);
}
export function appendSpreadAndScalar(a: number[], b: number[]): void {
  a.push(...b, 9);
}
",
    );
    // Three pushes for the scalar case, plus an extend + push for the second.
    assert!(source.matches(".push(").count() >= 4, "{source}");
    assert!(source.contains("extend") || source.contains(".chain("), "{source}");
}

#[test]
fn lowers_array_length_from_optional_and_member_numeric() {
    // `new Array(n)` where `n` is an optional/number-typed parameter, and
    // `new Array(xs.length)` where the length is a numeric member read, both
    // preallocate a list instead of rejecting the length as non-numeric.
    let source = source_for(
        r"
export function makeOptional(n?: number): number[] {
  const result = new Array(n);
  return result;
}
export function makeFromLength<T>(keys: T[]): T[] {
  const result = new Array(keys.length);
  return result;
}
",
    );
    assert!(source.contains("Vec::new") || source.contains("SmeltList"), "{source}");
}

#[test]
fn lowers_new_set_from_optional_and_erased_iterables() {
    // `new Set(iterable)` accepts an optional array, an existing set (copy), and
    // an erased iterable surface, instead of requiring a concretely-typed array.
    let source = source_for(
        r"
export function fromOptional(xs?: string[]): Set<string> {
  return new Set(xs);
}
export function fromSet(s: Set<number>): Set<number> {
  return new Set(s);
}
",
    );
    assert!(source.contains("HashSet") || source.contains("SmeltSet") || source.contains("Set"), "{source}");
}

#[test]
fn lowers_array_fill_with_compatible_value() {
    // `Array.prototype.fill` coerces an assignment-compatible value (generic
    // element type) instead of requiring an exact element-type match.
    let source = source_for(
        r"
export function fillAll<T>(a: T[], v: T): T[] {
  return a.fill(v);
}
export function fillRange(a: number[]): number[] {
  return a.fill(0, 1, 2);
}
",
    );
    assert!(source.contains("fill"), "{source}");
}

#[test]
fn lowers_named_opaque_predicate_for_array_some() {
    // A named/opaque predicate passed to `some` (`xs.some(matchFunc)`) lowers to
    // a wrapper closure returning an erased value. The truthy-predicate path must
    // accept the erased return type instead of rejecting it with "array callback
    // callback returns an unsupported type".
    let source = source_for(
        r"
export function run(values: unknown[], matchFunc: (value: unknown) => unknown): boolean {
  return values.some(matchFunc);
}
",
    );
    assert!(source.contains("fn run("), "{source}");
    assert!(source.contains(".any("), "expected a some/any lowering: {source}");
}

#[test]
fn lowers_conditionally_selected_array_callback() {
    // A callback chosen at runtime between callable values must lower as an
    // opaque element-forwarding callback instead of being rejected with
    // "array callback methods currently require arrow function callbacks".
    let source = source_for(
        r#"
import { identity } from "./identity";
export function run(values: unknown[], flag: boolean): unknown[] {
  return values.map(flag ? Object : identity);
}
"#,
    );
    assert!(source.contains("fn run("), "{source}");
}

#[test]
fn set_interval_typed_extra_args_use_wrapper_closure() {
    // `setInterval` shares the typed-wrapper path with `setTimeout`.
    let source = source_for(
        r#"
function tick(label: string, n: number): void {
  console.log(label);
}
setInterval(tick, 5, "tock", 2);
"#,
    );

    assert!(
        !source.contains("smelt_timer_args"),
        "typed interval extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
    assert!(
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1)"),
        "{source}"
    );
    assert!(source.contains("smelt_set_interval("), "{source}");
}

#[test]
fn unifies_array_destructuring_default_of_different_type() {
    // `const [s, n = 0] = str.split('e')` binds `n` from a `string` element with
    // a numeric `0` default: JavaScript types it `string | number`. The binding
    // must unify into that union (a concrete `SmeltUnion…`) rather than assert
    // the numeric default to `String`, which would leave a runtime `f64` typed
    // as `String` (E0308).
    let source = source_for(
        r#"
export function adjust(value: number): number {
  const [magnitude, exponent = 0] = value.toString().split("e");
  return Number(`${magnitude}e${Number(exponent)}`);
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // `source_for` panics on a blocker; reaching codegen and generating a union
    // for the mixed-type default proves the unification path was taken.
}

#[test]
fn lowers_function_expression_array_callback() {
    // A `function (...) { ... }` expression callback (issue #86) must lower into
    // the array method's callback closure just like the equivalent arrow, rather
    // than being rejected with "array callback methods currently require arrow
    // function callbacks". `source_for` panics on any blocker, so reaching
    // codegen proves the non-arrow form was accepted.
    let source = source_for(
        r"
function increment(values: number[]): number[] {
  return values.map(function (value) {
    return value + 1;
  });
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn lowers_named_function_item_array_callback() {
    // A bare identifier naming a module-level function item (`values.map(square)`)
    // must lower into the callback closure by calling the function by name, with
    // its typed signature preserved (issue #86).
    let source = source_for(
        r"
function square(value: number): number {
  return value * value;
}

function squares(values: number[]): number[] {
  return values.map(square);
}
",
    );

    assert!(source.contains(".map("), "{source}");
    assert!(source.contains("square"), "{source}");
}

#[test]
fn lowers_local_function_variable_array_callback() {
    // A local/parameter binding whose static type is a `Type::Function`, handed
    // to an array method by name (`values.map(transform)`), must lower into the
    // callback closure by calling the captured local (issue #86).
    let source = source_for(
        r"
function scale(values: number[]): number[] {
  const transform = (value: number): number => value * 3;
  return values.map(transform);
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn lowers_asserted_identifier_array_callback() {
    // A named callback wrapped in an erased TypeScript assertion
    // (`values.map(square as (value: number) => number)`) must resolve through
    // the same named-reference path as the unwrapped form, so the assertion is
    // transparent instead of hitting the arrow-only gate (issue #86).
    let source = source_for(
        r"
function square(value: number): number {
  return value * value;
}

function squares(values: number[]): number[] {
  return values.map(square as (value: number) => number);
}
",
    );

    assert!(source.contains(".map("), "{source}");
    assert!(source.contains("square"), "{source}");
}

/// A seeded reduce borrows its receiver for iteration, so the array argument
/// supplied to a four-parameter callback must never be MOVED out of the
/// surrounding function or closure.
///
/// It used to be cloned instead — one whole-list deep copy per element, which is
/// what made a four-parameter reduce O(n^2). The callback now takes that parameter
/// by shared reference (`callback_param_is_shared_reference`), so the fold body
/// binds the same borrow it is already iterating and nothing is copied at all. The
/// no-move assertion is the part of this test that was always the point.
#[test]
fn reduce_borrows_the_array_callback_argument_with_initial_value() {
    let source = source_for(
        r"
export function sum(values: number[]): number {
  const mapped = values.map(value => value);
  return mapped.reduce((acc, value, _index, array) => acc + value + array.length, 0);
}
",
    );

    assert!(source.contains("let smelt_array = &mapped;"), "{source}");
    assert!(!source.contains("let smelt_array = mapped;"), "{source}");
    assert!(!source.contains("let smelt_array = mapped.clone();"), "{source}");
}

/// Seedless reduce has the same ownership requirement after extracting its first
/// element: later iterations still borrow the receiver while invoking the callback,
/// so the array argument is that same borrow and is never moved.
#[test]
fn reduce_borrows_the_array_callback_argument_without_initial_value() {
    let source = source_for(
        r"
export function sum(values: number[]): number {
  const reversed = values.reverse();
  return reversed.reduce((acc, value, _index, array) => acc + value + array.length);
}
",
    );

    assert!(source.contains("let smelt_array = &reversed;"), "{source}");
    assert!(!source.contains("let smelt_array = reversed;"), "{source}");
    assert!(!source.contains("let smelt_array = reversed.clone();"), "{source}");
}

/// A module const whose initializer is an array spread (es-toolkit's
/// `arrayViews = [...typedArrays, 'DataView']` shape) inlines into a function
/// body as a concrete `SmeltList<String>` concat chain. The homogeneous
/// literal must keep its `String` item type — the previous blanket
/// `List<Unknown>` item type made the concat operands disagree and the
/// emitter silently produced an empty `SmeltList::default()` value.
#[test]
fn inlined_spread_const_emits_concrete_string_list_concat() {
    let source = source_for(
        r#"
const typedNames = ["Float32Array", "Int8Array", "Uint8Array"];
const viewNames = [...typedNames, "DataView"];
export function describeViews(): string {
  const names = viewNames;
  return names.join(",");
}
"#,
    );
    assert!(source.contains("fn describe_views()"), "{source}");
    assert!(
        source.contains("JsString::from(\"DataView\")"),
        "expected the spread tail literal to survive codegen: {source}"
    );
    assert!(
        source.contains("let names: SmeltList<JsString>"),
        "expected the inlined spread const to stay a concrete string list: {source}"
    );
    assert!(
        !source.contains("SmeltList::default()"),
        "expected no silently-defaulted list value in the concat chain: {source}"
    );
}

/// A zero-parameter named callback (`values.map(stubTrue)`) is called with no
/// arguments at all — JavaScript ignores the supplied `(value, index, array)`
/// triple when the callback declares no parameters.
#[test]
fn zero_parameter_named_map_callback_calls_with_no_arguments() {
    let source = source_for(
        r"
function stubTrue(): boolean {
  return true;
}
export function run(values: number[]): boolean[] {
  return values.map(stubTrue);
}
",
    );
    assert!(source.contains("(smelt_callback)()"), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// A zero-parameter named predicate emits a real filter over the receiver
/// instead of the former `Default::default()` placeholder.
#[test]
fn zero_parameter_named_filter_callback_emits_real_iteration() {
    let source = source_for(
        r"
function stubFalse(): boolean {
  return false;
}
export function run(values: number[]): number[] {
  return values.filter(stubFalse);
}
",
    );
    assert!(source.contains("stub_false()"), "{source}");
    assert!(source.contains(".iter().enumerate().filter_map("), "{source}");
}

/// A named callback declaring more parameters than the receiver supplies is
/// wrapped at the supplied arity; the item call pads the unsupplied optional
/// tail with its default (`None`), matching the JavaScript `undefined` tail.
#[test]
fn over_arity_named_map_callback_pads_optional_tail_with_none() {
    let source = source_for(
        r"
function withTail(value: number, index?: number, list?: number[], guard?: number): number {
  return value + (guard ?? 0);
}
export function run(values: number[]): number[] {
  return values.map(withTail);
}
",
    );
    assert!(
        source.contains("with_tail(closure_arg_0, closure_arg_1.clone(), closure_arg_2.clone(), None::<f64>)"),
        "{source}"
    );
}


#[test]
fn erased_rest_callback_maps_through_erased_call_abi() {
    // A `.map` callback whose value is an erased rest callable
    // (`SmeltErasedFunction`, e.g. the result of a currying/arity helper) is not
    // a Rust `Fn` and cannot be invoked with call syntax. The array-callback
    // lowering must route it through the erased callable ABI (`.call(..)`) rather
    // than emitting `(smelt_callback)(..)`, which fails to compile (E0618).
    let source = source_for(
        r"
function makeCapped(): (...args: unknown[]) => unknown {
  return (...args: unknown[]) => args[0];
}
export function run(items: string[]): unknown[] {
  const capped = makeCapped();
  return items.map(capped);
}
",
    );

    assert!(
        source.contains("smelt_callback.call("),
        "an erased rest callback must be invoked through the erased ABI\n{source}"
    );
    assert!(
        !source.contains("(smelt_callback)(SmeltList"),
        "an erased rest callback must not be called with call syntax\n{source}"
    );
}

#[test]
fn tuple_length_emits_constant_arity() {
    // A fixed-arity tuple has no Rust `.len()` method (E0599). Its JavaScript
    // `.length` is a compile-time constant, so the length rvalue must emit the
    // arity literal rather than a method call on the tuple.
    let source = source_for(
        r"
export function pairLength(pair: [string, number]): number {
  return pair.length;
}
",
    );

    assert!(
        source.contains("2 as f64"),
        "a tuple's `.length` should emit its constant arity\n{source}"
    );
    assert!(
        !source.contains("pair.len()"),
        "a tuple must not call the list `.len()` method\n{source}"
    );
}

#[test]
fn tuple_assertion_on_list_value_preserves_list_representation() {
    // Regression (es-toolkit xorBy): a TypeScript tuple assertion applied to a
    // list value (`xs.filter(...) as [T]`) is type-level only. Materializing the
    // tuple would repackage the whole list into a 1-tuple `(SmeltUnknown,)` that
    // no longer satisfies a `SmeltList` consumer (E0308). The list value and its
    // type must be preserved.
    let source = source_for(
        r"
export function pickList(xs: unknown[]): unknown[] {
  const ys = xs.filter(x => x != null) as [unknown];
  return ys;
}
",
    );
    assert!(
        !source.contains("(SmeltUnknown,)"),
        "tuple assertion on a list must not build a 1-tuple: {source}"
    );
}

#[test]
fn concrete_list_length_in_map_closure_erases_to_unknown() {
    // Regression (es-toolkit unzipWith): `arr.map(a => a.length)` where the map
    // result feeds an erased (`SmeltUnknown`) list. `.length` on a CONCRETE list
    // renders as `(x.len() as f64)` (an `f64`), so its resolved value type must
    // be `Float` — otherwise the map closure's return is treated as already
    // erased and the collected `Vec<f64>` cannot bridge to `SmeltList<SmeltUnknown>`.
    // With `.length` typed `Float`, the closure return correctly erases the
    // number to `SmeltUnknown::Number(..)`.
    let source = source_for(
        r"
export function maxWidth(rows: unknown[][]): number {
  return Math.max(...rows.map((r) => r.length));
}
",
    );
    assert!(
        source.contains("SmeltUnknown::Number"),
        "concrete list length in a map closure feeding an erased list must erase \
         through SmeltUnknown::Number: {source}"
    );
}

#[test]
fn plain_local_mut_list_argument_to_erased_callee_converts_in_place() {
    // Regression (es-toolkit pull/remove): an ordinary caller passes a concrete
    // local list into a callee whose mutable list parameter is erased
    // (`&mut SmeltList<SmeltUnknown>`). `&mut` is invariant, so the elements must
    // be erased into a temporary, the callee must mutate that temporary, and the
    // result must be un-erased back into the caller's array. The write-back goes
    // through the local's own buffer, so an alias of `xs` observes it; the place
    // is not a reference, so `*xs = ..` would not compile and `(*xs).clone()`
    // would clone a slice.
    let source = source_for(
        r"
function eraseInto(target: unknown[], value: unknown): void {
  target.push(value);
}
export function useErase(): number[] {
  const xs: number[] = [1, 2];
  eraseInto(xs, 3);
  return xs;
}
",
    );
    assert!(
        source.contains("let mut smelt_mut_arg_0: SmeltList<SmeltUnknown> = xs.clone()")
            && source.contains("into_smelt_unknown"),
        "the concrete local must be erased into the adapter temporary: {source}"
    );
    assert!(
        source.contains("xs.clone().replace_all(smelt_mut_arg_0")
            && source.contains("smelt_from_unknown")
            && !source.contains("*xs = ")
            && !source.contains("(*xs).clone()"),
        "the mutated temporary must be un-erased back into the owned local's \
         buffer, not written through a reference: {source}"
    );
}

/// Slicing a typed list into an erased (`SmeltUnknown`) destination must
/// materialize a real `SmeltList` and erase it, rather than leaking a bare
/// `Vec` into the `SmeltUnknown` slot (E0308). Mirrors es-toolkit's `ary`,
/// which caps a rest-argument list and forwards it through an erased value.
#[test]
fn list_slice_into_unknown_destination_materializes_smelt_list() {
    let source = source_for(
        r"
export function run(args: string[]): unknown {
  const capped: unknown = args.slice(0, 2);
  return capped;
}
",
    );

    // The slice is materialized as an identity-bearing array value and erased,
    // never assigned as a raw `Vec` collected straight into a `SmeltUnknown`.
    assert!(
        source.contains("SmeltArray::with_id") || source.contains("SmeltList::with_id"),
        "a slice into an erased destination must build an identity-bearing list\n{source}"
    );
    assert!(
        !source.contains(": SmeltUnknown = args.clone().iter().skip"),
        "the bare-Vec slice must not be assigned directly to a SmeltUnknown\n{source}"
    );
}

/// A spread call (`fn(...values)`) must route to the variadic overload rather
/// than a fixed-arity one: the spread's runtime length is unknown, so a
/// rest-less overload cannot claim it. Mirrors es-toolkit's
/// `cartesianProduct(...inputs)`, which must return `number[][]` and not a
/// list of 1-tuples-of-lists.
#[test]
fn spread_call_selects_variadic_overload() {
    let source = source_for(
        r"
export function cartesianProduct<T>(arr1: readonly T[]): Array<[T]>;
export function cartesianProduct<T, U>(arr1: readonly T[], arr2: readonly U[]): Array<[T, U]>;
export function cartesianProduct<T>(...arrs: Array<readonly T[]>): T[][];
export function cartesianProduct<T>(...arrs: Array<readonly T[]>): T[][] {
  return arrs as any;
}

export function run(inputs: number[][]): unknown {
  return cartesianProduct(...inputs);
}
",
    );

    assert!(
        source.contains("SmeltList<SmeltList<f64>>"),
        "a spread call must select the variadic overload returning number[][]\n{source}"
    );
    assert!(
        !source.contains("SmeltList<(SmeltList<f64>,)>"),
        "the spread call must not select the fixed 1-array overload\n{source}"
    );
}

/// A list of tuples flowing into a list-of-lists target (e.g. a `zip` result
/// passed to `unzipWith`'s `readonly T[][]` with `T` erased) must coerce each
/// tuple into a `SmeltList`, not pass it through unchanged (E0308).
#[test]
fn tuple_coerces_into_list_target() {
    let source = source_for(
        r"
function sink(rows: unknown[][]): void {}

export function run(pairs: Array<[number, number]>): void {
  sink(pairs);
}
",
    );

    assert!(
        source.contains("SmeltList::with_id(smelt_next_object_id(), vec!["),
        "a tuple flowing into a list target must be rebuilt as a SmeltList\n{source}"
    );
}

#[test]
fn callable_object_construction_emits_typed_interface_struct_literal() {
    // Callable-object construction: a function-typed local that collects
    // `counter.reset = …` writes and is returned at a callable-interface type
    // must emit a real struct literal carrying the base callable in
    // `__smelt_call` and each collected property in its like-named field — never
    // a `Default::default()` inert struct that drops the writes.
    let source = source_for(
        r"
interface Counter {
  (): number;
  reset(): void;
}
export function makeCounter(): Counter {
  let count = 0;
  const counter = function (): number {
    count = count + 1;
    return count;
  };
  counter.reset = function (): void {
    count = 0;
  };
  return counter;
}
",
    );
    let body = source
        .split("fn make_counter")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        body.contains("Counter {"),
        "callable construction must build a Counter struct literal: {source}"
    );
    assert!(
        body.contains("__smelt_call:"),
        "the base callable must fill the __smelt_call field: {source}"
    );
    assert!(
        body.contains("reset:"),
        "the collected property must fill the reset field: {source}"
    );
    assert!(
        !body.contains("Default::default()"),
        "callable construction must not fall back to an inert Default struct: {source}"
    );
}





/// Regression (warning-reduction R1): a read-only collection capture in a
/// callback prelude is cloned into a plain, non-`mut` binding. The capture
/// prelude used to force `mut` on every list/set/dict capture regardless of
/// use; mutability now follows `closure_capture_body_writes` only.
#[test]
fn read_only_list_capture_emits_non_mut_clone() {
    let source = source_for(
        r"
export function useCapture(values: number[], extra: number[]): number[] {
  return values.map((n: number): number => n + extra.length);
}
",
    );
    assert!(
        source.contains("let extra = extra.clone();"),
        "read-only list capture must clone into a non-mut binding: {source}"
    );
    assert!(
        !source.contains("let mut extra = extra.clone();"),
        "read-only list capture must not be spuriously mut: {source}"
    );
}

/// Regression (warning-reduction R1): a captured collection that the enclosing
/// function still reassigns (so its source local is mutable) keeps a `mut`
/// clone binding. Guards against under-approximating mutability after the
/// blanket collection rule was dropped.
#[test]
fn mutable_source_list_capture_keeps_mut_clone() {
    let source = source_for(
        r"
export function f(items: number[]): number {
  let acc = items;
  acc = items;
  return [1].map((x: number): number => acc.length + x).length;
}
",
    );
    assert!(
        source.contains("let mut acc = acc.clone();"),
        "a capture whose source local is reassigned must stay a mut clone: {source}"
    );
}

/// Regression (warning-reduction R1): a captured collection that the closure
/// body mutates in place is threaded through shared `RefCell` storage (so the
/// mutation is caller-visible) rather than being silently emitted as a
/// read-only clone. Guards the write-detection path.
#[test]
fn written_list_capture_uses_shared_storage_not_plain_clone() {
    let source = source_for(
        r"
export function g(): number[] {
  const store: number[] = [];
  const add = (x: number): void => { store.push(x); };
  [1, 2, 3].forEach(add);
  return store;
}
",
    );
    assert!(
        source.contains("smelt_capture_store")
            && source.contains("(*smelt_capture_store.borrow()).borrow_mut().push("),
        "a mutated captured list must route writes through shared storage: {source}"
    );
    assert!(
        !source.contains("let store = store.clone();"),
        "a mutated captured list must not be emitted as a read-only clone: {source}"
    );
}

/// A `splice` whose start index is an `i64` (e.g. a callback index parameter)
/// is coerced to `f64`. The coerced cast must be parenthesized so
/// `index as f64 < 0.0` does not parse as `index as (f64 < 0.0)` — rustc reads
/// the `<` as the start of generic arguments after a type. Regression for the
/// remeda `range`/`splice` E0742-style `<`-parse failure.
#[test]
fn splice_index_cast_is_parenthesized_before_comparison() {
    let source = source_for(
        r"
export function trimLast(items: number[]): number[] {
  [0].forEach((_, i) => {
    items.splice(i, 1);
  });
  return items;
}
",
    );
    assert!(
        source.contains("splice_start = if ("),
        "the coerced splice index must be parenthesized before `< 0.0`: {source}"
    );
    assert!(
        !source.contains("as f64 < 0.0"),
        "a bare `x as f64 < 0.0` mis-parses as generic arguments: {source}"
    );
}

/// A `Set.has(needle)` whose needle is an `i64` (a callback index) coerces the
/// needle to the set's `f64` element type, as a value of its own: `&x as f64`
/// would cast a reference, which is invalid. Regression for remeda `sample`.
#[test]
fn set_has_coerced_needle_reference_is_parenthesized() {
    let source = source_for(
        r"
export function pickByIndex(values: number[]): number[] {
  const seen = new Set<number>([1, 2, 3]);
  return values.filter((_, i) => seen.has(i));
}
",
    );
    assert!(
        source.contains("= (closure_arg_1 as f64);") && source.contains(".contains(&_smelt_tmp_"),
        "a coerced set needle must be widened before `contains` borrows it: {source}"
    );
}
/// An object rest pattern (`const { a, ...rest } = source`) whose source is a
/// named object type that erases to `SmeltUnknown` (here `Handle`, an interface)
/// must copy the source's remaining members into `rest`. Previously only a
/// native `Dict` or a literally `Type::Unknown` source was copied, so a named
/// object type fell through to `Default::default()` and produced an empty rest,
/// dropping spread-out members (the `cancel`/`flush` of an `Object.assign`-
/// wrapped funnel). The copy must route through `into_smelt_unknown()`.
#[test]
fn object_rest_copies_named_object_source_that_erases_to_unknown() {
    let source = source_for(
        r"
interface Handle {
  readonly call: () => void;
  readonly cancel: () => void;
  readonly flush: () => void;
}
declare function make(): Handle;
export function wrap(): Record<string, unknown> {
  const { call, ...rest } = make();
  call();
  return rest;
}
",
    );

    // The rest copy is materialized from the erased object form, not an empty
    // record.
    assert!(
        source.contains(".into_smelt_unknown() { SmeltUnknown::Object(smelt_map) => SmeltRecord::with_id_from_entries(smelt_map.id, smelt_map.into_iter())"),
        "object rest must copy the erased source object, not Default::default()\n{source}"
    );
    // The extracted key is still removed from the copied rest.
    assert!(
        source.contains(".remove(&JsString::from(\"call\"))"),
        "the destructured `call` key must be removed from rest\n{source}"
    );
}


#[test]
fn calling_a_callable_object_value_dispatches_through_its_call_slot() {
    // Regression (es-toolkit debounce/throttle specs): invoking a value whose
    // type is a callable interface (`debouncedFunc()` where `debouncedFunc:
    // DebouncedFunction`) lowers to a `closure_call` whose callee temporary is
    // function-typed. The record -> function coercion had no rule, so the
    // callee fell through to `default_value`, which FABRICATED an empty closure
    // and then called it: the real function never ran, no timer was scheduled,
    // and every `toHaveBeenCalledTimes` assertion failed. The call must read the
    // interface's synthetic `__smelt_call` slot instead.
    let source = source_for(
        r"
export interface Counter {
  (...args: number[]): void;
  reset: () => void;
}
export function makeCounter(): Counter {
  let count = 0;
  const reset = () => {
    count = 0;
  };
  const counter = function (...args: number[]) {
    count = count + args.length;
  };
  counter.reset = reset;
  return counter;
}
export function useCounter(): void {
  const c = makeCounter();
  c();
  c(1, 2);
}
",
    );
    assert!(
        source.contains("__smelt_call.clone()"),
        "calling a callable-object value must dispatch through its `__smelt_call` \
         slot: {source}"
    );
    // The dead-stub shape: a freshly built empty callable ASSIGNED to a
    // temporary (as opposed to a declaration-site default initializer), which is
    // then immediately invoked.
    let dead_stub_assignment = source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("_smelt_tmp_")
            && line.contains("= { let smelt_default_callback:")
            && line.contains("SmeltList<f64>")
    });
    assert!(
        !dead_stub_assignment,
        "a callable-object call must not materialize a fabricated empty callback \
         at the call site: {source}"
    );
}

#[test]
fn apply_on_a_callable_object_invokes_its_underlying_callable() {
    // Regression (es-toolkit debounce: `func.apply(pendingThis, pendingArgs)`
    // where `func` is a vitest mock). A callable object erases to
    // `SmeltUnknown::Object` carrying a `__smelt_call` slot and has no own
    // `apply` property, so the object arm of `smelt_function_method` returned
    // `undefined` and the call silently became a no-op. `apply`/`call`/`bind`
    // are `Function.prototype` members of the underlying callable and must
    // resolve there when the object itself does not define them.
    let source = source_for(
        r"
export function applyTo(func: unknown, args: unknown[]): unknown {
  return (func as (...values: unknown[]) => unknown).apply(null, args);
}
",
    );
    assert!(
        source.contains("fn smelt_function_method("),
        "expected the Function.prototype method helper: {source}"
    );
    assert!(
        source.contains(r#"match map.get("__smelt_call")"#),
        "`smelt_function_method` must fall back to a callable object's \
         `__smelt_call` slot: {source}"
    );
}

#[test]
fn arraybuffer_is_view_covers_every_registry_view_kind() {
    // `ArrayBuffer.isView(x)` is true for a *view* over byte storage and false for
    // the storage itself, which is exactly the `ByteBufferRole::View` split in the
    // shared host registry. es-toolkit defines `isTypedArray` as
    // `ArrayBuffer.isView(x) && !(x instanceof DataView)`, so a Node `Buffer` (a
    // `Uint8Array` subclass) has to answer `true` here or its clone path is never
    // taken. Checking only `DataView` left `isTypedArray(Buffer.from([1]))` false.
    let generated =
        source_for("export function f(value: any): boolean { return ArrayBuffer.isView(value); }");
    for marker in ["__smelt_buffer", "__smelt_dataview"] {
        assert!(
            generated.contains(&format!("value.contains_key(\"{marker}\")")),
            "`ArrayBuffer.isView` must recognize `{marker}`:\n{generated}"
        );
    }
    assert!(
        !generated.contains("matches!(value.clone().clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_arraybuffer\"))"),
        "`ArrayBuffer.isView` must stay false for byte *storage*:\n{generated}"
    );
}

#[test]
fn typed_array_views_each_report_their_own_spec_tag() {
    // All eleven views used to share one `Vec<f64>`, so every one of them reported
    // `[object Array]` — the same tag a plain `number[]` reports. Two views over
    // the same buffer were then indistinguishable, which is what makes
    // `isEqualWith(new Float32Array(buf), new Float64Array(buf))` wrongly `true`.
    // Length alone cannot separate them either: `Uint8Array` and
    // `Uint8ClampedArray` over one buffer have the *same* element count, so those
    // two need genuinely distinct tags.
    let generated = source_for(
        "export function f(value: any): string { return Object.prototype.toString.call(value); }",
    );
    for (marker, tag) in [
        ("__smelt_int8array", "Int8Array"),
        ("__smelt_uint8array", "Uint8Array"),
        ("__smelt_uint8clampedarray", "Uint8ClampedArray"),
        ("__smelt_int16array", "Int16Array"),
        ("__smelt_uint16array", "Uint16Array"),
        ("__smelt_int32array", "Int32Array"),
        ("__smelt_uint32array", "Uint32Array"),
        ("__smelt_float32array", "Float32Array"),
        ("__smelt_float64array", "Float64Array"),
        ("__smelt_bigint64array", "BigInt64Array"),
        ("__smelt_biguint64array", "BigUint64Array"),
    ] {
        assert!(
            generated.contains(&format!(
                "if map.contains_key(\"{marker}\") {{ return \"[object {tag}]\""
            )),
            "`{marker}` must tag as `[object {tag}]`:\n{generated}"
        );
    }
}

#[test]
fn typed_array_element_codec_covers_every_width_and_signedness() {
    // The element type is the load-bearing half of a typed array: the same eight
    // bytes are two `Float32Array` elements or one `Float64Array` element, and the
    // same byte `0xff` is `255` through `uint8` and `-1` through `int8`. The
    // generated codec must therefore carry one little-endian decode/encode pair per
    // element type, at the platform's `BYTES_PER_ELEMENT`, derived from the shared
    // registry rather than restated per call site.
    let generated =
        source_for("export function f(value: any): any { return (value as any)[0]; }");
    for decode in [
        "i8::from_le_bytes([raw[0]])",
        "i16::from_le_bytes([raw[0], raw[1]])",
        "u16::from_le_bytes([raw[0], raw[1]])",
        "i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])",
        "u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])",
        "f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])",
        "f64::from_le_bytes(raw)",
        "i64::from_le_bytes(raw)",
        "u64::from_le_bytes(raw)",
    ] {
        assert!(
            generated.contains(decode),
            "the element codec must decode with `{decode}`:\n{generated}"
        );
    }
    // `Uint8ClampedArray` is the one view that saturates and rounds half to even
    // where the other integer views wrap modulo their width.
    assert!(
        generated.contains("number.round_ties_even().clamp(0.0, 255.0) as u8"),
        "`uint8clamped` must saturate rather than wrap:\n{generated}"
    );
    assert!(
        generated.contains("(number as f32).to_le_bytes().to_vec()"),
        "`float32` must encode at single precision:\n{generated}"
    );
    // Widths come from the registry: `(marker, element tag, BYTES_PER_ELEMENT)`.
    for entry in [
        "(\"__smelt_uint8array\", \"uint8\", 1)",
        "(\"__smelt_int16array\", \"int16\", 2)",
        "(\"__smelt_float32array\", \"float32\", 4)",
        "(\"__smelt_float64array\", \"float64\", 8)",
        // Node `Buffer` subclasses `Uint8Array`, so it shares that element type.
        "(\"__smelt_buffer\", \"uint8\", 1)",
    ] {
        assert!(
            generated.contains(entry),
            "the element-kind table must carry `{entry}`:\n{generated}"
        );
    }
    // `ArrayBuffer`/`SharedArrayBuffer`/`DataView` are byte-addressed: their bytes
    // carry no single element type, so they must stay out of the element-kind table
    // (they do still appear in the byte-backed marker arrays and the
    // marker-to-class table, so this inspects the table line itself).
    let table_line = generated
        .lines()
        .find(|line| line.contains("fn smelt_host_buffer_element_kind("))
        .unwrap_or_else(|| panic!("element-kind table must be emitted:\n{generated}"));
    for absent in [
        "__smelt_arraybuffer",
        "__smelt_sharedarraybuffer",
        "__smelt_dataview",
    ] {
        assert!(
            !table_line.contains(absent),
            "byte-addressed kind `{absent}` must have no element type:\n{table_line}"
        );
    }
}

#[test]
fn typed_array_length_is_the_element_count_not_the_byte_count() {
    // `new Float64Array(new ArrayBuffer(8))` has ONE element; the old numeric-list
    // model reported 8, the byte count. The record's `length` is therefore
    // `byteLength / BYTES_PER_ELEMENT`, and a view also records the `buffer` it
    // windows plus its `byteOffset` — a typed array in JavaScript is always a
    // window onto an `ArrayBuffer`.
    let generated =
        source_for("export function f(value: any): any { return (value as any).slice(0); }");
    assert!(
        generated.contains(
            "(\"length\".to_owned(), SmeltUnknown::Number((byte_length / stride) as f64))"
        ),
        "`length` must be the element count:\n{generated}"
    );
    assert!(
        generated.contains(
            "(\"byteLength\".to_owned(), SmeltUnknown::Number(byte_length as f64))"
        ),
        "`byteLength` must stay the byte count:\n{generated}"
    );
    assert!(
        generated.contains("fields.push((\"buffer\".to_owned(), buffer))")
            && generated.contains("fields.push((\"byteOffset\".to_owned()"),
        "a view must record the buffer it windows and its offset:\n{generated}"
    );
    // `slice`/`subarray` bounds are element indices for a view and byte indices for
    // byte-addressed storage — one code path, once the stride comes from the
    // registry.
    assert!(
        generated.contains("let len = (bytes.len() / stride) as i64"),
        "slice bounds must be scaled by the element stride:\n{generated}"
    );
}

#[test]
fn typed_array_construction_views_storage_but_converts_elements() {
    // `new Float32Array(source)` has two distinct JavaScript meanings that a
    // shapeless byte copy cannot tell apart: over an `ArrayBuffer` it re-*views*
    // the bytes (eight bytes become two elements), and over another view or an
    // array it *converts* the elements one by one (so
    // `new Uint8Array(new Int8Array([-1]))` holds `255`). The role split in the
    // shared registry is what selects between them.
    let generated = source_for(
        "export function f(buffer: any): any { return new Float32Array(buffer as any); }",
    );
    // An argument whose type carries no shape is the one place the choice is
    // made at RUN time, through the family's single `from_erased` boundary.
    assert!(
        generated.contains("SmeltTypedArray::from_erased(SmeltTypedArrayKind::Float32"),
        "an erased source must go through the one runtime boundary:\n{generated}"
    );
    // The same two meanings, chosen STATICALLY when the argument has a type:
    // storage is re-viewed (shared, not copied) and another view is converted
    // element by element.
    let typed = source_for(
        r"
export function f(buffer: ArrayBuffer, view: Int8Array): number {
  return new Float32Array(buffer).length + new Uint8Array(view).length;
}
",
    );
    assert!(
        typed.contains("SmeltTypedArray::over_buffer(SmeltTypedArrayKind::Float32"),
        "an `ArrayBuffer` source must be re-viewed over the same storage:\n{typed}"
    );
    assert!(
        typed.contains("SmeltTypedArray::with_elements(SmeltTypedArrayKind::Uint8, &view"),
        "a view source must be converted element by element:\n{typed}"
    );
    assert!(
        generated.contains("fn smelt_host_buffer_is_storage(value: &SmeltUnknown) -> bool")
            && generated.contains(
                "[\"__smelt_arraybuffer\", \"__smelt_sharedarraybuffer\"].into_iter().any(|marker| map.contains_key(marker))"
            ),
        "the storage/view split must come from the registry roles:\n{generated}"
    );
    assert!(
        generated.contains("Some(value) if smelt_host_buffer_is_storage(value) =>"),
        "a storage source must be re-viewed byte-for-byte:\n{generated}"
    );
    assert!(
        generated.contains("smelt_host_buffer_encode_element(kind, item)"),
        "a view/array source must be converted element-by-element:\n{generated}"
    );
    // A numeric length allocates ELEMENTS, so a wider view over the same count is
    // wider in bytes.
    assert!(
        generated.contains("vec![SmeltUnknown::Number(0.0); count * stride]"),
        "`new Ctor(n)` must allocate `n` elements:\n{generated}"
    );
    assert!(
        typed.contains("vec![0_u8; length * kind.byte_width()]"),
        "the concrete family must allocate `n` elements too:\n{typed}"
    );
}

#[test]
fn typed_array_own_keys_are_its_element_indices() {
    // A typed array's own enumerable properties are exactly its indexed elements;
    // `length`/`byteLength`/`byteOffset`/`buffer` are prototype accessors and
    // `bytes` is internal storage. es-toolkit's `keys(new Uint8Array(1))` asserts
    // `['0']`, and leaking the storage keys instead would make a deep-equality walk
    // over two views compare internal fields — and recurse through `buffer` back
    // into the view.
    // An erased receiver is re-materialized as a `SmeltRecord` before its keys are
    // read, so this is the path both `any` and `Record<string, unknown>` take.
    for source in [
        "export function f(value: any): string[] { return Object.keys(value); }",
        "export function f(value: Record<string, unknown>): string[] { return Object.keys(value); }",
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains("smelt_host_buffer_record_index_keys(&"),
            "`Object.keys` on a view must answer its element indices:\n{generated}"
        );
    }
    // Both key helpers are emitted from one definition, so the tagged-`SmeltUnknown`
    // projection cannot drift from the structural-record one.
    let generated =
        source_for("export function f(value: any): string[] { return Object.keys(value); }");
    assert!(
        generated.contains("fn smelt_host_buffer_index_keys(value: &SmeltUnknown)")
            && generated.contains(
                "fn smelt_host_buffer_record_index_keys(record: &SmeltRecord<JsString, SmeltUnknown>) -> Option<Vec<JsString>> { smelt_host_buffer_index_keys("
            ),
        "the record-flavored index keys must delegate to the tagged one:\n{generated}"
    );
    let values = source_for(
        "export function f(value: any): unknown[] { return Object.values(value); }",
    );
    assert!(
        values.contains("smelt_host_buffer_record_elements(&"),
        "`Object.values` on a view must answer its decoded elements:\n{values}"
    );
    let entries = source_for(
        "export function f(value: any): [string, unknown][] { return Object.entries(value); }",
    );
    assert!(
        entries.contains("smelt_host_buffer_record_elements(&"),
        "`Object.entries` on a view must pair indices with decoded elements:\n{entries}"
    );
    // A property test (`k in obj` / `Object.hasOwn`) must also see the indexed
    // elements, which live in `bytes` rather than as record keys.
    let has_own = source_for(
        "export function f(value: any, key: string): boolean { return Object.hasOwn(value, key); }",
    );
    assert!(
        has_own.contains("smelt_host_buffer_element(map, key).is_some()"),
        "a property test on a view must see its element indices:\n{has_own}"
    );
}

#[test]
fn typed_array_instanceof_resolves_through_the_view_marker() {
    // `x instanceof Uint8Array` used to fold to a constant derived from the static
    // type — `true` for any `number[]` — because the numeric-list model left no
    // identity to test. It is now the registry marker probe, and Node's `Buffer`
    // satisfies `instanceof Uint8Array` because the registry records that subclass
    // relation in its spec tag.
    let generated = source_for(
        "export function f(value: unknown): boolean { return value instanceof Uint8Array; }",
    );
    assert!(
        generated.contains("value.contains_key(\"__smelt_uint8array\")"),
        "`instanceof Uint8Array` must probe the view marker:\n{generated}"
    );
    assert!(
        generated.contains("value.contains_key(\"__smelt_buffer\")"),
        "Node `Buffer` subclasses `Uint8Array`, so it must satisfy the probe:\n{generated}"
    );
    let other = source_for(
        "export function f(value: unknown): boolean { return value instanceof Float64Array; }",
    );
    assert!(
        other.contains("value.contains_key(\"__smelt_float64array\")")
            && !other.contains("value.contains_key(\"__smelt_uint8array\")"),
        "each view must probe only its own marker:\n{other}"
    );
}

#[test]
fn buffer_reports_the_uint8array_spec_tag() {
    // Node's `Buffer` subclasses `Uint8Array`, so the platform reports
    // `[object Uint8Array]`. es-toolkit's `isEqualWith` and `cloneDeepWith` dispatch
    // on that tag ("Buffers are also treated as [object Uint8Array]s"), and a
    // `[object Buffer]` tag falls off the end of both `switch` statements — two
    // equal buffers compared unequal and a buffer was not cloneable.
    let generated = source_for(
        "export function f(value: any): string { return Object.prototype.toString.call(value); }",
    );
    assert!(
        generated
            .contains("if map.contains_key(\"__smelt_buffer\") { return \"[object Uint8Array]\""),
        "a `Buffer` record must tag as `[object Uint8Array]`:\n{generated}"
    );
}

#[test]
fn arguments_object_carries_parameter_values_and_hides_length() {
    // The `arguments` exotic object stores the actual call arguments under index
    // keys with a non-enumerable `length`. It used to be a `{ length: n }` stand-in
    // holding no values at all, so `Object.keys(arguments)` enumerated
    // `["length"]` — the exact inverse of the real key set — and comparing an
    // `arguments` object against the plain object with the same indexed properties
    // could not work in either direction.
    let generated = source_for(
        r"
export function f(a: number, b: number): any {
  const unused = a + b;
  return arguments;
}
",
    );
    assert!(
        generated.contains("smelt_arguments_object(vec!["),
        "`arguments` must be built from the function's parameters:\n{generated}"
    );
    assert!(
        generated.contains("[object Arguments]"),
        "an `arguments` record must carry the `[object Arguments]` spec tag:\n{generated}"
    );
    assert!(
        generated.contains(
            "!(shape.arguments && matches!(key, \"__smelt_arguments\" | \"length\"))"
        ),
        "`length` must stay out of an `arguments` object's own-key enumeration:\n{generated}"
    );
}

#[test]
fn an_arguments_object_is_iterable() {
    // A JavaScript `arguments` object is iterable — its `Symbol.iterator` is
    // `Array.prototype.values` — so `Array.from(arguments)` and `[...arguments]`
    // both walk its elements. Smelt models it as an array-like marker record with
    // index keys and a hidden `length`, which carries no
    // `__smelt_symbol_iterator` slot, so the erased iterable-to-list coercion
    // fell straight through to `panic!("unknown is not iterable")`. Every
    // es-toolkit spec helper that reads its own call arguments through
    // `Array.from(arguments)` (`rest`, `ary`, `unary`, `partial`, `flow`, …) died
    // in that panic before asserting anything.
    let generated = source_for(
        r"
export function f(a: number, b: number): unknown[] {
  const unused = a + b;
  return Array.from(arguments as any);
}
",
    );
    assert!(
        generated.contains("fn smelt_arguments_elements(object: &SmeltObject)"),
        "the `arguments` iteration door must be emitted:\n{generated}"
    );
    assert!(
        generated.contains("smelt_arguments_elements(&smelt_value)"),
        "the erased iterable-to-list coercion must consult it:\n{generated}"
    );
}

/// A `(string | symbol)[]` key spread must keep both halves.
///
/// `[...Object.keys(o), ...Object.getOwnPropertySymbols(o)]` chains a
/// `List<String>` onto a `List<Unknown>`. The concat emitter bailed out on
/// mismatched element types and returned `Default::default()` — an EMPTY list — so
/// es-toolkit's `copyProperties` silently copied nothing. The concrete side's
/// elements are erased into the `SmeltUnknown` element type instead.
#[test]
fn mixed_string_and_symbol_key_spread_erases_instead_of_emptying() {
    let source = source_for(
        r"
export function allKeys(source: any): any[] {
  return [...Object.keys(source), ...Object.getOwnPropertySymbols(source)];
}
const keys = allKeys({});
console.log(keys.length);
",
    );

    let start = source.find("fn all_keys").expect("allKeys present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("allKeys closing brace");
    let body = &after[..end];

    assert!(
        !body.contains("Default::default()"),
        "a mixed string/symbol key spread must not collapse to an empty list:\n{body}"
    );
    assert!(
        body.contains(".chain("),
        "both spread halves must be chained into the result list:\n{body}"
    );
}

/// A callable object invoked inside a callback body must call its underlying
/// callable, not silently evaluate to `null`.
///
/// `Rvalue::ClosureCall` used to answer `default_value(dest_ty)` for every
/// callee that was not a `Type::Function`, so a callable-object record — which
/// *is* callable, through its synthetic `__smelt_call` slot — had its entire
/// call replaced by a null with no diagnostic. es-toolkit `memoize` (whose
/// result type is `F & { cache }`) lost every `memoized(value)` inside a
/// `props.map(..)` callback that way.
#[test]
fn typescript_callable_object_call_in_callback_invokes_the_call_slot() {
    let source = source_for(
        r"
interface Memo {
  (value: string): string;
  cache: number;
}

function makeMemo(fn: (value: string) => string): Memo {
  const memoized = function (value: string): string {
    return fn(value);
  };
  memoized.cache = 0;
  return memoized as Memo;
}

export function run(): string[] {
  const memoized = makeMemo(v => v);
  return ['a', 'b'].map(value => memoized(value));
}
",
    );

    let run_body = source
        .split("fn run(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `run`:\n{source}"));
    assert!(
        run_body.contains("__smelt_call"),
        "calling a callable object must go through its `__smelt_call` slot:\n{source}"
    );
    assert!(
        !run_body.contains("let _smelt_tmp_4: SmeltUnknown = SmeltUnknown::Null;\n    _smelt_tmp_4"),
        "the call must not be replaced by a null default:\n{source}"
    );
}

/// A callback parameter captured into a returned callable object must enter its
/// function owned.
///
/// The returned record OWNS its `__smelt_call` slot as a `'static` handle, so a
/// borrowed `&dyn Fn` parameter captured into it cannot satisfy the coercion
/// ("coercion requires that `'1` must outlive `'static`" in es-toolkit `curry`).
/// The escape analysis walks container types for exactly this reason; a class or
/// interface is storage in the same way.
#[test]
fn typescript_callback_captured_into_returned_callable_object_is_owned() {
    let source = source_for(
        r"
interface Wrapped {
  (value: string): string;
  tag: number;
}

export function wrap(fn: (value: string) => string): Wrapped {
  const wrapped = function (value: string): string {
    return fn(value);
  };
  wrapped.tag = 1;
  return wrapped as Wrapped;
}
",
    );

    let signature = source
        .split("fn wrap(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap_or_else(|| panic!("expected a generated `wrap`:\n{source}"));
    assert!(
        !signature.contains("&dyn Fn"),
        "a callback retained by the returned record must not stay borrowed:\n{source}"
    );
}

/// A generated record emits the inbound half of the erasure round-trip.
///
/// Every lifted type parameter is bounded by `SmeltFromUnknown`, so a class that
/// lacks the impl cannot be used as a generic argument at all — es-toolkit's
/// `meanBy`/`medianBy` specs call generic helpers with `Person[]` and failed
/// with "the trait bound `Person: SmeltFromUnknown` is not satisfied". Only
/// `IntoSmeltUnknown` was ever emitted, so concrete class values could flow out
/// to erased code but never back.
#[test]
fn generated_records_emit_from_smelt_unknown() {
    let source = source_for(
        r"
class Person { name: string = ''; age: number = 0; }
function firstOf<T>(items: T[], fallback: T): T {
  if (items.length > 0) { return items[0]; }
  return fallback;
}
const people: Person[] = [];
const p = firstOf(people, new Person());
",
    );

    assert!(
        source.contains("impl SmeltFromUnknown for Person"),
        "a generated record must be recoverable from its erased view:\n{source}"
    );
    assert!(
        source.contains("impl IntoSmeltUnknown for Person"),
        "the outbound half must still be emitted:\n{source}"
    );
}

/// An overload set whose signatures all return the same type is stored as ONE
/// concrete closure whose parameters accept every overload's arguments: a
/// position's type is the union of the overloads' types there, and a position
/// some overload omits or marks optional is `Option<_>`. That is the signature
/// a hand-written port stores (the TypeScript implementation signature), so
/// the slot is not erased and a call passes its arguments straight through.
#[test]
fn overload_set_sharing_a_result_is_one_concrete_slot() {
    let source = source_for(
        "interface Setter {
  (key: string, value: number): void;
  (key: string, value: string): void;
}
interface Header {
  (name: string): void;
  (name: string, value: string, append?: boolean): void;
}
class Store {
  seen: string[] = [];
  set: Setter = (key: string, value: number | string) => {
    this.seen.push(key + '=' + String(value));
  };
  header: Header = (name: string, value?: string, append?: boolean) => {
    this.seen.push(name + ':' + (value ?? '') + (append ? '+' : ''));
  };
}
const store = new Store();
store.set('a', 1);
store.set('b', 'two');
store.header('x');
store.header('y', 'z', true);
",
    );

    assert!(
        !source.contains("__smelt_call: SmeltErasedFunction"),
        "an overload set sharing one result must not erase its slot:\n{source}"
    );
    assert!(
        source.contains(
            "__smelt_call: ::std::rc::Rc<dyn Fn(JsString, Option<JsString>, Option<bool>) -> ()>"
        ),
        "positions an overload omits or marks optional merge to Option:\n{source}"
    );
    assert!(
        !source.contains("smelt_callback.call(vec!["),
        "calls must reach the concrete slot without an erased adapter:\n{source}"
    );
}

#[test]
fn erased_object_field_list_alias_push_writes_back_an_aliasing_array() {
    // `pipe`'s hot loop in remeda: destructure an array field off an erased
    // object, push into the local, and the object must observe the push.
    //
    // Extracting the field already ALIASES the stored array (see
    // `erased_to_list_expr`'s array arm, `SmeltList::with_storage`), so the
    // write-back has to hand back another handle on that same buffer. Rebuilding
    // the element vector detached the field from the local's buffer, re-breaking
    // the aliasing once per push, and cost an O(n) copy every time.
    let source = source_for(
        r"
type Holder = { index: number } & { items: unknown[] };
function record(holder: Holder, value: unknown): void {
  const { items } = holder;
  items.push(value);
}
",
    );

    assert!(
        source.contains("let smelt_value = SmeltUnknown::Array(items.clone().into());"),
        "{source}"
    );
    assert!(
        !source.contains("map(IntoSmeltUnknown::into_smelt_unknown).collect());"),
        "the per-element rebuild must not appear for SmeltUnknown elements: {source}"
    );
}

#[test]
fn erased_object_field_list_alias_push_still_converts_concrete_elements() {
    // The other half of the same general rule: a `string[]` field extracts into
    // `SmeltList<String>`, which shares no representation with `SmeltArray`, so
    // there is nothing to alias and the elements genuinely have to be converted.
    // The aliasing re-wrap is keyed on the element type rendering as
    // `SmeltUnknown`, never on a library or a spelling.
    let source = source_for(
        r"
type Holder = { items: string[] } & { other: unknown };
function record(holder: Holder, value: string): void {
  const items = holder.items;
  items.push(value);
}
",
    );

    assert!(
        source.contains(
            "let smelt_value = SmeltUnknown::Array(items.clone().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect());"
        ),
        "{source}"
    );
}

#[test]
fn an_erased_adapter_recovers_a_tuple_parameter_element_wise() {
    // The mirror of the tuple ERASURE fix: a Rust tuple has no
    // `SmeltFromUnknown` impl, so the blanket
    // `SmeltFromUnknown::smelt_from_unknown(..)` an erased method adapter
    // reaches for does not compile against a tuple parameter. It is recovered
    // from the erased array it was written as instead.
    let source = source_for(
        r"
type Entry = [string, number];
export class Bag {
  seen: string = '';
  take(entry: Entry): string {
    this.seen = entry[0];
    return this.seen;
  }
}
export function use(bag: Bag): string {
  return bag.take(['a', 1]);
}
",
    );

    // The enabling condition: an erased method adapter is actually emitted for
    // this class, which is the only place the blanket recovery appeared.
    assert!(
        source.contains("__smelt_method:take"),
        "the fixture must emit an erased method adapter: {source}"
    );
    assert!(
        source.contains("if let SmeltUnknown::Array(smelt_tuple_values)"),
        "a tuple parameter must be recovered element-wise: {source}"
    );
}

/// An array literal whose destination is an OPTIONAL tuple renders a Rust
/// tuple, not a homogeneous `vec![..]`.
///
/// The tuple arm renders a literal into a Rust tuple when the destination is
/// one, but an optional destination has to reach it through the
/// `Type::Optional` recursion, and `Type::Tuple` was missing from that arm's
/// list. The literal then fell through to the `vec![..]` fallback, which infers
/// its element type from the FIRST element and rejects the rest -- hono's
/// `Matcher<T> = [RegExp, HandlerData<T>[][], StaticMap<T>]`, returned as
/// `Matcher<T> | null`, reported it as "expected SmeltRegExp, found
/// SmeltList<SmeltUnknown>" on element 2.
#[test]
fn an_array_literal_at_an_optional_tuple_renders_a_tuple() {
    let source = source_for(
        r"
type Triple = [string, number[], boolean];

export function build(label: string, flag: boolean): Triple | null {
  if (!flag) {
    return null;
  }
  return [label, [1, 2], flag];
}
",
    );

    // The enabling condition: the destination really is an optional tuple, so
    // the literal has to travel through the `Optional` recursion to reach the
    // tuple rendering.
    assert!(
        source.contains("-> Option<(JsString, SmeltList<f64>, bool)>"),
        "the return really must be an optional tuple: {source}"
    );
    assert!(
        source.contains("Some((label.clone(),"),
        "an array literal at an optional tuple must render `Some((..))`: {source}"
    );
    // The homogeneous fallback would have annotated a Vec of one element type,
    // which is what could not hold the heterogeneous elements.
    assert!(
        !source.contains("let smelt_list_items: Vec<JsString> = vec![label.clone()"),
        "the literal must not go through the homogeneous Vec fallback: {source}"
    );
}

/// A host value converts to a structurally compatible record by READING each
/// target field off it.
///
/// A `Response` is assignable to `{ status?: number; statusText?: string }`
/// because it exposes both members — plain structural typing, not an overload
/// and not a union. The reads are not field reads: `SmeltResponse` keeps its
/// data behind accessors, so the conversion goes through the text-level member
/// read rather than through a `Name { field: base.field }` literal, which does
/// not compile against the prelude struct.
#[test]
fn adapts_a_host_value_to_a_structurally_compatible_record() {
    let source = source_for(
        r#"
interface Init {
  status?: number;
  statusText?: string;
  label?: string;
}

export function describe(init: Init): string {
  return String(init.status);
}

export function report(): string {
  return describe(new Response("hi", { status: 201 }));
}
"#,
    );

    assert!(
        source.contains("status: Some(smelt_host_value.status())"),
        "{source}"
    );
    assert!(
        source.contains("status_text: Some(JsString::from(smelt_host_value.status_text())"),
        "{source}"
    );
    // A member the source does not expose is absent, which is what an optional
    // property means — never a fabricated value.
    assert!(source.contains("label: None"), "{source}");
    // The source is bound once: `value_text` may be a call, and reading it per
    // field would evaluate it per field.
    assert_eq!(source.matches("let smelt_host_value =").count(), 1, "{source}");
}

/// The same conversion declines when the source exposes no members at all.
///
/// Without that gate a dictionary source reported "no member" for every target
/// field and an all-optional record was built entirely out of absent fields — a
/// conversion that compiles and silently drops the value.
#[test]
fn a_record_target_is_not_built_from_a_source_with_no_members() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

interface FormatLong {
  date: (options: Options) => string;
}

export function run(formatLong: FormatLong): string {
  return formatLong.date({ width: "short" });
}
"#,
    );

    assert!(!source.contains("smelt_host_value"), "{source}");
    assert!(
        source.contains("Options { width: smelt_record_map.get_text(\"width\").cloned().map(|smelt_value| smelt_value) }"),
        "{source}"
    );
}

/// An indexed read on a typed-array view is already an `f64`, so a `Number(..)`
/// over it must not run the erased numeric-conversion match.
///
/// `place_ty` used to answer the erased fallback for the read while
/// `typed_array_index_read_expr` rendered `view.get(i).unwrap_or(0.0)`, so the
/// conversion matched `SmeltUnknown` arms against an `f64` scrutinee and the
/// crate did not compile (E0308, ~50 of them in one `hex()` helper). The text
/// and the type are now decided by the same rule.
#[test]
fn a_typed_array_element_read_is_a_concrete_number() {
    let source = source_for(
        r#"
export function hex(bytes: Uint8Array): string {
  let out = "";
  for (let index = 0; index < bytes.length; index = index + 1) {
    out = out + Number(bytes[index]).toString(16);
  }
  return out;
}
"#,
    );

    assert!(
        source.contains(".get(") && source.contains("unwrap_or(0.0)"),
        "the element read should decode at the view's width:\n{source}"
    );
    for line in source.lines() {
        assert!(
            !(line.contains("unwrap_or(0.0)") && line.contains("__smelt_date")),
            "a concrete element read must not be converted through the erased \
             numeric match:\n{line}"
        );
    }
}

/// A program whose only identity-bearing container is a primitive-keyed `Set`
/// still defines the object-id counter its constructor calls.
///
/// `SmeltPrimSet::new` mints an id, but the identity gate's demand list did not
/// name the primitive set, so the emitted constructor called a function the
/// crate never declared (E0425).
#[test]
fn a_primitive_set_program_defines_the_object_id_counter() {
    let source = source_for(
        r"
export function unique(values: string[]): number {
  const seen = new Set<string>();
  for (const value of values) {
    seen.add(value);
  }
  return seen.size;
}
",
    );

    if source.contains("smelt_next_object_id()") {
        assert!(
            source.contains("fn smelt_next_object_id()"),
            "the crate calls the object-id counter but never defines it:\n{source}"
        );
    }
}

#[test]
/// A record adapter that reaches its own `(source, target)` pair through a
/// callable field (`swap(): Pair<T>`) calls a hoisted helper for the pair
/// instead of truncating the cycle to `Default::default()`, which dropped the
/// swapped value (`0 0`).
fn self_reaching_record_adapter_calls_a_hoisted_helper() {
    let source = source_for(
        r"
interface Pair<T> { left: T; right: T; same: boolean; swap(): Pair<T> }
function makeChecked<T>(left: T, right: T): Pair<T> {
  return { left, right, same: left === right, swap() { return makeChecked(right, left); } };
}
export function swapped(): number { return makeChecked(1, 2).swap().left; }
",
    );
    assert!(source.contains("fn __smelt_adapt_record_0(smelt_value: Pair<SmeltUnknown>) -> Pair<f64>"), "{source}");
    assert!(!source.contains("::std::rc::Rc::new(move || Default::default())"), "{source}");
}

#[test]
/// `Partial<X>` of a plain record is its own struct with every field optional,
/// and an object spread at a record type builds that struct field by field:
/// an optional source field writes only when present.
fn partial_records_and_typed_spreads_build_structs() {
    let source = source_for(
        r"
type Options = { origin: string; maxAge?: number };
export function merge(base: Options, options: Partial<Options>): Options {
  return { ...base, ...options };
}
",
    );
    assert!(source.contains("struct PartialOptions"), "{source}");
    assert!(source.contains("options: PartialOptions"), "{source}");
    assert!(!source.contains("dict_assign") && !source.contains("SmeltRecord<String, SmeltUnknown>") && !source.contains("SmeltRecord<JsString, SmeltUnknown>"), "{source}");
}

/// An interface's FUNCTION-typed fields survive erasure: the record's
/// `IntoSmeltUnknown` impl wraps each one in a callable adapter that registers
/// the typed callable as its origin, instead of writing `SmeltUnknown::Null`.
///
/// Hono copied its `Router` (`add`, `match`) through such an erased view in
/// `Object.assign(this, rest)`, and the router it read back answered `null`
/// for every `match`, so a `basePath()` app never routed. A borrowed `&T`
/// parameter is passed a reference to the converted argument.
#[test]
fn record_function_fields_erase_to_callable_adapters() {
    let source = source_for(
        r"
interface Router<T> {
  name: string
  add(method: string, handler: T): void
  match(method: string): string
}
declare function getRouter(): Router<string>
export function erase(router: Router<string>): unknown {
  const { name, ...rest } = router
  return rest
}
",
    );
    let impl_start = source
        .find("IntoSmeltUnknown for Router<")
        .unwrap_or_else(|| panic!("no Router erasure impl:\n{source}"));
    let impl_text = &source[impl_start..source[impl_start..].find("\n}\n").map_or(source.len(), |end| impl_start + end)];
    assert!(
        impl_text.contains("(\"match\".to_owned(), { let smelt_function_value"),
        "`match` must erase to an adapter, not `null`:\n{impl_text}"
    );
    assert!(
        impl_text.contains("smelt_register_function_origin(&smelt_erased_function, smelt_function_origin)"),
        "the adapter must register its typed origin:\n{impl_text}"
    );
    assert!(
        !impl_text.contains("(\"match\".to_owned(), SmeltUnknown::Null)"),
        "{impl_text}"
    );
}
