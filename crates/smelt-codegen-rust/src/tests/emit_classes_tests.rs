//! Generated Rust for classes, methods and accessors.

use super::*;

/// A class expression calling a method it inherits resolves that method and stays a direct
/// method call, even though its members lower before its own item is registered.
#[test]
fn class_expression_call_to_an_inherited_method_stays_a_method_call() {
    let source = source_for(
        r"
class Counter {
  #hits: string[] = [];

  add(key: string): void {
    this.#hits.push(key);
  }

  protected tally(): string {
    return this.#hits.join(',');
  }
}

const Reporting = class extends Counter {
  report(): string {
    return this.tally();
  }
};

export const reported = new Reporting().report();
",
    );

    // A direct call on the receiver, not a field read dispatched dynamically.
    assert!(source.contains("self.tally()"), "{source}");
    assert!(!source.contains("self.tally.clone()"), "{source}");
}

/// A class string index signature emits a real keyed store: a pure-index class exposes
/// keyed access with an honest `Option<T>` read, and a mixed class keeps its named field
/// concretely typed alongside it. The index value type stays concrete, never
/// `SmeltUnknown`.
#[test]
fn emits_class_index_signature_named_fields_and_keyed_store() {
    let source = source_for(
        r"
class StringBag {
  [key: string]: string;
}

class MixedBag {
  size: number = 0;
  [key: string]: number;
}

export function readBag(bag: StringBag, key: string): string | undefined {
  return bag[key];
}

export function writeBag(bag: StringBag, key: string, value: string): void {
  bag[key] = value;
}

export function mixedSize(bag: MixedBag): number {
  return bag.size;
}
",
    );

    assert_eq!(rust_items(&source, &["sig read_bag", "sig mixed_size"]), r"fn read_bag(bag: StringBag, key: JsString) -> Option<JsString>

fn mixed_size(bag: MixedBag) -> f64");
    assert!(source.contains("size: f64"), "{source}");
    assert!(source.contains(".__smelt_index_store.get(&"), "the keyed read routes to the store:\n{source}");
    assert!(source.contains(".__smelt_index_store.insert("), "the keyed write routes to the store:\n{source}");
}

#[test]
fn omits_private_class_fields_from_erased_object_projection() {
    // The observable `Object.keys` result is in the `classes_behaviour` node-parity fixture.
    let source = source_for(
        r#"
class PrivateBox {
  readonly #value = "hidden";
}
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const count = keyCount(new PrivateBox());
"#,
    );

    assert!(!source.contains("(\"value\".to_owned(), SmeltUnknown::String(self.value))"), "private class fields must not become enumerable erased object keys: {source}");
    assert!(!source.contains("smelt_object_entries.insert(\"value\".to_owned()"), "direct class-to-unknown erasure must also omit private fields: {source}");
    assert_eq!(rust_items(&source, &["sig key_count"]), r"fn key_count(value: SmeltUnknown) -> f64");
}

#[test]
fn omits_class_getters_from_erased_object_projection() {
    // The observable `Object.keys` result is in the `classes_behaviour` node-parity fixture.
    let source = source_for(
        r#"
class GetterBox {
  get value(): string {
    return "hidden";
  }
}
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const count = keyCount(new GetterBox());
"#,
    );

    assert!(!source.contains("smelt_object_entries.insert(\"value\".to_owned()"), "class accessors live on the prototype and must not erase as own enumerable fields: {source}");
    assert_eq!(rust_items(&source, &["sig key_count"]), r"fn key_count(value: SmeltUnknown) -> f64");
}

#[test]
fn emits_first_class_object_entries_as_real_projection() {
    let source = source_for(
        r"
declare function purry(fn: (value: unknown) => unknown, args: readonly unknown[]): unknown;

export function entries(...args: readonly unknown[]): unknown {
  return purry(Object.entries, args);
}
",
    );

    assert!(
        source.contains("SmeltRecord::with_id_from_entries"),
        "Object.entries callback should cast its argument through record projection: {source}"
    );
    assert!(
        source.contains(
            "SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|smelt_value| SmeltUnknown::Array"
        ),
        "Object.entries callback should return entry arrays, not a null placeholder: {source}"
    );
    assert!(
        !source
            .contains("::std::rc::Rc::new(|closure_arg_0: SmeltUnknown| {\n    SmeltUnknown::Null"),
        "Object.entries callback should not lower to the static-member placeholder closure: {source}"
    );
}

#[test]
fn emits_first_class_object_from_entries_as_real_conversion() {
    let source = source_for(
        r"
declare function purry(fn: (value: unknown) => unknown, args: readonly unknown[]): unknown;

export function fromEntries(...args: readonly unknown[]): unknown {
  return purry(Object.fromEntries, args);
}
",
    );

    assert!(
        source.contains("collect::<SmeltRecord<JsString, SmeltUnknown>>()"),
        "Object.fromEntries callback should collect entry arrays into a record: {source}"
    );
    assert!(
        !source
            .contains("::std::rc::Rc::new(|closure_arg_0: SmeltUnknown| {\n    SmeltUnknown::Null"),
        "Object.fromEntries callback should not lower to the static-member placeholder closure: {source}"
    );
}

#[test]
fn emits_error_constructor_values_with_runtime_error_identity() {
    // The observable behaviour (identity, `Object.keys` hiding `stack`) is in the
    // `classes_behaviour` node-parity fixture.
    let source = source_for(
        r#"
function makeError(): unknown {
  return new Error("bad");
}

const value = makeError();
const yes = value instanceof Error;
"#,
    );

    assert_eq!(rust_items(&source, &["sig make_error"]), r"fn make_error() -> SmeltUnknown");
}

#[test]
fn emits_runtime_sized_numeric_typed_array_constructors() {
    // `new Uint8Array(count)` allocates `count` elements at a runtime count, never an unrolled
    // literal, and the indexed write encodes one element at that width. The element stride is in
    // the `classes_behaviour` node-parity fixture.
    let source = source_for(
        r"
export function make(count: number): number[] {
  const output = new Uint8Array(count);
  for (let index = 0; index < count; index += 1) {
    output[index] = index + 1;
  }
  return output;
}
",
    );

    assert_eq!(rust_items(&source, &["fn make"]), r#"fn make(count: f64) -> SmeltList<f64> {
let mut _smelt_tmp_4: bool;
let mut _smelt_tmp_5: f64;
let mut _smelt_tmp_6: f64;
let mut output: SmeltTypedArray = SmeltTypedArray::with_length(SmeltTypedArrayKind::Uint8, ((count) as f64).max(0.0) as usize);
let mut index: f64 = 0.0;
loop {
_smelt_tmp_4 = index < count;
if !(_smelt_tmp_4) { break; }
_smelt_tmp_5 = index + 1.0;
output.set_index(index, _smelt_tmp_5);
_smelt_tmp_6 = index + 1.0;
index = _smelt_tmp_6;
}
return { let smelt_item = |smelt_value: SmeltUnknown| match smelt_value.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }; { let smelt_src = ((output.clone()).into_smelt_unknown()).clone().into_smelt_unknown(); match smelt_src { SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), SmeltUnknown::Array(smelt_values) => { let smelt_list_id = smelt_values.id; let smelt_list = SmeltList::with_id(smelt_list_id, smelt_values.into_iter().map(|smelt_value| smelt_item(smelt_value)).collect::<Vec<_>>()); smelt_array_props_adopt(smelt_list_id, &smelt_list.storage()); smelt_list }, SmeltUnknown::String(smelt_value) => SmeltList::new(smelt_value.code_point_strings().into_iter().map(|smelt_ch| smelt_item(SmeltUnknown::String(smelt_ch))).collect::<Vec<_>>()), SmeltUnknown::Object(smelt_value) => if let Some(smelt_bytes) = smelt_host_buffer_elements(&SmeltUnknown::Object(smelt_value.clone())) { SmeltList::new(smelt_bytes.into_iter().map(|smelt_value| smelt_item(smelt_value)).collect::<Vec<_>>()) } else if let Some(smelt_args) = smelt_arguments_elements(&smelt_value) { SmeltList::new(smelt_args.into_iter().map(|smelt_value| smelt_item(smelt_value)).collect::<Vec<_>>()) } else if let Some(SmeltUnknown::Array(pairs)) = smelt_value.get("__smelt_map") { SmeltList::new(pairs.into_vec().into_iter().map(|smelt_value| smelt_item(smelt_value)).collect::<Vec<_>>()) } else if let Some(SmeltUnknown::Array(members)) = smelt_value.get("__smelt_set") { SmeltList::new(members.into_vec().into_iter().map(|smelt_value| smelt_item(smelt_value)).collect::<Vec<_>>()) } else { match smelt_value.get("__smelt_symbol_iterator") { Some(SmeltUnknown::Function(iterator)) => SmeltList::new(smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null)).into_iter().map(|smelt_value| smelt_item(smelt_value)).collect::<Vec<_>>()), _ => panic!("unknown is not iterable") } }, _ => panic!("unknown is not iterable") } } };
}"#);
}

