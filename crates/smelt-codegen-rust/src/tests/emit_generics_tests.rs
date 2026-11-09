//! Generated Rust for generic functions and type parameters.

use super::*;

/// A generic erased to SmeltUnknown must not rebuild an array to cross an erased boundary.
#[test]
fn erased_generic_list_arguments_and_returns_do_not_copy_elements() {
    let source = source_for(include_str!("../../tests/fixtures/erased_generic_list.ts"));
    assert!(
        !source.contains(".map(|value| value).collect::<Vec<_>>()"),
        "passing an unknown list to an erased generic must not rebuild its storage"
    );
    for signature in ["fn dispatch", "fn erase_array"] {
        let body = emitted_function_body(&source, signature);
        assert!(
            body.contains("SmeltUnknown::Array("),
            "the fixture must exercise array erasure: {body}"
        );
        assert!(
            !body.contains("let smelt_values: Vec<_>"),
            "an already-erased array must share storage rather than copy every element: {body}"
        );
        assert!(
            body.contains(&format!("SmeltUnknown::Array({ERASED_GENERIC_ARRAY_ALIAS_FN}(")),
            "the alias goes through the named boundary helper the SmeltUnknown report keys on: {body}"
        );
    }
    // The helpers pin `SmeltUnknown` elements, so a crate that reached them
    // with any other element type would not compile.
    assert!(
        source.contains(&format!(
            "fn {ERASED_GENERIC_LIST_ALIAS_FN}(list: impl Into<SmeltList<SmeltUnknown>>) -> SmeltList<SmeltUnknown>"
        )),
        "{source}"
    );
    assert!(
        source.contains(&format!(
            "fn {ERASED_GENERIC_ARRAY_ALIAS_FN}(list: impl Into<SmeltArray>) -> SmeltArray"
        )),
        "{source}"
    );
}

/// An erased generic `T[]` leaving the erased-generic ABI into an `unknown[]`
/// slot takes the named list alias helper: same buffer, no per-element copy.
#[test]
fn erased_generic_list_into_unknown_list_takes_the_alias_helper() {
    let source = source_for(
        r"
export function forward<T>(data: T[]): unknown[] {
  return data
}
",
    );
    let body = emitted_function_body(&source, "fn forward");
    assert!(
        body.contains(&format!("{ERASED_GENERIC_LIST_ALIAS_FN}(")),
        "an erased `T[]` already has the `unknown[]` representation: {body}"
    );
    assert!(
        !body.contains("collect::<Vec<_>>()"),
        "an erased `T[]` must reach an `unknown[]` slot without a rebuild: {body}"
    );
}

/// A list argument coerced into an erased callee's `T[]` slot is judged on two
/// scopes: the operand at the caller's own scope, the slot at the callee seam's.
///
/// `Holder<T>` declares a `T` and so does `eraseArray<T>`; type parameters are
/// name-interned, so the caller's scope spells the callee's `T` even though the
/// erased callee's slot is `SmeltList<SmeltUnknown>`. Judging the slot at the
/// caller's scope rebuilds an array that already has the slot's representation
/// (and loses its identity); judging the operand at the callee's scope would
/// alias a real `SmeltList<T>` into a `SmeltList<SmeltUnknown>` slot.
#[test]
fn erased_generic_list_argument_judges_each_side_at_its_own_scope() {
    let source = source_for(include_str!("../../tests/fixtures/erased_generic_list.ts"));
    let method_body = |signature: &str| {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("generated source declares `{signature}`:\n{source}"));
        let rest = source.get(start..).unwrap_or_default();
        let end = rest
            .find("\n    }\n")
            .map_or(rest.len(), |offset| offset.saturating_add(7));
        rest.get(..end).unwrap_or(rest).to_owned()
    };
    let share = method_body("fn share(");
    assert!(
        share.contains("Into::<SmeltList<_>>::into("),
        "an erased array already has the erased slot's representation: {share}"
    );
    assert!(
        !share.contains("collect::<Vec<_>>()"),
        "an erased array must reach an erased slot without a rebuild: {share}"
    );
    let share_own = method_body("fn share_own(");
    assert!(
        !share_own.contains("Into::<SmeltList<_>>::into(")
            && !share_own.contains(ERASED_GENERIC_LIST_ALIAS_FN),
        "the class's own `SmeltList<T>` is not the erased slot's representation: {share_own}"
    );
    assert!(
        share_own.contains("into_smelt_unknown()"),
        "each `T` element must be converted into the erased slot: {share_own}"
    );
}

