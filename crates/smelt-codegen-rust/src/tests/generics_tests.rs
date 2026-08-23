//! Codegen regression tests for generic classes and interfaces (issue #99).
//!
//! These assert that generic class/interface declarations lower to real Rust
//! generics (`struct Container<T>`, `impl<T: ...> Container<T>`) and that use
//! sites (`new Container<number>(...)`, `b.get()`) keep the instantiation
//! concrete instead of erasing the generic arguments to `SmeltUnknown`.

use super::*;

#[test]
fn emits_generic_class_struct_and_impl() {
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

    // `Container` mutates `this.value` in `set`, so it is lifted to a reference
    // class: a handle newtype over `Rc<RefCell<ContainerInner<T>>>` whose inner
    // record carries the generic parameter and a PhantomData over it.
    assert!(source.contains(
        "struct Container<T>(::std::rc::Rc<::std::cell::RefCell<ContainerInner<T>>>);"
    ));
    assert!(source.contains("struct ContainerInner<T>"));
    assert!(source.contains("value: T,"));
    assert!(source.contains("_smelt_phantom: ::std::marker::PhantomData<(T)>"));
    // Identity `Clone` shares the cell (`Rc::clone`), never forking state.
    assert!(source.contains("Container(::std::rc::Rc::clone(&self.0))"));
    // Inherent impl block declares bounded generics; every method takes `&self`
    // uniformly (interior mutability), so `set` is `&self`, not `&mut self`.
    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Container<T>"));
    assert!(source.contains("fn get(&self) -> T"));
    assert!(source.contains("fn set(&self, value: T) -> ()"));
}

#[test]
fn instantiates_generic_class_with_concrete_arguments() {
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

    // The instantiation is `Container<f64>`, not an erased `SmeltUnknown`.
    assert!(source.contains("Container<f64>"));
    // Constructor and method arguments pass the concrete value through so Rust
    // monomorphizes; they are NOT wrapped in `SmeltUnknown::Number(..)`.
    assert!(source.contains("Container::new(3.0)"));
    assert!(source.contains("b.set(5.0)"));
    // The method result is typed with the concrete instantiation and needs no
    // `SmeltUnknown` extraction match.
    assert!(source.contains("let _smelt_tmp_3: f64 = b.get();"));
    assert!(!source.contains("Container::new(SmeltUnknown"));
}