#[test]
fn emits_bigint_typed_array_constructor_through_the_shared_host_constructor() {
    // `BigInt64Array`/`BigUint64Array` share the concrete family with the other views: an eight-
    // byte element, so three elements are 24 bytes and `.length` reads 3.
    let source = source_for(
        r"
export function make(): number {
  const values = new BigUint64Array([1, 2, 3]);
  return values.length;
}
",
    );

    assert_eq!(rust_items(&source, &["fn make"]), r"fn make() -> f64 {
let _smelt_tmp_1: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
let values: SmeltTypedArray = SmeltTypedArray::with_elements(SmeltTypedArrayKind::BigUint64, &_smelt_tmp_1.to_vec());
let _smelt_tmp_3: f64 = values.length();
return _smelt_tmp_3;
}");
}

/// An implicit derived constructor declares the base constructor's parameters, not one
/// erased forwarded argument: TypeScript types `new Derived(..)` against the base
/// signature, so the arity is known and the base's parameter properties run.
#[test]
fn emits_default_derived_class_constructor_with_the_base_constructors_parameters() {
    let source = source_for(
        r"
class Point {
  constructor(public x: number, public y: number) {}
}
class Point3 extends Point {
  z: number = 3;
}
const p = new Point3(1, 2);
",
    );

    assert!(source.contains("fn new(x: f64, y: f64) -> Self"), "{source}");
    assert!(source.contains("Point3::new(1.0, 2.0)"), "{source}");
    assert!(
        !source.contains("_smelt_super_arg"),
        "a reproducible base has a known arity, so nothing forwards through an erased slot:\n{source}"
    );
    // The forwarded `super(..)` is what makes the base's parameter properties
    // run at all; without it the derived struct kept the field defaults. It runs
    // the base constructor over the derived instance (see `base_init`).
    assert!(source.contains("Self::__smelt_init_Point(this, x, y)"), "{source}");
}

/// The erased forwarded argument survives only where the base is NOT a
/// source-declared class, and that is a genuine dynamic boundary.
///
/// A host constructor leaves this lowering with no parameter list to copy —
/// its arity and parameter types are not available to it — so no concrete
/// type, generated union, or scoped generic can carry the forwarded argument,
/// and the call-compatible erased slot is what keeps `new Subclass(x)` from
/// becoming a blocker.
#[test]
fn a_host_base_keeps_the_erased_forwarded_constructor_argument() {
    let source = source_for(
        r"
class Stamp extends Date {}
const stamp = new Stamp(0);
",
    );

    assert!(
        source.contains("fn new(_smelt_super_arg: Option<SmeltUnknown>) -> Self"),
        "{source}"
    );
}

#[test]
fn emits_no_arg_external_constructor_with_valid_empty_arg_tuple() {
    let source = source_for(
        r#"
import { UTCDate } from "./utc-date";
const date = new UTCDate();
"#,
    );

    assert!(
        source.contains("let _smelt_external_args = ();"),
        "{source}"
    );
    assert!(
        !source.contains("let _smelt_external_args = (,);"),
        "{source}"
    );
}

#[test]
fn lowers_a_unique_symbol_keyed_class_member_and_its_read() {
    // A module-level `const` bound to a unique `Symbol()` is evaluated once, so it is a stable
    // static key: the member and its read are both ordinary.
    let source = source_for(
        r"
const KEY = Symbol();

class Holder {
  get [KEY](): number {
    return 41;
  }
}

export function read(holder: Holder): number {
  return holder[KEY];
}
",
    );

    assert_eq!(rust_items(&source, &["fn read"]), r"fn read(holder: Holder) -> f64 {
return holder.__smelt_get___smelt_symbol_unique__at_13();
}");
}

#[test]
fn lowers_a_computed_method_call_over_known_members() {
    // `receiver[key]()` on a receiver whose member set is known is a choice among known methods
    // and lowers as one.
    let source = source_for(
        r"
class Body {
  json(): string {
    return 'json-body';
  }
  text(): string {
    return 'text-body';
  }
}

export function read(body: Body, key: 'json' | 'text'): string {
  return body[key]();
}
",
    );

    assert_eq!(rust_items(&source, &["fn read"]), r#"fn read(body: Body, key: JsString) -> JsString {
let mut _smelt_tmp_3: JsString;
let _smelt_tmp_2: bool = key == "json";
if _smelt_tmp_2 {
let _smelt_tmp_4: JsString = body.json();
_smelt_tmp_3 = _smelt_tmp_4;
return _smelt_tmp_3;
} else {
let _smelt_tmp_5: JsString = body.text();
_smelt_tmp_3 = _smelt_tmp_5;
return _smelt_tmp_3;
}
}"#);
}

#[test]
fn leaves_a_computed_method_call_with_arguments_alone() {
    // Arguments are lowered once and would be referenced from every arm, so the desugar declines a
    // call that has any; the call stays on the dynamic path.
    let source = source_for(
        r"
class Body {
  json(prefix: string): string {
    return prefix + 'json';
  }
  text(prefix: string): string {
    return prefix + 'text';
  }
}

export function read(body: Body, key: 'json' | 'text'): string {
  return body[key]('p');
}
",
    );

    assert_eq!(rust_items(&source, &["fn read"]), r#"fn read(body: Body, key: JsString) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_2: SmeltUnknown = { let smelt_function_value = smelt_get_unknown_field(&{ let smelt_object_value = body.clone(); let smelt_struct_value = smelt_object_value.clone(); let mut smelt_object_entries = Vec::<(String, _)>::new();  smelt_object_entries.push(("__smelt_class".to_owned(), SmeltUnknown::String("Body".into()))); smelt_object_entries.extend(smelt_struct_value.__smelt_proto_entries()); SmeltUnknown::Object(SmeltObject::new(smelt_object_entries)) }, &key.clone()).clone(); let smelt_call_args: Vec<SmeltUnknown> = Into::into(vec![SmeltUnknown::String("p".into())]); let smelt_callable = match smelt_function_value { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }, _ => None }; if let Some(smelt_function) = smelt_callable { (smelt_function)(smelt_call_args).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)) } else { smelt_value_not_callable() } };
return Ok(match _smelt_tmp_2.clone() { SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other { SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }) });
}"#);
}

#[test]
fn keeps_python_constructor_call_result_as_class_value() {
    let source = source_for_py(
        r#"
class Obj:
    id: str

    def __init__(self, id: str) -> None:
        self.id = id

obj: Obj = Obj("a")
"#,
    );

    assert!(source.contains("Obj::new(\"a\".to_owned())"), "{source}");
    assert!(
        !source.contains("obj: Obj = Default::default()"),
        "{source}"
    );
}

#[test]
fn emits_mutable_class_method_parameters_when_reassigned() {
    let source = source_for(
        r"
class Parser {
  set(date: number, value: number): number {
    date = value + 1;
    return date;
  }
}
",
    );

    assert!(
        source.contains("fn set(&self, mut date: f64, value: f64) -> f64"),
        "{source}"
    );
}

