//! Generated Rust for callbacks and optional parameters.

use super::*;

/// A tag check against an ambient global the target profile models as ABSENT
/// folds to a compile-time constant instead of emitting `matches!((),
/// SmeltUnknown::…)`, which does not type-check (E0308). Mirrors es-toolkit's
/// `isBrowser`, which guards on `typeof window !== 'undefined'` where `window`
/// is absent in the non-DOM profile.
///
/// The answer comes from the profile registry (`window` is absent), so the guard
/// decides the whole `&&` and the function folds to `return false;`.
#[test]
fn folds_tag_check_of_absent_ambient_global() {
    let source = source_for(
        "declare let window: { document: unknown } | undefined;\n\
         export function isBrowser(): boolean {\n\
           return typeof window !== 'undefined' && window?.document != null;\n\
         }\n",
    );

    // The absent global's guard decides the whole expression, so the dead
    // operand is never lowered and the function folds to a constant.
    assert_eq!(
        rust_fn(&source, "is_browser"),
        "fn is_browser() -> bool {\nreturn false;\n}"
    );
}

/// A list reaching the exact-write path renders the way the console renders
/// it — Node's `util.inspect` (`[ 1, 2 ]`) through the list's own
/// `SmeltInspect` impl — never Rust's `Debug` (`[1.0, 2.0]`), which is a
/// spelling no JavaScript runtime prints.
#[test]
fn exact_console_write_renders_lists_like_the_console() {
    use smelt_mir::{BuiltinFn, Callee, Terminator};

    let mut ctx = HirCtx::new();
    assert!(
        to_hir(
            "const values: number[] = [1, 2];\nconsole.log(values);\n",
            FileId(0),
            &mut ctx,
        )
        .is_ok(),
        "HIR"
    );
    let mut mir = smelt_mir::lower_hir(&ctx.krate).unwrap_or_else(|_| panic!("MIR"));
    for function in &mut mir.functions {
        for block in &mut function.blocks {
            if let Some(Terminator::Call { callee, .. }) = &mut block.terminator
                && matches!(callee, Callee::Builtin(BuiltinFn::ConsoleLog { .. }))
            {
                *callee = Callee::Builtin(BuiltinFn::ConsoleWrite);
            }
        }
    }
    let source = emit_source(&mir).unwrap_or_else(|error| panic!("Rust source: {error}"));
    assert!(
        source.contains("print!(\"{}\", SmeltInspect::smelt_console_text(&values"),
        "{source}"
    );
    assert!(!source.contains("print!(\"{:?}\", values"), "{source}");
}

#[test]
fn emits_function_array_some_without_cloning_callbacks() {
    let source = source_for(
        r"
function anyPass(data: unknown, fns: Array<(value: unknown) => boolean>): boolean {
  return fns.some((fn) => fn(data));
}
",
    );

    assert!(source.contains("mut fns:"), "{source}");
    assert!(source.contains(".iter().enumerate().any"), "{source}");
    assert!(source.contains("(closure_arg_0)(&data)"), "{source}");
    assert!(!source.contains("let item = (*item).clone()"), "{source}");
    assert!(!source.contains("smelt_default_callback"), "{source}");
}

/// The array callback ABI: the array parameter is `&SmeltList<..>`, and the
/// per-element argument is a borrow rather than a copy.
///
/// This is the whole callback-by-reference change stated as one assertion pair.
/// JavaScript hands an array callback the array itself as its third argument at no
/// cost; lowered by value that is a full `SmeltList` deep copy per ELEMENT, which
/// makes every array callback O(n^2) (measured: 955x on es-toolkit `partition`).
/// Both halves matter — a `&`-typed parameter fed a clone, or an owned parameter
/// fed a borrow, would not compile — so the test pins the declaration and the
/// argument together. See `callback_param_is_shared_reference`.
#[test]
fn array_callback_array_parameter_is_passed_by_reference_and_not_cloned() {
    let source = source_for(
        r"
function view(values: number[]): number[] {
  return values.map((value, index, array) => value + array[index]);
}
",
    );

    // The declaration side: the callback's array parameter is a shared reference.
    assert!(
        source.contains("closure_arg_2: &SmeltList<f64>"),
        "{source}"
    );
    // The argument side: the snapshot is borrowed, once per call, not copied per
    // element. `smelt_array.clone()` anywhere in this program IS the per-element
    // copy — the snapshot binding itself is `values.clone()`.
    assert!(source.contains("&smelt_array"), "{source}");
    assert!(!source.contains("smelt_array.clone()"), "{source}");
}

