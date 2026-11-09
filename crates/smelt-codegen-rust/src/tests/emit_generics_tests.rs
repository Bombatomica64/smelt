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

/// An implicit constructor over a GENERIC base forwards the base constructor's
/// parameters at the derived class's `extends` type arguments.
///
/// Before round 8 a generic base was not runnable at all, so the implicit
/// constructor fell back to one erased `Option<SmeltUnknown>` slot and never
/// ran the base's parameter properties. The base's `T` is substituted with the
/// `extends Box<string>` argument, so the forwarded parameter is a `String`.
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
    // `Box` is renamed `Box_` so it does not shadow Rust's `Box`.
    assert!(source.contains("Self::__smelt_init_Box_(this, "), "{source}");
    assert!(!source.contains("_smelt_super_arg"), "{source}");
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

    assert!(source.contains("struct Boxed<T>"), "{source}");
    assert!(source.contains("value: T,"), "{source}");
    assert!(!source.contains("value: SmeltUnknown,"), "{source}");
    assert!(
        source.contains("_smelt_phantom: ::std::marker::PhantomData<(T)>,"),
        "{source}"
    );
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
fn emits_generic_hash_map_into_unknown_runtime_conversion() {
    let source = source_for(
        r#"
function keyed(key: unknown): unknown {
  const values = new Map<unknown, unknown>([[key, "ready"]]);
  return Object.fromEntries(values);
}
"#,
    );

    assert!(
        source.contains("impl<K, T> IntoSmeltUnknown for ::std::collections::HashMap<K, T>"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Number(value) => smelt_number_to_string(value)"),
        "{source}"
    );
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
    // `Object.hasOwn` lowers for an array receiver (numeric in-bounds check), an
    // optional record receiver (asserted to its inner shape), and a generic
    // `T extends object` receiver (treated as a string-keyed record).
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
    // Array case becomes a length-bounded comparison.
    assert!(source.contains(".len()") || source.contains("len("), "{source}");
}

#[test]
fn lowers_for_in_over_generic_object_receiver() {
    // `for...in` over an unconstrained generic / erased object receiver casts to
    // a string-keyed record and iterates its keys.
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
    assert!(source.contains("for "), "{source}");
}

#[test]
fn record_to_generic_struct_adapter_erases_out_of_scope_type_params() {
    // Regression (es-toolkit flowRight_spec / curry): a record adapted into a
    // parameterized callable interface (`CurriedFunction1<T1, R>`) at a
    // NON-generic call site rendered its `__smelt_call` field default with the
    // interface's own type param spelled literally (`Rc<dyn Fn() ->
    // CurriedFunction1<T1, SmeltUnknown>>`). `T1` is not in scope in the
    // non-generic caller, so it was an unresolvable name (was E0425). The
    // adapter must only keep type params that are actually in scope for the
    // emitted function and erase the rest to `SmeltUnknown`.
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
    // Only inspect the non-generic `make_curried` body; the generic struct's own
    // `Default`/impl blocks legitimately spell `T1`/`R` because those are in
    // scope there.
    let body = source
        .split("fn make_curried")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        !body.contains("CurriedFunction1<T1"),
        "record-to-struct adapter must not spell an out-of-scope type param `T1` \
         in the non-generic caller: {source}"
    );
    // `CurriedFunction1` declares two call signatures of different arities, so
    // its single `__smelt_call` slot is now the erased variadic callable (see
    // `add_interface_call_signature_field`) rather than the first signature's
    // concrete `Fn() -> CurriedFunction1<T1, R>`. That removes the interface's
    // own type parameters from the slot entirely, which is a strictly stronger
    // form of the property this test guards: there is no longer any type
    // argument on the slot that COULD be out of scope. The assertion below
    // therefore pins the new representation; the out-of-scope check above --
    // the actual regression -- is unchanged.
    assert!(
        body.contains("__smelt_call: SmeltErasedFunction"),
        "an overloaded callable interface's slot should be the erased variadic \
         callable, leaving no interface type argument to fall out of scope: {source}"
    );
}

