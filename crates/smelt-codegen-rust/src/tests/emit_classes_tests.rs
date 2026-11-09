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

    let program = program_body(&source);
    assert!(program.contains(".chain("), "the inlined concat emits list concatenation:\n{program}");
    assert!(program.contains(".skip("), "the inlined slice emits a skip-based slice:\n{program}");
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

    assert!(
        source.contains("Self::__smelt_init_Base(this, message"),
        "{source}"
    );
    assert!(
        source.contains("fn __smelt_init_Base(smelt_receiver: Self, message: JsString) -> Self"),
        "{source}"
    );
    assert!(!source.contains("__smelt_super"), "no base value is built: {source}");
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

    let impl_c = source
        .split("impl C {")
        .nth(1)
        .and_then(|rest| rest.split("\nimpl ").next())
        .unwrap_or_default();
    assert!(impl_c.contains("fn __smelt_init_B(smelt_receiver: Self) -> Self"), "{source}");
    assert!(impl_c.contains("fn __smelt_init_A(smelt_receiver: Self) -> Self"), "{source}");
    // B's initializer, re-emitted over `C`, runs A's over the same receiver.
    assert!(impl_c.contains("Self::__smelt_init_A(this)"), "{source}");
    assert!(!source.contains("__smelt_super"), "{source}");
}

/// A closure the base constructor stores captures the DERIVED instance.
///
/// The base is a reference class (its constructor lets `this` escape), so the
/// derived class is too, and the receiver handed through the initializer is
/// the derived handle itself: the derived constructor's later write is visible
/// to the closure. The old lowering built a separate base object, so the
/// closure read the base object's `name` (Node prints `base:smart:hi`, Smelt
/// printed `base::x`).
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

    assert!(
        source.contains("struct Smart(::std::rc::Rc<::std::cell::RefCell<SmartInner>>)"),
        "{source}"
    );
    let impl_smart = source
        .split("impl Smart {")
        .nth(1)
        .and_then(|rest| rest.split("\nimpl ").next())
        .unwrap_or_default();
    let init_call = impl_smart.find("Self::__smelt_init_Base(this)");
    let own_field = impl_smart.find("this.0.borrow_mut().msg = JsString::from(\"hi\")");
    // The derived field initializer runs AFTER the base initialization, so the
    // redeclared `msg` is not overwritten by the base's `'x'`.
    assert!(
        init_call.is_some() && own_field.is_some() && init_call < own_field,
        "{source}"
    );
    assert!(impl_smart.contains("let mut this: Self = smelt_receiver;"), "{source}");
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

    assert!(
        source.contains("let smelt_call_arg_0 = self.0.borrow().total.clone(); self.bump(smelt_call_arg_0)"),
        "{source}"
    );
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

    assert!(
        source.contains("struct Plain(::std::rc::Rc<::std::cell::RefCell<PlainInner>>)"),
        "{source}"
    );
    assert!(
        source.contains("struct Mutated(::std::rc::Rc<::std::cell::RefCell<MutatedInner>>)"),
        "{source}"
    );
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

    assert!(
        source.contains(r#"this.name = JsString::from("TypeError")"#),
        "{source}"
    );
    assert!(source.contains("this.message = message"), "{source}");
}

/// The inherited `Error` slots keep concrete types; only `cause` erases.
///
/// TypeScript's lib types are `name: string`, `message: string`,
/// `stack?: string`, `cause?: unknown`, and `tsc` rejects source that violates
/// them before Smelt runs, so three of the four slots are ordinary strings
/// rather than dynamic boundaries. This guards against a future change
/// "consistently" re-erasing them to `SmeltUnknown`.
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

    assert!(source.contains("name: JsString,"), "{source}");
    assert!(source.contains("message: JsString,"), "{source}");
    assert!(source.contains("stack: Option<JsString>,"), "{source}");
    // `cause` is the one genuine dynamic boundary: ES2022 types it `unknown`
    // because it carries an arbitrary thrown value.
    assert!(source.contains("cause: SmeltUnknown,"), "{source}");
    assert!(
        !source.contains("message: SmeltUnknown,") && !source.contains("name: SmeltUnknown,"),
        "{source}"
    );
    // A concrete `String` slot must still erase to `SmeltUnknown::String` when the
    // instance is erased, because the throw path reads `message` off the erased
    // object and expects that variant.
    assert!(
        source.contains(r#"("message".to_owned(), SmeltUnknown::String(self.message.into()))"#),
        "{source}"
    );
}

