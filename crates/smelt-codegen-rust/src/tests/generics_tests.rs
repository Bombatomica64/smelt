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
fn callback_generic_function_still_demotes() {
    // INVARIANT for Increment 0b: the callback gate (`type_param_in_callback`,
    // `classes.rs`) is untouched, so a type parameter appearing inside a
    // callback parameter still forces the whole function to erase. This test is
    // expected to FLIP in Increment 1; its flip is the marker that the gate
    // came down, and it must not flip before then.
    let source = source_for(
        r"
export function takeWhile<T>(xs: T[], keep: (item: T) => boolean): T[] {
  return xs.filter(keep);
}
",
    );

    assert!(!source.contains("fn take_while<T"));
    assert!(source.contains("fn take_while(xs: SmeltList<SmeltUnknown>"));
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