#[test]
fn emits_two_parameter_generic_class() {
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

    assert!(source.contains("struct Pair<A, B>"));
    assert!(source.contains("impl<A: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, B: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Pair<A, B>"));
    // Both concrete arguments are passed through; the receiver is `Pair<f64, String>`.
    assert!(source.contains("Pair<f64, String>"));
    assert!(source.contains(r#"Pair::new(1.0, "x".to_owned())"#));
    // `getSecond(): B` on `Pair<f64, String>` returns `String`, not `SmeltUnknown`.
    assert!(source.contains("let _smelt_tmp_2: String = p.get_second();"));
}

#[test]
fn emits_generic_free_function_with_real_rust_generics() {
    // Issue #99: a generic free function lowers to real Rust generics rather
    // than erasing `T` to `SmeltUnknown`.
    let source = source_for(
        r"
export function identity<T>(x: T): T {
  return x;
}
",
    );

    // The signature carries a bounded generic parameter, and `T` appears in
    // both the parameter and return positions.
    assert!(source.contains(
        "fn identity<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(x: T) -> T"
    ));
    // `T` is NOT erased to the runtime carrier at either position.
    assert!(!source.contains("fn identity(x: SmeltUnknown) -> SmeltUnknown"));
}

#[test]
fn monomorphizes_generic_free_function_call_site() {
    // Issue #99: calling a generic free function with a concrete argument passes
    // the value through so Rust monomorphizes the call; the result is typed with
    // the concrete instantiation and needs no `SmeltUnknown` extraction.
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

    // The argument is passed through concretely (`3.0`), not wrapped in
    // `SmeltUnknown::Number(..)`, so Rust infers `identity::<f64>`.
    assert!(source.contains("identity(3.0)"));
    assert!(!source.contains("identity(SmeltUnknown"));
}

#[test]
fn emits_two_parameter_generic_free_function() {
    // Issue #99: multiple type parameters each render as their own Rust generic
    // in the parameter list and return position.
    let source = source_for(
        r"
export function pair<A, B>(first: A, second: B): B {
  return second;
}
",
    );

    assert!(source.contains(
        "fn pair<A: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, B: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(first: A, second: B) -> B"
    ));
}

#[test]
fn emits_generic_free_function_over_list_parameter() {
    // Issue #99: a type parameter nested inside a `T[]` parameter still renders
    // the parameter's generic shape (`SmeltList<T>`) instead of erasing it.
    let source = source_for(
        r"
export function first<T>(xs: T[]): T {
  return xs[0];
}
",
    );

    assert!(source.contains(
        "fn first<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(xs: SmeltList<T>) -> T"
    ));
}

#[test]
fn erases_generic_free_function_with_bounded_type_param() {
    // Issue #99 (deferred slice): a BOUNDED type parameter still needs method
    // dispatch through the erased boundary, so the function falls back to full
    // erasure rather than emitting an unusable generic.
    let source = source_for(
        r"
export function invalid<D extends Date>(value: D): boolean {
  return isNaN(value.getTime());
}
",
    );

    // No real generic parameter is declared; the parameter is erased.
    assert!(!source.contains("fn invalid<D"));
    assert!(source.contains("fn invalid(value: SmeltUnknown) -> bool"));
}

#[test]
fn erases_generic_free_function_when_body_inspects_type_param() {
    // Issue #99 (safety fallback): a body that inspects, compares, or erases a
    // `T`-typed value cannot keep real generics (the operations are only defined
    // on the erased carrier), so the whole function falls back to erasure. Here
    // the `===` comparison forces the erased path.
    let source = source_for(
        r"
export function same<T>(a: T, b: T): boolean {
  return a === b;
}
",
    );

    // The signature declares no real generic parameter; both parameters erase.
    assert!(!source.contains("fn same<T"));
    assert!(source.contains("fn same(a: SmeltUnknown, b: SmeltUnknown) -> bool"));
}

#[test]
fn erases_generic_free_function_called_with_erased_argument() {
    // Issue #99 (safety fallback): when a generic free function is invoked with
    // an already-erased argument, Rust cannot infer its type parameter at the
    // call site (`E0283`). The crate-wide decision demotes such a function to the
    // erased signature so the argument binds directly.
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

    // `identity` is called with an erased `unknown`, so it is emitted erased.
    assert!(source.contains("fn identity(x: SmeltUnknown) -> SmeltUnknown"));
    assert!(!source.contains("fn identity<T"));
}

#[test]
fn emits_generic_interface_type_params() {
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

    // The interface storage keeps its generic parameters and instantiates
    // concretely at the return position.
    assert!(source.contains("struct Outcome<T, E>"));
    assert!(source.contains("-> Outcome<f64, String>"));
}

#[test]
fn generic_class_map_key_gains_key_eq_bound() {
    // A class whose generic parameter is used as a `Map` key must carry the
    // `SmeltJsKeyEq` bound on its impl block, because `SmeltJsMap`'s methods
    // (`get`, `set`, `has`, ...) require the key type to implement it. Without
    // this bound the generated method calls fail with E0599/E0277 ("trait
    // bounds were not satisfied"). The bound is inferred generally from the
    // field's key position, not special-cased per class.
    let source = source_for(
        r"
class Cache<T> {
  data: Map<T, string> = new Map();
  get(key: T): string | undefined { return this.data.get(key); }
  set(key: T, value: string): void { this.data.set(key, value); }
}
",
    );

    assert!(
        source.contains("SmeltJsKeyEq"),
        "impl block should carry the SmeltJsKeyEq bound on the map-key generic"
    );
    // The bound is added on top of the standard class-generic bounds.
    assert!(source.contains(
        "Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static + SmeltJsKeyEq"
    ));
}

#[test]
fn generic_class_without_map_key_has_no_key_eq_bound() {
    // A generic parameter that is never used as a map key must NOT gain the
    // `SmeltJsKeyEq` bound, so unrelated generic classes keep the minimal
    // bound set.
    let source = source_for(
        r"
class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
}
",
    );

    // The runtime prelude always defines the `SmeltJsKeyEq` trait, so only the
    // class impl block is checked: its generic bound must not include it.
    assert!(source.contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Box<T>"));
}

#[test]
fn generic_function_array_callback_preserves_type_param() {
    // A generic free function whose body filters a `T[]` with an inline callback
    // must keep its generics: the callback closure is emitted inside the generic
    // function, so its `T`-typed parameter stays `T` rather than erasing to
    // `SmeltUnknown`. Previously the callback rendered `closure_arg_0:
    // SmeltUnknown`, which tripped the body-cleanliness trial and forced the
    // whole `difference<T>` signature to erase (regression for the es-toolkit
    // `difference`/`without`/`isSubset` E0308 family).
    let source = source_for(
        r"
export function difference<T>(firstArr: readonly T[], secondArr: readonly T[]): T[] {
  const secondSet = new Set(secondArr);
  return firstArr.filter(item => !secondSet.has(item));
}
",
    );

    // The signature keeps real generics over `T`.
    assert!(source.contains(
        "fn difference<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(first_arr: SmeltList<T>, second_arr: SmeltList<T>) -> SmeltList<T>"
    ));
    // The inline `.filter` callback keeps the element parameter typed as `T`.
    assert!(source.contains("closure_arg_0: T,"));
    // The captured set keeps its generic element type.
    assert!(source.contains("SmeltJsSet<T>"));
    // The erased carrier does not leak into the callback element position.
    assert!(!source.contains("closure_arg_0: SmeltUnknown"));
}

#[test]
fn spread_call_erases_optional_union_callee() {
    // A callee captured into a closure whose static type is an
    // `Optional<union>` (here `iteratee` popped off a rest array, then narrowed)
    // renders as `Option<SmeltUnknown>`. The runtime dynamic-dispatch snippet
    // matches the callee over `SmeltUnknown` discriminants, so the callee must
    // be erased to the bare runtime carrier before the match. Previously the
    // raw `Option<SmeltUnknown>` was fed into the match, producing an
    // `Option<SmeltUnknown>` vs `SmeltUnknown` mismatch (es-toolkit `zipWith`
    // E0308 family).
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
    let dispatch = source
        .lines()
        .find(|line| line.contains("let smelt_function_value = iteratee"))
        .expect("expected a dynamic-dispatch snippet over the optional callee");
    // The callee is erased to the runtime carrier before the discriminant match
    // instead of a bare `Option` being matched directly.
    assert!(
        dispatch.contains("iteratee.clone().clone().map_or("),
        "optional callee must be erased before the runtime match: {dispatch}"
    );
    assert!(dispatch.contains("match smelt_function_value { SmeltUnknown::Function"));
}

#[test]
fn monomorphizes_generic_free_function_list_return() {
    // Plan 197 Increment 0b: a generic free function whose return is a
    // *composite* built from its own type parameter (`T[]`, not a bare `T`) is
    // monomorphized at a call site that pins `T` concretely. Before this the
    // call-site decision accepted only a bare `TypeParam` return, so the whole
    // `T[] -> T[]` family (es-toolkit's `difference`/`without`/`uniq`) erased
    // its argument element-by-element into `SmeltList<SmeltUnknown>` and
    // un-erased every returned element again.
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

    // The definition is unchanged: it was already emitted with real generics.
    assert!(source.contains(
        "fn pair<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(xs: SmeltList<T>) -> SmeltList<T>"
    ));
    // The call site passes the concrete list straight through and takes the
    // result at the substituted return type: no erasure on either side.
    assert!(source.contains("let _smelt_tmp_2: SmeltList<f64> = pair(data);"));
}

#[test]
fn monomorphizes_generic_free_function_nested_return() {
    // The return substitution is recursive, not one level deep: `T[][]` with
    // `T = f64` resolves to `SmeltList<SmeltList<f64>>`.
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

    assert!(source.contains("let _smelt_tmp_2: SmeltList<SmeltList<f64>> = wrap(data);"));
}

#[test]
fn generic_free_function_demotes_on_erased_argument() {
    // Fail-closed guard for E0283. An `unknown[]` argument binds `T` to the
    // erased carrier, which is evidence the matcher reports as `Erased`, not
    // `Concrete`. The call site must keep today's coercion path rather than
    // claim a monomorphization rustc cannot reproduce.
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

    // The definition stays generic (the crate-wide gate only demotes on a bare
    // type-parameter position), but this *call site* demotes: the argument is
    // still rebuilt element-wise and the result is still converted back.
    assert!(source.contains("fn pair<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(xs: SmeltList<T>)"));
    assert!(!source.contains("= pair(us"));
    assert!(source.contains("into_smelt_unknown()"));
}

#[test]
fn generic_free_function_demotes_on_conflicting_bindings() {
    // Two arguments pinning the same type parameter to different concrete types
    // is `Conflict`, which `all_concrete` rejects: passing either through would
    // give rustc `?T = f64` and `?T = String` at once (E0308).
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

    // Neither argument passes through: both are erased, as before.
    assert!(!source.contains("both(xs, ys)"));
    assert!(source.contains("SmeltUnknown::Number(value as f64)"));
    assert!(source.contains("SmeltUnknown::String(value)"));
}

#[test]
fn generic_free_function_demotes_on_union_return() {
    // Unions erase to the tagged carrier in emitted Rust and their member order
    // is not a sound correspondence, so the substitution refuses to walk one at
    // any depth. Here the union return also demotes the whole callee upstream,
    // which is the stronger statement: nothing about this shape monomorphizes.
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

    assert!(source.contains("fn maybe(xs: SmeltList<SmeltUnknown>) -> SmeltUnknown"));
    assert!(!source.contains("maybe(xs)"));
}

#[test]
fn generic_free_function_demotes_when_argument_omitted() {
    // An omitted argument in a position that mentions a type parameter is
    // rendered by the trailing default loop, which emits the *erased* default
    // (`None::<SmeltUnknown>`) and would pin `T = SmeltUnknown` while the return
    // claimed `SmeltList<f64>`. The site must demote wholesale instead.
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

    assert!(source.contains("None::<SmeltUnknown>"));
    assert!(!source.contains("padded(xs,"));
}

#[test]
fn generic_method_list_return_stays_concrete() {
    // The class-method half of the same widening: `all(): T[]` on a `Box<f64>`
    // receiver really evaluates to `SmeltList<f64>`, because the receiver pins
    // the class type argument. Emitting an extraction against that
    // already-concrete value is what the bare-`TypeParam`-only predicate forced.
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

    assert!(source.contains("fn all(&self) -> SmeltList<T>"));
    assert!(source.contains("let _smelt_tmp_2: SmeltList<f64> = b.all();"));
}

#[test]
fn callback_generic_function_emits_generics() {
    // INVARIANT for Increment 1: a type parameter that appears inside a callback
    // parameter *and* in a direct value parameter is a real Rust generic. Both
    // halves of the callback type render in the callee's own lexical scope, so
    // the signature says `Fn(T)` and not `Fn(SmeltUnknown)`.
    //
    // This test replaces `callback_generic_function_still_demotes`, which pinned
    // the deleted gate. Its flip is the marker that the gate came down.
    let source = source_for(
        r"
export function takeWhile<T>(xs: T[], keep: (item: T) => boolean): T[] {
  return xs.filter(keep);
}
",
    );

    assert!(source.contains(
        "fn take_while<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T) -> bool + ?Sized>(xs: SmeltList<T>, keep: &F0) -> SmeltList<T>"
    ));
}

#[test]
fn callback_generic_uses_a_bounded_fn_param() {
    // INVARIANT for Increment 2 (Option B). A direct required borrowed callback
    // parameter of a generic free function is monomorphized as `&F0` with an
    // `F0: Fn(..) + ?Sized` bound, not dispatched dynamically through
    // `&dyn Fn(..)`.
    //
    // This test replaces `callback_generic_stays_dyn_not_bounded`, which guarded
    // the pre-swap representation; its inversion is the marker that the swap
    // landed. `?Sized` is asserted explicitly because dropping it is the one
    // edit that still compiles this shape while breaking both the `&*rc_handle`
    // argument and callback forwarding into another generic helper.
    let source = source_for(
        r"
export function takeWhile<T>(xs: T[], keep: (item: T) => boolean): T[] {
  return xs.filter(keep);
}
",
    );

    assert!(source.contains("F0: Fn(T) -> bool + ?Sized"));
    assert!(source.contains("keep: &F0"));
    assert!(!source.contains("keep: &dyn Fn"));
}

#[test]
fn callback_generic_name_skips_a_colliding_source_type_param() {
    // The generated callback generic names are deterministic by declaration
    // index and must not collide with a sanitized SOURCE type-parameter
    // identifier. A source `<F0>` therefore pushes the first generated name to
    // `F1`; without the skip the signature declares `F0` twice (E0403).
    let source = source_for(
        r"
export function takeWhile<F0>(xs: F0[], keep: (item: F0) => boolean): F0[] {
  return xs.filter(keep);
}
",
    );

    assert!(source.contains("F1: Fn(F0) -> bool + ?Sized"));
    assert!(source.contains("keep: &F1"));
}

#[test]
fn two_callback_params_get_distinct_generic_names() {
    // Two liftable callbacks on one generic function take F0 and F1 in
    // declaration order. Neither compat corpus contains this shape, so nothing
    // else in the test surface pins the naming loop.
    let source = source_for(
        r"
export function both<T>(xs: T[], a: (item: T) => boolean, b: (item: T) => boolean): T[] {
  return xs.filter(a).filter(b);
}
",
    );

    assert!(source.contains("F0: Fn(T) -> bool + ?Sized, F1: Fn(T) -> bool + ?Sized"));
    assert!(source.contains("a: &F0"));
    assert!(source.contains("b: &F1"));
}

#[test]
fn callback_bound_renders_its_return_through_the_canonical_helper() {
    // The `F0` bound must be produced by `callback_fn_trait_text`, which routes
    // the return through `function_value_return_type_text` — the single
    // canonical renderer for the two return-position refinements a
    // `Type::Function` carries. Re-formatting the return inside the bound is how
    // the bound and the `&F0` parameter would drift apart, and how a throwing
    // callback's `Result` would be dropped at the parameter boundary.
    //
    // A `Future` return is the reachable half of that composition: it renders as
    // the promise value `SmeltFuture<T>`, substituted with the callee's own `T`.
    // The `may_throw` half (`Fn(T) -> Result<T, Box<dyn std::error::Error>>`)
    // shares the exact same code path but cannot be produced from TypeScript
    // today — a declared callback *parameter* type always lowers with
    // `may_throw == false` (see `part_7_tests`'s borrowed-callback ABI cases),
    // which is also why neither compat corpus contains a `&dyn Fn(..) -> Result`
    // parameter to regress.
    let source = source_for(
        r"
export function mapAll<T>(xs: T[], make: (item: T) => Promise<T>): T[] {
  make(xs[0]);
  return xs;
}
",
    );

    assert!(
        source.contains("F0: Fn(T) -> SmeltFuture<T> + ?Sized"),
        "the bound's return goes through the canonical renderer:\n{source}"
    );
    assert!(source.contains("make: &F0"));
    assert!(!source.contains("Result<SmeltFuture<T>"));
}

#[test]
fn a_borrowed_callback_forwards_into_another_generic_helper() {
    // The forwarding shape (`unionWith` -> `uniqWith` in es-toolkit). The caller
    // hands its own `&F0` to a second generic helper as `&*cb`; the callee's
    // `F0` unifies with the caller's. This is the case `?Sized` on the CALLEE is
    // load-bearing for: without it the callee requires `Sized`, which the
    // caller's own `?Sized` `F0` cannot prove (E0277).
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

    assert!(source.contains("fn inner<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T) -> bool + ?Sized>(xs: SmeltList<T>, cb: &F0)"));
    assert!(source.contains("fn outer<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T) -> bool + ?Sized>(xs: SmeltList<T>, cb: &F0)"));
    assert!(source.contains("inner(xs.clone(), &*cb)"));
}

#[test]
fn callback_only_type_param_still_demotes() {
    // INVARIANT: Increment 1 keeps `type_param_directly_inferable` required, so
    // a type parameter reachable ONLY through a callback still erases the whole
    // function. Lifting it is Increment 3's job, and it needs an `Fn` bound
    // rather than an unsize coercion to a `dyn Fn` to infer through. If this
    // flips without that work, a call site passing an erased callable (a
    // `vi.fn()` spy) leaves `T` unconstrained: `E0283`.
    let source = source_for(
        r"
export function attempt<T>(make: () => T): T[] {
  return [make()];
}
",
    );

    assert!(!source.contains("fn attempt<T"));
    assert!(source.contains("fn attempt(make: &dyn Fn() -> SmeltUnknown)"));
}

#[test]
fn constrained_type_param_still_demotes_with_callback() {
    // GUARDS Increment 5. The generic decision is still per *function*, not per
    // type parameter: one constrained parameter demotes the whole signature even
    // when its sibling `T` is perfectly inferable. Deleting the callback gate
    // must not partially enable per-parameter decisions.
    let source = source_for(
        r"
export function groupBy<T, K extends string>(xs: T[], key: (x: T) => K): K[] {
  return xs.map(key);
}
",
    );

    assert!(!source.contains("fn group_by<"));
}

#[test]
fn erased_callable_argument_keeps_the_definition_generic() {
    // The counterpart to `callback_only_type_param_still_demotes`: when `T` IS
    // pinned by a direct value parameter, an erased callable in the callback
    // position is harmless. Nothing has to infer `T` from the callback, so the
    // definition stays generic and only the call site erases — the same
    // whole-function/per-site split `generic_free_function_demotes_on_erased_argument`
    // pins for bare positions.
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

    assert!(source.contains(
        "fn each<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T) -> () + ?Sized>(xs: SmeltList<T>, cb: &F0)"
    ));
}

#[test]
fn defaulted_callback_inside_a_generic_body_renders_in_scope() {
    // The synthesized default callback is emitted INSIDE the generic body and
    // bound to a handle whose type renders in that body's lexical scope. Both
    // halves must therefore agree: an `Rc<dyn Fn(T, ..)>` handle initialised
    // with `move |arg0: SmeltUnknown, ..|` does not type-check (E0631/E0308).
    // This is a fifth renderer beyond the four call-site adapters, and it is
    // what keeps `uniqWith`-shaped functions in the lift.
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

    assert!(source.contains("fn uniq_with<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static, F0: Fn(T, T) -> bool + ?Sized>"));
    assert!(!source.contains("move |arg0: SmeltUnknown"));
}

#[test]
fn concrete_return_generic_callee_passes_arguments_through() {
    // A deliberate widening of the general rule: what licenses passing an
    // argument through is that the call site pins every type parameter, not what
    // the callee returns. A generic callee with a *concrete* return
    // (`isSubset<T>(a: T[], b: T[]): boolean`) therefore stops erasing its
    // arguments too — nothing about a `bool` return makes an argument need
    // erasing. If this ever has to be narrowed, the rule gains a "the return
    // must mention a bound type parameter" condition; it never gains a carve-out
    // for a function.
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

    assert!(source.contains("let _smelt_tmp_4: bool = is_subset(xs, ys);"));
}


#[test]
fn optional_callback_type_param_demotes() {
    // INVARIANT for §4.4 of `blocker-logs/estk-callback-generics-plan.md`: an
    // OPTIONAL callback parameter lowers to `Option<Rc<dyn Fn(T) -> _>>`, and a
    // call site that omits it must name a concrete element type for the `None`.
    // It has no closure to take one from, so it names the erased one and the
    // call no longer matches the generic signature (measured: 21 x E0308 plus
    // 1 x E0631 on the synthesized default closure). The whole function must
    // therefore keep the erased representation until an increment designs the
    // optional case.
    //
    // NON-VACUOUS: with the §4.4 narrowing removed this emits
    // `fn pick<T: ..>(x: T, cb: Option<Rc<dyn Fn(T) -> bool>>) -> T`, so the
    // first assertion fails.
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

    assert!(!source.contains("fn pick<T"));
    assert!(source.contains("fn pick(x: SmeltUnknown, cb: Option<"));
}

#[test]
fn owned_callback_type_param_demotes() {
    // INVARIANT for §4.4: a callback the emitter's ownership fixpoint classifies
    // as ESCAPING (here it is returned) lowers to an owned `Rc<dyn Fn(..)>` with
    // a `'static` bound rather than a borrowed `&dyn Fn`. The borrowed-callback
    // argument ladder — the only path that renders a call-site adapter under the
    // site's bindings — declines those positions, so a generic signature would be
    // met with an erased argument. The gate consults
    // `emitter::compute_owned_callback_params`, the SAME set the renderer uses,
    // so the two cannot grow different notions of "owned".
    //
    // NON-VACUOUS: without the narrowing this emits
    // `fn keep<T: ..>(x: T, cb: Rc<dyn Fn(T) -> bool>) -> Rc<dyn Fn(T) -> bool>`.
    let source = source_for(
        r"
export function keep<T>(x: T, cb: (v: T) => boolean): (v: T) => boolean {
  return cb;
}
",
    );

    assert!(!source.contains("fn keep<T"));
    assert!(source.contains("fn keep(x: SmeltUnknown, cb: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> bool>)"));
}

#[test]
fn callback_nested_in_a_container_demotes() {
    // INVARIANT for §4.4: a function type nested inside another container
    // (`((v: T) => boolean)[]`) is not a direct callback parameter. No adapter
    // renderer walks into the container, so the callee would advertise `T` in a
    // position no call site substitutes.
    //
    // NON-VACUOUS: without the narrowing this emits
    // `fn pick<T: ..>(xs: SmeltList<T>, cbs: SmeltList<Rc<dyn Fn(T) -> bool>>)`.
    let source = source_for(
        r"
export function pick<T>(xs: T[], cbs: ((v: T) => boolean)[]): T[] {
  return xs.filter(cbs[0]);
}
",
    );

    assert!(!source.contains("fn pick<T"));
    assert!(source.contains("cbs: SmeltList<::std::rc::Rc<dyn Fn(SmeltUnknown) -> bool>>"));
}

#[test]
fn rest_callback_type_param_demotes() {
    // INVARIANT for §4.4: a packed rest callback parameter is emitted as an
    // erased sequence of callables, so there is no single declared callback type
    // for a call site to be substituted against. (It is also container-nested,
    // which is why the emitted text below matches
    // `callback_nested_in_a_container_demotes`; the `function.rest` half of the
    // rule is what covers a rest parameter whose declared MIR type is not a
    // container.)
    //
    // NON-VACUOUS: without the narrowing this emits `fn pick<T: ..>`.
    let source = source_for(
        r"
export function pick<T>(xs: T[], ...cbs: ((v: T) => boolean)[]): T[] {
  return xs.filter(cbs[0]);
}
",
    );

    assert!(!source.contains("fn pick<T"));
}

#[test]
fn higher_order_callback_type_param_demotes() {
    // INVARIANT for §4.4: the callback occurrence must be at the DIRECT
    // callback's own top level. A `T` buried inside a further nested function
    // type (`(f: (v: T) => boolean) => boolean`) sits behind an owned
    // `Rc<dyn Fn>` the adapter renderers do not descend into.
    //
    // NON-VACUOUS: without the narrowing this emits
    // `fn apply<T: ..>(x: T, cb: &dyn Fn(Rc<dyn Fn(T) -> bool>) -> bool)`.
    let source = source_for(
        r"
export function apply<T>(x: T, cb: (f: (v: T) => boolean) => boolean): boolean {
  return cb((v: T) => true);
}
",
    );

    assert!(!source.contains("fn apply<T"));
    assert!(source.contains(
        "fn apply(x: SmeltUnknown, cb: &dyn Fn(::std::rc::Rc<dyn Fn(SmeltUnknown) -> bool>) -> bool)"
    ));
}

#[test]
fn callback_in_the_return_type_still_lifts() {
    // The boundary of §4.4's rule, stated positively: it scopes only PARAMETER
    // positions. A callback in the return type is produced by the callee's own
    // body rather than supplied by a call site, so it has no argument that could
    // disagree with the substituted signature and it stays generic.
    let source = source_for(
        r"
export function make<T>(x: T): (v: T) => boolean {
  return (v: T) => true;
}
",
    );

    assert!(source.contains(
        "fn make<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(x: T) -> ::std::rc::Rc<dyn Fn(T) -> bool>"
    ));
}