#[test]
fn emits_mutable_constructor_parameters_when_reassigned() {
    let source = source_for(
        r"
class Box {
  value: number;
  constructor(value: number) {
    value = value + 1;
    this.value = value;
  }
}
",
    );

    assert!(
        source.contains("fn new(mut value: f64) -> Self"),
        "{source}"
    );
}

#[test]
fn deduplicates_interface_fields_after_inheritance_expansion() {
    let source = source_for(
        r#"
interface ContextOptions {
  in?: unknown;
}
interface FormatOptions extends ContextOptions {
  locale?: string;
  in?: unknown;
}
const options: FormatOptions = { locale: "en", in: null };
const text = JSON.stringify(options);
"#,
    );

    assert!(source.contains("struct FormatOptions"), "{source}");
    let format_options = source
        .split("struct FormatOptions")
        .nth(1)
        .and_then(|text| text.split("}\n").next())
        .expect("FormatOptions struct should be emitted");
    assert_eq!(
        format_options.matches("in_: Option<SmeltUnknown>,").count(),
        1,
        "{source}"
    );
}

#[test]
fn emits_virtual_method_slots_for_abstract_base_adapters() {
    // `ParseResult<T>` is an object-shaped alias that lowers to a real generic struct. The base
    // declares the slot at its own `T`; the subclass's flattened layout stores it at the argument
    // it binds. Classes with virtual slots are reference classes, read through the shared cell.
    let source = source_for(
        r#"
type ParseResult<T> = { value: T };

abstract class Parser<T> {
  run(input: string): T {
    const result = this.parse(input);
    if (!this.validate(result.value)) {
      return result.value;
    }
    return result.value;
  }

  validate(value: T): boolean {
    return true;
  }

  abstract parse(input: string): ParseResult<T>;
}

class YearParser extends Parser<number> {
  parse(input: string): ParseResult<number> {
    return { value: 2017 };
  }
}

const parsers: Record<string, Parser<any>> = { y: new YearParser() };

export function read(): number {
  return parsers["y"].run("ignored");
}
"#,
    );

    // `ParseResult<T>` is an object-shaped alias, which now lowers to a real
    // generic struct (it used to erase to `SmeltUnknown`). The base declares
    // the slot at its own `T`; the subclass's flattened layout stores it at the
    // argument it binds (`extends Parser<number>`).
    assert!(
        source.contains("parse: ::std::rc::Rc<dyn Fn(JsString) -> ParseResult<T>>"),
        "{source}"
    );
    assert!(
        source.contains("parse: ::std::rc::Rc<dyn Fn(JsString) -> ParseResult<f64>>"),
        "{source}"
    );
    // The inherited `run` copy in `impl YearParser` spells `T` as `f64`: the
    // impl does not declare `T` (was E0425).
    assert!(
        source.contains("fn run(&self, input: JsString) -> Result<f64,"),
        "{source}"
    );
    assert!(
        source.contains("::std::rc::Rc::new(move |smelt_arg0: JsString|"),
        "{source}"
    );
    assert!(
        source.contains("smelt_method_receiver.parse(smelt_arg0.clone())"),
        "{source}"
    );
    // These classes store virtual slots, so they are reference classes
    // (`classify::collect_virtual_slot_triggers`) and fields are read through
    // the shared cell; the requirement (dispatch through the stored slot) is
    // unchanged.
    assert!(
        source.contains("&self.0.borrow().parse.clone()); (smelt_callable)(input.clone())"),
        "{source}"
    );
    assert!(!source.contains("parse: Default::default()"), "{source}");
}

#[test]
fn calls_base_typed_virtual_methods_through_stored_function_fields() {
    // Classes with virtual slots are reference classes; a base-typed virtual call dispatches
    // through the stored field in the shared cell.
    let source = source_for(
        r"
abstract class Setter {
  validate(value: number): boolean {
    return true;
  }
}

class ValueSetter extends Setter {
  validate(value: number): boolean {
    return value >= 0 && value <= 11;
  }
}

const setter: Setter = new ValueSetter();
const accepted = setter.validate(12);
",
    );

    // These classes store virtual slots, so they are reference classes
    // (`classify::collect_virtual_slot_triggers`) and fields are read through
    // the shared cell; the requirement (dispatch through the stored slot) is
    // unchanged.
    assert!(
        source.contains("&setter.0.borrow().validate.clone()); (smelt_callable)(12.0)"),
        "base-typed virtual calls should dispatch through the stored function field: {source}"
    );
    assert!(
        !source.contains("setter.validate(12.0)"),
        "base inherent method calls lose subclass overrides: {source}"
    );
}

#[test]
fn binds_stored_virtual_methods_when_reerasing_base_class_values() {
    let source = source_for(
        r"
type Result<T> = { value: T };

abstract class Parser<T> {
  abstract parse(input: string): Result<T>;
}

class ForwardingParser<T> extends Parser<T> {
  constructor(readonly delegate: Parser<T>) {
    super();
  }

  parse(input: string): Result<T> {
    return this.delegate.parse(input);
  }
}

class YearParser extends Parser<number> {
  parse(input: string): Result<number> {
    return { value: 2017 };
  }
}

const parser: Parser<any> = new ForwardingParser(new YearParser());
",
    );

    assert!(
        source
            .contains("ParserInner { parse: { let smelt_virtual_receiver = smelt_struct_value.clone();"),
        "{source}"
    );
    assert!(
        source.contains("smelt_method_receiver.parse(smelt_arg0.clone())"),
        "{source}"
    );
    assert!(!source.contains("parse: Default::default()"), "{source}");
}

#[test]
fn emits_optional_constructor_parameter_without_unwrapping_optional_argument() {
    let source = source_for(
        r"
class Parser {
  subPriority?: number;
  constructor() {}
}
class ValueSetter {
  constructor(subPriority?: number) {}
}
function make(parser: Parser): ValueSetter {
  return new ValueSetter(parser.subPriority);
}
",
    );

    assert!(
        source.contains("fn new(sub_priority: Option<f64>) -> Self"),
        "{source}"
    );
    assert!(
        !source.contains(
            "sub_priority.clone().expect(\"optional value was absent after narrowing\")"
        ),
        "{source}"
    );
}

#[test]
fn emits_non_destructive_object_getter_reads() {
    let source = source_for(
        r"
function makeCounter(): { readonly value: number } {
  let value = 0;
  return {
    get value() {
      value += 1;
      return value;
    },
  };
}

const counter = makeCounter();
const first = counter.value;
const second = counter.value;
",
    );

    assert!(
        source.contains("match getter.get(\"__smelt_get\")"),
        "{source}"
    );
    assert!(
        !source.contains("getter.remove(\"__smelt_get\")"),
        "{source}"
    );
}