/// `reduce` supplies the same array argument: a by-reference array parameter binds
/// the borrow the fold is already iterating rather than copying the list per element.
#[test]
fn array_reduce_callback_array_parameter_is_passed_by_reference() {
    let source = source_for(
        r"
function total(values: number[]): number {
  return values.reduce((acc, value, index, array) => acc + value + array[index], 0);
}
",
    );

    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    assert!(
        source.contains("closure_arg_3: &SmeltList<f64>"),
        "{source}"
    );
    assert!(source.contains("let smelt_array = &values"), "{source}");
    assert!(
        !source.contains("let smelt_array = values.clone()"),
        "{source}"
    );
}

/// A closure body that needs an OWNED list still gets one, from the reference.
///
/// The by-reference decision is made on the callback TYPE, but whether a body can
/// live with a borrow is a property of the individual closure: this one rebinds its
/// own array parameter, so it needs a `mut` owned binding. The signature cannot
/// change — it has to keep matching the `dyn Fn` the closure is cast to — so the
/// body copies the reference into an owned value. That copy is per CALL, and only
/// in bodies that need it.
#[test]
fn a_by_reference_callback_parameter_is_owned_again_when_the_body_rebinds_it() {
    let source = source_for(
        r"
function view(values: number[]): number[] {
  return values.map((value, index, array) => {
    array = [value];
    return array[0];
  });
}
",
    );

    assert!(
        source.contains("closure_arg_2: &SmeltList<f64>"),
        "{source}"
    );
    assert!(
        source.contains("let mut closure_arg_2: SmeltList<f64> = closure_arg_2.clone();"),
        "{source}"
    );
}

#[test]
fn nested_array_callbacks_borrow_each_receiver_without_double_cloning() {
    let source = source_for(
        r"
function nested(groups: number[][], limit: number): boolean[] {
  return groups.map((group) => group.filter((value) => value > limit).some((value) => value > 0));
}
",
    );

    assert!(source.contains(".iter().enumerate().map"), "{source}");
    assert!(
        source.contains(".iter().enumerate().filter_map"),
        "{source}"
    );
    assert!(source.contains(".iter().enumerate().any"), "{source}");
    assert!(source.contains("limit.clone()"), "{source}");
    assert!(!source.contains("smelt_array.clone().clone()"), "{source}");
    assert!(!source.contains(".clone().clone()"), "{source}");
}

#[test]
fn spread_rest_callback_closure_clones_captured_callback_once() {
    let source = source_for(
        r"
type RestCallback = (...args: unknown[]) => unknown;
function invokeAll(callbacks: RestCallback[], args: unknown[]): unknown[] {
  return callbacks.map((callback) => callback(...args));
}
",
    );

    assert!(
        source.contains("smelt_array.borrow().iter().enumerate().map"),
        "{source}"
    );
    assert!(source.contains("args.clone()"), "{source}");
    assert!(
        source.contains("let smelt_array = callbacks.clone();"),
        "{source}"
    );
    assert!(!source.contains("callback.clone().clone()"), "{source}");
}

#[test]
fn skips_unused_function_callback_item_bindings_in_literal_false_branch() {
    let source = source_for(
        r"
function lazy(functions: Array<(value: unknown) => unknown>): Array<unknown | null> {
  return functions.map((fn) => false ? fn(null) : null);
}
",
    );

    assert!(source.contains(".iter().enumerate().map"), "{source}");
    assert!(!source.contains("let item = (*item).clone()"), "{source}");
    assert!(!source.contains("(&mut *item.borrow_mut())"), "{source}");
}

#[test]
fn emits_typescript_async_closure_values() {
    let source = source_for(
        r"
async function run(): Promise<number> {
  const lift = async (value: number): Promise<number> => value + 1;
  const result = await lift(4);
  return result;
}
",
    );

    assert!(source.contains("Box::pin(async move"));
    assert!(source.contains(".await"));
}