#[test]
fn generic_mut_list_forwarded_to_erased_callee_uses_convert_in_place_adapter() {
    // Regression (es-toolkit pull): a generic function forwards its `&mut T[]`
    // parameter into an erased helper whose parameter is `&mut SmeltUnknown[]`.
    // Rust `&mut` is invariant, so the reborrow needs a convert-in-place adapter:
    // build an erased temp list, pass `&mut temp`, write the mutated elements
    // back, and un-erase the returned value. The caller must STAY generic.
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
    assert!(
        source.contains("fn push_typed<T"),
        "the forwarding function must stay generic: {source}"
    );
    // The `&mut T[]` argument is erased into a temporary `SmeltUnknown` list
    // (`into_smelt_unknown`) and the mutated temp is written back through the
    // reference with per-element un-erasure (`smelt_from_unknown`).
    assert!(
        source.contains("smelt_mut_arg_0")
            && source.contains("into_smelt_unknown")
            && source.contains("smelt_from_unknown"),
        "the forwarded &mut list must be adapted in place through erase/un-erase: {source}"
    );
}

#[test]
fn plain_local_mut_list_argument_to_generic_callee_passes_elements_through() {
    // Regression (es-toolkit pullAt): an ordinary caller passes a concrete local
    // list into a callee that mutates it and IS emitted with real Rust generics
    // (`fn drop_first<T>(arr: &mut SmeltList<T>)`). Rust binds `T` from the
    // caller's `SmeltList<f64>`, so the elements must pass through unconverted and
    // the callee must borrow the caller's local directly — no conversion, no copy,
    // no write-back. Rendering the argument as `&mut <erased clone>` silently
    // discarded the mutation.
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
    assert!(
        source.contains("fn drop_first<T"),
        "the mutating callee must stay generic: {source}"
    );
    assert!(
        source.contains("drop_first(&mut xs)") && source.contains("drop_first(&mut ys)"),
        "both concrete locals must be borrowed mutably in place, with no erasing \
         temporary: {source}"
    );
    assert!(
        !source.contains("drop_first(&mut {"),
        "the argument must not be a converted temporary — that would discard the \
         mutation: {source}"
    );
    assert!(
        source.contains("let mut xs:") && source.contains("let mut ys:"),
        "a local borrowed mutably at a call site must be declared `mut`: {source}"
    );
}

#[test]
fn generic_class_return_of_a_monomorphized_mut_list_call_passes_through() {
    // Regression (haiku audit F0044): the adapter decides whether the callee's
    // return mentions the generics it monomorphized, and a narrower private
    // walk answered that by stopping at class arguments. `Holder<T>` therefore
    // read as a fully concrete return, and the already-`Holder<f64>` result was
    // rebuilt field by field through a `SmeltUnknown` extraction of an `f64`
    // field (E0308). Rust binds `T` from the borrowed `SmeltList<f64>`, so the
    // result is the destination's type and passes through unchanged.
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
    assert!(
        source.contains("fn take_held<T"),
        "the mutating callee must stay generic: {source}"
    );
    assert!(
        source.contains("let smelt_mut_call_result = take_held(&mut ys); smelt_mut_call_result }"),
        "the monomorphized class return must pass through unchanged: {source}"
    );
    assert!(
        !source.contains("smelt_struct_value"),
        "the concrete result must not be rebuilt field by field: {source}"
    );
}

/// A GENERIC base's constructor runs too.
///
/// No base value is ever built, so the base's type arguments never have to fit
/// the derived class's flattened layout — the reason the construct-and-copy
/// lowering used to drop `super(..)` to a generic base entirely (Hono's
/// `class Hono<E> extends HonoBase<E>`).
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

/// An unannotated arrow passed to a *generic* function is contextually typed
/// from the instantiation its sibling arguments imply, not from the callee's
/// raw type parameters.
///
/// The declared parameter type of `fn` is `(item: T) => U`. Handing that to the
/// arrow as its contextual type bound the closure's parameter to a type
/// variable outside its own scope, which lowered to `SmeltUnknown` — and then
/// the already-concrete `number[]` argument had to be erased to match the
/// instantiation, and the result un-erased again. One missing annotation cost
/// four erasure sites. The bindings are recoverable from the value arguments,
/// which is what this asserts.
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
    assert!(
        !main_body.contains("SmeltUnknown"),
        "the call site must stay concrete, with no erasure round-trip:\n{source}"
    );
    assert!(
        main_body.contains("closure_arg_0: f64"),
        "the arrow parameter must be contextually typed as `f64`:\n{source}"
    );
    assert!(
        main_body.contains("SmeltList<f64>"),
        "the concrete argument must not be re-erased to call the instantiation:\n{source}"
    );
}