#[test]
fn lowers_string_methods_on_coercible_receivers() {
    // String padding/charAt/repeat and prefix-with-position accept string-compatible receivers (`T
    // extends string`, erased returns) and numeric-like arguments. Behaviour is in the
    // `classes_behaviour` node-parity fixture.
    let source = source_for(
        r"
export function pad(s: string, n: number, c: string): string {
  return s.padStart(n, c);
}
export function firstChar<T extends string>(s: T): string {
  return s.charAt(0);
}
export function repeated(s: string, n: number): string {
  return s.repeat(n);
}
export function startsAt(s: string, t: string, p: number): boolean {
  return s.startsWith(t, p);
}
",
    );

    assert_eq!(rust_items(&source, &["sig pad", "sig first_char", "sig repeated", "sig starts_at"]), r"fn pad(s: JsString, n: f64, c: JsString) -> JsString

fn first_char(s: SmeltUnknown) -> JsString

fn repeated(s: JsString, n: f64) -> JsString

fn starts_at(s: JsString, t: JsString, p: f64) -> bool");
}

#[test]
fn lowers_compact_unsupported_method_inside_callback_through_closure_body() {
    // `String.prototype.repeat` is modeled by the general method-call lowering
    // but not by the restricted compact-callback method dispatcher. An
    // expression-bodied `map` callback first lowers through the compact path,
    // which rejects `repeat` with "is not lowered into closure bodies yet"; the
    // fallback must retry through the full closure-body path, which routes the
    // receiver through the general `expression` lowering and emits the real
    // `.repeat(...)` call against the closure's element-typed parameter.
    let source = source_for(
        r"
export function cbRepeat(xs: string[]): string[] {
  return xs.map(value => value.repeat(2));
}
",
    );
    assert!(
        source.contains("closure_arg_0.clone().repeat("),
        "callback body should emit the real string repeat call: {source}"
    );
}

#[test]
fn lowers_optional_class_field_to_option_with_explicit_construction() {
    // An optional TypeScript class field (`y?: number`) must lower to a concrete
    // `Option<f64>` struct slot. Construction that supplies the field passes
    // `Some(..)`; construction that omits it passes `None::<f64>`, mirroring how
    // optional interface fields already lower. The named non-optional field
    // (`x: number`) stays concrete `f64` with no `Option` wrapper.
    let source = source_for(
        r"
class Point {
  x: number;
  y?: number;
  constructor(x: number, y?: number) {
    this.x = x;
    this.y = y;
  }
  total(): number {
    return this.x + (this.y ?? 0);
  }
}

function make(): number {
  const a = new Point(1, 2);
  const b = new Point(3);
  return a.total() + b.total();
}
",
    );

    assert!(source.contains("struct Point"), "{source}");
    assert!(source.contains("x: f64,"), "{source}");
    assert!(source.contains("y: Option<f64>,"), "{source}");
    // Construction with the field present wraps the value in `Some(..)`.
    assert!(source.contains("Point::new(1.0, Some(2.0))"), "{source}");
    // Construction that omits the trailing optional field supplies a typed `None`.
    assert!(source.contains("Point::new(3.0, None::<f64>)"), "{source}");
    // Reading the field through `??` consumes the `Option<f64>` directly.
    assert!(source.contains("unwrap_or(0.0)"), "{source}");
}

#[test]
fn lowers_optional_dataclass_field_to_option_with_explicit_construction() {
    // A Python dataclass field annotated `Optional[int]` (with a `None` default)
    // must lower to a concrete `Option<i64>` struct slot, and construction that
    // omits the field must synthesize the typed `None` default while a present
    // argument is wrapped as `Some(..)`. The required `int` field stays concrete.
    let source = source_for_py(
        r"
from dataclasses import dataclass
from typing import Optional


@dataclass
class Point:
    x: int
    y: Optional[int] = None


def make() -> Optional[int]:
    a = Point(1, 2)
    b = Point(3)
    return a.y
",
    );

    assert!(source.contains("struct Point"), "{source}");
    assert!(source.contains("x: i64,"), "{source}");
    assert!(source.contains("y: Option<i64>,"), "{source}");
    assert!(source.contains("Point::new(1, Some(2))"), "{source}");
    assert!(source.contains("Point::new(3, None::<i64>)"), "{source}");
    // Reading the optional field yields the `Option<i64>` value unchanged.
    assert!(source.contains("a.y.clone()"), "{source}");
}

/// A TypeScript `static` method lowers to a receiver-free associated function and a
/// qualified static call resolves to `Class::method(..)`.
#[test]
fn emits_typescript_static_method_as_associated_function() {
    let source = source_for(
        r"
class MathUtils {
  static square(value: number): number {
    return value * value;
  }
}
export function area(radius: number): number {
  return MathUtils.square(radius);
}
",
    );

    // The static method is emitted inside the class impl with no `self`.
    assert!(source.contains("fn square(value: f64) -> f64"), "{source}");
    assert!(
        !source.contains("fn square(&self") && !source.contains("fn square(&mut self"),
        "static method must not take a receiver: {source}"
    );
    // The qualified call resolves to the associated function.
    assert!(source.contains("MathUtils::square("), "{source}");
}

/// A Python `@staticmethod` lowers to a receiver-free associated function and a class-level
/// variable lowers to a static field read.
#[test]
fn emits_python_static_method_and_class_var() {
    let source = source_for_py(
        r"
class MathUtils:
    PI = 3

    @staticmethod
    def square(value: float) -> float:
        return value * value

def area(radius: float) -> float:
    return MathUtils.square(radius) * MathUtils.PI
",
    );

    assert!(source.contains("fn square(value: f64) -> f64"), "{source}");
    assert!(!source.contains("fn square(&self") && !source.contains("fn square(&mut self"), "static method must not take a receiver: {source}");
    assert!(source.contains("MathUtils::square("), "{source}");
    assert_eq!(rust_items(&source, &["fn __smelt_static_PI"]), r"fn __smelt_static_PI() -> i64 {
3
}");
}

/// A module const whose initializer is a concat/slice method chain
/// (es-toolkit's `empties = [[], {}].concat(falsey.slice(1))` shape) inlines
/// into a function body and emits the full chain instead of erroring as an
/// unsupported const item expression shape.
#[test]
fn inlined_method_chain_const_emits_concat_and_slice() {
    let source = source_for(
        r"
const smallNumbers = [0, 1].concat([2, 3, 4].slice(1));
export function numberCount(): number {
  const values = smallNumbers;
  return values.length;
}
",
    );

    assert_eq!(rust_items(&source, &["fn number_count"]), r"fn number_count() -> f64 {
let values: SmeltList<f64> = Into::<SmeltList<_>>::into(SMELT_GLOBAL_SMALL_NUMBERS_0.with(|smelt_value| smelt_value.borrow().clone()));
let _smelt_tmp_2: f64 = values.len() as f64;
return _smelt_tmp_2;
}");
}

/// A derived constructor's `super(...)` runs the base constructor against the derived
/// `this`: base fields are flattened into the derived struct and the base body is re-
/// emitted into the derived impl as `__smelt_init_Base`, never as a separate
/// `Base::new(..)` copied over.
#[test]
fn derived_constructor_super_call_runs_the_base_constructor() {
    let source = source_for(
        r"
class Base {
  message: string;
  constructor(message: string) {
    this.message = message;
  }
}
class Child extends Base {
  constructor(message: string) {
    super(message);
  }
}
const child = new Child('hello');
",
    );

    assert_eq!(rust_items(&source, &["impl Child {"]), r"impl Child {
fn new(message: JsString) -> Self {
let mut this: Self = Child { message: JsString::new() };
let _smelt_tmp_2: Self = Self::__smelt_init_Base(this, message.clone());
this = _smelt_tmp_2;
return this;
}
fn __smelt_init_Base(smelt_receiver: Self, message: JsString) -> Self {
let mut this: Self = smelt_receiver;
this.message = message.clone();
return this;
}
}");
}

/// `super(...)` chains through every inheritance level.
///
/// Each level's initializer calls its own base's initializer, and a class's impl
/// carries every ancestor initializer its chain reaches, so the base-most field
/// is written on the leaf instance itself.
#[test]
fn derived_constructor_super_call_composes_across_inheritance_levels() {
    let source = source_for(
        r"
class A {
  a: number;
  constructor() {
    this.a = 1;
  }
}
class B extends A {
  b: number;
  constructor() {
    super();
    this.b = 2;
  }
}
class C extends B {
  c: number;
  constructor() {
    super();
    this.c = 3;
  }
}
const value = new C();
",
    );

    assert_eq!(rust_items(&source, &["impl C {"]), r"impl C {
fn new() -> Self {
let mut this: Self = C { a: 0.0, b: 0.0, c: 0.0 };
let _smelt_tmp_1: Self = Self::__smelt_init_B(this);
this = _smelt_tmp_1;
this.c = 3.0;
return this;
}
fn __smelt_init_B(smelt_receiver: Self) -> Self {
let mut this: Self = smelt_receiver;
let _smelt_tmp_1: Self = Self::__smelt_init_A(this);
this = _smelt_tmp_1;
this.b = 2.0;
return this;
}
fn __smelt_init_A(smelt_receiver: Self) -> Self {
let mut this: Self = smelt_receiver;
this.a = 1.0;
return this;
}
}");
}

/// A closure the base constructor stores captures the DERIVED instance.
///
/// The base is a reference class (its constructor lets `this` escape), so the
/// derived class is too; `class_inheritance_behaviour` observes that the derived
/// constructor's later write is visible to the closure.
#[test]
fn base_constructor_closures_capture_the_derived_instance() {
    let source = source_for(
        r"
class Base {
  name: string = '';
  msg: string = 'x';
  greet: () => string;
  constructor() {
    this.greet = () => 'base:' + this.name + ':' + this.msg;
  }
}
class Smart extends Base {
  msg = 'hi';
  constructor() {
    super();
    this.name = 'smart';
  }
}
console.log(new Smart().greet());
",
    );

    assert_eq!(rust_items(&source, &["struct Smart("]), r"struct Smart(::std::rc::Rc<::std::cell::RefCell<SmartInner>>);");
}

/// An argument read through a reference class's cell is bound before a call
/// on a reference receiver, so its `Ref` guard cannot outlive into a callee
/// that writes the same cell ("RefCell already borrowed").
#[test]
fn a_ref_cell_argument_is_bound_before_a_reference_method_call() {
    let source = source_for(
        r"
class Tally {
  total: number = 1;
  bump(by: number): void {
    this.total = this.total + by;
  }
  twice(): number {
    this.bump(this.total);
    return this.total;
  }
}
console.log(new Tally().twice());
",
    );

    assert_eq!(rust_items(&source, &["fn twice"]), r"fn twice(&self) -> f64 {
let _ = { let smelt_call_arg_0 = self.0.borrow().total.clone(); self.bump(smelt_call_arg_0) };
return self.0.borrow().total.clone();
}");
}

/// A base of a reference class is a reference class too.
///
/// The base constructor body runs re-emitted over the derived receiver, and
/// that copy renders field access in the DECLARING class's representation: a
/// value base under a reference subclass would write `this.x = ..` on a handle.
#[test]
fn a_base_of_a_reference_class_is_a_reference_class() {
    let source = source_for(
        r"
class Plain {
  x: number;
  constructor() {
    this.x = 1;
  }
}
class Mutated extends Plain {
  bump(): void {
    this.x = this.x + 1;
  }
}
const m = new Mutated();
m.bump();
console.log(m.x);
",
    );

    assert_eq!(rust_items(&source, &["struct Plain(", "struct Mutated("]), r"struct Plain(::std::rc::Rc<::std::cell::RefCell<PlainInner>>);

struct Mutated(::std::rc::Rc<::std::cell::RefCell<MutatedInner>>);");
}

/// A class extending a host `Error` constructor gets the base constructor's
/// observable behaviour: `message` from the first argument and `name` from the
/// base constructor's own name.
///
/// The rule is keyed on the resolved base type, not on the subclass name, so
/// the same lowering covers `extends Error` and every standard error subclass.
#[test]
fn error_subclass_super_call_assigns_the_error_base_slots() {
    let source = source_for(
        r"
class ParseFailure extends TypeError {
  constructor(message: string) {
    super(message);
  }
}
const failure = new ParseFailure('bad input');
",
    );

    assert_eq!(rust_items(&source, &["impl ParseFailure {"]), r#"impl ParseFailure {
fn new(message: JsString) -> Self {
let mut this: Self = ParseFailure { name: JsString::new(), message: JsString::new(), stack: None::<JsString>, cause: SmeltUnknown::Null };
this.name = JsString::from("TypeError");
this.message = message.clone();
this.stack = Some(JsString::new());
return this;
}
}"#);
}

/// The inherited `Error` slots keep concrete types; only `cause` erases.
///
/// TypeScript's lib types are `name: string`, `message: string`,
/// `stack?: string`, `cause?: unknown`, and `tsc` rejects source that violates
/// them before Smelt runs, so three of the four slots are ordinary strings.
/// `cause` is the one genuine dynamic boundary: ES2022 types it `unknown`
/// because it carries an arbitrary thrown value.
#[test]
fn error_subclass_inherited_slots_stay_concretely_typed() {
    let source = source_for(
        r"
class ParseFailure extends TypeError {
  constructor(message: string) {
    super(message);
  }
}
const failure = new ParseFailure('bad input');
",
    );

    assert_eq!(rust_items(&source, &["struct ParseFailure"]), r"struct ParseFailure {
name: JsString,
message: JsString,
stack: Option<JsString>,
cause: SmeltUnknown,
}");
}

/// Constructor parameter defaults are applied in the constructor body, exactly
/// like plain function parameter defaults.
///
/// The ABI parameter widens to `Option<T>` and the body applies the declared
/// initializer, so an omitted argument evaluates the source default.
#[test]
fn constructor_parameter_default_is_applied_in_the_body() {
    let source = source_for(
        r"
class Greeting {
  text: string;
  constructor(text = 'hi there') {
    this.text = text;
  }
}
const greeting = new Greeting();
",
    );

    assert_eq!(rust_items(&source, &["impl Greeting {"]), r#"impl Greeting {
fn new(text: Option<JsString>) -> Self {
let mut this: Self = Greeting { text: JsString::new() };
let text_1: JsString = text.clone().unwrap_or(JsString::from("hi there"));
this.text = text_1;
return this;
}
}"#);
}

/// `(globalThis as any)[name]` is the erased read of a dynamic global: the key is
/// only known at runtime and the parameter is source `any`. What it resolves to is
/// observed in `class_inheritance_behaviour`.
#[test]
fn a_dynamic_global_object_read_resolves_a_modeled_builtin_constructor() {
    let source = source_for(
        "export function f(name: any): any { return (globalThis as any)[name]; }",
    );
    assert_eq!(rust_items(&source, &["fn f"]), r"fn f(name: SmeltUnknown) -> SmeltUnknown {
let _smelt_tmp_1: SmeltUnknown = smelt_global_object();
return match _smelt_tmp_1.clone() {
SmeltUnknown::String(smelt_value) => {
let smelt_len = smelt_value.len() as i64;
let smelt_index = match name.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN } as i64;
let smelt_normalized = if smelt_index < 0 { smelt_len + smelt_index } else { smelt_index };
usize::try_from(smelt_normalized).ok().and_then(|smelt_position| smelt_value.char_at(smelt_position).map(|smelt_ch| SmeltUnknown::String(smelt_ch.into()))).unwrap_or(SmeltUnknown::Undefined)
}
SmeltUnknown::Array(smelt_values) => {
let smelt_len = smelt_values.len() as i64;
let smelt_index = match name.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN } as i64;
let smelt_normalized = if smelt_index < 0 { smelt_len + smelt_index } else { smelt_index };
usize::try_from(smelt_normalized).ok().and_then(|smelt_position| smelt_values.get(smelt_position).cloned()).unwrap_or(SmeltUnknown::Undefined)
}
SmeltUnknown::Object(smelt_values) => smelt_get_object_field(&smelt_values, &JsString::from(smelt_property_key(name.clone()))),
_ => SmeltUnknown::Undefined,
};
}");
}

