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