/// An implicit constructor over a generic base forwards the base constructor's parameters
/// at the derived class's `extends` type arguments: `extends Box<string>` substitutes `T`,
/// so the forwarded parameter is a `JsString`.
#[test]
fn an_implicit_constructor_over_a_generic_base_forwards_typed_parameters() {
    let source = source_for(
        r#"
class Box<T> {
  constructor(public value: T) {}
}
class StringBox extends Box<string> {}
const withArg = new StringBox("value");
"#,
    );

    assert!(source.contains("fn new(value: JsString) -> Self"), "{source}");
    assert!(source.contains("Self::__smelt_init_Box_(this, "), "`Box` is renamed `Box_` so it does not shadow Rust's `Box`:\n{source}");
    assert!(!source.contains("Self::__smelt_init_Box_(this, SmeltUnknown"), "the argument stays at the bound `JsString`:\n{source}");
}

#[test]
fn emits_generic_interface_storage_with_phantom_parameter() {
    let source = source_for(
        r"
interface Boxed<T> {
  value: T;
}
const boxed: Boxed<number> = { value: 1 };
const copied: Boxed<number> = boxed;
",
    );

    assert_eq!(rust_items(&source, &["struct Boxed<T>"]), r#"struct Boxed<T> {
value: T,
_smelt_phantom: ::std::marker::PhantomData<(T)>,
}"#);
    assert!(source.contains("boxed: Boxed<f64>"), "{source}");
}

#[test]
fn emits_array_entries_for_guarded_erased_generic() {
    let source = source_for(
        r"
function copy<T>(value: T): unknown[] {
  const copied: unknown[] = [];
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      copied[index] = item;
    }
  }
  return copied;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Array(smelt_values) => smelt_values.into_iter().enumerate()"),
        "{source}"
    );
    assert!(
        !source.contains("Vec<(i64, SmeltUnknown)> = Default::default()"),
        "{source}"
    );
}

#[test]
fn emits_runtime_index_for_erased_string_generics() {
    let source = source_for(
        r"
function first<S extends string>(value: S): string {
  return value[0];
}
",
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("smelt_value.char_at(smelt_position)"), "{source}");
    assert!(!source.contains("SmeltUnknown::Null.clone()"), "{source}");
}