/// JavaScript exposes one object per global builtin name, so a bare host
/// constructor reference evaluates to the interned namespace value; the identity
/// itself is observed in `class_inheritance_behaviour`.
#[test]
fn a_bare_host_constructor_reference_is_interned() {
    let source = source_for("export function f(): any { return Blob; }");
    assert_eq!(rust_items(&source, &["fn f"]), r#"fn f() -> SmeltUnknown {
let _smelt_tmp_0: SmeltUnknown = smelt_builtin_namespace("Blob");
return _smelt_tmp_0;
}"#);
}

/// A subclass's `impl` block carries the methods it inherits.
///
/// Smelt flattens inheritance: a subclass struct stores its base's fields inline,
/// and because Rust has no method inheritance a base method is only callable on a
/// subclass receiver if it is emitted into the subclass's own `impl`.
#[test]
fn subclass_impl_block_carries_inherited_methods() {
    let source = source_for_py(
        r"
class A:
    def __init__(self, x: int) -> None:
        self.x = x
    def fetch(self) -> int:
        return self.x

class B(A):
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y
    def total(self) -> int:
        return self.fetch() + self.y
",
    );

    assert_eq!(rust_items(&source, &["impl B {"]), r"impl B {
fn new(x: i64, y: i64) -> Self {
let mut self_: B = B { x: 0, y: 0 };
self_.x = x;
self_.y = y;
return self_;
}
fn fetch(&self) -> i64 {
return self.x;
}
fn __smelt_super_fetch(&self) -> i64 {
return self.x;
}
fn total(&self) -> i64 {
let _smelt_tmp_2: i64;
let _smelt_tmp_1: i64 = self.fetch();
_smelt_tmp_2 = _smelt_tmp_1 + self.y;
return _smelt_tmp_2;
}
}");
}