/// A constrained type parameter erases only itself; an unconstrained sibling
/// still lifts to a real Rust generic.
///
/// `function_emits_rust_generics` used to demote a whole function the moment any
/// type parameter carried a constraint, so `K extends string` erased `T` too and
/// every position here came out `SmeltUnknown` — even `fallback: T` and the
/// return, which are directly inferable. 215 of es-toolkit's 800 generic
/// exported functions carry a constrained parameter.
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

    let signature = source
        .split("fn pick_first")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or_else(|| panic!("expected a generated `pick_first`:\n{source}"));
    assert!(
        signature.contains("arr: SmeltList<T>") && signature.contains("fallback: T"),
        "the unconstrained `T` must lift to a real generic:\n{source}"
    );
    assert!(
        signature.contains("SmeltUnknown"),
        "the constrained `K` must still erase:\n{source}"
    );
}

/// A type parameter inspected inside a *closure* body does not lift, even when
/// the enclosing function body only ever moves it.
///
/// `classes::type_param_only_moved` used to walk `function.blocks` alone. A
/// closure is a separate MIR body with its own locals and blocks, so an
/// inspection inside one was invisible and the parameter lifted anyway.
/// es-toolkit's `flatten<T, D extends number>` is exactly this shape: `T` lifted
/// out of `arr: T[]`, while the `Array.isArray(item)` that inspects it lives in
/// the inner recursive closure. The emitted
/// `matches!(item, SmeltUnknown::Array(_))` then ran against a `T` and the
/// generated library failed to compile with "expected type parameter `T`, found
/// `SmeltUnknown`".
#[test]
fn type_param_inspected_inside_a_closure_does_not_lift() {
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

    let signature = source
        .split("fn scan")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or_else(|| panic!("expected a generated `scan`:\n{source}"));
    // The generic parameter list is what must be absent; `SmeltList<..>` in the
    // parameter types legitimately contains angle brackets.
    assert!(
        !signature.starts_with('<'),
        "a closure that inspects `T` must not declare Rust generics:\n{source}"
    );
    assert!(
        signature.contains("arr: SmeltList<SmeltUnknown>"),
        "the inspected `T` must not reach the parameter list:\n{source}"
    );
}

/// A type parameter moved into a differently-typed container does not lift.
///
/// `Rvalue::Use` used to be whitelisted as "a bare move", which short-circuited
/// the destination check. But a move is opacity-preserving only when source and
/// destination agree about `T`: moving a `T` into a slot of another type erases
/// it. es-toolkit's `unzipWith` is exactly this shape — `group` comes from
/// `new Array(n)` so its type is `unknown[]`, each `T` element is moved into it,
/// and the erased list is then passed to a callback whose bound reads
/// `Fn(SmeltList<T>)`, so the generated library failed with "expected `Vec<T>`,
/// found `Vec<SmeltUnknown>`".
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

    let signature = source
        .split("fn collect")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or_else(|| panic!("expected a generated `collect`:\n{source}"));
    assert!(
        !signature.starts_with('<'),
        "a `T` moved into an `unknown[]` must not lift:\n{source}"
    );
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
    // H29. `#[derive(Clone)]` on a generic struct generates `impl<T: Clone>` —
    // its own bound and nothing more. That cannot satisfy a field whose type is
    // another generated class, because those carry the full generated bound set,
    // so hono's `class TrieRouter<T> { root: Node<T> }` reported "the trait
    // `Clone`/`Default`/`IntoSmeltUnknown`/`SmeltFromUnknown` is not implemented
    // for `T`" once per derive.
    //
    // The impls are spelled out with a field-type `where` clause: that is the
    // exact requirement of a field-by-field body, where bounding `T` says either
    // too little or too much.
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

    // The enabling condition: the class really is generic, which is the shape the
    // derives could not bound.
    assert!(
        source.contains("struct Trunk<T>"),
        "the fixture must produce a generic class: {source}"
    );
    assert!(
        !source.contains("#[derive(Clone, Debug, Default, PartialEq)]\n#[allow(dead_code)]\nstruct Trunk<T>"),
        "a generic value class must not rely on the derives: {source}"
    );
    for spelled in [
        "impl<T> Clone for Trunk<T>",
        "impl<T> PartialEq for Trunk<T>",
    ] {
        assert!(
            source.contains(spelled),
            "expected a spelled-out `{spelled}` impl: {source}"
        );
    }
    // `Default` takes the crate's generated bound set, not the derive-equivalent
    // `T: Default` (round 31, Agent E item 2). A field default can DELEGATE to
    // another generated class — here `Leaf<T>::default()` — whose own impl
    // carries the full set, and `T: Default` cannot prove it. Every generic item
    // the crate emits carries the same set, so no in-crate consumer is worse off.
    assert!(
        source.contains(
            "impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Default for Trunk<T>"
        ),
        "a generic value class takes the generated bound set on `Default`: {source}"
    );
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
