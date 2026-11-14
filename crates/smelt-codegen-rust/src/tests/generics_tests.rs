//! Codegen tests for generic classes, interfaces and functions.
//!
//! These assert that generic class/interface declarations lower to real Rust
//! generics (`struct Container<T>`, `impl<T: ...> Container<T>`) and that use
//! sites (`new Container<number>(...)`, `b.get()`) keep the instantiation
//! concrete instead of erasing the generic arguments to `SmeltUnknown`.

use super::*;

#[test]
fn emits_generic_class_struct_and_impl() {
    // `Container` mutates `this.value` in `set`, so it is a reference class: a handle newtype over
    // the shared cell, with an identity `Clone`, and methods that all take `&self` through
    // interior mutability. The inner record carries the generic parameter.
    let source = source_for(
        r"
class Container<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
  set(value: T): void { this.value = value; }
}
",
    );

    assert_eq!(
        rust_items(
            &source,
            &[
                "struct Container<T>",
                "struct ContainerInner<T>",
                "sig get",
                "sig set"
            ]
        ),
        r"struct Container<T>(::std::rc::Rc<::std::cell::RefCell<ContainerInner<T>>>);

struct ContainerInner<T> {
value: T,
_smelt_phantom: ::std::marker::PhantomData<(T)>,
}

fn get(&self) -> T

fn set(&self, value: T) -> ()"
    );
    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Container<T> {"), "{source}");
    assert!(
        source.contains("Container(::std::rc::Rc::clone(&self.0))"),
        "identity `Clone` shares the cell:\n{source}"
    );
}

#[test]
fn instantiates_generic_class_with_concrete_arguments() {
    // The instantiation is `Container<f64>`: arguments pass through concretely so Rust
    // monomorphizes, and the method result needs no `SmeltUnknown` extraction.
    let source = source_for(
        r"
class Container<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
  set(value: T): void { this.value = value; }
}
function useBox(): number {
  const b = new Container<number>(3);
  b.set(5);
  return b.get();
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn use_box"]),
        r"fn use_box() -> f64 {
let b: Container<f64>;
let _smelt_tmp_1: Container<f64> = Container::new(3.0);
b = _smelt_tmp_1;
let _ = b.set(5.0);
let _smelt_tmp_3: f64 = b.get();
return _smelt_tmp_3;
}"
    );
}

#[test]
fn emits_two_parameter_generic_class() {
    // Both concrete arguments pass through; `getSecond(): B` on `Pair<f64, JsString>` returns the
    // string, not `SmeltUnknown`.
    let source = source_for(
        r#"
class Pair<A, B> {
  first: A;
  second: B;
  constructor(first: A, second: B) { this.first = first; this.second = second; }
  getFirst(): A { return this.first; }
  getSecond(): B { return this.second; }
}
function usePair(): string {
  const p = new Pair<number, string>(1, "x");
  return p.getSecond();
}
"#,
    );

    assert_eq!(
        rust_items(&source, &["struct Pair<A, B>", "fn use_pair"]),
        r#"struct Pair<A, B> {
first: A,
second: B,
_smelt_phantom: ::std::marker::PhantomData<(A, B)>,
}

fn use_pair() -> JsString {
let p: Pair<f64, JsString>;
let _smelt_tmp_1: Pair<f64, JsString> = Pair::new(1.0, JsString::from("x"));
p = _smelt_tmp_1;
let _smelt_tmp_2: JsString = p.get_second();
return _smelt_tmp_2;
}"#
    );
}