/// An override replaces the inherited slot instead of emitting a second method
/// of the same name, which would not compile.
#[test]
fn subclass_override_replaces_the_inherited_method() {
    let source = source_for_py(
        r"
class A:
    def __init__(self, x: int) -> None:
        self.x = x
    def fetch(self) -> int:
        return self.x

class B(A):
    def fetch(self) -> int:
        return 99
",
    );

    assert_eq!(rust_items(&source, &["impl B {"]), r"impl B {
fn new(x: i64) -> Self {
let __smelt_super: A;
let mut self_: B = B { x: 0 };
let _smelt_tmp_3: A = A::new(x);
__smelt_super = _smelt_tmp_3;
self_.x = __smelt_super.x;
return self_;
}
fn fetch(&self) -> i64 {
return 99;
}
fn __smelt_super_fetch(&self) -> i64 {
return self.x;
}
}");
}

/// Python `super().method()` calls the immediate base implementation without
/// recursing into the derived override.
#[test]
fn python_super_method_uses_a_typed_base_alias() {
    let source = source_for_py(
        r"
class A:
    def greet(self, value: int) -> int:
        return value + 1

class B(A):
    def greet(self, value: int) -> int:
        return super().greet(value) + 10
",
    );

    assert_eq!(rust_items(&source, &["impl B {"]), r"impl B {
fn new() -> Self {
let self_: B = B {  };
return self_;
}
fn greet(&self, value: i64) -> i64 {
let _smelt_tmp_3: i64;
let _smelt_tmp_2: i64 = self.__smelt_super_greet(value);
_smelt_tmp_3 = _smelt_tmp_2 + 10;
return _smelt_tmp_3;
}
fn __smelt_super_greet(&self, value: i64) -> i64 {
let _smelt_tmp_2: i64 = value + 1;
return _smelt_tmp_2;
}
}");
}

/// A Python `super().__init__(..)` runs the base constructor and copies the
/// flattened base slots onto the derived instance, composing across levels.
#[test]
fn python_super_init_composes_across_inheritance_levels() {
    let source = source_for_py(
        r"
class A:
    def __init__(self, x: int) -> None:
        self.x = x

class B(A):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x)
        self.y = y

class C(B):
    def __init__(self) -> None:
        super().__init__(1, 2)
        self.z = 3
",
    );

    assert_eq!(rust_items(&source, &["impl C {"]), r"impl C {
fn new() -> Self {
let __smelt_super: B;
let mut self_: C = C { x: 0, y: 0, z: 0 };
let _smelt_tmp_2: B = B::new(1, 2);
__smelt_super = _smelt_tmp_2;
self_.x = __smelt_super.x;
self_.y = __smelt_super.y;
self_.z = 3;
return self_;
}
}");
}

/// A Python `@property` getter emits a Rust method, and a read through the
/// property's *field* syntax emits the call.
///
/// Python spells the definition as a method and the use as a field; Rust has no
/// properties, so the hand-written mapping is a plain method plus a call at the
/// use site. The property must not become a struct field of its own.
#[test]
fn python_property_emits_a_getter_method_and_call() {
    let source = source_for_py(
        r"
class Ok:
    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def ok_value(self) -> int:
        return self._value

def read(o: Ok) -> int:
    return o.ok_value
",
    );

    assert_eq!(rust_items(&source, &["struct Ok_", "fn ok_value", "fn read"]), r"struct Ok_ {
_value: i64,
}

fn ok_value(&self) -> i64 {
return self._value;
}

fn read(o: Ok_) -> i64 {
return o.ok_value();
}");
}

