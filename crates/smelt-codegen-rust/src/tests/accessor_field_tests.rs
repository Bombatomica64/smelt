//! Object-literal getters stored as record accessors, and the call-shape
//! rules the remeda round-3 fixes added.
//!
//! Requirements for getters, independent of the implementation:
//!
//! * Behaviour: a record field an object literal supplies through `get f()`
//!   answers the getter's CURRENT value on every read — a direct read, a
//!   destructuring, a spread (which copies the value at spread time) and the
//!   erased view (evaluated at erase time). Node prints `true false / 1 true`
//!   for the counter below; a value captured at construction printed
//!   `false true / NaN true`.
//! * Type preservation: the field keeps its declared type `T` everywhere it is
//!   read; only its storage becomes `SmeltAccessor<T>`, and only on records
//!   that some literal supplies through a getter. A plain data record keeps
//!   plain fields.

use super::*;

/// The counter from the remeda `debounce` shape.
const COUNTER: &str = r"
type Counter = {
  readonly bump: () => void;
  readonly isPending: boolean;
  readonly cachedValue: number | undefined;
};
type Plain = { readonly count: number };

function makeCounter(): Counter {
  let value: number | undefined;
  let pending = false;
  return {
    bump: () => { value = (value ?? 0) + 1; pending = true; },
    get isPending() { return pending; },
    get cachedValue() { return value; },
  };
}

export function show(): void {
  const counter = makeCounter();
  const plain: Plain = { count: 1 };
  console.log(counter.cachedValue === undefined, counter.isPending, plain.count);
  counter.bump();
  const { cachedValue } = counter;
  const erased: unknown = counter;
  console.log(cachedValue, JSON.stringify(erased));
}
";

/// The getter fields are accessor storage built from the getters; the plain
/// record and the plain callback field keep their own types.
#[test]
fn getter_fields_store_accessors_only_on_their_record() {
    let source = source_for(COUNTER);
    let counter = source
        .find("struct Counter")
        .and_then(|at| source.get(at..))
        .and_then(|rest| rest.get(..rest.find('}').unwrap_or(rest.len())))
        .unwrap_or_else(|| panic!("Counter is emitted:\n{source}"));
    assert!(
        counter.contains("is_pending: SmeltAccessor<bool>"),
        "{counter}"
    );
    assert!(
        counter.contains("cached_value: SmeltAccessor<Option<f64>>"),
        "{counter}"
    );
    assert!(
        counter.contains("bump: ::std::rc::Rc<dyn Fn() -> ()>"),
        "{counter}"
    );
    let plain = source
        .find("struct Plain")
        .and_then(|at| source.get(at..))
        .and_then(|rest| rest.get(..rest.find('}').unwrap_or(rest.len())))
        .unwrap_or_else(|| panic!("Plain is emitted:\n{source}"));
    assert!(
        plain.contains("count: f64") && !plain.contains("SmeltAccessor"),
        "{plain}"
    );
    let make = emitted_function_body(&source, "fn make_counter(");
    assert!(make.contains("is_pending: SmeltAccessor::new("), "{make}");
    assert!(make.contains("cached_value: SmeltAccessor::new("), "{make}");
}

/// Every read runs the getter: direct reads and destructuring call `.get()`,
/// and the erased view reads each accessor at erase time.
#[test]
fn getter_fields_are_read_through_the_getter() {
    let source = source_for(COUNTER);
    let main_body = emitted_function_body(&source, "fn show(");
    assert!(main_body.contains(".cached_value.get()"), "{main_body}");
    assert!(main_body.contains(".is_pending.get()"), "{main_body}");
    assert!(
        !main_body.contains(".cached_value.clone()") && !main_body.contains(".is_pending.clone()"),
        "no read takes the accessor itself: {main_body}"
    );
    assert!(
        source.contains("self.is_pending.get()")
            || source.contains("smelt_object_value.is_pending.get()"),
        "the erased view reads the getter: {source}"
    );
}

/// An implementation parameter some overload omits is optional at the
/// implementation: `convert()` passes `None`, not the type's default.
#[test]
fn overload_omitted_parameter_is_optional_at_the_implementation() {
    let source = source_for(
        r"
type Options = { readonly upper?: boolean };
function convert(data: string, options?: Options): string;
function convert(options?: Options): (data: string) => string;
function convert(dataOrOptions: Options | string, options?: Options): unknown {
  return typeof dataOrOptions === 'string' ? dataOrOptions : (data: string) => data;
}
export function later(): unknown { return convert(); }
",
    );
    let implementation = source
        .find("fn convert(")
        .and_then(|at| source.get(at..))
        .and_then(|rest| rest.lines().next())
        .unwrap_or_else(|| panic!("convert is emitted:\n{source}"));
    assert!(
        implementation.contains("data_or_options: Option<"),
        "{implementation}"
    );
    let later = emitted_function_body(&source, "fn later(");
    assert!(later.contains("convert(None"), "{later}");
}

/// A slot that packs its trailing arguments into a rest list
/// (`extractor(response, index, ...params)`) spreads them into a callback
/// that names them (`(response, _, word) => ..`): `word` is the list's first
/// element, not the whole list converted to a string.
#[test]
fn slot_rest_list_spreads_into_named_callback_parameters() {
    let source = source_for(
        r"
function batch(extractor: (response: number, index: number, ...params: string[]) => string): string {
  return extractor(1, 0, 'short', 'extra');
}
export function run(): string {
  return batch((response, _, word) => word);
}
",
    );
    let run = emitted_function_body(&source, "fn run(");
    assert!(run.contains("arg2.borrow().get(0)"), "{run}");
}