#[test]
fn does_not_apply_free_function_abi_to_same_named_captured_callback() {
    let source = source_for(
        r"
function fn(value: unknown[]): unknown[] {
  return value;
}
function zip(
  first: unknown[],
  second: unknown[],
  fn: (first: unknown, second: unknown, index: number, data: unknown[]) => unknown,
): unknown[] {
  return first.map((item, index) => fn(item, second[index], index, first));
}
",
    );

    // The parameter renders apart from the free function it shares a source
    // name with (a body's locals never take a crate function's name), so the
    // call is unambiguously the captured callback.
    assert!(
        source.contains("fn__1(closure_arg_0,"),
        "captured callback should not adopt the same-named free function ABI: {source}"
    );
    assert!(
        !source.contains("fn_(match item.clone()"),
        "same-named free function ABI leaked into a captured callback: {source}"
    );
}

/// A JS relational comparison mixing a `string` operand with a numeric operand
/// must `ToNumber`-coerce the string side (non-numeric text -> `NaN`, so the
/// comparison is `false`), matching JS semantics. Without the coercion the
/// emitter would produce a raw `String >= f64` that does not type-check. This
/// mirrors `es-toolkit`'s `cloneDeepWith`, which assigns named properties on an
/// array value (`result['index']`), driving a `'index' >= 0 && 'index' < len`
/// index guard. String-vs-string comparison must stay lexical.
#[test]
fn emits_tonumber_coercion_for_mixed_string_number_relational() {
    let source = source_for(
        r"
export function mixedRelational(key: string, len: number): boolean {
  return key >= 0 && key < len;
}
export function lexicalRelational(a: string, b: string): boolean {
  return a < b;
}
",
    );

    // The string side of a string-vs-number comparison is ToNumber-coerced.
    assert!(
        source.contains("smelt_tmp_2: bool = (smelt_js_string_to_number(&key)) >= (0.0)"),
        "string >= number must ToNumber-coerce the string side: {source}"
    );
    // A raw, non-coerced `String >= 0.0` must never be emitted.
    assert!(
        !source.contains("\"\".to_owned() >= 0.0") && !source.contains("JsString::new() >= 0.0"),
        "raw String relational must not be emitted"
    );
    // String-vs-string comparison stays lexical (no ToNumber coercion applied).
    assert!(
        source.contains("< b") && !source.contains("(a).parse::<f64>"),
        "string-vs-string comparison must stay lexical: {source}"
    );
}

/// An async method is an ordinary `fn(&self, ..) -> SmeltFuture<T>` that clones
/// `self` into an owned handle and runs the awaited body inside a moved `async`
/// block, which keeps the returned future `'static` so a spawned or detached
/// `receiver.method()` task does not borrow the local receiver. It is never an
/// `async fn`.
#[test]
fn async_method_emits_owned_self_smelt_future() {
    let out = source_for(
        r"
class Semaphore {
  private available: number = 1;
  async acquire(): Promise<void> {
    if (this.available > 0) {
      this.available = this.available - 1;
    }
  }
}
async function run(): Promise<void> {
  const sema = new Semaphore();
  sema.acquire();
}
",
    );
    assert!(
        out.contains("fn acquire(&self) -> SmeltFuture<()> {"),
        "async method not emitted as owned-self SmeltFuture fn: {out}"
    );
    assert!(
        !out.contains("async fn acquire"),
        "async method still emitted as `async fn`: {out}"
    );
    assert!(
        out.contains("let self_owned = self.clone();"),
        "async method body did not clone self into an owned handle: {out}"
    );
    assert!(
        out.contains("SmeltFuture::<()>::from_future_primed(Box::pin(async move {"),
        "async method body not wrapped in a primed moved async block: {out}"
    );
    assert!(
        !out.contains("self.available") && out.contains("self_owned"),
        "async method body still references borrowed `self`: {out}"
    );
}

/// A value-class async method whose state is a shared reference-class field keeps identity through the
/// owned clone (the clone shares the inner `Rc` handle). The transform applies
/// uniformly regardless of value/reference classification of the outer class.
#[test]
fn async_method_owned_self_applies_to_value_class() {
    let out = source_for(
        r"
class Latch {
  done: boolean = false;
  async wait(): Promise<boolean> {
    return this.done;
  }
}
",
    );
    assert!(
        out.contains("fn wait(&self) -> SmeltFuture<bool> {"),
        "value-class async method not emitted as owned-self SmeltFuture fn: {out}"
    );
    assert!(
        !out.contains("async fn wait"),
        "value-class async method still emitted as `async fn`: {out}"
    );
}