/// A callback stored into an INHERITED field of a returned subclass enters its
/// function owned, exactly like one stored into the class's own field: the escape
/// analysis follows a class into its effective layout, not only its own `fields`.
#[test]
fn typescript_callback_stored_into_an_inherited_field_is_owned() {
    let source = source_for(
        r"
class Handler {
  cb: (x: number) => number;
  constructor(cb: (x: number) => number) {
    this.cb = cb;
  }
}
class Sub extends Handler {
  tag: string = 'sub';
}
export function makeBase(cb: (x: number) => number): Handler {
  return new Handler(cb);
}
export function makeSub(cb: (x: number) => number): Sub {
  return new Sub(cb);
}
",
    );

    assert_eq!(rust_items(&source, &["sig make_base", "sig make_sub"]), r"fn make_base(cb: ::std::rc::Rc<dyn Fn(f64) -> f64>) -> Handler

fn make_sub(cb: ::std::rc::Rc<dyn Fn(f64) -> f64>) -> Sub");
}

/// A method body predeclares its function-scope locals.
///
/// MIR locals are function-scoped and generated Rust branch bodies are lexically
/// scoped, so a temporary first assigned inside one `if` arm is declared before
/// the branch for the sibling arm's assignment to name it.
#[test]
fn a_method_body_predeclares_locals_assigned_in_more_than_one_branch() {
    let source = source_for(
        r"
class Counter {
  total = 0;

  step(flag: boolean, amount: number): number {
    let next: number;
    if (flag) {
      next = this.total + amount;
    } else {
      next = this.total - amount;
    }
    this.total = next;
    return next;
  }
}
const counter = new Counter();
console.log(counter.step(true, 2));
",
    );

    assert_eq!(rust_items(&source, &["fn step"]), r"fn step(&self, flag: bool, amount: f64) -> f64 {
let _smelt_tmp_4: f64;
let _smelt_tmp_5: f64;
let mut next: f64 = 0.0;
if flag {
_smelt_tmp_4 = self.0.borrow().total.clone() + amount;
next = _smelt_tmp_4;
} else {
_smelt_tmp_5 = self.0.borrow().total.clone() - amount;
next = _smelt_tmp_5;
}
self.0.borrow_mut().total = next;
return next;
}");
}

/// A value asked for at `bool` is a truthiness test, not a cast: wherever
/// JavaScript expects a boolean it applies truthiness, and for a class instance
/// the answer is "present".
#[test]
fn a_class_valued_optional_in_a_boolean_position_tests_presence() {
    let source = source_for(
        r"
class Node {
  key = '';
}
export function has(nodes: Record<string, Node>, key: string): boolean {
  const found: Node | undefined = nodes[key];
  const present = !!found;
  return present;
}
console.log(has({}, 'a'));
",
    );

    assert_eq!(rust_items(&source, &["fn has"]), r"fn has(nodes: SmeltRecord<JsString, Node>, key: JsString) -> bool {
let found: Option<Node> = nodes.clone().get(&key.clone());
let _smelt_tmp_5: bool = !(found.is_some());
let present: bool = !(_smelt_tmp_5);
return present;
}");
}

/// A subclass of a reference class is itself a reference class.
///
/// A classification trigger fires on the class that DECLARES the mutated field,
/// so a subclass that only inherits mutated state is never named by one; it is
/// still the same kind of object, and inherited method bodies are written against
/// the base's handle representation.
#[test]
fn a_subclass_of_a_reference_class_is_a_reference_class() {
    let source = source_for(
        r"
class Store {
  entries: Record<string, number> = {};

  put(key: string, value: number): void {
    this.entries[key] = value;
  }
}

class CountingStore extends Store {
  reads = 0;

  total(): number {
    let sum = 0;
    for (const key of Object.keys(this.entries)) {
      sum += this.entries[key];
    }
    return sum;
  }
}

const store = new CountingStore();
store.put('a', 2);
console.log(store.total());
",
    );

    assert_eq!(rust_items(&source, &["struct Store(", "struct CountingStore(", "fn total"]), r#"struct Store(::std::rc::Rc<::std::cell::RefCell<StoreInner>>);

struct CountingStore(::std::rc::Rc<::std::cell::RefCell<CountingStoreInner>>);

fn total(&self) -> f64 {
let mut key: JsString;
let mut _smelt_tmp_5: usize;
let mut _smelt_tmp_6: bool;
let mut _smelt_tmp_7: SmeltRecord<JsString, f64>;
let mut _smelt_tmp_8: f64;
let mut sum: f64 = 0.0;
let _smelt_tmp_3: SmeltList<JsString> = Into::<SmeltList<_>>::into(self.0.borrow().entries.clone().keys().filter(|smelt_key| !smelt_key.starts_with("__smelt_symbol") && smelt_is_for_in_record_key(&self.0.borrow().entries.clone(), smelt_key)).collect::<Vec<_>>());
let mut _smelt_tmp_4: usize = 0;
loop {
_smelt_tmp_5 = _smelt_tmp_3.len();
_smelt_tmp_6 = _smelt_tmp_4 < _smelt_tmp_5;
if !(_smelt_tmp_6) { break; }
key = _smelt_tmp_3.borrow().get(_smelt_tmp_4).cloned().unwrap_or_else(|| JsString::new());
_smelt_tmp_7 = self.0.borrow().entries.clone();
_smelt_tmp_8 = sum + _smelt_tmp_7.get(&key.clone()).unwrap_or(0.0);
sum = _smelt_tmp_8;
_smelt_tmp_4 = _smelt_tmp_4 + 1;
}
return sum;
}"#);
}

/// A method VALUE read off a reference class captures the handle: the class has
/// exactly one field, the `Rc<RefCell<..>>`, so cloning it is the only capture
/// that can be spelled and the only one whose mutations are seen.
#[test]
fn a_method_value_of_a_reference_class_captures_the_handle() {
    let source = source_for(
        r"
type Handler = (key: string) => number;

class Counter {
  hits: Record<string, number> = {};

  bump(key: string): number {
    this.hits[key] = 1;
    return 1;
  }

  handler(): Handler {
    return this.bump;
  }
}

const counter = new Counter();
const bump = counter.handler();
console.log(bump('a'));
",
    );

    assert_eq!(rust_items(&source, &["struct Counter(", "fn handler"]), r#"struct Counter(::std::rc::Rc<::std::cell::RefCell<CounterInner>>);

fn handler(&self) -> ::std::rc::Rc<dyn Fn(JsString) -> f64> {
return { let smelt_source_value = (({ let smelt_receiver = self.clone(); let smelt_method: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Number(smelt_receiver.bump(match smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null).clone() { SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other { SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }) }) as f64))); smelt_link_function_identity_key(&smelt_method, smelt_method_identity("Counter::bump")); SmeltUnknown::Function(smelt_method) }).clone()); let smelt_function = match smelt_source_value.clone() { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None }; if let Some(smelt_function) = smelt_function { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function); let smelt_callback: ::std::rc::Rc<dyn Fn(JsString) -> f64> = if let Some(smelt_original) = smelt_restore_function_origin::<::std::rc::Rc<dyn Fn(JsString) -> f64>>(&smelt_function) { if matches!(smelt_source_value, SmeltUnknown::Object(_)) { ::std::rc::Rc::new(move |smelt_arg0: JsString| (smelt_original)(smelt_arg0)) } else { smelt_original } } else { ::std::rc::Rc::new(move |smelt_arg0: JsString| -> f64 { let smelt_result = (smelt_function)({ let mut smelt_call_args = Vec::new(); smelt_call_args.push(SmeltUnknown::String(smelt_arg0.into())); smelt_call_args }).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)); match smelt_result.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN } }) }; smelt_register_callable_object(&smelt_callback, smelt_source_value); smelt_link_function_identity_key(&smelt_callback, smelt_origin_identity); smelt_callback } else { { let smelt_default_callback: ::std::rc::Rc<dyn Fn(JsString) -> f64> = ::std::rc::Rc::new(move |smelt_arg0: JsString| -> f64 { smelt_value_not_callable() }); smelt_default_callback } } };
}"#);
}

/// A getter that can throw is emitted returning `Result`, so reading the
/// property is a fallible operation and never hands its consumer the `Result`.
///
/// WHICH fallible form is emitted depends on the enclosing item, because `?`
/// only travels where the enclosing function returns `Result`: a reader that is
/// itself fallible propagates with `?`, and one that is not takes the panic
/// path, the floor every non-fallible callback seam uses
/// (`blocker-logs/hono-fallible-ops.md`). The program stops at the throw rather
/// than continuing with a fabricated value.
#[test]
fn a_throwing_getter_read_is_fallible_in_both_enclosures() {
    let source = source_for(
        r"
export class Holder {
  tag: unknown = 0;
  private inner: string = 'a';
  private get active(): string {
    if (this.inner === '') {
      throw new Error('empty');
    }
    return this.inner;
  }
  spread(): unknown {
    return { ...this.active };
  }
  rethrow(): string {
    if (this.inner === 'x') {
      throw new Error('x');
    }
    return this.active;
  }
}
",
    );

    assert_eq!(rust_items(&source, &["fn __smelt_get_active", "fn spread", "fn rethrow"]), r#"fn __smelt_get_active(&self) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_1: bool = self.inner == "";
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::Object(SmeltObject::from_unknown_record((SmeltRecord::from([(JsString::from("__smelt_error"), SmeltUnknown::String("Error".into())), (JsString::from("message"), SmeltUnknown::String("empty".into())), (JsString::from("stack"), SmeltUnknown::Undefined), (JsString::from("cause"), SmeltUnknown::Undefined)])).clone()))));
} else {
return Ok(self.inner.clone());
}
}

fn spread(&self) -> SmeltUnknown {
let _smelt_tmp_1: SmeltRecord<JsString, SmeltUnknown> = SmeltRecord::from([]);
let _smelt_tmp_2: SmeltRecord<JsString, SmeltUnknown> = { _smelt_tmp_1.extend(match IntoSmeltUnknown::into_smelt_unknown(self.__smelt_get_active().unwrap_or_else(|error| smelt_panic_throw(error)).clone().clone()) { SmeltUnknown::Object(smelt_map) => SmeltRecord::with_id_from_entries(smelt_map.id, smelt_map.into_iter().map(|(smelt_key, smelt_value)| (smelt_key, smelt_value))), _ => SmeltRecord::new() }.iter().map(|(smelt_key, smelt_value)| (smelt_key.clone(), smelt_value.clone()))); _smelt_tmp_1.clone() };
return SmeltUnknown::Object(SmeltObject::from_unknown_record(_smelt_tmp_2.clone()));
}

fn rethrow(&self) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_1: bool = self.inner == "x";
if _smelt_tmp_1 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::Object(SmeltObject::from_unknown_record((SmeltRecord::from([(JsString::from("__smelt_error"), SmeltUnknown::String("Error".into())), (JsString::from("message"), SmeltUnknown::String("x".into())), (JsString::from("stack"), SmeltUnknown::Undefined), (JsString::from("cause"), SmeltUnknown::Undefined)])).clone()))));
} else {
return Ok((self.__smelt_get_active()?).clone());
}
}"#);
}