#[test]
fn adapts_generic_record_fields_with_instantiated_payload_types() {
    let source = source_for(
        r"
type MatchFnResult<T> = { value: T; rest: string };

declare const genericResult: MatchFnResult<unknown>;
const numericResult = genericResult as MatchFnResult<number>;
const value = numericResult.value;
",
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
    assert!(!source.contains("let value: SmeltUnknown"), "{source}");
}

#[test]
fn instantiates_generic_option_defaults_without_leaking_type_params() {
    let source = source_for(
        r"
interface ContextOptions<DateType extends Date = Date> {
  in?: (value: unknown) => DateType;
}

interface ParseOptions<DateType extends Date = Date> extends ContextOptions<DateType> {
  token?: string;
}

interface IsMatchOptions {
  token?: string;
}

function parseValue(options?: ParseOptions<unknown>): unknown {
  return options?.token ?? null;
}

function isMatch(options?: IsMatchOptions): unknown {
  return parseValue(options);
}
",
    );

    assert!(
        source.contains("None::<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>>"),
        "generic callback defaults should use the instantiated option payload: {source}"
    );
    assert!(
        source.contains(
            "ParseOptions { in_: None::<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>>"
        ),
        "struct literal defaults must not leak declaration type parameters: {source}"
    );
}

#[test]
fn emits_spread_sort_for_erased_iterable_generic() {
    let source = source_for(
        r"
type IterableContainer<T> = readonly T[];

const numberComparator = (a: number, b: number): number => a - b;

function sorted<T extends IterableContainer<number>>(data: T): unknown {
  return [...data].sort(numberComparator);
}
",
    );

    // The spread of the erased, list-constrained `data` materializes a fresh,
    // CONCRETE `Vec<f64>` (its `SmeltUnknown` elements unwrapped to the `number`
    // element type from the `IterableContainer<number>` constraint) instead of
    // staying an erased alias. That concrete binding is what lets the following
    // `.sort(cmp)` take the typed in-place path rather than the dynamic path
    // whose sorted result is discarded.
    assert!(source.contains(": SmeltList<f64> ="), "{source}");
    assert!(source.contains(".sort_by(|left, right|"), "{source}");
}

#[test]
fn emits_generic_iterable_spread_concat_through_unknown_lists() {
    let source = source_for(
        r"
function concatImplementation<T1, T2>(arr1: T1, arr2: T2): unknown[] {
  return [...arr1 as unknown[], ...arr2 as unknown[]];
}
",
    );

    assert!(
        source.contains(".iter().cloned().chain("),
        "generic iterable spread concat should not collapse to a default vector: {source}"
    );
    assert!(
        !source.contains("return Default::default();"),
        "generic iterable spread concat should preserve the unknown array values: {source}"
    );
}

#[test]
fn lowers_object_has_own_on_optional_array_and_generic_receivers() {
    // `Object.hasOwn` lowers for an array, an optional record and a generic `T extends object`
    // receiver; the receiver types stay as declared. Behaviour is in the `generics_behaviour`
    // node-parity fixture.
    let source = source_for(
        r"
export function indexPresent(arr: number[], i: number): boolean {
  return Object.hasOwn(arr, i);
}
export function keyPresent(obj?: Record<string, number>, key?: string): boolean {
  return obj != null && key != null && Object.hasOwn(obj, key);
}
export function genericKey<T extends object>(object: T, key: string): boolean {
  return Object.hasOwn(object, key);
}
",
    );

    assert_eq!(rust_items(&source, &["sig index_present", "sig key_present", "sig generic_key"]), r#"fn index_present(arr: SmeltList<f64>, i: f64) -> bool

fn key_present(obj: Option<SmeltRecord<JsString, f64>>, key: Option<JsString>) -> bool

fn generic_key(object: SmeltUnknown, key: JsString) -> bool"#);
}

#[test]
fn lowers_for_in_over_generic_object_receiver() {
    // `for...in` over an erased object receiver iterates its keys; the signature keeps the
    // concrete `string[]` result. Behaviour is in the `generics_behaviour` node-parity fixture.
    let source = source_for(
        r"
export function keysOf<T>(object: T): string[] {
  const out: string[] = [];
  for (const key in object) {
    out.push(key);
  }
  return out;
}
",
    );

    assert_eq!(rust_items(&source, &["sig keys_of"]), r#"fn keys_of(object: SmeltUnknown) -> SmeltList<JsString>"#);
}

#[test]
fn record_to_generic_struct_adapter_erases_out_of_scope_type_params() {
    // A record adapted into a parameterized callable interface at a non-generic call site must not
    // spell the interface's own type parameters, which are not in scope there. An interface with
    // several call signatures of different arities stores the erased variadic callable.
    let source = source_for(
        r"
interface CurriedFunction1<T1, R> {
  (): CurriedFunction1<T1, R>;
  (t1: T1): R;
  tag: string;
}
export function makeCurried(): CurriedFunction1<number, string> {
  const built: CurriedFunction1<number, string> = { tag: 'x' } as CurriedFunction1<number, string>;
  return built;
}
",
    );

    let body = rust_fn(&source, "make_curried");
    assert!(body.contains("__smelt_call: SmeltErasedFunction"), "an overloaded callable interface's slot is the erased variadic callable, leaving no interface type argument to fall out of scope: {body}");
    assert!(!body.contains("CurriedFunction1<T1"), "a non-generic caller never spells the interface's own `T1`: {body}");
}

#[test]
fn generic_mut_list_forwarded_to_erased_callee_uses_convert_in_place_adapter() {
    // A generic function forwarding its `&mut T[]` into an erased helper needs a convert-in-place
    // adapter, because `&mut` is invariant: build an erased temporary, pass it, write the mutated
    // elements back. The caller stays generic.
    let source = source_for(
        r#"
function eraseInto(target: unknown[], value: unknown): void {
  target.push(value);
}
export function pushTyped<T>(arr: T[], value: T): T[] {
  eraseInto(arr, value);
  return arr;
}
export function useNums(): number[] {
  const xs: number[] = [1, 2, 3];
  return pushTyped(xs, 4);
}
export function useStrs(): string[] {
  const ys: string[] = [""];
  return pushTyped(ys, "a");
}
"#,
    );

    assert!(rust_items(&source, &["sig push_typed"]).starts_with("fn push_typed<T"), "the forwarding function must stay generic: {source}");
    let body = rust_fn(&source, "push_typed");
    assert!(body.contains("smelt_mut_arg_0") && body.contains("into_smelt_unknown") && body.contains("smelt_from_unknown"), "the forwarded &mut list must be adapted in place through erase/un-erase: {body}");
    assert!(rust_fn(&source, "use_nums").contains("push_typed(&mut xs, 4.0)"), "the value argument binds `T` at its own type, like the list beside it:\n{source}");
}

#[test]
fn plain_local_mut_list_argument_to_generic_callee_passes_elements_through() {
    // A concrete local list passed to a mutating generic callee binds `T` from the caller's type:
    // the callee borrows the caller's local directly, with no conversion, copy or write-back. A
    // local borrowed mutably is declared `mut`.
    let source = source_for(
        r"
function dropFirst<T>(arr: T[]): T[] {
  return arr.splice(0, 1);
}
export function useDrop(): number[] {
  const xs: number[] = [1, 2, 3];
  dropFirst(xs);
  return xs;
}
export function useDropStrings(): string[] {
  const ys: string[] = ['a', 'b'];
  dropFirst(ys);
  return ys;
}
",
    );

    assert_eq!(rust_items(&source, &["sig drop_first", "fn use_drop", "fn use_drop_strings"]), r#"fn drop_first<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(mut arr: &mut SmeltList<T>) -> SmeltList<T>

fn use_drop() -> SmeltList<f64> {
let mut xs: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
let _smelt_tmp_2: SmeltList<f64> = { let smelt_mut_call_result = drop_first(&mut xs); smelt_mut_call_result };
return xs;
}

fn use_drop_strings() -> SmeltList<JsString> {
let mut ys: SmeltList<JsString> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![JsString::from("a"), JsString::from("b")]; smelt_list_items }));
let _smelt_tmp_2: SmeltList<JsString> = { let smelt_mut_call_result = drop_first(&mut ys); smelt_mut_call_result };
return ys;
}"#);
}

#[test]
fn generic_class_return_of_a_monomorphized_mut_list_call_passes_through() {
    // `Holder<T>` returned from a monomorphized mutating callee is already the destination type,
    // so it passes through unchanged rather than being rebuilt field by field.
    let source = source_for(
        r"
class Holder<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
function takeHeld<T>(arr: T[]): Holder<T> {
  return new Holder(arr.splice(0, 1)[0]);
}
export function useTake(): number {
  const ys: number[] = [4, 5];
  const held = takeHeld(ys);
  return held.value + ys.length;
}
",
    );

    assert_eq!(rust_items(&source, &["sig take_held", "fn use_take"]), r#"fn take_held<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(mut arr: &mut SmeltList<T>) -> Holder<T>

fn use_take() -> f64 {
let held: Holder<f64>;
let _smelt_tmp_4: f64;
let _smelt_tmp_5: f64;
let mut ys: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![4.0, 5.0]; smelt_list_items }));
let _smelt_tmp_3: Holder<f64> = { let smelt_mut_call_result = take_held(&mut ys); smelt_mut_call_result };
held = _smelt_tmp_3;
_smelt_tmp_4 = ys.len() as f64;
_smelt_tmp_5 = held.value + _smelt_tmp_4;
return _smelt_tmp_5;
}"#);
}