/// Constructor parameter defaults are applied in the constructor body, exactly
/// like plain function parameter defaults.
///
/// The ABI parameter widens to `Option<T>` and the body applies the declared
/// initializer, so an omitted argument evaluates the source default instead of
/// the Rust type's zero value (`String::new()` before the fix).
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

    assert!(source.contains("fn new(text: Option<JsString>)"), "{source}");
    assert!(source.contains(r#"unwrap_or(JsString::from("hi there"))"#), "{source}");
    assert!(source.contains("Greeting::new(None"), "{source}");
}

#[test]
fn a_dynamic_global_object_read_resolves_a_modeled_builtin_constructor() {
    // `globalThis.Error` normalizes to the modeled constructor at lowering time,
    // but the SAME read spelled with a runtime key used to fold to a constant
    // `undefined`, so `new (globalThis[type])(msg)` fabricated a null-returning
    // closure call and every error it "constructed" compared equal to every
    // other. Both spellings now resolve against one registry: the read reaches
    // the global-object marker record, and the record answers a modeled
    // constructor name with the interned builtin namespace value.
    let source = source_for(
        "export function f(name: any): any { return (globalThis as any)[name]; }",
    );
    assert!(
        source.contains("__smelt_global_object"),
        "a dynamic global read must reach the global-object value:\n{source}"
    );
    assert!(
        source.contains(
            "if map.contains_key(\"__smelt_global_object\") && !map.contains_key(field) && smelt_builtin_construct_kind(field).is_some()"
        ),
        "the global object must resolve a modeled builtin constructor by name:\n{source}"
    );
    // A name this profile models no constructor for stays genuinely absent
    // rather than becoming a fabricated empty namespace record.
    assert!(
        source.contains("smelt_builtin_namespace(field)"),
        "the resolved value must be the interned namespace value:\n{source}"
    );
}

#[test]
fn a_bare_host_constructor_reference_is_interned() {
    // JavaScript exposes one object per global builtin name, so `Blob === Blob` and
    // `blob.constructor === Blob` both hold. A record literal mints a fresh identity
    // on construction, so building a `__smelt_builtin_namespace` record per
    // reference made both comparisons false. The interning helper is also what a
    // host record's `.constructor` resolves through, so the two spellings meet.
    let generated = source_for("export function f(): any { return Blob; }");
    assert!(
        generated.contains("smelt_builtin_namespace(\"Blob\")"),
        "a bare host-constructor reference must intern:\n{generated}"
    );
    let field = source_for("export function f(value: any): any { return value.constructor; }");
    assert!(
        field.contains("smelt_marker_constructor_class(map)"),
        "an erased `.constructor` read must resolve a marker record's own global:\n{field}"
    );
}

/// A subclass's `impl` block carries the methods it inherits.
///
/// Smelt flattens inheritance — a subclass struct stores its base's fields
/// inline — but Rust has no method inheritance, so a base method is only
/// callable on a subclass receiver if it is emitted into the subclass's own
/// `impl`. Without that, a body calling an inherited method lowered fine in the
/// frontend and then failed to compile with `no method named 'fetch' found for
/// reference '&B'`.
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

    let impl_b = source
        .split("impl B {")
        .nth(1)
        .unwrap_or_else(|| panic!("expected an `impl B` block:\n{source}"));
    assert!(
        impl_b.contains("fn fetch(&self)"),
        "`B` must carry the inherited `fetch`, or `self.fetch()` cannot \
         compile:\n{source}"
    );
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

    let impl_b = source
        .split("impl B {")
        .nth(1)
        .unwrap_or_else(|| panic!("expected an `impl B` block:\n{source}"));
    assert_eq!(
        impl_b.matches("fn fetch(&self)").count(),
        1,
        "the override must replace the inherited slot, not duplicate it:\n{source}"
    );
    assert!(
        impl_b.contains("return 99"),
        "the surviving `fetch` must be the override:\n{source}"
    );
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

    let impl_b = source
        .split("impl B {")
        .nth(1)
        .unwrap_or_else(|| panic!("expected an `impl B` block:\n{source}"));
    assert!(
        impl_b.contains("fn __smelt_super_greet(&self, value: i64) -> i64"),
        "the base implementation must be available under a typed alias:\n{source}"
    );
    assert!(
        impl_b.contains("self.__smelt_super_greet(value)"),
        "super().greet(value) must call the base alias, not the override:\n{source}"
    );
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

    assert!(
        source.contains("let __smelt_super: B"),
        "`C` must construct its immediate base `B`:\n{source}"
    );
    // `C::new` copies BOTH inherited slots out of the constructed `B`, so the
    // base-most field reaches the leaf instance.
    assert!(
        source.contains("self_.x = __smelt_super.x") && source.contains("self_.y = __smelt_super.y"),
        "both inherited slots must reach the leaf instance:\n{source}"
    );
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

    assert!(
        source.contains("fn ok_value(&self) -> i64"),
        "the property getter must emit as a method:\n{source}"
    );
    assert!(
        source.contains("o.ok_value()"),
        "a property read must emit the getter call, not a field access:\n{source}"
    );
    let struct_body = source
        // `Ok` is renamed `Ok_` so it does not shadow Rust's `Ok`.
        .split("struct Ok_ {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or_else(|| panic!("expected a generated `struct Ok_`:\n{source}"));
    assert!(
        !struct_body.contains("ok_value"),
        "a property is computed, so it must not also become a struct field:\n{source}"
    );
}

/// A callback stored into an INHERITED field of a returned subclass must enter
/// its function owned.
///
/// Regression (haiku audit F0185): the escape analysis follows a class into its
/// fields, but read the class's own `fields` rather than its effective layout,
/// so `Sub extends Handler {}` was storage with no function in it. `cb` stayed
/// `&dyn Fn` and the `Rc` capture failed borrowck ("`'1` must outlive
/// `'static`"). The base-class return beside it was already owned.
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

    for name in ["make_base", "make_sub"] {
        let signature = source
            .split(&format!("fn {name}("))
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .unwrap_or_else(|| panic!("expected a generated `{name}`:\n{source}"));
        assert!(
            !signature.contains("&dyn Fn"),
            "`{name}`: a callback retained by the returned class must not stay borrowed:\n{source}"
        );
    }
}