#[test]
fn emits_generic_free_function_with_real_rust_generics() {
    // A generic free function lowers to real Rust generics with `T` in both positions.
    let source = source_for(
        r"
export function identity<T>(x: T): T {
  return x;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig identity"]),
        r"fn identity<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(x: T) -> T"
    );
}

#[test]
fn monomorphizes_generic_free_function_call_site() {
    // A concrete argument passes through so Rust monomorphizes the call, with no `SmeltUnknown`
    // wrapping or extraction.
    let source = source_for(
        r"
export function identity<T>(x: T): T {
  return x;
}
export function useIdentity(): number {
  return identity(3);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn use_identity"]),
        r"fn use_identity() -> f64 {
let _smelt_tmp_0: f64 = identity(3.0);
return _smelt_tmp_0;
}"
    );
}

#[test]
fn emits_two_parameter_generic_free_function() {
    // Each type parameter renders as its own Rust generic.
    let source = source_for(
        r"
export function pair<A, B>(first: A, second: B): B {
  return second;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pair"]),
        r"fn pair<A: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, B: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(first: A, second: B) -> B"
    );
}

#[test]
fn emits_generic_free_function_over_list_parameter() {
    // A type parameter nested in `T[]` renders as `SmeltList<T>`.
    let source = source_for(
        r"
export function first<T>(xs: T[]): T {
  return xs[0];
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig first"]),
        r"fn first<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(xs: SmeltList<T>) -> T"
    );
}

#[test]
fn erases_generic_free_function_with_bounded_type_param() {
    // A `Date` bound lifts to `DateLike` and reads its time value through the handle. A builtin
    // bound with no generic trait (`R extends RegExp`) falls back to full erasure.
    let source = source_for(
        r"
export function invalid<D extends Date>(value: D): boolean {
  return isNaN(value.getTime());
}
export function flags<R extends RegExp>(value: R): string {
  return value.flags;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn invalid", "sig flags"]),
        r"fn invalid<D: DateLike + Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(value: D) -> bool {
let _smelt_tmp_1: f64 = ((value.clone()).smelt_date()).get_time();
let _smelt_tmp_2: bool = _smelt_tmp_1.is_nan();
return _smelt_tmp_2;
}

fn flags(value: SmeltUnknown) -> JsString"
    );
}

#[test]
fn lifts_class_bounded_type_param_with_generated_trait() {
    // `A extends Animal` lifts to `A: AnimalLike`. The trait carries exactly the members read
    // through the bound, the method as a method and the field as a getter, and is implemented for
    // the bound and its subclass, so a call through it dispatches to the receiver's override.
    let source = source_for(
        r#"
class Animal {
  constructor(public legs: number) {}
  speak(): string { return "..."; }
}
class Dog extends Animal {
  speak(): string { return "woof"; }
}
export function loud<A extends Animal>(a: A): A {
  console.log(a.speak(), a.legs);
  return a;
}
export function main(): void {
  loud(new Dog(4));
}
"#,
    );

    assert_eq!(
        rust_items(
            &source,
            &[
                "fn loud",
                "trait AnimalLike",
                "impl AnimalLike for Animal",
                "impl AnimalLike for Dog"
            ]
        ),
        r#"fn loud<A: AnimalLike + Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(a: A) -> A {
let _smelt_tmp_1: JsString = a.speak();
let _smelt_tmp_2: f64 = a.legs();
let _ = { println!("{} {}", _smelt_tmp_1, smelt_console_number(_smelt_tmp_2)); };
return a.clone();
}

trait AnimalLike {
fn speak(&self) -> JsString;
fn legs(&self) -> f64;
}

impl AnimalLike for Animal {
fn speak(&self) -> JsString { let smelt_callable = self.0.borrow().speak.clone(); (smelt_callable)() }
fn legs(&self) -> f64 { self.0.borrow().legs.clone() }
}

impl AnimalLike for Dog {
fn speak(&self) -> JsString { let smelt_callable = self.0.borrow().speak.clone(); (smelt_callable)() }
fn legs(&self) -> f64 { self.0.borrow().legs.clone() }
}"#
    );
}

#[test]
fn bounded_type_param_written_through_keeps_erased_fallback() {
    // A write through the bound has no trait setter, so that parameter keeps the erased signature;
    // the read-only sibling lifts.
    let source = source_for(
        r"
class Animal {
  constructor(public legs: number) {}
}
export function grow<A extends Animal>(a: A): number {
  a.legs = a.legs + 1;
  return a.legs;
}
export function legsOf<A extends Animal>(a: A): number {
  return a.legs;
}
export function main(): void {
  console.log(grow(new Animal(1)), legsOf(new Animal(2)));
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig grow", "sig legs_of"]),
        r"fn grow(mut a: SmeltUnknown) -> f64

fn legs_of<A: AnimalLike + Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(a: A) -> f64"
    );
}

#[test]
fn erases_generic_free_function_when_body_inspects_type_param() {
    // A body that inspects or compares a `T` value needs the erased carrier, so the whole function
    // falls back to erasure; `===` forces it here.
    let source = source_for(
        r"
export function same<T>(a: T, b: T): boolean {
  return a === b;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig same"]),
        r"fn same(a: SmeltUnknown, b: SmeltUnknown) -> bool"
    );
}

#[test]
fn erases_generic_free_function_called_with_erased_argument() {
    // A generic function invoked with an already-erased argument cannot infer `T` (E0283), so it
    // is demoted to the erased signature and the argument binds directly.
    let source = source_for(
        r"
export function identity<T>(x: T): T {
  return x;
}
export function passErased(u: unknown): unknown {
  return identity(u);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig identity", "fn pass_erased"]),
        r"fn identity(x: SmeltUnknown) -> SmeltUnknown

fn pass_erased(u: SmeltUnknown) -> SmeltUnknown {
let _smelt_tmp_1: SmeltUnknown = (identity(u.clone())).into_smelt_unknown();
return _smelt_tmp_1;
}"
    );
}

#[test]
fn emits_generic_interface_type_params() {
    // The interface storage keeps its generic parameters and instantiates concretely at the return
    // position.
    let source = source_for(
        r#"
interface Outcome<T, E> {
  ok: boolean;
  value: T;
  error: E;
}
function makeOk(v: number): Outcome<number, string> {
  return { ok: true, value: v, error: "" };
}
"#,
    );

    assert_eq!(
        rust_items(&source, &["struct Outcome<T, E>", "sig make_ok"]),
        r"struct Outcome<T, E> {
ok: bool,
value: T,
error: E,
_smelt_phantom: ::std::marker::PhantomData<(T, E)>,
}

fn make_ok(v: f64) -> Outcome<f64, JsString>"
    );
}

#[test]
fn generic_class_map_key_gains_key_eq_bound() {
    // A class whose generic parameter is a `Map` key carries `SmeltJsKeyEq` on its impl block,
    // because the map methods require it. The bound is inferred from the field's key position.
    let source = source_for(
        r"
class Cache<T> {
  data: Map<T, string> = new Map();
  get(key: T): string | undefined { return this.data.get(key); }
  set(key: T, value: string): void { this.data.set(key, value); }
}
",
    );

    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static + SmeltJsKeyEq> Cache<T> {"), "the map-key generic carries SmeltJsKeyEq on the impl block:\n{source}");
}

#[test]
fn generic_class_without_map_key_has_no_key_eq_bound() {
    // A generic parameter never used as a map key keeps the minimal bound set. A class named `Box`
    // is renamed `Box_` so it does not shadow Rust's `Box`.
    let source = source_for(
        r"
class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
}
",
    );

    assert!(
        source.contains(
            "impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Box_<T> {"
        ),
        "the impl block keeps the minimal bound set:\n{source}"
    );
}

#[test]
fn generic_function_array_callback_preserves_type_param() {
    // A generic function filtering a `T[]` with an inline callback keeps its generics; the
    // callback element stays `&T` inside the generic scope.
    let source = source_for(
        r"
export function difference<T>(firstArr: readonly T[], secondArr: readonly T[]): T[] {
  const secondSet = new Set(secondArr);
  return firstArr.filter(item => !secondSet.has(item));
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig difference"]),
        r"fn difference<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(first_arr: SmeltList<T>, second_arr: SmeltList<T>) -> SmeltList<T>"
    );
    let body = rust_fn(&source, "difference");
    assert!(
        body.contains("move |closure_arg_0: &T,"),
        "the callback element stays `&T`:\n{body}"
    );
    assert!(
        body.contains("let second_set: SmeltJsSet<T> ="),
        "the captured set keeps its element type:\n{body}"
    );
}

#[test]
fn spread_call_erases_optional_union_callee() {
    // A callee captured into a closure whose static type is an optional union renders as
    // `Option<SmeltUnknown>`. The `iteratee == null` guard above the closure holds inside it, so
    // the capture reaches the runtime dispatch at its narrowed carrier.
    let source = source_for(
        r"
export function zipLike<T, R>(
  ...combine: Array<((...g: T[]) => R) | ArrayLike<T>>
): R[] {
  const iteratee = combine.pop();
  const groups = combine as Array<ArrayLike<T>>;
  if (iteratee == null) {
    return [];
  }
  return groups.map(group => iteratee(...(group as T[]))) as R[];
}
",
    );

    let body = rust_fn(&source, "zip_like");
    assert!(
        body.contains("let _smelt_tmp_2: SmeltUnknown = iteratee.clone().expect("),
        "the guarded callee must reach the runtime call without its Option: {body}"
    );
    assert!(
        body.contains("match smelt_value.clone() { SmeltUnknown::Function"),
        "{body}"
    );
}

#[test]
fn monomorphizes_generic_free_function_list_return() {
    // A generic function returning a composite built from its own parameter (`T[]`) is
    // monomorphized at a call site that pins `T`: the list passes straight through and the result
    // keeps the substituted type.
    let source = source_for(
        r"
export function pair<T>(xs: T[]): T[] {
  return xs;
}
export function usePair(): number[] {
  const data = [1, 2, 3];
  return pair(data);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pair", "fn use_pair"]),
        r"fn pair<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(xs: SmeltList<T>) -> SmeltList<T>

fn use_pair() -> SmeltList<f64> {
let data: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0, 3.0]; smelt_list_items }));
let _smelt_tmp_2: SmeltList<f64> = pair(data);
return _smelt_tmp_2;
}"
    );
}

#[test]
fn monomorphizes_generic_free_function_nested_return() {
    // The return substitution is recursive: `T[][]` with `T = f64` resolves to
    // `SmeltList<SmeltList<f64>>`.
    let source = source_for(
        r"
export function wrap<T>(xs: T[]): T[][] {
  return [xs];
}
export function useWrap(): number[][] {
  const data = [1, 2];
  return wrap(data);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn use_wrap"]),
        r"fn use_wrap() -> SmeltList<SmeltList<f64>> {
let data: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0, 2.0]; smelt_list_items }));
let _smelt_tmp_2: SmeltList<SmeltList<f64>> = wrap(data);
return _smelt_tmp_2;
}"
    );
}