/// A generic base's constructor runs over the derived instance. No base value is built, so
/// the base's type arguments never have to fit the derived class's flattened layout.
#[test]
fn generic_base_constructor_runs_over_the_derived_instance() {
    let source = source_for(
        r"
class AppBase<E = string> {
  prefix: string = '';
  handle: (path: string) => string;
  constructor(label: string) {
    this.handle = (path: string) => label + this.prefix + path;
  }
}
class App<E = string> extends AppBase<E> {
  constructor() {
    super('app');
    this.prefix = '/api';
  }
}
console.log(new App<number>().handle('/x'));
",
    );

    assert!(source.contains("Self::__smelt_init_AppBase(this, JsString::from(\"app\"))"), "{source}");
    assert!(source.contains("fn __smelt_init_AppBase(smelt_receiver: Self, label: JsString)"), "{source}");
}

/// An unannotated arrow passed to a generic function is contextually typed from the
/// instantiation its sibling arguments imply, not from the callee's raw type parameters, so
/// the call site stays concrete with no erasure round-trip.
#[test]
fn unannotated_arrow_into_generic_callee_stays_concrete() {
    let source = source_for(
        r"
function mapArr<T, U>(arr: T[], fn: (item: T) => U): U[] {
  const out: U[] = [];
  for (const item of arr) { out.push(fn(item)); }
  return out;
}
const nums: number[] = [1, 2, 3];
const doubled = mapArr(nums, (x) => x * 2);
",
    );

    let main_body = source
        .split("fn main()")
        .nth(1)
        .unwrap_or_else(|| panic!("expected a generated `main`:\n{source}"));
    assert!(!main_body.contains("SmeltUnknown"), "the call site must stay concrete, with no erasure round-trip:\n{source}");
    assert!(main_body.contains("closure_arg_0: f64"), "the arrow parameter must be contextually typed as `f64`:\n{source}");
    assert!(main_body.contains("SmeltList<f64>"), "the concrete argument must not be re-erased to call the instantiation:\n{source}");
}