/// A method body predeclares its function-scope locals (H22).
///
/// MIR locals are function-scoped; generated Rust branch bodies are lexically
/// scoped. A free function body has always predeclared the locals first assigned
/// outside its entry block, but a METHOD body did not, so a temporary first
/// assigned inside one `if` arm was declared *inside* that arm and the sibling
/// arm's assignment named something out of scope — E0425 in the generated crate,
/// 19053 of them in Hono's routers, with no diagnostic before rustc.
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

    let body = emitted_function_body(&source, "fn step(");
    let branch_start = body
        .find("if flag {")
        .expect("the branch is emitted: {body}");
    let (prelude, branches) = body.split_at(branch_start);
    assert!(
        prelude.contains("let _smelt_tmp_4: f64;"),
        "the branch temporary is declared before the branch: {body}"
    );
    assert!(
        !branches
            .lines()
            .any(|line| line.trim_start().starts_with("let _smelt_tmp_")),
        "no temporary is introduced inside a branch arm: {body}"
    );
    assert!(
        branches.contains("_smelt_tmp_4 =") && branches.contains("_smelt_tmp_5 ="),
        "both arms assign their predeclared temporary: {body}"
    );
}

/// A value asked for at `bool` is a truthiness test, not a cast (H24).
///
/// Wherever JavaScript expects a boolean it applies truthiness, and for a class
/// instance the answer is "present". The coercion used to hand the value back
/// unchanged, so an `Option<Node>` in a boolean position emitted
/// `.map_or(false, |value| value)` — E0308, 48 sites in Hono's trie router.
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

    let body = emitted_function_body(&source, "fn has(");
    assert!(
        !body.contains("|value| value)"),
        "the class value is not handed back where a bool is expected: {body}"
    );
    assert!(
        body.contains("is_some()"),
        "presence is the truthiness of a class-valued optional: {body}"
    );
}