#[test]
fn generic_free_function_demotes_on_erased_argument() {
    // An `unknown[]` argument binds `T` to the erased carrier. The definition stays generic but
    // this call site demotes: the argument is rebuilt element-wise and the result converted back.
    let source = source_for(
        r"
export function pair<T>(xs: T[]): T[] {
  return xs;
}
export function usePair(us: unknown[]): unknown[] {
  return pair(us);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pair", "fn use_pair"]),
        r"fn pair<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(xs: SmeltList<T>) -> SmeltList<T>

fn use_pair(us: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown> {
let _smelt_tmp_1: SmeltList<SmeltUnknown> = smelt_alias_erased_generic_list(pair(Into::<SmeltList<_>>::into(us.clone())).clone());
return _smelt_tmp_1;
}"
    );
}

#[test]
fn generic_free_function_demotes_on_conflicting_bindings() {
    // Two arguments pinning the same type parameter to different concrete types cannot both pass
    // through (E0308), so both are erased at the call.
    let source = source_for(
        r#"
export function both<T>(a: T[], b: T[]): T[] {
  return a;
}
export function useBoth(): number[] {
  const xs = [1];
  const ys = ["a"];
  return both(xs, ys);
}
"#,
    );

    let body = rust_fn(&source, "use_both");
    assert!(
        body.contains("SmeltUnknown::Number(smelt_value as f64)"),
        "{body}"
    );
    assert!(
        body.contains("SmeltUnknown::String(smelt_value.into())"),
        "{body}"
    );
    assert!(!body.contains("both(xs, ys)"), "{body}");
}

#[test]
fn generic_free_function_demotes_on_union_return() {
    // A union mentioning `T` erases to the tagged carrier and its member order is not a sound
    // correspondence, so nothing about this shape monomorphizes.
    let source = source_for(
        r"
export function maybe<T>(xs: T[]): T[] | string {
  return xs;
}
export function useMaybe(): number[] | string {
  const xs = [1];
  return maybe(xs);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig maybe", "fn use_maybe"]),
        r"fn maybe(xs: SmeltList<SmeltUnknown>) -> SmeltUnknown

fn use_maybe() -> SmeltUnion7 {
let xs: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0]; smelt_list_items }));
let _smelt_tmp_2: SmeltUnion7 = SmeltUnion7::from_smelt_unknown((maybe({ let smelt_l: SmeltList<_> = xs.clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|smelt_value| SmeltUnknown::Number(smelt_value as f64)).collect::<Vec<_>>()) })).into_smelt_unknown());
return _smelt_tmp_2;
}"
    );
}

#[test]
fn generic_free_function_demotes_when_argument_omitted() {
    // An omitted argument in a position mentioning a type parameter renders the erased default,
    // which would pin `T = SmeltUnknown` while the return claimed `SmeltList<f64>`; the site
    // demotes wholesale.
    let source = source_for(
        r"
export function padded<T>(xs: T[], fill?: T): T[] {
  return xs;
}
export function usePadded(): number[] {
  const xs = [1];
  return padded(xs);
}
",
    );

    let body = rust_fn(&source, "use_padded");
    assert!(body.contains(", None::<SmeltUnknown>)"), "{body}");
    assert!(
        body.contains("SmeltUnknown::Number(smelt_value as f64)"),
        "the site demotes wholesale:\n{body}"
    );
    assert!(!body.contains("padded(xs,"), "{body}");
}

#[test]
fn generic_method_list_return_stays_concrete() {
    // `all(): T[]` on a `Box<f64>` receiver evaluates to `SmeltList<f64>` because the receiver
    // pins the class argument, so no extraction is emitted.
    let source = source_for(
        r"
class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  all(): T[] { return [this.value]; }
}
function useBox(): number[] {
  const b = new Box<number>(1);
  return b.all();
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig all", "fn use_box"]),
        r"fn all(&self) -> SmeltList<T>

fn use_box() -> SmeltList<f64> {
let b: Box_<f64>;
let _smelt_tmp_1: Box_<f64> = Box_::new(1.0);
b = _smelt_tmp_1;
let _smelt_tmp_2: SmeltList<f64> = b.all();
return _smelt_tmp_2;
}"
    );
}