/// A constrained type parameter erases only itself; an unconstrained sibling still lifts to
/// a real Rust generic, so `fallback: T` and the return stay directly inferable.
#[test]
fn constrained_type_param_erases_without_demoting_its_siblings() {
    let source = source_for(
        r"
function pickFirst<T, K extends string>(arr: T[], key: (item: T) => K, fallback: T): T {
  if (arr.length > 0) { return arr[0]; }
  return fallback;
}
const nums: number[] = [1, 2, 3];
const v = pickFirst(nums, (n) => (n > 1 ? 'big' : 'small'), 0);
",
    );

    assert_eq!(rust_items(&source, &["sig pick_first"]), r#"fn pick_first<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> SmeltUnknown + ?Sized>(arr: SmeltList<T>, key: &F0, fallback: T) -> T"#);
}

/// A type parameter inspected inside a closure body does not lift even when the enclosing
/// function body only moves it, because the emitted inspection runs against the erased
/// carrier.
#[test]
fn type_param_inspected_inside_a_closure_does_not_lift() {
    // The generic parameter list must be absent and the inspected `T` must not reach the
    // parameters.
    let source = source_for(
        r"
function scan<T, K extends string>(arr: T[], key: (item: T) => K): T[] {
  const out: T[] = [];
  arr.forEach((item) => { if (typeof item === 'string') { out.push(item); } });
  return out;
}
const r = scan(['a', 'b'], (s) => 'k');
",
    );

    assert_eq!(rust_items(&source, &["sig scan"]), r#"fn scan(arr: SmeltList<SmeltUnknown>, key: &dyn Fn(&SmeltUnknown) -> SmeltUnknown) -> SmeltList<SmeltUnknown>"#);
}

/// A type parameter moved into a differently-typed container does not lift: moving a `T`
/// into an `unknown[]` slot erases it, and the erased list could not reach a
/// `Fn(SmeltList<T>)` bound.
#[test]
fn type_param_moved_into_an_erased_container_does_not_lift() {
    let source = source_for(
        r"
function collect<T, K extends string>(rows: T[][], key: (vals: T[]) => K): K[] {
  const out: K[] = [];
  for (let i = 0; i < rows.length; i++) {
    const group = new Array(rows.length);
    for (let j = 0; j < rows.length; j++) { group[j] = rows[j][i]; }
    out.push(key(group));
  }
  return out;
}
const r = collect([[1, 2]], (v) => 'k');
",
    );

    assert_eq!(rust_items(&source, &["sig collect"]), r#"fn collect(rows: SmeltList<SmeltList<SmeltUnknown>>, key: &dyn Fn(&SmeltList<SmeltUnknown>) -> SmeltUnknown) -> SmeltList<SmeltUnknown>"#);
}

/// A generic call signature stores its callable at the CONSTRAINT of each of
/// its own type parameters: an `Rc<dyn Fn>` cannot be generic, so
/// `<K extends string>(key: K): K` is `Fn(String) -> String`, and a generic
/// arrow or function-type annotation follows the same rule.
#[test]
fn generic_call_signature_is_stored_at_its_constraints() {
    let source = source_for(
        "interface Picker {
  <K extends string>(key: K): K;
}
class Holder {
  pick: Picker = <K extends string>(key: K): K => key;
  direct: <K extends string>(key: K) => K = <K extends string>(key: K): K => key;
}
const holder = new Holder();
const picked: string = holder.pick('a') + holder.direct('b');
",
    );

    assert!(
        source.contains("__smelt_call: ::std::rc::Rc<dyn Fn(JsString) -> JsString>"),
        "the call signature erases K to its constraint:\n{source}"
    );
    assert!(
        source.contains("direct: ::std::rc::Rc<dyn Fn(JsString) -> JsString>"),
        "a generic function-type annotation follows the same rule:\n{source}"
    );
    assert!(
        !source.contains("&SmeltUnknown| {"),
        "no generic closure may take its argument as SmeltUnknown:\n{source}"
    );
}

#[test]
fn a_generic_class_spells_its_derived_impls_instead_of_deriving_them() {
    // `#[derive(Clone)]` on a generic struct generates `impl<T: Clone>`, which cannot satisfy a
    // field whose type is another generated class. The impls are spelled out with a field-type
    // `where` clause, and `Default` takes the crate's generated bound set because a field default
    // can delegate to another generated class.
    let source = source_for(
        r"
class Leaf<T> {
  items: T[] = [];
}
class Trunk<T> {
  leaf: Leaf<T> = new Leaf<T>();
}
export function hold<T>(trunk: Trunk<T>): Trunk<T> {
  return trunk;
}
",
    );

    assert!(source.contains("struct Trunk<T>"), "the fixture must produce a generic class: {source}");
    assert!(!source.contains("#[derive(Clone, Debug, Default, PartialEq)]\n#[allow(dead_code)]\nstruct Trunk<T>"), "a generic value class must not rely on the derives: {source}");
    for spelled in ["impl<T> Clone for Trunk<T>", "impl<T> PartialEq for Trunk<T>"] {
        assert!(source.contains(spelled), "expected a spelled-out `{spelled}` impl: {source}");
    }
    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Default for Trunk<T>"), "a generic value class takes the generated bound set on `Default`: {source}");
}

#[test]
/// A self-recursive generic free function keeps its type parameter: its own
/// recursive call pins `T` to itself, and its body trial assumes the function
/// generic for that call.
fn self_recursive_generic_function_keeps_its_type_parameter() {
    let source = source_for(
        r"
export function repeat<T>(x: T, n: number): T { return n > 0 ? repeat(x, n - 1) : x; }
",
    );
    assert!(source.contains("fn repeat<T: "), "{source}");
    assert!(!source.contains("into_smelt_unknown(), _smelt_tmp"), "{source}");
}
