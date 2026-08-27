//! Split codegen tests chunk.

use super::*;

/// A tag check against a statically-absent (`None`-typed) ambient global folds
/// to a compile-time constant instead of emitting `matches!((), SmeltUnknown::…)`
/// which does not type-check (E0308). Mirrors es-toolkit `isBrowser`/`isNode`,
/// which guard on `typeof window !== 'undefined'` / `typeof process !==
/// 'undefined'` where the ambient global is absent in the non-DOM profile.
#[test]
fn folds_tag_check_of_absent_ambient_global() {
    let source = source_for(
        "declare let window: { document: unknown } | undefined;\n\
         export function isBrowser(): boolean {\n\
           return typeof window !== 'undefined' && window?.document != null;\n\
         }\n",
    );

    // No unit-vs-SmeltUnknown tag match is emitted for the absent global.
    assert!(
        !source.contains("matches!((), SmeltUnknown"),
        "absent-global tag check must fold, not emit matches!((), ...): {source}"
    );
    // The absent global's nullish check folds to a constant boolean.
    assert!(
        source.contains("let _smelt_tmp_0: bool = true;"),
        "absent-global nullish tag check should fold to true: {source}"
    );
}

#[test]
fn emits_main_with_console_log() {
    let source = source_for("let count = 42;\nconsole.log(count);\n");

    assert!(source.contains("fn main() {"));
    assert!(source.contains("let count: f64 = 42.0;"));
    assert!(source.contains("let _ = { println!(\"{}\", count); };"));
}

#[test]
fn exact_console_write_uses_debug_format_for_lists() {
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
                && matches!(callee, Callee::Builtin(BuiltinFn::ConsoleLog))
            {
                *callee = Callee::Builtin(BuiltinFn::ConsoleWrite);
            }
        }
    }
    let source = emit_source(&mir).unwrap_or_else(|error| panic!("Rust source: {error}"));
    assert!(source.contains("print!(\"{:?}\", values"), "{source}");
}

#[test]
fn emits_owned_string_literals() {
    let source = source_for("const message = \"hello smelt\";\nconsole.log(message);\n");

    assert!(source.contains("let message: String = \"hello smelt\".to_owned();"));
}

#[test]
fn emits_direct_length_lowering() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3];
const count = values.length;
const word = "smelt";
const letters = word.length;
"#,
    );

    assert!(source.contains(".len() as f64;"));
    assert!(source.contains(".chars().count() as f64;"));
}

#[test]
fn emits_typescript_first_class_closure_values() {
    let source = source_for(
        r"
const offset = 2;
const addOffset = (value: number): number => value + offset;
function apply(value: number, fn: (value: number) => number): number {
  return fn(value);
}
function makeAdder(base: number): (value: number) => number {
  const add = (value: number): number => value + base;
  return add;
}
const direct = addOffset(3);
const passed = apply(4, addOffset);
const adder = makeAdder(5);
const returned = adder(6);
",
    );

    assert!(source.contains("Rc<dyn Fn(f64) -> f64>"));
    assert!(source.contains("(3.0)"));
    assert!(source.contains("apply(4.0,"));
    assert!(source.contains("make_adder(5.0)"));
    assert!(source.contains("(adder)(6.0)"));
    assert!(source.contains("move |"));
}

#[test]
fn shares_outer_bindings_mutated_by_local_closures() {
    let source = source_for(
        r#"
function collect(): string[] {
  const values: string[] = [];
  let current = "x";
  const flush = (): void => {
    values.push(current);
    current = "";
  };
  flush();
  current = "y";
  flush();
  return values;
}
"#,
    );

    assert!(
        source.contains("let smelt_capture_values: ::std::rc::Rc<::std::cell::RefCell<"),
        "{source}"
    );
    assert!(
        source
            .contains("let smelt_capture_current: ::std::rc::Rc<::std::cell::RefCell<"),
        "{source}"
    );
    assert!(
        source.contains("let smelt_capture_values = smelt_capture_values.clone();"),
        "{source}"
    );
    assert!(
        source.contains("(*smelt_capture_current.borrow_mut()) = \"y\".to_owned();"),
        "{source}"
    );
    assert!(
        source.contains("return (*smelt_capture_values.borrow()).clone();"),
        "{source}"
    );
}

/// Guards the shared-capture borrow invariant against RefCell double-borrow panics.
///
/// A single-threaded JS closure may synchronously call a sibling closure that
/// reads or writes the same captured binding while the first closure is midway
/// through evaluating an expression over that binding. In the generated Rust,
/// shared captures live in `Rc<RefCell<T>>` and each use expands to
/// `(*smelt_capture_x.borrow())` / `(*smelt_capture_x.borrow_mut())`. A
/// `borrow`/`borrow_mut` guard is a temporary that lives to the end of the
/// FULL enclosing statement, so if a borrow text were interpolated into the
/// same statement as a closure call that re-borrows the same cell, the nested
/// borrow would panic with "already borrowed" — a crash JS never produces.
///
/// The invariant that prevents this: MIR is three-address form, so every
/// `Call`/`ClosureCall` is lowered to its own SSA temp binding
/// (`let _smelt_tmp_N = (closure)();`) BEFORE any statement that consumes the
/// result, and the copy-propagation / move-on-last-use passes only rewrite
/// local aliases (they never fuse a call rvalue into a consuming statement).
/// Consequently every emitted borrow guard is confined to a single
/// three-address statement whose operands are already-materialized locals or
/// literals — never a live call. This test pins that shape: the sibling call
/// `bump()` must be bound to its own temp, and the borrow of the shared cell
/// must appear in a later statement, so no guard is ever held across the call.
#[test]
fn shared_capture_borrow_never_spans_a_sibling_closure_call() {
    let source = source_for(
        r"
function run(): number {
  let count = 0;
  function bump(): number {
    count += 1;
    return count;
  }
  function acc(): void {
    count = count + bump();
  }
  acc();
  return count;
}
",
    );

    // `count` is shared through an `Rc<RefCell<f64>>` capture cell.
    assert!(
        source
            .contains("let smelt_capture_count: ::std::rc::Rc<::std::cell::RefCell<"),
        "{source}"
    );
    // The sibling call is materialized into its own temp before any borrow.
    assert!(source.contains("= (bump)();"), "{source}");
    // The `acc` body must never place a `borrow()`/`borrow_mut()` guard in the
    // same statement as the `(bump)()` call: the line that calls `bump` is a
    // bare `let _smelt_tmp = (bump)();` with no borrow text on it.
    let call_line = source
        .lines()
        .find(|line| line.contains("= (bump)();"))
        .unwrap_or_else(|| panic!("no bump call line\n{source}"));
    assert!(
        !call_line.contains(".borrow()") && !call_line.contains(".borrow_mut()"),
        "borrow guard shares a statement with the sibling call: {call_line}\n{source}"
    );
    // The assignment back into the shared cell reads a pre-evaluated temp, not
    // an inline call, so the `borrow_mut()` target guard cannot span a call.
    assert!(
        source.contains("(*smelt_capture_count.borrow_mut()) = _smelt_tmp"),
        "{source}"
    );
}