/// H30a: a subclass of a reference class is itself a reference class.
///
/// A classification trigger fires on the class that DECLARES the mutated field,
/// so a subclass that only inherits mutated state is never named by one. It is
/// still the same kind of object, and Smelt re-emits every inherited method body
/// into the subclass's own `impl` -- bodies written against the base's handle
/// representation. Hono's `class<T> extends RegExpRouter<T>` was emitted as a
/// value struct carrying inherited bodies that read `self.0.borrow()._tries`:
/// `no field 0 on __smelt_anon_class_3050<T>` (E0609 x9 on the router slice).
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

    // The base really is lifted, so the subclass assertion is about propagation
    // and not about a shape that was never a handle.
    assert!(
        source.contains("struct Store(::std::rc::Rc<::std::cell::RefCell<StoreInner>>)"),
        "the base class is not a reference class here, so nothing propagates:\n{source}"
    );
    assert!(
        source.contains(
            "struct CountingStore(::std::rc::Rc<::std::cell::RefCell<CountingStoreInner>>)"
        ),
        "a subclass of a reference class was emitted by value:\n{source}"
    );
    // And the inherited state is reached through the handle in the subclass's
    // own methods, which is what failed to compile before.
    assert!(
        source.contains("self.0.borrow().entries"),
        "the inherited field is not read through the subclass's handle:\n{source}"
    );
}

/// H30b: a method VALUE read off a reference class captures the handle.
///
/// Reading a method as a value (`this.match`, passed on as a callback) captures
/// a receiver. For a value class that capture re-instantiates the struct field
/// by field; a reference class has exactly one field -- the `Rc<RefCell<..>>` --
/// so the rebuild could not be spelled at all (`struct PatternRouter<SmeltUnknown>
/// has no field named _routes`, E0560/E0609). Cloning the handle is also the only
/// capture with the right meaning: a method read off an object is bound to THAT
/// object, and a rebuild would hand the closure a copy whose mutations no one
/// ever sees.
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

    assert!(
        source.contains("struct Counter(::std::rc::Rc<::std::cell::RefCell<CounterInner>>)"),
        "the class under test is not a reference class:\n{source}"
    );
    assert!(
        source.contains("let smelt_receiver = self.clone();"),
        "the method value did not capture the handle:\n{source}"
    );
    assert!(
        !source.contains("let smelt_receiver = Counter {"),
        "the method value still rebuilds the struct field by field:\n{source}"
    );
}

/// A getter that can throw is emitted returning `Result`, so reading the
/// property is a fallible operation and never hands its consumer the `Result`
/// itself. The first thing done to a read value is usually `.clone()`, and
/// `Box<dyn Error>` is not `Clone`, so that surfaced as `E0599` on the `Result`
/// rather than as a type mismatch naming the property.
///
/// WHICH fallible form is emitted depends on the enclosing item, because `?`
/// only travels where the enclosing function returns `Result`:
///
/// * a reader that is itself fallible propagates with `?`;
/// * a reader that is NOT — a plain method, or a closure whose contextual
///   callback type is a plain `Fn(..) -> T` — takes the panic path, the same
///   floor every other non-fallible callback seam uses
///   (`blocker-logs/hono-fallible-ops.md`). Emitting `?` there was `E0277`
///   ("the `?` operator can only be used in a closure that returns `Result`"),
///   which is what three of Hono's closures hit.
///
/// The throw is not dropped in the second case: the program stops at it rather
/// than continuing with a fabricated value. Making the READER fallible instead,
/// so the `?` form applies more often, is a MIR throwing-propagation change —
/// that pass keys off calls, and a property read is not one — and is recorded
/// in `blocker-logs/hono-round30-emitter.md`.
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

    // The enabling condition: the getter really is emitted as fallible. If this
    // stops holding the assertions below would pass for the wrong reason.
    assert!(
        source
            .contains("fn __smelt_get_active(&self) -> Result<JsString, Box<dyn std::error::Error>>"),
        "a throwing getter must be emitted returning Result: {source}"
    );
    // A fallible reader propagates.
    assert!(
        source.contains("self.__smelt_get_active()?"),
        "a fallible reader must propagate with `?`: {source}"
    );
    // A non-fallible reader takes the panic path instead of an invalid `?`.
    assert!(
        source.contains(
            "self.__smelt_get_active().unwrap_or_else(|error| smelt_panic_throw(error))"
        ),
        "a non-fallible reader must take the panic path: {source}"
    );
    assert!(
        !source.contains("self.__smelt_get_active().clone()"),
        "a throwing getter read must not clone the Result: {source}"
    );
}