/// A method the class reassigns through `this` becomes a function-typed field;
/// a sibling method that is never assigned stays an inherent method.
///
/// A Rust inherent method cannot be assigned, so the reassigned one is a field
/// of the method's own signature initialised to the method's body. The scoping
/// half of the rule is the point of pinning the sibling: turning every method
/// into an `Rc<dyn Fn>` field would trade a static call for a dynamic one.
#[test]
fn a_method_reassigned_through_this_becomes_a_callable_field() {
    let source = source_for(
        r"
export class Picker {
  tag: unknown = 0;
  pick(value: string): string { return value; }
  other(value: string): string { return value + '?'; }
  run(value: string): string {
    this.pick = (v: string) => v + '!';
    return this.pick(value) + this.other(value);
  }
}
",
    );

    assert_eq!(rust_items(&source, &["struct PickerInner", "impl Picker {"]), r#"struct PickerInner {
tag: SmeltUnknown,
pick: ::std::rc::Rc<dyn Fn(JsString) -> JsString>,
}

impl Picker {
fn new() -> Self {
let mut this: Self = Picker(::std::rc::Rc::new(::std::cell::RefCell::new(PickerInner { tag: SmeltUnknown::Null, pick: { let smelt_default_callback: ::std::rc::Rc<dyn Fn(JsString) -> JsString> = ::std::rc::Rc::new(move |smelt_arg0: JsString| -> JsString { JsString::new() }); smelt_default_callback } })));
this.0.borrow_mut().tag = SmeltUnknown::Number(0.0 as f64);
let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: JsString| {
closure_arg_0.clone()
});
this.0.borrow_mut().pick = _smelt_tmp_1.clone();
return this;
}
fn other(&self, value: JsString) -> JsString {
let _smelt_tmp_2: JsString = value.clone() + &JsString::from("?");
return _smelt_tmp_2;
}
fn run(&self, value: JsString) -> Result<JsString, Box<dyn std::error::Error>> {
let _smelt_tmp_5: JsString;
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: JsString| {
let _smelt_tmp_1: JsString = closure_arg_0.clone() + &JsString::from("!");
_smelt_tmp_1.clone()
});
self.0.borrow_mut().pick = _smelt_tmp_2.clone();
let _smelt_tmp_3: JsString = { let smelt_callable = ::std::clone::Clone::clone(&self.0.borrow().pick.clone()); (smelt_callable)(value.clone()) };
let _smelt_tmp_4: JsString = self.other(value.clone());
_smelt_tmp_5 = _smelt_tmp_3 + &_smelt_tmp_4;
return Ok(_smelt_tmp_5);
}
/// Prototype-carried members of this class, as receiver-bound erased functions.
#[allow(dead_code)]
fn __smelt_proto_entries(&self) -> Vec<(String, SmeltUnknown)> {
let mut smelt_proto_entries: Vec<(String, SmeltUnknown)> = Vec::new();
smelt_proto_entries.push(("__smelt_method:other".to_owned(), { let smelt_method: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new({ let smelt_receiver = self.clone(); move |smelt_args: Vec<SmeltUnknown>| { let _ = &smelt_args; let smelt_result = smelt_receiver.other(SmeltFromUnknown::smelt_from_unknown(smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Undefined))); Ok(SmeltUnknown::String(smelt_result.into())) } }); smelt_link_function_identity_key(&smelt_method, smelt_method_identity("Picker::other")); SmeltUnknown::Function(smelt_method) }));
smelt_proto_entries.push(("__smelt_method:run".to_owned(), { let smelt_method: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new({ let smelt_receiver = self.clone(); move |smelt_args: Vec<SmeltUnknown>| { let _ = &smelt_args; let smelt_result = smelt_receiver.run(SmeltFromUnknown::smelt_from_unknown(smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Undefined)))?; Ok(SmeltUnknown::String(smelt_result.into())) } }); smelt_link_function_identity_key(&smelt_method, smelt_method_identity("Picker::run")); SmeltUnknown::Function(smelt_method) }));
smelt_proto_entries
}
}"#);
}

/// `ConstructorParameters<C>` is the constructor position of `Parameters<F>`.
///
/// Both name the tuple of parameter types a callable is invoked with, and
/// `typeof C` for a class resolves to that class's constructor function type, so
/// the two spellings read the same field off the same `Type::Function`.
#[test]
fn constructor_parameters_lowers_like_parameters() {
    let source = source_for(
        r"
class Point {
  constructor(
    public x: number,
    public y: number,
  ) {}
}

type Make = (...args: ConstructorParameters<typeof Point>) => Point;

const make: Make = (x, y) => new Point(x, y);
console.log(make(1, 2).x, make(3, 4).y);
",
    );

    assert!(source.contains("Point::new("), "{source}");
}

/// The same annotation over a HOST constructor with no modeled signature lowers
/// to an erased array, the honest "some arguments, shape unknown" answer: it is
/// a parameter type of a host constructor Smelt has no signature for.
#[test]
fn constructor_parameters_of_an_unmodeled_host_constructor_lowers() {
    let source = source_for(
        r"
type ClientRequestOptions = {
  webSocket?: (...args: ConstructorParameters<typeof WebSocket>) => WebSocket;
};

const options: ClientRequestOptions = {};
console.log(options.webSocket === undefined);
",
    );

    assert_eq!(rust_items(&source, &["struct ClientRequestOptions"]), r"struct ClientRequestOptions {
web_socket: Option<SmeltErasedFunction>,
}");
}

/// A directly constructed subclass binds its virtual method slots to its own
/// most-derived methods through a `Weak` handle on the new instance, so
/// `Shape::describe`'s `this.name()` (inherited into `Square`) reaches
/// `Square::name`.
#[test]
fn constructed_instance_binds_its_virtual_slots_to_its_own_methods() {
    let source = source_for(
        r"
class Shape {
  describe(): string { return `${this.name()} ${this.sides()}`; }
  name(): string { return 'shape'; }
  sides(): number { return 0; }
}
class Square extends Shape {
  name(): string { return 'square'; }
  sides(): number { return 4; }
}
export function show(): string { return new Square().describe(); }
",
    );

    assert_eq!(rust_items(&source, &["impl Square {"]), r#"impl Square {
fn new() -> Self {
let mut this: Self = { let smelt_instance = Square(::std::rc::Rc::new(::std::cell::RefCell::new(SquareInner { name: { let smelt_default_callback: ::std::rc::Rc<dyn Fn() -> JsString> = ::std::rc::Rc::new(move || -> JsString { JsString::new() }); smelt_default_callback }, sides: { let smelt_default_callback: ::std::rc::Rc<dyn Fn() -> f64> = ::std::rc::Rc::new(move || -> f64 { 0.0 }); smelt_default_callback } }))); smelt_instance.0.borrow_mut().name = { let smelt_virtual_receiver = ::std::rc::Rc::downgrade(&smelt_instance.0); let smelt_virtual_method: ::std::rc::Rc<dyn Fn() -> JsString> = ::std::rc::Rc::new(move || -> JsString { let smelt_method_receiver = Self(smelt_virtual_receiver.upgrade().expect("a virtual method slot outlived its instance")); smelt_method_receiver.name() }); smelt_virtual_method }; smelt_instance.0.borrow_mut().sides = { let smelt_virtual_receiver = ::std::rc::Rc::downgrade(&smelt_instance.0); let smelt_virtual_method: ::std::rc::Rc<dyn Fn() -> f64> = ::std::rc::Rc::new(move || -> f64 { let smelt_method_receiver = Self(smelt_virtual_receiver.upgrade().expect("a virtual method slot outlived its instance")); smelt_method_receiver.sides() }); smelt_virtual_method }; smelt_instance };
let _smelt_tmp_1: Self = Self::__smelt_init_Shape(this);
this = _smelt_tmp_1;
return this;
}
fn __smelt_init_Shape(smelt_receiver: Self) -> Self {
let this: Self = smelt_receiver;
return this;
}
fn describe(&self) -> JsString {
let _smelt_tmp_2: JsString;
let _smelt_tmp_3: JsString;
let _smelt_tmp_5: JsString;
let _smelt_tmp_1: JsString = { let smelt_callable = self.clone().0.borrow().name.clone(); (smelt_callable)() };
_smelt_tmp_2 = JsString::new() + &_smelt_tmp_1;
_smelt_tmp_3 = _smelt_tmp_2 + &JsString::from(" ");
let _smelt_tmp_4: f64 = { let smelt_callable = self.clone().0.borrow().sides.clone(); (smelt_callable)() };
_smelt_tmp_5 = _smelt_tmp_3 + &JsString::from(smelt_number_to_string(_smelt_tmp_4));
return _smelt_tmp_5;
}
fn name(&self) -> JsString {
return JsString::from("square");
}
fn sides(&self) -> f64 {
return 4.0;
}
}"#);
}