#[test]
fn callback_generic_uses_a_bounded_fn_param() {
    // A type parameter in a callback and a direct value parameter is a real Rust generic: a direct
    // required borrowed callback is monomorphized as `&F0` with `F0: Fn(&T) + ?Sized`, not
    // `Fn(SmeltUnknown)` or `&dyn Fn`. `?Sized` is load-bearing: dropping it breaks the
    // `&*rc_handle` argument and forwarding into another generic helper.
    let source = source_for(
        r"
export function takeWhile<T>(xs: T[], keep: (item: T) => boolean): T[] {
  return xs.filter(keep);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig take_while"]),
        r"fn take_while<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> bool + ?Sized>(xs: SmeltList<T>, keep: &F0) -> SmeltList<T>"
    );
}

#[test]
fn callback_generic_name_skips_a_colliding_source_type_param() {
    // Generated callback generic names must not collide with a source type parameter: a source
    // `<F0>` pushes the first generated name to `F1`.
    let source = source_for(
        r"
export function takeWhile<F0>(xs: F0[], keep: (item: F0) => boolean): F0[] {
  return xs.filter(keep);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig take_while"]),
        r"fn take_while<F0: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F1: Fn(&F0) -> bool + ?Sized>(xs: SmeltList<F0>, keep: &F1) -> SmeltList<F0>"
    );
}

#[test]
fn two_callback_params_get_distinct_generic_names() {
    // Two liftable callbacks take F0 and F1 in declaration order.
    let source = source_for(
        r"
export function both<T>(xs: T[], a: (item: T) => boolean, b: (item: T) => boolean): T[] {
  return xs.filter(a).filter(b);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig both"]),
        r"fn both<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> bool + ?Sized, F1: Fn(&T) -> bool + ?Sized>(xs: SmeltList<T>, a: &F0, b: &F1) -> SmeltList<T>"
    );
}

#[test]
fn callback_bound_renders_its_return_through_the_canonical_helper() {
    // The `F0` bound routes the return through the canonical function-return renderer, so a
    // `Future` return is the promise value `SmeltFuture<T>` substituted with the callee's `T`.
    let source = source_for(
        r"
export function mapAll<T>(xs: T[], make: (item: T) => Promise<T>): T[] {
  make(xs[0]);
  return xs;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig map_all"]),
        r"fn map_all<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> SmeltFuture<T> + ?Sized>(xs: SmeltList<T>, make: &F0) -> SmeltList<T>"
    );
}

#[test]
fn a_borrowed_callback_forwards_into_another_generic_helper() {
    // A caller hands its own `&F0` to a second generic helper as `&*cb`; `?Sized` on the callee is
    // what lets the caller's `?Sized` `F0` satisfy it (E0277 otherwise).
    let source = source_for(
        r"
export function inner<T>(xs: T[], cb: (item: T) => boolean): T[] {
  return xs.filter(cb);
}

export function outer<T>(xs: T[], cb: (item: T) => boolean): T[] {
  return inner(xs, cb);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig inner", "fn outer"]),
        r"fn inner<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> bool + ?Sized>(xs: SmeltList<T>, cb: &F0) -> SmeltList<T>

fn outer<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> bool + ?Sized>(xs: SmeltList<T>, cb: &F0) -> SmeltList<T> {
let _smelt_tmp_2: SmeltList<T> = inner(xs.clone(), &*cb);
return _smelt_tmp_2;
}"
    );
}

#[test]
fn callback_only_type_param_now_lifts() {
    // A type parameter reachable only through a callback lifts: the `F0: Fn() -> T + ?Sized` bound
    // is an inference source.
    let source = source_for(
        r"
export function attempt<T>(make: () => T): T[] {
  return [make()];
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig attempt"]),
        r"fn attempt<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn() -> T + ?Sized>(make: &F0) -> SmeltList<T>"
    );
}

#[test]
fn callback_only_type_param_lifts_in_the_union_by_shape() {
    // `T` is pinned from the arrays and `U` only through the mapper's return; both reach the
    // signature and the mapper is `&F0`.
    let source = source_for(
        r"
export function unionBy<T, U>(arr1: T[], arr2: T[], mapper: (item: T) => U): T[] {
  return arr1.concat(arr2);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig union_by"]),
        r"fn union_by<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, U: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> U + ?Sized>(arr1: SmeltList<T>, arr2: SmeltList<T>, mapper: &F0) -> SmeltList<T>"
    );
}

#[test]
fn callback_only_type_param_in_a_callback_parameter_position_lifts() {
    // A callback parameter position is as good an inference source as a callback return.
    let source = source_for(
        r"
export function feed<T>(cb: (item: T) => boolean): boolean {
  return true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig feed"]),
        r"fn feed<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> bool + ?Sized>(cb: &F0) -> bool"
    );
}

#[test]
fn callback_only_type_param_inside_a_union_still_demotes() {
    // A union erases to `SmeltUnknown`, so `T` never reaches the emitted `Fn` bound and there is
    // nothing to infer from.
    let source = source_for(
        r"
export function pick<T>(cb: (value: T | string) => boolean): boolean {
  return true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pick"]),
        r"fn pick(cb: &dyn Fn(&SmeltUnknown) -> bool) -> bool"
    );
}

#[test]
fn callback_only_type_param_in_a_nested_callback_still_demotes() {
    // The occurrence is not at the callback's own top level, and a nested function-value position
    // is invariant.
    let source = source_for(
        r"
export function higher<T>(cb: (inner: (value: T) => void) => void): boolean {
  return true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig higher"]),
        r"fn higher(cb: &dyn Fn(::std::rc::Rc<dyn Fn(&SmeltUnknown) -> ()>) -> ()) -> bool"
    );
}

#[test]
fn callback_only_type_param_in_an_optional_callback_still_demotes() {
    // An optional callback is `Option<Rc<dyn Fn(..)>>`, not an `F{n}`, so `T` has no inference
    // source. If this lifts, the gate and `callback_generic_params` disagree on eligibility and
    // the signature declares a `T` nothing can infer.
    let source = source_for(
        r"
export function maybe<T>(cb?: (value: T) => boolean): boolean {
  return true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig maybe"]),
        r"fn maybe(cb: Option<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>>) -> bool"
    );
}

#[test]
fn callback_only_type_param_in_an_owned_callback_still_demotes() {
    // An escaping callback is owned, so it lowers to `Rc<dyn Fn(..)>` rather than `&F{n}`.
    let source = source_for(
        r"
export function keep<T>(cb: (value: T) => boolean): (value: T) => boolean {
  return cb;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig keep"]),
        r"fn keep(cb: ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>) -> ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>"
    );
}

#[test]
fn callback_only_type_param_in_an_erased_rest_callback_still_demotes() {
    // `(...args: unknown[]) => T` is the erased-rest callback shape; its callers hand over an
    // erased callable, so `T` in its return has nothing to infer from and the function demotes.
    let source = source_for(
        r"
export function spread<T>(cb: (...args: unknown[]) => T): boolean {
  return true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig spread"]),
        r"fn spread(cb: &dyn Fn(SmeltList<SmeltUnknown>) -> SmeltUnknown) -> bool"
    );
}

#[test]
fn callback_only_type_param_survives_an_erased_callable_argument() {
    // An erased callback argument renders a definite adapter pinning the parameter to
    // `SmeltUnknown` at that site, while the definition stays generic for every other caller.
    let source = source_for(
        r"
export function attempt<T>(make: () => T): T[] {
  return [make()];
}
export function run(spy: unknown): unknown[] {
  return attempt(spy as () => unknown);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig attempt", "sig run"]),
        r"fn attempt<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn() -> T + ?Sized>(make: &F0) -> SmeltList<T>

fn run(spy: SmeltUnknown) -> SmeltList<SmeltUnknown>"
    );
    let body = rust_fn(&source, "run");
    assert!(
        body.contains("attempt(&mut {"),
        "the call hands an adapter to the generic definition:\n{body}"
    );
}

#[test]
fn callback_only_type_param_demotes_on_conflicting_call_site_evidence() {
    // Two callbacks demanding incompatible instantiations of a parameter with no other pinning
    // source: nothing can arbitrate, so the definition demotes.
    let source = source_for(
        r"
export function attemptTwo<T>(make: () => T, other: () => T): T[] {
  return [make(), other()];
}
export function run(): unknown[] {
  return attemptTwo(() => 1, () => 'a') as unknown[];
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig attempt_two"]),
        r"fn attempt_two(make: &dyn Fn() -> SmeltUnknown, other: &dyn Fn() -> SmeltUnknown) -> SmeltList<SmeltUnknown>"
    );
}

#[test]
fn call_site_adapter_annotates_its_return_at_a_pinned_callback_generic() {
    // The callee renders `&F0` with `F0: Fn(T) -> U`, so the closure return is an inference
    // variable; the adapter annotation makes it definite.
    let source = source_for(
        r"
export function unionBy<T, U>(arr1: T[], arr2: T[], mapper: (item: T) => U): T[] {
  return arr1.concat(arr2);
}
export function pinned(xs: number[], ys: number[]): number[] {
  return unionBy(xs, ys, (item: number) => item.toString());
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn pinned"]),
        r"fn pinned(xs: SmeltList<f64>, ys: SmeltList<f64>) -> SmeltList<f64> {
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: f64| {
let _smelt_tmp_1: JsString = JsString::from(smelt_number_to_string(closure_arg_0));
_smelt_tmp_1.clone()
});
let _smelt_tmp_3: SmeltList<f64> = union_by(xs.clone(), ys.clone(), &mut { let _smelt_adapted_callback = _smelt_tmp_2.clone(); move |smelt_arg0: &f64| -> JsString { (_smelt_adapted_callback)(smelt_arg0.clone()) } });
return _smelt_tmp_3;
}"
    );
}

#[test]
fn call_site_adapter_omits_its_return_annotation_at_an_unmonomorphized_callee() {
    // `sink` demotes (its body assigns the callback result into `unknown`), `outer` demotes with
    // it, and the omitted callback renders the erased default, never annotated with a caller's
    // unrelated `T`.
    let source = source_for(
        r"
export function sink<T>(x: number, cb: (v: number) => T): number {
  const box: unknown = cb(1);
  return 1;
}
export function outer<T>(items: T[]): number {
  return sink(1) + items.length;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig sink", "fn outer"]),
        r"fn sink(x: f64, cb: &dyn Fn(f64) -> SmeltUnknown) -> f64

fn outer(items: SmeltList<SmeltUnknown>) -> f64 {
let _smelt_tmp_2: f64;
let _smelt_tmp_3: f64;
let _smelt_tmp_1: f64 = sink(1.0, &|smelt_arg0: f64| SmeltUnknown::Null);
_smelt_tmp_2 = items.len() as f64;
_smelt_tmp_3 = _smelt_tmp_1 + _smelt_tmp_2;
return _smelt_tmp_3;
}"
    );
}

#[test]
fn lifted_caller_of_a_demoted_callee_demotes_too() {
    // `inner`'s body needs the erased carrier (a `Map` keyed by `T`), so it demotes; `outer`'s
    // `SmeltList<T>` could not be handed to `inner`'s `SmeltList<SmeltUnknown>`, so the fixed
    // point demotes it too.
    let source = source_for(
        r"
export function inner<T>(xs: T[]): T[] {
  const seen = new Map<T, T>();
  for (const item of xs) {
    seen.set(item, item);
  }
  return xs;
}

export function outer<T>(xs: T[]): T[] {
  return inner(xs);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig inner", "sig outer"]),
        r"fn inner(xs: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown>

fn outer(xs: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown>"
    );
}

#[test]
fn constrained_type_param_erases_alone_with_callback() {
    // `groupBy<T, K extends PropertyKey>` emits `T` generically while `K` erases: lifting is
    // decided per parameter.
    let source = source_for(
        r"
export function groupBy<T, K extends string>(xs: T[], key: (x: T) => K): K[] {
  return xs.map(key);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig group_by"]),
        r"fn group_by<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> SmeltUnknown + ?Sized>(xs: SmeltList<T>, key: &F0) -> SmeltList<SmeltUnknown>"
    );
}

#[test]
fn erased_callable_argument_keeps_the_definition_generic() {
    // When `T` is pinned by a direct value parameter, an erased callable in the callback position
    // is harmless: the definition stays generic and only the call site erases.
    let source = source_for(
        r"
export function each<T>(xs: T[], cb: (x: T) => void): T[] {
  return xs.filter(cb) as T[];
}
export function run(xs: unknown[], cb: unknown): unknown[] {
  return each(xs, cb as (x: unknown) => void);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig each"]),
        r"fn each<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T) -> () + ?Sized>(xs: SmeltList<T>, cb: &F0) -> SmeltList<T>"
    );
}

#[test]
fn defaulted_callback_inside_a_generic_body_renders_in_scope() {
    // A callback closure inside a generic body must render in that scope: an `Rc<dyn Fn(T, ..)>`
    // handle initialised with `move |arg0: SmeltUnknown, ..|` does not type-check.
    let source = source_for(
        r"
export function uniqWith<T>(xs: T[], same: (a: T, b: T) => boolean): T[] {
  const seen: T[] = [];
  for (const item of xs) {
    if (!seen.some((other) => same(item, other))) {
      seen.push(item);
    }
  }
  return seen;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig uniq_with"]),
        r"fn uniq_with<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(&T, &T) -> bool + ?Sized>(xs: SmeltList<T>, same: &F0) -> SmeltList<T>"
    );
    let body = rust_fn(&source, "uniq_with");
    assert!(
        body.contains("_smelt_tmp_8 = ::std::rc::Rc::new({")
            && body.contains("move |closure_arg_0: &T, _arg0: i64, _arg1: &SmeltList<T>| {"),
        "the handle and its closure agree in the body's scope:\n{body}"
    );
}

#[test]
fn concrete_return_generic_callee_passes_arguments_through() {
    // Passing an argument through is licensed by the call site pinning every type parameter, not
    // by the callee's return.
    let source = source_for(
        r"
export function isSubset<T>(a: T[], b: T[]): boolean {
  return a.length < b.length;
}
export function useSubset(): boolean {
  const xs = [1];
  const ys = [2];
  return isSubset(xs, ys);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn use_subset"]),
        r"fn use_subset() -> bool {
let xs: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![1.0]; smelt_list_items }));
let ys: SmeltList<f64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<f64> = vec![2.0]; smelt_list_items }));
let _smelt_tmp_4: bool = is_subset(xs, ys);
return _smelt_tmp_4;
}"
    );
}

#[test]
fn optional_callback_type_param_demotes() {
    // An optional callback lowers to `Option<Rc<dyn Fn(T) -> _>>`, and a call site omitting it
    // must name a concrete element type for the `None`; it names the erased one, so the function
    // keeps the erased representation.
    let source = source_for(
        r"
export function pick<T>(x: T, cb: (v: T) => boolean = () => true): T {
  return cb(x) ? x : x;
}
export function useDefault(): number {
  return pick(1);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pick"]),
        r"fn pick(x: SmeltUnknown, cb: Option<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>>) -> SmeltUnknown"
    );
}

#[test]
fn owned_callback_type_param_demotes() {
    // A callback classified as escaping lowers to an owned `Rc<dyn Fn(..)>`. The borrowed-callback
    // ladder declines that position, so a generic signature would meet an erased argument.
    let source = source_for(
        r"
export function keep<T>(x: T, cb: (v: T) => boolean): (v: T) => boolean {
  return cb;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig keep"]),
        r"fn keep(x: SmeltUnknown, cb: ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>) -> ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>"
    );
}

#[test]
fn callback_nested_in_a_container_demotes() {
    // A function type nested in a container is not a direct callback parameter; no adapter walks
    // into the container.
    let source = source_for(
        r"
export function pick<T>(xs: T[], cbs: ((v: T) => boolean)[]): T[] {
  return xs.filter(cbs[0]);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pick"]),
        r"fn pick(xs: SmeltList<SmeltUnknown>, cbs: SmeltList<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>>) -> SmeltList<SmeltUnknown>"
    );
}

#[test]
fn rest_callback_type_param_demotes() {
    // A packed rest callback parameter is an erased sequence of callables, with no single declared
    // callback type to substitute against.
    let source = source_for(
        r"
export function pick<T>(xs: T[], ...cbs: ((v: T) => boolean)[]): T[] {
  return xs.filter(cbs[0]);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig pick"]),
        r"fn pick(xs: SmeltList<SmeltUnknown>, cbs: SmeltList<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>>) -> SmeltList<SmeltUnknown>"
    );
}

#[test]
fn higher_order_callback_type_param_demotes() {
    // A `T` inside a further nested function type sits behind an owned `Rc<dyn Fn>` the adapters
    // do not descend into.
    let source = source_for(
        r"
export function apply<T>(x: T, cb: (f: (v: T) => boolean) => boolean): boolean {
  return cb((v: T) => true);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig apply"]),
        r"fn apply(x: SmeltUnknown, cb: &dyn Fn(::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool>) -> bool) -> bool"
    );
}

#[test]
fn callback_in_the_return_type_still_lifts() {
    // The demotion rule scopes only parameter positions; a callback in the return is produced by
    // the callee's own body and stays generic.
    let source = source_for(
        r"
export function make<T>(x: T): (v: T) => boolean {
  return (v: T) => true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig make"]),
        r"fn make<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(x: T) -> ::std::rc::Rc<dyn Fn(&T) -> bool>"
    );
}

#[test]
fn an_erased_union_return_demotes_the_parameters_with_it() {
    // A signature's parameters and return are decided together: a union return mentioning `T`
    // renders `SmeltUnknown`, so the parameters and body land on the erased ABI with it.
    let source = source_for(
        r"
export function run<T>(items: T[], cb: (v: number) => number): T | number {
  const touched: number = cb(1);
  if (touched < 0) {
    return items[0];
  }
  const out: T = items[0];
  return out;
}
export function use0(): number {
  const items0: number[] = [1, 2, 3];
  return run(items0, (v: number) => v + 1);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn run"]),
        r"fn run(items: SmeltList<SmeltUnknown>, cb: &dyn Fn(f64) -> f64) -> SmeltUnknown {
let out: SmeltUnknown;
let touched: f64 = cb(1.0);
let _smelt_tmp_5: bool = touched < 0.0;
if _smelt_tmp_5 {
return items.borrow().get({ let smelt_normalized = 0.0 as i64; usize::try_from(smelt_normalized).unwrap_or(usize::MAX) }).cloned().map(|smelt_value| (smelt_value).into_smelt_unknown()).unwrap_or(SmeltUnknown::Undefined);
} else {
out = items.borrow().get({ let smelt_normalized = 0.0 as i64; usize::try_from(smelt_normalized).unwrap_or(usize::MAX) }).cloned().unwrap_or_else(|| SmeltUnknown::Undefined);
return out;
}
}"
    );
}

#[test]
fn a_union_return_without_a_type_parameter_keeps_its_generics() {
    // A union return mentioning no type parameter never erases a `T`, so the parameters keep their
    // monomorphized spelling.
    let source = source_for(
        r"
export function count<T>(items: T[]): number | string {
  return items.length;
}
export function use0(): number {
  const items0: number[] = [1, 2, 3];
  const c: number | string = count(items0);
  return 1;
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig count"]),
        r"fn count<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(items: SmeltList<T>) -> SmeltUnion4"
    );
}
#[test]
fn borrowed_callback_packed_into_container_is_owned() {
    // A callback a caller passes into a container-shaped callee parameter enters as an owned
    // `Rc<dyn Fn..>`: containers own their function elements and carry no lifetime.
    let source = source_for(
        r"
export function run<T>(items: T[], cbs: ((v: T) => T)[]): T {
  const out: T = cbs[0](items[0]);
  return out;
}
export function use0(cbp0: (v: number) => number): number {
  const items0: number[] = [1, 2, 3];
  return run(items0, [cbp0]);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig use0"]),
        r"fn use0(cbp0: ::std::rc::Rc<dyn Fn(f64) -> f64>) -> Result<f64, Box<dyn std::error::Error>>"
    );
}

#[test]
fn borrowed_callback_packed_into_a_rest_argument_is_owned() {
    // A rest parameter packs into the same list temporary, so the escape rule follows the
    // destination's representation.
    let source = source_for(
        r"
export function run<T>(items: T[], ...cbs: ((v: T) => T)[]): T {
  const out: T = cbs[0](items[0]);
  return out;
}
export function use0(cbp0: (v: number) => number): number {
  const items0: number[] = [1, 2, 3];
  return run(items0, cbp0);
}
",
    );

    assert_eq!(
        rust_items(&source, &["sig use0"]),
        r"fn use0(cbp0: ::std::rc::Rc<dyn Fn(f64) -> f64>) -> Result<f64, Box<dyn std::error::Error>>"
    );
}

#[test]
fn generic_class_default_impl_carries_the_generated_bound_set() {
    // A generic reference class's inner-record `Default` declares the full generated bound set,
    // because a delegating field default resolves through another generated class's `Default`.
    let source = source_for(
        r"
class Leaf<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  set(value: T): void { this.value = value; }
}
class Holder<T> {
  leaf: Leaf<T>;
  constructor(leaf: Leaf<T>) { this.leaf = leaf; }
  swap(leaf: Leaf<T>): void { this.leaf = leaf; }
}
",
    );

    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Default for HolderInner<T> where Leaf<T>: Default {"), "{source}");
}

#[test]
fn a_callable_field_is_never_a_delegation_of_its_own_type() {
    // A callable field's default is a constructed no-op closure, so the field type is not what
    // `default()` resolves through; nothing implements `Rc<dyn Fn(..)>: Default`.
    let source = source_for(
        r"
class Chain<T> {
  value: T;
  rewind: () => Chain<T>;
  constructor(value: T) {
    this.value = value;
    this.rewind = () => { this.value = value; return this; };
  }
}
",
    );

    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Default for ChainInner<T> where T: Default {"), "the impl header has no where clause over the callable field:\n{source}");
}

#[test]
fn a_generic_interface_slot_is_called_with_its_declared_abi() {
    // `Store<T>` declares `add: (label, value: T) => void`, so its slot is `Rc<dyn Fn(String,
    // &T)>`: a bare type parameter takes the by-reference ABI, and a call through `Store<[string,
    // number]>` passes the substituted tuple by reference.
    let source = source_for(
        r"
interface Store<T> {
  add: (label: string, value: T) => void;
  size: () => number;
}
export function fill(store: Store<[string, number]>): number {
  store.add('first', ['x', 1]);
  return store.size();
}
",
    );

    let body = rust_fn(&source, "fill");
    assert!(
        body.contains("(store.add.clone())(JsString::from(\"first\"), &("),
        "the declared `&T` slot is called by reference:\n{body}"
    );
    assert!(
        rust_items(&source, &["sig fill"]).starts_with("fn fill(store: Store<(JsString, f64)>)"),
        "{source}"
    );
}

#[test]
fn a_generic_interface_slot_is_built_at_its_declared_abi() {
    // The callable a projection rebuilds for `Sink<[string, number]>.add` is bridged to the
    // declaration's `&T` ABI by a typed adapter, with no erasure.
    let source = source_for(
        r"
interface Sink<T> {
  add: (label: string, value: T) => void;
  size: () => number;
}
export function fill(raw: unknown): number {
  const sink = raw as Sink<[string, number]>;
  sink.add('first', ['x', 1]);
  return sink.size();
}
",
    );

    let body = rust_declaration(&source, "fn __smelt_from_unknown_record_0");
    assert!(
        body.contains(
            "smelt_slot_adapted: ::std::rc::Rc<dyn Fn(JsString, &(JsString, f64)) -> ()>"
        ),
        "the rebuilt slot must be adapted to the declared `&T` ABI: {body}"
    );
}

#[test]
fn a_constructor_parameter_mentioning_a_class_parameter_is_taken_at_its_argument() {
    // MIR hands the call site the declared constructor parameter `Cell<A>` with nothing
    // substituted; the argument's own type is the evidence, so it is taken at `Cell<f64>` and no
    // structural adapter rebuilds it with an erased field.
    let source = source_for(
        r"
class Cell<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
class Pair<A> {
  left: Cell<A>;
  constructor(left: Cell<A>) {
    this.left = left;
  }
}
export function build(): number {
  return new Pair<number>(new Cell<number>(7)).left.value;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn build"]),
        r"fn build() -> f64 {
let _smelt_tmp_2: Cell<f64>;
let _smelt_tmp_0: Cell<f64> = Cell::new(7.0);
let _smelt_tmp_1: Pair<f64> = Pair::new(_smelt_tmp_0);
_smelt_tmp_2 = _smelt_tmp_1.left.clone();
return _smelt_tmp_2.value;
}"
    );
}

// --- Type-parameter elision (see `crate::generic_elision`) ------------------
//
// A generic class's Rust arity is the set of parameters its emitted Rust
// actually spells, computed as a least fixpoint. These pin the rule on the
// shapes it has to get right; each one states what the emitted arity must be
// and why.

#[test]
fn a_type_parameter_no_field_carries_is_not_declared_in_rust() {
    // `Tag` reaches nothing: the struct is not generic over it and has no phantom field.
    let source = source_for(
        r"
class Tagged<Tag> {
  label: string;
  constructor(label: string) { this.label = label; }
  read(): string { return this.label; }
}
export function useTagged(): string { return new Tagged<number>('x').read(); }
",
    );

    assert_eq!(
        rust_items(&source, &["struct Tagged"]),
        r"struct Tagged {
label: JsString,
}"
    );
}

#[test]
fn a_type_parameter_a_field_carries_stays_declared() {
    // `T` is the type of stored data, so it stays declared and the instantiation stays concrete.
    let source = source_for(
        r"
class Cell<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
}
export function useCell(): string { return new Cell<string>('hi').get(); }
",
    );

    assert_eq!(
        rust_items(&source, &["struct Cell<T>", "fn use_cell"]),
        r#"struct Cell<T> {
value: T,
_smelt_phantom: ::std::marker::PhantomData<(T)>,
}

fn use_cell() -> JsString {
let _smelt_tmp_0: Cell<JsString> = Cell::new(JsString::from("hi"));
let _smelt_tmp_1: JsString = _smelt_tmp_0.get();
return _smelt_tmp_1;
}"#
    );
}

#[test]
fn a_parameter_reached_only_through_its_own_class_is_not_declared() {
    // `Env` appears only in a field naming the class itself at `Env`'s own position, so the least
    // fixpoint never forces it.
    let source = source_for(
        r"
class Session<Env> {
  name: string;
  next: ((session: Session<Env>) => string) | null;
  constructor(name: string) { this.name = name; this.next = null; }
  label(): string { return this.name; }
}
export function useSession(): string { return new Session<number>('s').label(); }
",
    );

    assert_eq!(
        rust_items(&source, &["struct Session"]),
        r"struct Session {
name: JsString,
next: Option<::std::rc::Rc<dyn Fn(Session) -> JsString>>,
}"
    );
}

#[test]
fn a_parameter_reaching_another_class_follows_that_class_answer() {
    // `Carried` reaches a declared `Cell` position and survives; `Dropped` reaches only `Tagged`,
    // which is not declared, and does not.
    let source = source_for(
        r"
class Tagged<Tag> {
  label: string;
  constructor(label: string) { this.label = label; }
}
class Cell<T> {
  value: T;
  constructor(value: T) { this.value = value; }
}
class Pair<Carried, Dropped> {
  left: Cell<Carried>;
  right: Tagged<Dropped>;
  constructor(left: Cell<Carried>, right: Tagged<Dropped>) {
    this.left = left;
    this.right = right;
  }
}
export function usePair(): number {
  const pair = new Pair<number, string>(new Cell<number>(1), new Tagged<string>('t'));
  return pair.left.value;
}
",
    );

    assert_eq!(
        rust_items(&source, &["struct Pair<Carried>"]),
        r"struct Pair<Carried> {
left: Cell<Carried>,
right: Tagged,
_smelt_phantom: ::std::marker::PhantomData<(Carried)>,
}"
    );
}

#[test]
fn a_parameter_a_method_signature_spells_stays_declared() {
    // `T` is never stored, but a method parameter names it and the emitted `impl` has to spell it.
    let source = source_for(
        r"
class Sink<T> {
  count: number;
  constructor() { this.count = 0; }
  accept(value: T): number { this.count = this.count + 1; return this.count; }
}
export function useSink(): number {
  const sink = new Sink<string>();
  return sink.accept('a');
}
",
    );

    assert_eq!(
        rust_items(&source, &["struct Sink<T>"]),
        r"struct Sink<T>(::std::rc::Rc<::std::cell::RefCell<SinkInner<T>>>);"
    );
}

/// A union only an inherited member copy spells has to be declared.
///
/// `run` is emitted into `YearParser` at `T = f64`, and nothing else in the
/// program names that instantiation. Declarations come from the type table,
/// ahead of every body, so `inherited_types` must put the substituted union
/// there; without it the copy names an enum nothing declares (E0412).
#[test]
fn a_union_only_an_inherited_copy_spells_is_declared() {
    let source = source_for(
        r#"
type ParseResult<T> = { value: T; rest: string } | null;
class ValueSetter<T> {
  value: T;
  priority: number;
  constructor(value: T, priority: number) { this.value = value; this.priority = priority; }
}
abstract class Parser<T> {
  abstract priority: number;
  abstract parse(text: string): ParseResult<T>;
  run(text: string): { setter: ValueSetter<T>; rest: string } | null {
    const result = this.parse(text);
    if (!result) { return null; }
    return { setter: new ValueSetter(result.value, this.priority), rest: result.rest };
  }
}
class YearParser extends Parser<number> {
  priority = 130;
  parse(text: string): ParseResult<number> { return { value: text.length, rest: "" }; }
}
console.log(new YearParser().priority);
"#,
    );
    let names: std::collections::BTreeSet<&str> = source
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|word| word.starts_with("SmeltUnion"))
        .collect();
    assert!(
        !names.is_empty(),
        "the program should need a union: {source}"
    );
    for name in names {
        assert!(
            source.contains(&format!("enum {name}")),
            "{name} is not declared: {source}"
        );
    }
}