/// A string literal that happens to spell a shared-captured variable's name must
/// be emitted verbatim.
///
/// Shared-capture uses inside an escaping closure are substituted textually over
/// the already-rendered closure body (`replace_shared_capture_uses`). That scan
/// must skip Rust literals: in es-toolkit's `flatten` a closure named `recursive`
/// rewrote the emitter's own `panic!("recursive closure control flow …")` message
/// into `panic!("(*smelt_capture_recursive.borrow_mut()) closure …")`, and the
/// same corruption silently rewrites user program string data.
#[test]
fn preserves_string_literal_naming_a_shared_capture() {
    let source = source_for(
        r#"
function apply(fn: (x: number) => string): string {
  return fn(1);
}
function label(): number {
  let current = 0;
  const bump = (x: number): string => {
    current = current + x;
    return "current went up";
  };
  apply(bump);
  return current;
}
"#,
    );

    // `current` is shared through a capture cell, so the textual rewrite ran.
    assert!(
        source
            .contains("let smelt_capture_current: ::std::rc::Rc<::std::cell::RefCell<"),
        "{source}"
    );
    assert!(
        source.contains("let smelt_capture_current = smelt_capture_current.clone();"),
        "{source}"
    );
    // The literal is data: it keeps the source spelling of `current`.
    assert!(source.contains(r#""current went up""#), "{source}");
    assert!(
        !source.contains("smelt_capture_current.borrow_mut()) went up"),
        "string literal was rewritten\n{source}"
    );
}

/// A list index READ on a shared-capture array must borrow the backing `Vec`
/// immutably. The read lowers to `arr.get({ ... arr.len() ... }).cloned()`, so
/// if the receiver used `borrow_mut()` the `.len()` inside the normalized-index
/// argument would take a SECOND `borrow_mut()` of the same `Rc<RefCell<Vec<_>>>`
/// cell within one expression — two simultaneous mutable borrows that panic at
/// runtime with "already borrowed". Two shared `borrow()`s coexist fine. Here
/// `order` is forced into a shared capture cell because the nested `record`
/// closure mutates it while the outer scope reads `order[0]`.
#[test]
fn shared_capture_list_index_read_borrows_immutably() {
    let source = source_for(
        r"
function run(): number {
  let order: number[] = [];
  function record(): void {
    order.push(1);
  }
  record();
  return order[0];
}
",
    );

    // `order` is shared through an `Rc<RefCell<Vec<_>>>` capture cell.
    assert!(
        source
            .contains("let smelt_capture_order: ::std::rc::Rc<::std::cell::RefCell<"),
        "order should be a shared capture cell: {source}"
    );
    // The index read must not place two `borrow_mut()` of the same cell in one
    // expression: the `.get(...)` receiver and the `.len()` argument both read,
    // so both use `borrow()`.
    let read_line = source
        .lines()
        .find(|line| line.contains(".get(") && line.contains("smelt_capture_order"))
        .unwrap_or_else(|| panic!("no order index-read line\n{source}"));
    assert!(
        !read_line.contains(".borrow_mut()"),
        "list index read must borrow immutably, not borrow_mut: {read_line}\n{source}"
    );
    assert!(
        read_line.contains("(*smelt_capture_order.borrow())"),
        "list index read should use borrow(): {read_line}\n{source}"
    );
}

/// The write-path twin of [`shared_capture_list_index_read_borrows_immutably`].
///
/// `order[i] = v` on a shared-capture array grows and writes the backing `Vec`
/// through `resize`/`IndexMut` (which need `borrow_mut()`), but the length
/// reads that normalize and bounds-check the index need only `&self`. The index
/// is bound to `smelt_assign_index` in its own statement BEFORE the mutable
/// borrow, and the length reads borrow immutably, so the receiver's
/// `borrow_mut()` never coexists with a second borrow of the same `RefCell` in
/// one statement. The regressed form
/// `(*cap.borrow_mut())[{ ... (*cap.borrow_mut()).len() ... }]` — an inline
/// index that reborrows the receiver — panics at runtime with "already
/// borrowed". Here `order` is a shared capture because the nested `put` closure
/// writes it by index.
#[test]
fn shared_capture_list_index_write_precomputes_index_before_borrow_mut() {
    let source = source_for(
        r"
function run(k: number, v: number): number[] {
  const order: number[] = [0, 0, 0];
  function put(i: number, value: number): void {
    order[i] = value;
  }
  put(k, v);
  return order;
}
",
    );

    // `order` is shared through an `Rc<RefCell<Vec<_>>>` capture cell.
    assert!(
        source
            .contains("let smelt_capture_order: ::std::rc::Rc<::std::cell::RefCell<"),
        "order should be a shared capture cell: {source}"
    );
    // The index is precomputed into a temp, so the indexed write borrows the
    // receiver mutably exactly once with a plain temp subscript.
    assert!(
        source.contains("(*smelt_capture_order.borrow_mut())[smelt_assign_index] ="),
        "list index write must subscript with a precomputed index temp: {source}"
    );
    // The length reads that normalize/bounds-check the index borrow immutably.
    assert!(
        source.contains("(*smelt_capture_order.borrow()).len()"),
        "index-length reads must borrow the shared capture immutably: {source}"
    );
    // The regressed double-`borrow_mut` signature is an inline index block right
    // after a mutable borrow: `borrow_mut())[{`. It must not appear anywhere.
    assert!(
        !source.contains("borrow_mut())[{"),
        "list index write must not inline a reborrowing index into the mutable \
         receiver (two simultaneous borrow_mut of one cell): {source}"
    );
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
    assert!(source.contains("(closure_arg_0)(data.clone())"), "{source}");
    assert!(!source.contains("let item = (*item).clone()"), "{source}");
    assert!(!source.contains("smelt_default_callback"), "{source}");
}

#[test]
fn array_callback_value_index_paths_snapshot_source_array_for_js_callback_abi() {
    let source = source_for(
        r"
function offsets(values: number[], factor: number): number[] {
  return values.map((value, index) => value * factor + index);
}
function positive(values: number[]): boolean {
  return values.some((value, index) => value + index > 2);
}
",
    );

    assert!(
        source.contains("smelt_array.iter().enumerate().map"),
        "{source}"
    );
    assert!(
        source.contains("smelt_array.iter().enumerate().any"),
        "{source}"
    );
    assert!(
        source.contains("closure_arg_0.clone() * factor.clone()"),
        "{source}"
    );
    assert!(
        source.contains("let smelt_array = values.clone();"),
        "{source}"
    );
    assert!(!source.contains(".clone().clone()"), "{source}");
}

#[test]
fn array_callback_third_array_parameter_snapshots_once() {
    let source = source_for(
        r"
function view(values: number[]): number[] {
  return values.map((value, index, array) => value + array[index]);
}
",
    );

    assert!(
        source.contains("let smelt_array = values.clone();"),
        "{source}"
    );
    assert!(
        source.contains("smelt_array.iter().enumerate().map"),
        "{source}"
    );
    // The snapshot is taken ONCE, and each element then receives it by reference.
    // This assertion used to require `smelt_array.clone()` — a full copy of the array
    // per element, which is precisely what the test's name says must not happen, and
    // what made every array callback O(n^2). See `callback_param_is_shared_reference`.
    assert!(source.contains("&smelt_array"), "{source}");
    assert!(!source.contains("smelt_array.clone()"), "{source}");
    assert!(!source.contains("values.clone().clone()"), "{source}");
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

/// `reduce` supplies the same array argument, and pays the same cost by value.
///
/// The fold body used to bind `let array = <list>.clone();` — one whole-list deep
/// copy per element, exactly the shape the array-callback sites had. A
/// by-reference array parameter binds the borrow the fold is already iterating.
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
    assert!(source.contains("let array = &values"), "{source}");
    assert!(!source.contains("let array = values.clone()"), "{source}");
}

/// A closure body that needs an OWNED list still gets one, from the reference.
///
/// The by-reference decision is made on the callback TYPE, but whether a body can
/// live with a borrow is a property of the individual closure: this one rebinds its
/// own array parameter, so it needs a `mut` owned binding. The signature cannot
/// change — it has to keep matching the `dyn Fn` the closure is cast to — so the
/// body copies the reference into the value the by-value ABI used to hand it. That
/// copy is per CALL, and only in bodies that need it.
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
        source.contains("smelt_array.iter().enumerate().map"),
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
fn emits_string_concat_with_optional_primitive_rhs() {
    let source = source_for(
        r"
function label(prefix: string, value: string | undefined): string {
  return prefix + value;
}
",
    );

    assert!(
        source.contains("prefix.clone() + &value.clone().unwrap_or_default()"),
        "{source}"
    );
}

#[test]
fn emits_unknown_slice_inside_callback_as_runtime_string_match() {
    let source = source_for(
        r#"
const values: unknown[] = ["abc"];
const tails = values.map((value) => value.slice(1));
"#,
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(!source.contains(".slice(1.0)"), "{source}");
}

#[test]
fn emits_typescript_generic_and_default_closure_values() {
    let source = source_for(
        r"
const bump = <T extends number>(value: number = 1): number => value + 1;
const defaulted = bump();
const explicit = bump(4);
",
    );

    assert!(
        source.contains("impl FnMut(f64) -> f64")
            || source.contains("|closure_arg_0: f64|")
            || source.contains("move |closure_arg_0: f64|"),
        "{source}"
    );
    assert!(source.contains("(1.0)"));
    assert!(source.contains("(4.0)"));
}

#[test]
fn emits_typescript_destructured_tuple_callback() {
    let source = source_for(
        r"
const pairs: [number, number][] = [[1, 2], [3, 4]];
const sums = pairs.map(([left, right]) => left + right);
",
    );

    assert!(source.contains("closure_arg_0.0.clone() + closure_arg_0.1.clone()"));
}

#[test]
fn emits_typescript_destructured_record_callback() {
    let source = source_for(
        r"
const rows: Record<string, number>[] = [];
const doubled = rows.map(({ value }) => value * 2);
",
    );

    assert!(
        source.contains(".get(&\"value\".to_owned()).expect(\"missing field\")"),
        "{source}"
    );
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
fn emits_typescript_rest_closure_values() {
    let source = source_for(
        r"
const sum = (...values: number[]): number => values[0] + values[1];
const total = sum(2, 3, 4);
",
    );

    assert!(
        source.contains("|closure_arg_0: SmeltList<f64>|"),
        "{source}"
    );
    assert!(source.contains("vec![2.0, 3.0, 4.0]"));
    assert!(source.contains("closure_arg_0.get("), "{source}");
    // The subject is the TOTAL read — `.cloned().unwrap_or(default)` rather than
    // a fallible one. The read already yields an owned value, so it carries no
    // trailing `.clone()`; see `index_place_read_is_owned`.
    assert!(
        source.contains(".cloned().unwrap_or(0.0)"),
        "{source}"
    );
    assert!(
        !source.contains(".cloned().unwrap_or(0.0).clone()"),
        "an index read is already owned and must not be cloned again:\n{source}"
    );
}

#[test]
fn emits_typescript_top_level_rest_functions() {
    let source = source_for(
        r"
function sum(...values: number[]): number {
  return values[0] + values[1];
}
const total = sum(2, 3, 4);
",
    );

    assert!(source.contains("fn sum(values: SmeltList<f64>) -> f64"));
    assert!(source.contains("vec![2.0, 3.0, 4.0]"));
}

#[test]
fn extracts_generic_spread_arrays_when_packing_rest_calls() {
    let source = source_for(
        r"
function collect(context: unknown, ...rest: unknown[]): unknown[] {
  return rest;
}
function call<Values extends unknown[]>(first: unknown, values: Values): unknown[] {
  return collect(undefined, first, ...values);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Array(value) => value"),
        "{source}"
    );
    assert!(
        source.contains(".iter().cloned().chain("),
        "spread list was replaced by a default value: {source}"
    );
}

#[test]
fn erases_typed_optional_struct_spread_source_to_smelt_unknown() {
    // Spreading an optional typed options struct (`{ ...options, extra }`) used
    // to emit a `match options { SmeltUnknown::Object(map) => .. }` directly on
    // the `Option<Struct>` value, which does not type-check. The spread source
    // must first be erased to `SmeltUnknown` through its boundary adapter.
    let source = source_for(
        r"
interface WeekOptions {
  weekStartsOn?: number;
}
function inner(date: number, options?: WeekOptions): number {
  return date;
}
function outer(date: number, options?: WeekOptions): number {
  return inner(date, { ...options, weekStartsOn: 1 });
}
",
    );

    assert!(
        source.contains("IntoSmeltUnknown::into_smelt_unknown(options"),
        "typed optional spread source was not erased through IntoSmeltUnknown: {source}"
    );
    assert!(
        !source.contains("match options.clone().clone() { SmeltUnknown::Object(map)"),
        "spread match still inspects the typed Option value directly: {source}"
    );
}

#[test]
fn emits_optional_number_typeof_as_a_presence_check() {
    let source = source_for(
        r#"
function isNumber(value?: number): boolean {
  return typeof value === "number";
}
function isUndefined(value?: number): boolean {
  return typeof value === "undefined";
}
"#,
    );

    assert!(source.contains("value.clone().is_none()"), "{source}");
    assert!(source.contains("bool = !("), "{source}");
    assert!(
        !source.contains("fn is_number(value: Option<f64>) -> bool {\n    return false;"),
        "{source}"
    );
}

#[test]
fn emits_math_abs_call() {
    let source = source_for(
        r"
const value = -5;
const positive = Math.abs(value);
",
    );

    assert!(source.contains(".abs();"));
}

#[test]
fn emits_math_rounding_calls() {
    let source = source_for(
        r"
const value = 5.5;
const floor = Math.floor(value);
const ceil = Math.ceil(value);
const round = Math.round(value);
const trunc = Math.trunc(value);
",
    );

    assert!(source.contains(".floor();"));
    assert!(source.contains(".ceil();"));
    assert!(source.contains(".trunc();"));
    // `Math.round` cannot be `f64::round`: JavaScript rounds a tie toward +∞ and
    // Rust rounds a tie away from zero, so `Math.round(-1.5)` differs. It routes
    // through the runtime helper instead — see
    // `math_round_uses_the_javascript_tie_rule`.
    assert!(source.contains("smelt_math_round("));
}

#[test]
fn emits_math_trunc_on_integer_expressions_without_zeroing() {
    let source = source_for(
        r"
const value = 1234;
const whole = Math.trunc(value / 1000);
const text = whole.toString();
",
    );

    assert!(source.contains("value / 1000.0;"));
    assert!(source.contains("_smelt_tmp_3.trunc();"));
    assert!(source.contains("whole.to_string();"));
    assert!(!source.contains("= 0_i64;"));
    assert!(!source.contains("= 0.0;"));
}

#[test]
fn emits_math_extrema_calls() {
    let source = source_for(
        r"
const first = 1;
const second = 2;
const highest = Math.max(first, second, 3);
const lowest = Math.min(first, second, -1);
",
    );

    assert!(source.contains(".max("));
    assert!(source.contains(".min("));
}

#[test]
fn emits_array_callback_methods() {
    let source = source_for(
        r"
const values: number[] = [1, 2, 3];
const mapped = values.map(value => value + 1);
const filtered = values.filter(value => value > 1);
const found = values.find(value => value > 1);
const foundIndex = values.findIndex(value => value > 1);
const hasAny = values.some(value => value > 1);
const hasEvery = values.every(value => value > 0);
values.forEach(value => value + 1);
const total = values.reduce((acc, value) => acc + value, 0);
const indexed = values.map((value, index) => value + index);
const factor = 2;
const captured = values.map((value, index) => value * factor + index);
const scale = (value: number): number => value + factor;
const localClosure = values.map(scale);
function construct(context: unknown, value: unknown): unknown {
  return value;
}
const normalize = construct.bind(null, undefined);
const normalized = values.map(normalize);
let mutableTotal = 0;
values.forEach(value => mutableTotal += value);
const noInitial = values.reduce((acc, value, index) => acc + value + index);
",
    );

    assert!(source.contains(".iter().enumerate().map(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().filter_map(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().find_map(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().any(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().all(|(index, item)|"));
    assert!(source.matches("loop {").count() >= 2);
    assert!(source.contains(".iter().enumerate().fold("));
    assert!(source.contains("reduce of empty array with no initial value"));
    assert!(source.contains(".collect::<Vec<_>>()"));
    assert!(source.contains(".unwrap_or(-1.0"));
    assert!(source.contains("closure_arg_0.clone() * 2.0"), "{source}");
    assert!(source.contains("closure_arg_0.clone() + 2.0"));
    assert!(
        source.contains("(smelt_callback)(SmeltUnknown::Number(item.clone() as f64))"),
        "{source}"
    );
    assert!(source.contains("let mut mutable_total"));
    // The accumulator's old value is dead after the read (the next statement
    // reassigns it), so move-on-last-use drops the clone.
    assert!(source.contains("mutable_total +"));
    assert!(source.contains("mutable_total ="));
}

#[test]
fn emits_captured_multi_argument_callback_calls_inside_array_map() {
    let source = source_for(
        r"
function zip(
  first: unknown[],
  second: unknown[],
  fn: (first: unknown, second: unknown, index: number, data: unknown[]) => unknown,
): unknown[] {
  return first.map((item, index) => fn(item, second[index], index, first));
}
",
    );

    assert!(
        source.contains("fn_(closure_arg_0.clone(),"),
        "captured callback should retain its four-argument ABI: {source}"
    );
    assert!(
        !source.contains("fn_(match closure_arg_0.clone()"),
        "captured callback was typed from a synthetic map local: {source}"
    );
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

    assert!(
        source.contains("fn_(closure_arg_0.clone(),"),
        "captured callback should not adopt the same-named free function ABI: {source}"
    );
    assert!(
        !source.contains("fn_(match item.clone()"),
        "same-named free function ABI leaked into a captured callback: {source}"
    );
}

#[test]
fn emits_string_index_and_for_of() {
    let source = source_for(
        r#"
const word = "abc";
const first = word[0];
const last = word.at(-1);
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#,
    );

    assert!(source.contains("let normalized = if index < 0 { len + index } else { index }"));
    assert!(source.contains(".chars().nth({ let len = word.chars().count() as i64;"));
    assert!(source.contains(".chars().count() as f64"));
    assert!(source.contains("let index = _smelt_tmp_"));
}

#[test]
fn emits_callback_regex_replace_uppercase() {
    let source = source_for(
        r"
export function format(units: string[]): string[] {
  return units.map((unit) => `x${unit.replace(/(^.)/, (m) => m.toUpperCase())}` as string);
}
",
    );

    assert!(source.contains("regex::Regex::new"));
    assert!(source.contains("to_uppercase()"));
}

#[test]
fn emits_callback_string_key_record_access() {
    let source = source_for(
        r"
export function values(record: Record<string, number>, keys: string[]): number[] {
  return keys.map((key) => record[key]);
}
",
    );

    assert!(
        source.contains(".get(&closure_arg_0.clone().clone())"),
        "{source}"
    );
}

#[test]
fn emits_string_case_methods() {
    let source = source_for(
        r#"
const word = "Smelt";
const lower = word.toLowerCase();
const upper = word.toUpperCase();
"#,
    );

    assert!(source.contains(".to_lowercase();"));
    assert!(source.contains(".to_uppercase();"));
}

#[test]
fn emits_string_trim_inside_filter_callback_body() {
    let source = source_for(
        r#"
const values = [" a ", " "].filter(value => !!value.trim());
"#,
    );

    assert!(source.contains(".trim().to_owned()"), "{source}");
    assert!(source.contains(".filter("), "{source}");
}

#[test]
fn emits_erased_string_coercion_before_callback_trim() {
    let source = source_for(
        r"
function clean(value: any): any[] {
  return [value].filter(item => !!item.trim());
}
",
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains(".trim().to_owned()"), "{source}");
}

#[test]
fn emits_string_trim_method() {
    let source = source_for(
        r#"
const word = " Smelt ";
const trimmed = word.trim();
const left = word.trimStart();
const right = word.trimEnd();
"#,
    );

    assert!(source.contains(".trim().to_owned();"));
    assert!(source.contains(".trim_start().to_owned();"));
    assert!(source.contains(".trim_end().to_owned();"));
}

#[test]
fn emits_string_prefix_suffix_methods() {
    let source = source_for(
        r#"
const word = "Smelt";
const starts = word.startsWith("Sm");
const ends = word.endsWith("lt");
"#,
    );

    assert!(source.contains(".starts_with(&"));
    assert!(source.contains(".ends_with(&"));
}

#[test]
fn emits_string_search_methods() {
    let source = source_for(
        r#"
const word = "Smelt";
const first = word.indexOf("m");
const last = word.lastIndexOf("t");
const bounded = word.lastIndexOf("t", 2);
"#,
    );

    assert!(source.contains(".find(&"));
    assert!(source.contains(".rfind(&"));
    assert!(source.contains(".map_or(-1.0"));
    assert!(source.contains("let smelt_end_char = smelt_from.saturating_add("));
}

#[test]
fn emits_optional_string_field_search_without_losing_narrowed_value() {
    let source = source_for(
        r#"
interface Options {
  separator?: string | RegExp;
}
function lastSeparator(data: string, options: Options = {}): number {
  const { separator } = options;
  if (typeof separator === "string") {
    return data.lastIndexOf(separator);
  }
  return -1;
}
const result = lastSeparator("a,b", { separator: "," });
"#,
    );

    assert!(
        source.contains("map_or_else(String::new"),
        "narrowed optional dynamic string values must be extracted for string methods"
    );
}

#[test]
fn emits_string_replace_method() {
    let source = source_for(
        r#"
const word = "hello hello";
const replaced = word.replace("hello", "hi");
"#,
    );

    assert!(source.contains(".replacen(&"));
    assert!(source.contains(", 1);"));
}

#[test]
fn emits_string_replace_all_method_as_literal_replace() {
    // `replaceAll` with a plain string search is a literal substring replace,
    // so it must map to `str::replace` (all occurrences) and NOT treat the
    // search value as a regex.
    let source = source_for(
        r#"
const word = "a.b.c";
const replaced = word.replaceAll(".", "-");
"#,
    );

    assert!(source.contains(".replace(&"));
    assert!(!source.contains(".replacen(&"));
}

#[test]
fn emits_string_repeat_method() {
    let source = source_for(
        r#"
const word = "ha";
const repeated = word.repeat(3);
"#,
    );

    assert!(source.contains(".repeat(3.0 as usize);"));
}

#[test]
fn emits_string_padding_methods() {
    let source = source_for(
        r#"
const word = "7";
const paddedStart = word.padStart(3, "0");
const paddedEnd = word.padEnd(3);
"#,
    );

    assert!(source.contains("pad.chars().cycle().take(needed).collect()"));
    assert!(source.contains("format!(\"{}{}\", padding, value)"));
    assert!(source.contains("format!(\"{}{}\", value, padding)"));
}

#[test]
fn emits_string_char_at_method() {
    let source = source_for(
        r#"
const word = "Smelt";
const char = word.charAt(1);
const code = word.charCodeAt(2);
"#,
    );

    assert!(
        source.contains(".chars().nth(1.0 as usize).map(|ch| ch.to_string()).unwrap_or_default();")
    );
    assert!(source.contains(".chars().nth(2.0 as usize).map_or(f64::NAN, |ch| ch as u32 as f64);"));
}

#[test]
fn emits_math_sqrt_pow_sign() {
    let source = source_for(
        r"
const value = 4;
const root = Math.sqrt(value);
const cubeRoot = Math.cbrt(value);
const raised = Math.pow(value, 2);
const signed = Math.sign(value);
const sine = Math.sin(value);
const cosine = Math.cos(value);
const tangent = Math.tan(value);
const arcsine = Math.asin(value);
const arccosine = Math.acos(value);
const arctangent = Math.atan(value);
const arctangentTwo = Math.atan2(value, 2);
const logged = Math.log(value);
const logTen = Math.log10(value);
const logTwo = Math.log2(value);
const exponent = Math.exp(value);
const distance = Math.hypot(value, 3);
const sample = Math.random();
",
    );

    assert!(source.contains(".sqrt();"));
    assert!(source.contains(".cbrt();"));
    assert!(source.contains(".powf("));
    assert!(source.contains(".signum();"));
    assert!(source.contains(".sin();"));
    assert!(source.contains(".cos();"));
    assert!(source.contains(".tan();"));
    assert!(source.contains(".asin();"));
    assert!(source.contains(".acos();"));
    assert!(source.contains(".atan();"));
    assert!(source.contains(".atan2("));
    assert!(source.contains(".ln();"));
    assert!(source.contains(".log10();"));
    assert!(source.contains(".log2();"));
    assert!(source.contains(".exp();"));
    assert!(source.contains("0.0f64.hypot("));
    assert!(source.contains(".hypot(3.0);"));
    assert!(source.contains("rand::random::<f64>();"));
}

#[test]
fn emits_typescript_primitive_conversions() {
    let source = source_for(
        r#"
const value = 42;
const digits = "42";
const asText = String(value);
const asNumber = Number(digits);
const emptyNumber = Number("");
const asBool = Boolean("");
"#,
    );

    assert!(source.contains(".to_string()"));
    assert!(source.contains("smelt_text.is_empty() { 0.0 }"));
    assert!(source.contains(".parse::<f64>().unwrap_or(f64::NAN)"));
    assert!(!source.contains("float() parse failed"));
    assert!(source.contains(".is_empty()"));
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
        source.contains(").parse::<f64>().unwrap_or(f64::NAN)) >= (0.0)"),
        "string >= number must ToNumber-coerce the string side: {source}"
    );
    // A raw, non-coerced `String >= 0.0` must never be emitted.
    assert!(
        !source.contains("\"\".to_owned() >= 0.0"),
        "raw String relational must not be emitted"
    );
    // String-vs-string comparison stays lexical (no ToNumber coercion applied).
    assert!(
        source.contains("< b") && !source.contains("(a).parse::<f64>"),
        "string-vs-string comparison must stay lexical: {source}"
    );
}

#[test]
fn emits_captured_class_method_call_inside_map_callback() {
    // Issue #64: a captured class instance whose method is called inside a
    // `.map()` callback body must lower into the synthesized closure and emit
    // valid Rust. The compact callback IR does not model this call shape when
    // the method body is non-trivial, so it routes through the closure-body
    // fallback; the whole HIR -> MIR -> Rust pipeline must still succeed (any
    // failure panics inside `source_for`). The emitted closure captures the
    // receiver and dispatches the method with the callback's element argument.
    let source = source_for(
        r"
class Counter {
  base: number;
  constructor(b: number) {
    this.base = b;
  }
  scaled(x: number): number {
    const factor = this.base;
    return x * factor;
  }
}

export function scaleAll(xs: number[]): number[] {
  const c = new Counter(2);
  return xs.map((x) => c.scaled(x));
}
",
    );

    assert!(
        source.contains(".scaled("),
        "captured class method call did not survive callback lowering: {source}"
    );
    assert!(
        source.contains(".map("),
        "array map iteration was dropped: {source}"
    );
}

/// Extracting an iterable into a list must normalize a non-erased source
/// (`Option<SmeltList<_>>` / `SmeltList<_>`) through the `IntoSmeltUnknown`
/// boundary adapter before matching `SmeltUnknown::` arms. `Array.from(arr)`
/// where `arr: unknown[] | null | undefined` previously matched
/// `SmeltUnknown::` patterns against the `Option<SmeltList<..>>` value directly
/// and failed to type-check (E0308).
#[test]
fn erases_optional_list_source_before_iterable_extraction() {
    let source = source_for(
        r"
export function toList(arr: unknown[] | null | undefined): unknown[] {
  return Array.from(arr);
}
",
    );

    assert!(
        source.contains(".into_smelt_unknown(); let smelt_id = if let SmeltUnknown::Array"),
        "iterable extraction did not normalize its source through IntoSmeltUnknown: {source}"
    );
    assert!(
        !source.contains("let smelt_src = arr.clone().clone(); let smelt_id"),
        "iterable extraction still matched SmeltUnknown arms on a non-erased source: {source}"
    );
}

/// A fallible (`may_throw`) array predicate is emitted as a closure returning
/// `Result<_, Box<dyn Error>>`. The predicate call site (`find`/`findIndex`/
/// `some`/`every`/`filter`) consumes the result in boolean position and must
/// unwrap the `Result` — otherwise the `if …`/`.any`/`.all` sees a `Result`
/// where a `bool` is required (E0308).
#[test]
fn unwraps_fallible_predicate_result_in_list_query() {
    let source = source_for(
        r"
export function firstBad(values: unknown[]): number {
  return values.findIndex((value) => {
    if (value) {
      throw new Error('boom');
    }
    return false;
  });
}
",
    );

    assert!(source.contains("find_map("), "{source}");
    assert!(
        source.contains(".unwrap_or_else(|error: Box<dyn std::error::Error>| panic!"),
        "fallible predicate result was not unwrapped before boolean use: {source}"
    );
}

/// Rebinding a whole list parameter (`items = …`) is a local reassignment that
/// JavaScript never propagates to the caller, so the parameter stays an owned
/// `mut` binding. It must not be promoted to the shared `&mut SmeltList<..>`
/// ABI, which cannot accept an owned assignment (E0308).
#[test]
fn keeps_rebound_list_parameter_owned() {
    let source = source_for(
        r"
export function rebindList(items: unknown[]): unknown[] {
  items = [items[0]];
  return items;
}
",
    );

    assert!(
        source.contains("mut items: SmeltList<SmeltUnknown>"),
        "rebound list parameter was not an owned mut binding: {source}"
    );
    assert!(
        !source.contains("&mut SmeltList<SmeltUnknown>"),
        "rebound list parameter was promoted to a mutable reference: {source}"
    );
}

/// Rebinding a callback parameter (`cb = …`) assigns an owned `Rc<dyn Fn…>`
/// handle, which a borrowed `&dyn Fn` binding cannot hold. Such a parameter
/// must enter the function as an owned callback handle (E0308).
#[test]
fn keeps_rebound_callback_parameter_owned() {
    let source = source_for(
        r"
export function rebindCb(cb: (value: unknown) => unknown): unknown {
  cb = (value) => value;
  return cb(1);
}
",
    );

    assert!(
        source.contains("::std::rc::Rc<dyn Fn(SmeltUnknown) ->"),
        "rebound callback parameter was not an owned handle: {source}"
    );
    assert!(
        !source.contains("cb: &dyn Fn(SmeltUnknown)"),
        "rebound callback parameter stayed a borrowed &dyn Fn: {source}"
    );
}

/// A loop-shaped closure body that `return`s from inside the loop must lower
/// each MIR `Return` to an explicit Rust `return`, not a bare tail expression.
///
/// A closure whose body is a `for`/`while` loop is rendered as a statement
/// `loop { ... }`. A `return value` reached inside the loop used to be emitted
/// as a bare tail expression (`value`) followed by `; break;`, which mistyped
/// the loop-body block as `value`'s type instead of the required `()` and left
/// the `loop` yielding `()` — both E0308 mismatches. Every in-loop return must
/// become `return value;` so the diverging `loop` unifies with the closure's
/// declared result type.
#[test]
fn returns_from_loop_shaped_closure_body_use_explicit_return() {
    let source = source_for(
        r"
export function firstMatch(pairs: Array<[number, string]>): (val: number) => string | undefined {
  const length = pairs.length;
  return function (val: number): string | undefined {
    for (let i = 0; i < length; i++) {
      const pair = pairs[i];
      if (pair[0] === val) {
        return pair[1];
      }
    }
    return undefined;
  };
}
",
    );

    assert!(
        source.contains("return Some("),
        "in-loop closure return was not an explicit return: {source}"
    );
    assert!(
        !source.contains("    ;\n    break;\n"),
        "loop-shaped closure body still emitted `value; break;`: {source}"
    );
}

/// Truthiness of an `Option<ConcreteUnion>` must erase the inner concrete union
/// to `SmeltUnknown` before matching `SmeltUnknown::` arms.
///
/// A generated union (`SmeltUnionNNNN`) is not a `SmeltUnknown`, so matching its
/// inner value directly against `Some(SmeltUnknown::…)` arms is a type mismatch
/// (E0308). The inner value must first pass through the union's
/// `IntoSmeltUnknown` boundary adapter; only the resulting `bool` escapes.
#[test]
fn erases_optional_union_inner_before_truthiness_match() {
    let source = source_for(
        r"
export function pickFlag(opts?: { separator?: string | number }): string {
  const sep = opts?.separator;
  if (sep) {
    return 'has';
  }
  return 'none';
}
",
    );

    assert!(
        source.contains(".map(|value| value.into_smelt_unknown())"),
        "optional-union truthiness did not erase its inner union: {source}"
    );
}

/// Coercing an already-concrete `Option<String>` to `String` must unwrap the
/// `Option`, not route through the erased-`SmeltUnknown` extraction match.
///
/// `extract` narrows an erased `SmeltUnknown` and its arms only type-check
/// against a `SmeltUnknown` scrutinee. When a caller reaches it with a concrete
/// `Option<String>` source (e.g. an array element used as a `String(...)`
/// argument), it must delegate to the general concrete coercion, which unwraps
/// the option — otherwise the arms mismatch the `Option<String>` value (E0308).
#[test]
fn coerces_concrete_optional_source_without_unknown_extraction() {
    let source = source_for(
        r"
export function firstLower(words: string[]): string {
  const first: string | undefined = words[0];
  return String(first).toLowerCase();
}
",
    );

    assert!(
        source.contains("first.unwrap_or_default()"),
        "concrete Option<String> source was not unwrapped for string coercion: {source}"
    );
    assert!(
        !source.contains("match first.clone() { SmeltUnknown::"),
        "concrete Option<String> source was routed through the SmeltUnknown extraction match: {source}"
    );
}

/// A closure nested inside another shared-capture closure must reuse the
/// already-rendered `smelt_capture_x` cell instead of wrapping it again.
///
/// When an escaping outer closure mutates a captured binding, the binding lives
/// in an `Rc<RefCell<T>>` cell and every use renders as
/// `(*smelt_capture_x.borrow_mut())`. A closure nested inside that outer closure
/// re-captures the same binding; its source name is therefore already the
/// rendered `(*smelt_capture_x.borrow_mut())` form. The nested closure must (a)
/// reuse that rendered form verbatim rather than emitting the invalid double
/// wrap `(*smelt_capture_(*smelt_capture_x.borrow_mut())...)` (which fails to
/// parse / resolve, E0425), and (b) still clone the `Rc` cell into its own
/// header so the outer closure can keep using the cell after the nested closure
/// moves its clone.
#[test]
fn nested_closure_reuses_enclosing_shared_capture_cell() {
    let source = source_for(
        r"
export function makeTimers(onEnd: () => void): { schedule: () => void; cancel: () => void } {
  let timeoutId: number | null = null;
  const schedule = () => {
    const cb = () => {
      timeoutId = null;
      onEnd();
    };
    timeoutId = 1;
  };
  const cancel = () => {
    timeoutId = null;
  };
  return { schedule, cancel };
}
",
    );

    // The invalid double-wrapped capture name must never appear.
    assert!(
        !source.contains("smelt_capture_(*smelt_capture_"),
        "nested capture re-wrapped an already-rendered shared cell: {source}"
    );
    // The nested closure clones the enclosing `Rc` cell into its own header
    // (the source `timeoutId` is emitted as the snake-cased `timeout_id`).
    assert!(
        source.contains("let smelt_capture_timeout_id = smelt_capture_timeout_id.clone();"),
        "nested closure did not clone the enclosing shared capture cell: {source}"
    );
    // The nested assignment writes straight through the single-wrapped cell.
    assert!(
        source.contains("(*smelt_capture_timeout_id.borrow_mut()) = None"),
        "nested assignment did not target the single-wrapped shared cell: {source}"
    );
}

/// `.length` on an optional whose unwrapped value is a concrete union must
/// inspect the value dynamically rather than call `SmeltUnknown::len`.
///
/// A `string | string[]` parameter lowers to a concrete union, so an optional
/// of it is `Option<SmeltUnionN>`, not `Option<SmeltUnknown>`. Emitting
/// `map_or(0, SmeltUnknown::len)` there gives the mapper a `&SmeltUnknown`
/// receiver that mismatches the concrete `&SmeltUnionN` borrow (E0631). JS
/// `.length` is a dynamic property whose meaning depends on the runtime variant,
/// so the mapper must erase the borrowed value and inspect it.
#[test]
fn optional_union_length_inspects_dynamically() {
    let source = source_for(
        r"
export function measure(str: string, chars?: string | string[]): number {
  if (chars === undefined) {
    return str.length;
  }
  switch (typeof chars) {
    case 'string': {
      return chars.length;
    }
    default: {
      return 0;
    }
  }
}
",
    );

    assert!(
        !source.contains("map_or(0, SmeltUnknown::len)"),
        "optional union `.length` used the SmeltUnknown::len mapper: {source}"
    );
    assert!(
        source.contains("into_smelt_unknown()")
            && source.contains("SmeltUnknown::String(value) => value.chars().count()"),
        "optional union `.length` did not inspect the erased value: {source}"
    );
}
/// Regression: a mutable variable captured by an async closure (read *and*
/// written inside the closure body) is stored in shared `Rc<RefCell>` storage.
/// The `Box::pin(async move { .. })` prelude must clone the `smelt_capture_*`
/// handle, never re-bind the dereferenced `(*smelt_capture_x.borrow_mut())`
/// lvalue in a `let` pattern position (which is not a valid pattern).
#[test]
fn async_shared_capture_prelude_clones_handle() {
    let out = source_for(
        r"
async function delayMs(ms: number): Promise<void> {}
async function run(arr: number[]): Promise<void> {
  let running = 0;
  let maxRunning = 0;
  const fn = async (item: number): Promise<boolean> => {
    running = running + 1;
    if (running > maxRunning) {
      maxRunning = running;
    }
    await delayMs(20);
    running = running - 1;
    return item % 2 === 0;
  };
  await Promise.all(arr.map(fn));
}
",
    );
    assert!(
        !out.contains("let (*smelt_capture"),
        "capture lvalue text emitted in a `let` binding position: {out}"
    );
    assert!(
        out.contains("let smelt_capture_max_running = smelt_capture_max_running.clone();"),
        "async prelude did not clone the shared capture handle: {out}"
    );
}

/// Regression: reading a numeric field that lowers to an `as f64` cast (here a
/// RegExp `lastIndex`) must be parenthesized so a following postfix `.clone()`
/// does not mis-parse as part of the cast target type
/// (`... as f64.clone()` is invalid).
#[test]
fn numeric_field_cast_parenthesized_before_clone() {
    let out = source_for(
        r#"
function f(): Record<string, unknown> {
  const re = /a/g;
  const out: Record<string, unknown> = {};
  out["idx"] = re.lastIndex;
  return out;
}
"#,
    );
    assert!(
        !out.contains("as f64.clone()"),
        "cast followed by `.clone()` was not parenthesized: {out}"
    );
    assert!(
        out.contains("(*re.last_index.borrow() as f64).clone()"),
        "expected parenthesized cast before clone: {out}"
    );
}

/// Regression: `Array.prototype.fill` with no end argument defaults the end to a
/// `len as f64` cast used in an `if X < 0.0` comparison. A bare `len as f64 <`
/// mis-parses `<` as the start of generic arguments for `f64`, so the cast must
/// be parenthesized.
#[test]
fn array_fill_length_cast_parenthesized_before_comparison() {
    let out = source_for(
        r"
function f(): number[] {
  const a: number[] = [1, 2, 3];
  a.fill(0);
  return a;
}
",
    );
    assert!(
        !out.contains("fill_len as f64 <"),
        "cast followed by `<` was not parenthesized: {out}"
    );
    assert!(
        out.contains("if (fill_len as f64) < 0.0"),
        "expected parenthesized length cast in comparison: {out}"
    );
}

/// Regression (async-method owned-self transform): an async method must be
/// emitted as an ordinary `fn(&self, ..) -> SmeltFuture<T>` that clones `self`
/// into an owned handle and runs the awaited body inside a moved `async` block.
/// This keeps the returned future `'static` so a spawned/detached
/// `receiver.method()` task does not borrow the local receiver (previously
/// E0597). The method must NOT be emitted as `async fn`.
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

/// Regression (async-method owned-self transform): a value-class async method
/// whose state is a shared reference-class field keeps identity through the
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

/// Regression (throwing-await output ABI): awaiting a future whose output type
/// differs from the destination binding must coerce from the future's inner
/// item type, not bind the raw awaited value. A value awaited in `void`/`()`
/// context inside a `try` (the throwing-await path) is discarded rather than
/// bound to a `()`-typed local as a `SmeltUnknown` (previously E0308, seen in
/// es-toolkit `withTimeout` specs).
#[test]
fn throwing_await_discards_value_in_void_context() {
    let out = source_for(
        r"
async function makeValue(): Promise<number> {
  return 1;
}
async function run(): Promise<void> {
  try {
    await makeValue();
  } catch (e) {}
}
",
    );
    assert!(
        !out.contains(": () = __smelt_value;"),
        "throwing await bound a non-() awaited value to a () local: {out}"
    );
}

/// Regression: a `switch` without a `default` clause whose arms all return must
/// still emit the statements that follow the `switch` for unmatched values.
///
/// JavaScript falls through a default-less `switch` to the code after it when no
/// label matches. Smelt lowers the `switch` to a `Match` with a synthesized
/// empty default that jumps to a shared continuation ("join") block holding the
/// post-`switch` tail. The Rust emitter treats a `goto` to a lower-id block as a
/// loop back-edge; MIR lowering used to allocate the join block *before* the arm
/// blocks, so when no join could be hoisted (an arm ends in a call, as `helper()`
/// does here) the synthesized default's forward `goto join` was misread as an
/// unterminating back-edge and the entire tail was silently dropped
/// (`_ => { return SmeltUnknown::Null; }`). The join now outranks every arm and
/// the default, keeping it a forward edge so the tail is emitted.
#[test]
fn switch_without_default_emits_tail_after_call_arm() {
    let source = source_for(
        r#"
function helper(x: number): number {
  return x + 1;
}
export function f(tag: string, a: number): number {
  switch (tag) {
    case "call": {
      return helper(a);
    }
    case "lit": {
      return 7;
    }
  }

  let total = a;
  for (let i = 0; i < 3; i++) {
    total = total + i;
  }
  return total + 1000;
}
"#,
    );
    assert!(
        source.contains("1000"),
        "post-switch tail (`total + 1000`) was dropped: {source}"
    );
    assert!(
        source.contains("total = a.clone()"),
        "post-switch tail statements were dropped: {source}"
    );
    assert!(
        !source.contains("_ => {\n    return SmeltUnknown::Null;"),
        "default arm fabricated a return instead of falling through to the tail: {source}"
    );
}

/// A `switch` *with* an exhaustive-looking `default` still lowers correctly: the
/// default arm runs its own body and each arm's early return is preserved. This
/// guards the join-block reordering against regressing the with-default path.
#[test]
fn switch_with_default_preserves_arm_returns() {
    let source = source_for(
        r#"
export function classify(tag: string): number {
  switch (tag) {
    case "a": {
      return 1;
    }
    case "b": {
      return 2;
    }
    default: {
      return -1;
    }
  }
}
"#,
    );
    assert!(source.contains("\"a\" => {"), "missing arm a: {source}");
    assert!(source.contains("\"b\" => {"), "missing arm b: {source}");
    assert!(
        source.contains("_ => {"),
        "missing default arm: {source}"
    );
    assert!(
        source.contains("return -1_f64") || source.contains("- 1.0") || source.contains("-1.0"),
        "default arm body was dropped: {source}"
    );
}