/// A method the class reassigns through `this` becomes a function-typed field;
/// a sibling method that is never assigned stays an inherent method.
///
/// JavaScript lets an instance replace its own method, and Hono's
/// `SmartRouter` does it to skip its router-selection loop after the first
/// request (`smart-router/router.ts`). A Rust inherent method cannot be
/// assigned, so the write emitted `self.match_ = ...` against a method and
/// failed with `E0615`.
///
/// The scoping half of the rule is the point of the sibling assertion: this
/// must not turn every method into an `Rc<dyn Fn>` field, because that would
/// trade a static call for a dynamic one everywhere and is the opposite of
/// carrying a concrete type down to runtime.
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

    // The reassigned method is a field of the method's own signature ...
    assert!(
        source.contains("pick: ::std::rc::Rc<dyn Fn(JsString) -> JsString>"),
        "a reassigned method must become a function-typed field: {source}"
    );
    // ... initialised to the method's own body, so a class that never assigns
    // still behaves as declared ...
    assert!(
        source.contains("this.0.borrow_mut().pick = "),
        "the field must be initialised in the constructor: {source}"
    );
    // ... and the assignment writes that field rather than a method.
    assert!(
        source.contains("self.0.borrow_mut().pick = "),
        "the reassignment must write the field: {source}"
    );

    // The untouched sibling keeps its inherent method and its STATIC call.
    assert!(
        source.contains("fn other(&self, value: JsString) -> JsString"),
        "an unassigned sibling must stay an inherent method: {source}"
    );
    assert!(
        source.contains("self.other(value.clone())"),
        "an unassigned sibling must keep a static call: {source}"
    );
    assert!(
        !source.contains("other: ::std::rc::Rc<dyn Fn"),
        "an unassigned sibling must NOT become a field: {source}"
    );
}

/// `ConstructorParameters<C>` is the constructor position of `Parameters<F>`.
///
/// Both name the tuple of parameter types a callable is invoked with, and
/// `typeof C` for a class already resolves to that class's CONSTRUCTOR function
/// type, so the two spellings read the same field off the same
/// `Type::Function`. Before this was modeled the name fell through to the
/// ordinary type-reference path and produced a type that is neither a list, a
/// tuple, nor erased — so a REST PARAMETER annotated with it blocked the whole
/// build with `rest parameter type must resolve to an array type`, and the
/// crate was never emitted. Hono's `src/client/types.ts` is exactly that shape.
///
/// A host constructor with no modeled signature keeps the honest "some
/// arguments, shape unknown" answer of an erased array, which is what makes the
/// annotation lower rather than block.
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

/// The same annotation over a HOST constructor with no modeled signature must
/// still lower, because that is the shape that actually blocked a real crate:
/// the answer degrades to an erased array rather than to a type a rest
/// parameter cannot be spelled with.
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

    assert!(source.contains("fn main"), "{source}");
}

#[test]
/// A directly constructed subclass binds its virtual method slots to its own
/// most-derived methods: `Square::new` binds `name` and `sides` through a
/// `Weak` handle on the new instance, so `Shape::describe`'s `this.name()`
/// (inherited into `Square`) reaches `Square::name`. Before, the slots kept
/// their default closures and `new Square().describe()` read `""`/`0`.
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
    let square_new = source
        .split("impl Square {")
        .nth(1)
        .and_then(|rest| rest.split("fn new(").nth(1))
        .unwrap_or_default();
    assert!(
        square_new.contains("::std::rc::Rc::downgrade(&smelt_instance.0)")
            && square_new.contains("smelt_instance.0.borrow_mut().name =")
            && square_new.contains("smelt_instance.0.borrow_mut().sides ="),
        "{source}"
    );
}
