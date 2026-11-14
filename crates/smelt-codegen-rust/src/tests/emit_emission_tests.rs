//! Generated Rust for other emission rules.

use super::*;

#[test]
fn typed_option_bag_parameter_defaults_do_not_require_missing_fields() {
    let source = source_for(
        r"
export function pick(
  {
    leading = false,
    trailing = true,
    maxWait,
  }: {
    readonly leading?: boolean;
    readonly trailing?: boolean;
    readonly maxWait?: number;
  } = {},
): unknown {
  return [leading, trailing, maxWait];
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn pick"]),
        r"fn pick(__param0: SmeltRecord99210bdaad6292f3) -> SmeltUnknown {
let leading: bool = __param0.leading.clone().unwrap_or(false);
let trailing: bool = __param0.trailing.clone().unwrap_or(true);
let max_wait: Option<f64> = __param0.max_wait.clone();
let _smelt_tmp_6: SmeltUnknown = SmeltUnknown::Array(vec![SmeltUnknown::Bool(leading), SmeltUnknown::Bool(trailing), max_wait.clone().map_or(SmeltUnknown::Undefined, |smelt_value| SmeltUnknown::Number(smelt_value as f64))].into());
return _smelt_tmp_6;
}"
    );
}

#[test]
fn emits_calls_through_local_export_aliases_as_static_calls() {
    let source = source_for(
        r#"
export function format(value: string): string {
  return value;
}
export { format as formatDate };
const result = formatDate("ok");
"#,
    );

    assert_eq!(
        rust_items(&source, &["fn main"]),
        r#"fn main() {
let result: JsString;
let _smelt_tmp_1: JsString = format(JsString::from("ok"));
result = _smelt_tmp_1;
return;
}"#
    );
}

#[test]
fn keeps_lifting_a_module_arrow_over_scalars() {
    // The counter-case to the lift refusal. A scalar has no identity, so
    // re-materializing one inside a lifted function is indistinguishable from
    // reading it, and such an arrow is still free to lift. The refusal is about
    // reference identity, not about module-level arrows in general.
    let source = source_for(
        r#"
const label = "scale";

const scaled = (value: number): string => `${label}:${value * 2}`;

const useScaled = (value: number): string => apply(scaled, value);

function apply(f: (value: number) => string, value: number): string {
  return f(value);
}

export function run(): string {
  return useScaled(3);
}
"#,
    );

    assert!(source.contains("fn scaled__module_"), "{source}");
}

#[test]
fn keeps_a_module_scope_reassignment() {
    // A module-scope `let` has a local, and a top-level `x = e` writes it:
    // the table of declared module types says nothing about storage.
    let source = source_for(
        r"
let annotated: number | undefined;
annotated = 5;

let initialized: number = 1;
initialized = 6;
",
    );

    assert_eq!(
        rust_items(&source, &["fn main"]),
        r"fn main() {
let mut annotated: Option<f64> = None::<f64>;
annotated = Some(5.0);
let mut initialized: f64 = 1.0;
initialized = 6.0;
return;
}"
    );
}

#[test]
fn types_an_iife_returned_arrow_from_the_assignment_target() {
    // A contextual type flows through an immediately-invoked function
    // expression's return position, so the arrow the IIFE returns takes its
    // parameter types from the target rather than erasing to `SmeltUnknown`.
    // The rule is one rule for all four spellings: `const f: T = (..)()`,
    // `f = (..)()`, `f ||= (..)()` and `f ??= (..)()` reach it through the same
    // contextual type.
    let source = source_for(
        r"
type Sizer = (value: string) => number;

let viaAssign: Sizer | undefined;
viaAssign = (() => {
  const offset = 10;
  return (value) => value.length + offset;
})();

let viaOrAssign: Sizer | undefined;
viaOrAssign ||= (() => {
  const offset = 20;
  return (value) => value.length + offset;
})();

const direct: Sizer = (() => {
  const offset = 30;
  return (value) => value.length + offset;
})();
",
    );

    assert_eq!(
        rust_items(&source, &["fn main"]),
        r"fn main() {
let __smelt_logical_value_2: ::std::rc::Rc<dyn Fn(JsString) -> f64>;
let direct: ::std::rc::Rc<dyn Fn(JsString) -> f64>;
let _smelt_tmp_8: ::std::rc::Rc<dyn Fn() -> ::std::rc::Rc<dyn Fn(JsString) -> f64>>;
let _smelt_tmp_9: ::std::rc::Rc<dyn Fn(JsString) -> f64>;
let _smelt_tmp_10: ::std::rc::Rc<dyn Fn() -> ::std::rc::Rc<dyn Fn(JsString) -> f64>>;
let _smelt_tmp_11: ::std::rc::Rc<dyn Fn(JsString) -> f64>;
let mut via_assign: Option<::std::rc::Rc<dyn Fn(JsString) -> f64>> = None::<::std::rc::Rc<dyn Fn(JsString) -> f64>>;
let _smelt_tmp_4 = ::std::rc::Rc::new(|| -> ::std::rc::Rc<dyn Fn(JsString) -> f64> {
let offset: f64 = 10.0;
let _smelt_tmp_1 = ::std::rc::Rc::new({
let offset = offset.clone();
move |closure_arg_0: JsString| {
let _smelt_tmp_2: f64 = closure_arg_0.len() as f64;
let _smelt_tmp_3: f64 = _smelt_tmp_2 + offset;
_smelt_tmp_3
}
});
_smelt_tmp_1.clone()
});
let _smelt_tmp_5 = ((_smelt_tmp_4)()).clone();
via_assign = Some(_smelt_tmp_5.clone());
let mut via_or_assign: Option<::std::rc::Rc<dyn Fn(JsString) -> f64>> = None::<::std::rc::Rc<dyn Fn(JsString) -> f64>>;
let _smelt_tmp_6: bool = !(via_or_assign.is_none());
let _smelt_tmp_7: bool = !(_smelt_tmp_6);
if _smelt_tmp_7 {
_smelt_tmp_8 = ::std::rc::Rc::new(|| -> ::std::rc::Rc<dyn Fn(JsString) -> f64> {
let offset: f64 = 20.0;
let _smelt_tmp_1 = ::std::rc::Rc::new({
let offset = offset.clone();
move |closure_arg_0: JsString| {
let _smelt_tmp_2: f64 = closure_arg_0.len() as f64;
let _smelt_tmp_3: f64 = _smelt_tmp_2 + offset;
_smelt_tmp_3
}
});
_smelt_tmp_1.clone()
});
_smelt_tmp_9 = ((_smelt_tmp_8)()).clone();
__smelt_logical_value_2 = _smelt_tmp_9.clone();
via_or_assign = Some(__smelt_logical_value_2.clone());
}
_smelt_tmp_10 = ::std::rc::Rc::new(|| -> ::std::rc::Rc<dyn Fn(JsString) -> f64> {
let offset: f64 = 30.0;
let _smelt_tmp_1 = ::std::rc::Rc::new({
let offset = offset.clone();
move |closure_arg_0: JsString| {
let _smelt_tmp_2: f64 = closure_arg_0.len() as f64;
let _smelt_tmp_3: f64 = _smelt_tmp_2 + offset;
_smelt_tmp_3
}
});
_smelt_tmp_1.clone()
});
_smelt_tmp_11 = ((_smelt_tmp_10)()).clone();
direct = _smelt_tmp_11.clone();
return;
}"
    );
}

#[test]
fn an_annotated_iife_callee_keeps_its_own_return_type() {
    // The negative half. An explicit return-type annotation on the callee wins
    // over the contextual type, exactly as it does in TypeScript; handing an
    // already-annotated callee a second, contextual answer only creates a way
    // for the two to disagree.
    let source = source_for(
        r"
export function run(): number {
  const value: number = (function (): number { return 3; })();
  return value;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn run"]),
        r"fn run() -> f64 {
let _smelt_tmp_1 = ::std::rc::Rc::new(|| {
3.0
});
let value: f64 = (_smelt_tmp_1)();
return value;
}"
    );
}

#[test]
fn flattens_interface_extends_into_generated_storage() {
    let source = source_for(
        r"
interface WeekOptions {
  weekStartsOn?: number;
}

interface FirstWeekContainsDateOptions {
  firstWeekContainsDate?: number;
}

interface LocaleOptions extends WeekOptions, FirstWeekContainsDateOptions {}

function read(options: LocaleOptions): number {
  return options.weekStartsOn ?? options.firstWeekContainsDate ?? 0;
}
",
    );

    assert_eq!(
        rust_items(&source, &["struct LocaleOptions"]),
        r"struct LocaleOptions {
week_starts_on: Option<f64>,
first_week_contains_date: Option<f64>,
}"
    );
}

#[test]
fn emits_interface_storage_without_json_dependency() {
    let source = source_for(
        r"
interface Options {
  flag?: boolean;
}
function enabled(options?: Options): boolean {
  return true;
}
",
    );

    assert_eq!(
        rust_items(&source, &["struct Options"]),
        r"struct Options {
flag: Option<bool>,
}"
    );
    assert!(!source.contains("serde::Serialize"), "{source}");
}

#[test]
fn preserves_runtime_value_through_never_assertion() {
    let source = source_for(
        r"
function consume(value: unknown): boolean {
  return value === null;
}
const result = consume(null as unknown as never);
const tuple = [1] as unknown as [never];
const record = {} as Record<string, never>;
",
    );

    assert_eq!(
        rust_items(&source, &["fn main"]),
        r"fn main() {
let result: bool;
let tuple: SmeltUnknown;
let record: SmeltRecord<JsString, SmeltUnknown>;
let _smelt_tmp_3: SmeltUnknown = SmeltUnknown::Null;
let _smelt_tmp_4: bool = consume(_smelt_tmp_3);
result = _smelt_tmp_4;
tuple = SmeltUnknown::Array(vec![SmeltUnknown::Number(1.0 as f64)].into());
record = SmeltRecord::from([]);
return;
}"
    );
}

#[test]
fn emits_loop_with_join_blocks_as_loop() {
    let source = source_for(
        r"
function countPresent(values: string[]): Record<string, number> {
  const out = new Map<string, number>();
  for (const value of values) {
    const count = out.get(value);
    if (count === undefined) {
      out.set(value, 1);
    } else {
      out.set(value, count + 1);
    }
  }
  return Object.fromEntries(out);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn count_present"]),
        r#"fn count_present(values: SmeltList<JsString>) -> SmeltRecord<JsString, f64> {
let mut value: JsString;
let mut count: Option<f64>;
let mut _smelt_tmp_6: usize;
let mut _smelt_tmp_7: bool;
let mut _smelt_tmp_9: bool;
let mut _smelt_tmp_10: SmeltJsMap<JsString, f64>;
let mut _smelt_tmp_11: f64;
let mut _smelt_tmp_12: f64;
let mut _smelt_tmp_13: SmeltJsMap<JsString, f64>;
let mut _smelt_tmp_14: SmeltRecord<JsString, f64>;
let mut out: SmeltJsMap<JsString, f64> = SmeltJsMap::from([]);
let mut _smelt_tmp_5: usize = 0;
loop {
_smelt_tmp_6 = values.len();
_smelt_tmp_7 = _smelt_tmp_5 < _smelt_tmp_6;
if !(_smelt_tmp_7) { break; }
value = values.borrow().get(_smelt_tmp_5).cloned().unwrap_or_else(|| JsString::new());
count = out.get(&value);
_smelt_tmp_9 = count.clone().is_none();
if _smelt_tmp_9 {
_smelt_tmp_10 = { (out).insert(value, 1.0); (out).clone() };
_smelt_tmp_5 = _smelt_tmp_5 + 1;
continue;
} else {
_smelt_tmp_11 = count.clone().expect("optional value was absent after narrowing");
_smelt_tmp_12 = _smelt_tmp_11 + 1.0;
_smelt_tmp_13 = { (out).insert(value, _smelt_tmp_12); (out).clone() };
_smelt_tmp_5 = _smelt_tmp_5 + 1;
continue;
}
}
_smelt_tmp_14 = out.into_iter().map(|(smelt_key, smelt_value)| (smelt_key.clone(), smelt_value)).collect::<SmeltRecord<JsString, f64>>();
return _smelt_tmp_14;
}"#
    );
}

/// A module binding with an initializer, read from a function, reads the
/// module's ONE value through its slot: `getDefaultOptions() ===
/// getDefaultOptions()` holds in JavaScript; the slot's initializer is the
/// source's own `{}`.
#[test]
fn module_bound_intersection_alias_reads_its_slot() {
    let source = source_for(
        r"
type A = { locale?: unknown };
type B = { weekStartsOn?: number };
type DefaultOptions = A & B;

let defaultOptions: DefaultOptions = {};

export function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
",
    );

    assert!(
        source
            .contains("static SMELT_GLOBAL_DEFAULT_OPTIONS_0: ::std::cell::RefCell<SmeltUnknown>"),
        "{source}"
    );
    assert!(
        source.contains(
            "fn get_default_options() -> SmeltUnknown {\n    let _smelt_tmp_0: SmeltUnknown = \
             SMELT_GLOBAL_DEFAULT_OPTIONS_0.with(|smelt_value| smelt_value.borrow().clone());"
        ),
        "{source}"
    );
}

#[test]
fn emits_numeric_not_as_parenthesized_truthiness() {
    let source = source_for(
        r"
function isZero(amount: number): boolean {
  return !amount;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn is_zero"]),
        r"fn is_zero(amount: f64) -> bool {
let _smelt_tmp_1: bool = !({ let smelt_number = amount; smelt_number != 0.0 && !smelt_number.is_nan() });
return _smelt_tmp_1;
}"
    );
}

#[test]
fn emits_nan_truthiness_without_invalid_nan_comparison() {
    let source = source_for(
        r"
function truthy(): boolean {
  return Boolean(NaN);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn truthy"]),
        r"fn truthy() -> bool {
let _smelt_tmp_0: bool = { let smelt_number = f64::NAN; smelt_number != 0.0 && !smelt_number.is_nan() };
return _smelt_tmp_0;
}"
    );
}

#[test]
fn adapts_structural_option_bags_at_call_boundaries() {
    let source = source_for(
        r"
interface IsWeekendOptions {
  in?: (value: unknown) => unknown;
}

interface AddBusinessDaysOptions<DateType> {
  in?: (value: DateType) => DateType;
}

function isWeekend(date: unknown, options?: IsWeekendOptions): boolean {
  return false;
}

function addBusinessDays<DateType>(date: DateType, options?: AddBusinessDaysOptions<DateType>): boolean {
  return isWeekend(date, options);
}
",
    );

    assert!(source.contains("IsWeekendOptions {"), "{source}");
    assert!(
        source.contains("in_: smelt_struct_value.in_.clone()"),
        "{source}"
    );
    assert!(
        !source.contains("map_or(SmeltUnknown::Null, |value| value)"),
        "{source}"
    );
}

#[test]
fn emits_runtime_branch_for_an_optional_sort_comparator() {
    // ECMA-262 `Array.prototype.sort` step 1: `sort(undefined)` is `sort()`.
    // An optional comparator must therefore stay a real `Option<..>` down to
    // runtime and be matched, NOT be wrapped in an erased callback whose absent
    // call yields `undefined` (which the numeric coercion reads as 0, making
    // every comparison `Equal` and the whole sort a silent no-op).
    let source = source_for(
        r"
export function sortKeysLike(keys: string[], compare?: (a: string, b: string) => number): string[] {
  return keys.slice().sort(compare);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn sort_keys_like"]),
        r"fn sort_keys_like(keys: SmeltList<JsString>, compare: Option<::std::rc::Rc<dyn Fn(JsString, JsString) -> f64>>) -> SmeltList<JsString> {
let mut _smelt_tmp_2: SmeltList<JsString> = Into::<SmeltList<_>>::into(keys.borrow().iter().skip(0usize).take(keys.len().saturating_sub(0usize)).cloned().collect::<Vec<_>>());
let _smelt_tmp_3: SmeltList<JsString> = Into::<SmeltList<_>>::into({ match compare.clone() { Some(smelt_comparator) => { _smelt_tmp_2.borrow_mut().sort_by(|smelt_left, smelt_right| { let smelt_ordering = (smelt_comparator)(smelt_left.clone(), smelt_right.clone()); if smelt_ordering < 0.0 { std::cmp::Ordering::Less } else if smelt_ordering > 0.0 { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal } }); } None => { _smelt_tmp_2.borrow_mut().sort_by(|left, right| left.cmp(right)); } }; _smelt_tmp_2.clone() });
return _smelt_tmp_3;
}"
    );
}

#[test]
fn emits_locale_compare_through_the_runtime_collation_helper() {
    // `String.prototype.localeCompare` is a modeled builtin.
    let source = source_for(
        r"
export function collate(a: string, b: string): number {
  return a.localeCompare(b);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn collate"]),
        r"fn collate(a: JsString, b: JsString) -> f64 {
let _smelt_tmp_2: JsString = a.clone();
let _smelt_tmp_3: JsString = b.clone();
let _smelt_tmp_4: f64 = smelt_locale_compare(&a.clone(), &b.clone());
return _smelt_tmp_4;
}"
    );
}

#[test]
fn emits_nan_equal_leaves_for_a_nested_float_comparison() {
    // JavaScript deep equality compares numeric leaves with `Object.is`, so
    // `NaN` equals `NaN`. Rust's `f64: PartialEq` is IEEE `==`, under which it
    // does not, and `SmeltList<f64> == SmeltList<f64>` inherits that — so
    // `expect([NaN, 1]).toEqual([NaN, 1])` failed with the right value on both
    // sides. The scalar case already had this rule; this is the nested one.
    //
    // The comparison is `toEqual`, the deep equality: `left == right` used to
    // stand in for it, but JavaScript's `==` on two arrays compares identity
    // (`[1] == [1]` is `false`), and now lowers as such.
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("nested NaN", () => {
  const left: number[] = [NaN, 1];
  const right: number[] = [NaN, 1];
  expect(left).toEqual(right);
});
"#,
    );
    assert!(
        source.contains("smelt_left_item.is_nan() && smelt_right_item.is_nan()"),
        "a float leaf inside a list must compare NaN-equal: {source}"
    );
}

/// Synchronous generators must preserve their suspension points instead of
/// eagerly collecting every yielded value before the caller can resume them.
#[test]
fn emits_resumable_synchronous_generator() {
    let source = source_for(
        r#"
function* sequence(): Generator<number, string, unknown> {
  yield 1;
  yield 2;
  return "done";
}

function* delegated(): Generator<number, string, unknown> {
  return yield* sequence();
}

class IterableValue {
  value = 3;

  *[Symbol.iterator](): Generator<number, string, unknown> {
    yield this.value;
    return "iterable done";
  }
}

function* delegatedIterable(value: IterableValue): Generator<number, string, unknown> {
  return yield* value;
}

function delayedThrow(message: string): Generator<number, string, unknown> {
  return (function* (): Generator<number, string, unknown> {
    yield 4;
    throw new Error(message);
  })();
}

const iterator = sequence();
const first = iterator.next();
const firstDone = first.done;
const firstValue = first.value;
"#,
    );

    assert_eq!(
        rust_items(
            &source,
            &[
                "fn sequence",
                "fn delegated",
                "sig __smelt_symbol_iterator",
                "fn delegated_iterable"
            ]
        ),
        r#"fn sequence() -> SmeltGenerator<f64, JsString, SmeltUnknown> {
let smelt_generator_input = ::std::rc::Rc::new(::std::cell::RefCell::new(None));
let smelt_generator_producer_input = smelt_generator_input.clone();
let smelt_generator = genawaiter::rc::Gen::new(move |smelt_co| { let smelt_generator_input = smelt_generator_producer_input; async move {
let _smelt_tmp_0: f64 = 1.0;
let _smelt_tmp_1: SmeltUnknown = { smelt_co.yield_(_smelt_tmp_0).await; let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command { SmeltGeneratorCommand::Next(smelt_value) => smelt_value, SmeltGeneratorCommand::Return(smelt_value) => return smelt_value, SmeltGeneratorCommand::Throw(smelt_error) => smelt_panic_throw(smelt_throw(smelt_error)) } };
let _smelt_tmp_2: f64 = 2.0;
let _smelt_tmp_3: SmeltUnknown = { smelt_co.yield_(_smelt_tmp_2).await; let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command { SmeltGeneratorCommand::Next(smelt_value) => smelt_value, SmeltGeneratorCommand::Return(smelt_value) => return smelt_value, SmeltGeneratorCommand::Throw(smelt_error) => smelt_panic_throw(smelt_throw(smelt_error)) } };
return JsString::from("done");
} });
let mut smelt_generator = smelt_generator;
SmeltGenerator::new(move |smelt_value| { *smelt_generator_input.borrow_mut() = Some(smelt_value); match smelt_generator.resume() { genawaiter::GeneratorState::Yielded(smelt_value) => Ok(SmeltGeneratorResult::Yielded(smelt_value)), genawaiter::GeneratorState::Complete(smelt_value) => Ok(SmeltGeneratorResult::Complete(smelt_value)) } })
}

fn delegated() -> SmeltGenerator<f64, JsString, SmeltUnknown> {
let smelt_generator_input = ::std::rc::Rc::new(::std::cell::RefCell::new(None));
let smelt_generator_producer_input = smelt_generator_input.clone();
let smelt_generator = genawaiter::rc::Gen::new(move |smelt_co| { let smelt_generator_input = smelt_generator_producer_input; async move {
let _smelt_tmp_0: SmeltGenerator<f64, JsString, SmeltUnknown> = sequence();
let _smelt_tmp_1: JsString = { let smelt_delegate = _smelt_tmp_0; let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); loop { match smelt_delegate.try_resume(smelt_delegate_command) { Ok(SmeltGeneratorResult::Yielded(smelt_value)) => { smelt_co.yield_(smelt_value).await; smelt_delegate_command = { let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command { SmeltGeneratorCommand::Next(smelt_value) => SmeltGeneratorCommand::Next(smelt_value), SmeltGeneratorCommand::Return(smelt_value) => SmeltGeneratorCommand::Return(smelt_value), SmeltGeneratorCommand::Throw(smelt_error) => SmeltGeneratorCommand::Throw(smelt_error) } }; }, Ok(SmeltGeneratorResult::Complete(smelt_value)) => break Ok(smelt_value), Err(smelt_error) => break Err(smelt_error) } } }.map(JsString::from)?;
return Ok::<JsString, Box<dyn std::error::Error>>(_smelt_tmp_1);
} });
let mut smelt_generator = smelt_generator;
SmeltGenerator::new(move |smelt_value| { *smelt_generator_input.borrow_mut() = Some(smelt_value); match smelt_generator.resume() { genawaiter::GeneratorState::Yielded(smelt_value) => Ok(SmeltGeneratorResult::Yielded(smelt_value)), genawaiter::GeneratorState::Complete(smelt_value) => smelt_value.map(SmeltGeneratorResult::Complete) } })
}

fn __smelt_symbol_iterator(&self) -> SmeltGenerator<f64, JsString, SmeltUnknown>

fn delegated_iterable(value: IterableValue) -> SmeltGenerator<f64, JsString, SmeltUnknown> {
let smelt_generator_input = ::std::rc::Rc::new(::std::cell::RefCell::new(None));
let smelt_generator_producer_input = smelt_generator_input.clone();
let smelt_generator = genawaiter::rc::Gen::new(move |smelt_co| { let smelt_generator_input = smelt_generator_producer_input; async move {
let _smelt_tmp_1: SmeltGenerator<f64, JsString, SmeltUnknown> = value.__smelt_symbol_iterator();
let _smelt_tmp_2: JsString = { let smelt_delegate = _smelt_tmp_1; let mut smelt_delegate_command = SmeltGeneratorCommand::Next(Default::default()); loop { match smelt_delegate.try_resume(smelt_delegate_command) { Ok(SmeltGeneratorResult::Yielded(smelt_value)) => { smelt_co.yield_(smelt_value).await; smelt_delegate_command = { let smelt_command = smelt_generator_input.borrow_mut().take().unwrap_or_else(|| SmeltGeneratorCommand::Next(Default::default())); match smelt_command { SmeltGeneratorCommand::Next(smelt_value) => SmeltGeneratorCommand::Next(smelt_value), SmeltGeneratorCommand::Return(smelt_value) => SmeltGeneratorCommand::Return(smelt_value), SmeltGeneratorCommand::Throw(smelt_error) => SmeltGeneratorCommand::Throw(smelt_error) } }; }, Ok(SmeltGeneratorResult::Complete(smelt_value)) => break Ok(smelt_value), Err(smelt_error) => break Err(smelt_error) } } }.map(JsString::from)?;
return Ok::<JsString, Box<dyn std::error::Error>>(_smelt_tmp_2);
} });
let mut smelt_generator = smelt_generator;
SmeltGenerator::new(move |smelt_value| { *smelt_generator_input.borrow_mut() = Some(smelt_value); match smelt_generator.resume() { genawaiter::GeneratorState::Yielded(smelt_value) => Ok(SmeltGeneratorResult::Yielded(smelt_value)), genawaiter::GeneratorState::Complete(smelt_value) => smelt_value.map(SmeltGeneratorResult::Complete) } })
}"#
    );
}

#[test]
fn emits_void_return_inside_loop_branch() {
    let source = source_for(
        r"
function stopWhenSorted(items: number[]): void {
  let index = 0;
  while (index < items.length) {
    if (items[index]! >= 0) {
      return;
    }
    index += 1;
  }
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn stop_when_sorted"]),
        r"fn stop_when_sorted(items: SmeltList<f64>) -> () {
let mut _smelt_tmp_2: usize;
let mut _smelt_tmp_3: bool;
let mut _smelt_tmp_4: bool;
let mut _smelt_tmp_5: usize;
let mut index: usize = 0;
loop {
_smelt_tmp_2 = items.len();
_smelt_tmp_3 = index < _smelt_tmp_2;
if !(_smelt_tmp_3) { break; }
_smelt_tmp_4 = items.borrow().get(index).cloned().unwrap_or_else(|| 0.0) >= 0.0;
if _smelt_tmp_4 {
return;
} else {
_smelt_tmp_5 = index + 1;
index = _smelt_tmp_5;
continue;
}
}
return;
}"
    );
}

#[test]
fn emits_default_for_none_constant_assigned_to_concrete_destination() {
    let source = source_for(
        r#"
function choose(flag: boolean): unknown[] {
  let values: unknown[];
  if (flag) {
    values = ["ready"];
  } else {
    values = [];
  }
  return values;
}
"#,
    );

    assert_eq!(
        rust_items(&source, &["fn choose"]),
        r#"fn choose(flag: bool) -> SmeltList<SmeltUnknown> {
let _smelt_tmp_2: SmeltList<SmeltUnknown>;
let _smelt_tmp_3: SmeltList<SmeltUnknown>;
let mut values: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(SmeltList::new(Vec::<SmeltUnknown>::new()));
if flag {
_smelt_tmp_2 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![SmeltUnknown::String("ready".into())]; smelt_list_items }));
values = Into::<SmeltList<_>>::into(_smelt_tmp_2);
} else {
_smelt_tmp_3 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![]; smelt_list_items }));
values = Into::<SmeltList<_>>::into(_smelt_tmp_3);
}
return values;
}"#
    );
}

#[test]
fn emits_first_assignment_to_uninitialized_local_as_declaration() {
    let source = source_for(
        r"
function choose(flag: boolean): number {
  let result: number;
  if (flag) {
    result = 1;
  } else {
    result = 2;
  }
  return result;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn choose"]),
        r"fn choose(flag: bool) -> f64 {
let mut result: f64 = 0.0;
if flag {
result = 1.0;
} else {
result = 2.0;
}
return result;
}"
    );
}

#[test]
fn preserves_optional_callable_values_across_compatible_parameter_adaptation() {
    let source = source_for(
        r"
interface Options {
  in?: (value: unknown) => unknown;
}

function consume(context?: (value: Date) => unknown): void {}

export function run(options?: Options): void {
  consume(options?.in);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn run"]),
        r"fn run(options: Option<Options>) -> () {
let _smelt_tmp_1: Option<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>> = options.clone().as_ref().and_then(|_smelt_value| _smelt_value.in_.clone());
let _ = consume(_smelt_tmp_1.map(|smelt_value| { let smelt_callback = smelt_value.clone(); let smelt_adapted: ::std::rc::Rc<dyn Fn(SmeltDate) -> SmeltUnknown> = ::std::rc::Rc::new(move |smelt_arg0: SmeltDate| (smelt_callback)(&(smelt_arg0.clone().into_smelt_unknown()))); smelt_adapted }));
return;
}"
    );
}

#[test]
fn flattens_python_nested_optional_annotations() {
    let source = source_for_py(
        r"
from typing import Optional

def read(value: Optional[Optional[int]]) -> Optional[int]:
    return value
",
    );

    assert_eq!(
        rust_items(&source, &["sig read"]),
        r"fn read(value: Option<i64>) -> Option<i64>"
    );
}

#[test]
fn instanceof_boxed_primitive_wrapper_emits_marker_check() {
    // `value instanceof Boolean` over a source `any` value checks the wrapper's
    // marker on `SmeltUnknown::Object`; a primitive is not an `Object`, so the
    // check is `false`. The answers are observed in `emission_rules_behaviour`.
    let source =
        source_for("export function f(value: any): boolean { return value instanceof Boolean; }");
    assert_eq!(
        rust_items(&source, &["fn f"]),
        r#"fn f(value: SmeltUnknown) -> bool {
let _smelt_tmp_1: bool = matches!(value.clone(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key("__smelt_boolean"));
return _smelt_tmp_1;
}"#
    );
}

#[test]
fn instanceof_buffer_and_is_buffer_emit_marker_check() {
    // Both spellings emit the shared `__smelt_buffer` marker check over the source
    // `any` value.
    let instance =
        source_for("export function f(value: any): boolean { return value instanceof Buffer; }");
    let is_buffer =
        source_for("export function f(value: any): boolean { return Buffer.isBuffer(value); }");
    assert_eq!(
        rust_items(&instance, &["fn f"]),
        rust_items(&is_buffer, &["fn f"])
    );
    assert_eq!(
        rust_items(&instance, &["fn f"]),
        r#"fn f(value: SmeltUnknown) -> bool {
let _smelt_tmp_1: bool = matches!(value.clone(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key("__smelt_buffer"));
return _smelt_tmp_1;
}"#
    );
}

#[test]
fn user_arrow_does_not_route_through_an_accessor() {
    // A user-written arrow is NOT a bare function-item-as-value wrapper. It must
    // keep JavaScript's fresh identity (a new closure value each evaluation), so
    // erasing it to `SmeltUnknown` must NOT route it through a per-item
    // accessor; it keeps its plain per-reference erased wrapper.
    let source = source_for(
        r"
function takesOne(a: unknown): boolean { return true; }
const g = (x: unknown) => x;
const r = takesOne(g);
",
    );

    assert_eq!(
        rust_items(&source, &["fn main"]),
        r"fn main() {
let r: bool;
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown| {
closure_arg_0.clone()
});
let g = _smelt_tmp_2.clone();
let _smelt_tmp_3: bool = takes_one(SmeltUnknown::Function({ let smelt_source_fn = g.clone(); let smelt_callback = smelt_source_fn.clone(); let smelt_erased_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = if let Some(SmeltUnknown::Function(smelt_narrowed_fn)) = smelt_lookup_callable_object(&smelt_source_fn) { smelt_narrowed_fn } else { ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((smelt_callback)(&(smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null))))) }; smelt_link_function_identity(&smelt_erased_fn, &smelt_source_fn); smelt_register_function_length(&smelt_erased_fn, 1.0); smelt_erased_fn }));
r = _smelt_tmp_3;
return;
}"
    );
}

#[test]
fn emits_materialized_property_reads_and_writes_as_accessor_calls() {
    let mut ctx = py_frontend::HirCtx::new();
    let source = r"
class Model:
    _value: int
    value: int

    def __init__(self, value: int) -> None:
        self._value = value

    def __smelt_get_value(self) -> int:
        return self._value

    def __smelt_set_value(self, value: int) -> None:
        self._value = value

def update(model: Model) -> int:
    model.value = 7
    return model.value
";
    assert!(
        py_frontend::to_hir(source, FileId(0), &mut ctx).is_ok(),
        "HIR"
    );
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    let class_name = mir
        .classes
        .iter()
        .find(|class| mir.symbols.get(class.name) == Some("Model"))
        .map(|class| class.name)
        .expect("Model class");
    let value_field = mir
        .classes
        .iter()
        .find(|class| class.name == class_name)
        .and_then(|class| {
            class
                .fields
                .iter()
                .find(|field| mir.symbols.get(field.name) == Some("value"))
        })
        .map(|field| (field.name, field.ty))
        .expect("value field");
    let method = |expected: &str| {
        mir.functions
            .iter()
            .find(|function| {
                matches!(
                    function.origin,
                    HirOrigin::ClassMethod { class, method, .. }
                        if class == class_name && mir.symbols.get(method) == Some(expected)
                )
            })
            .unwrap_or_else(|| panic!("{expected} method"))
            .id
    };
    let getter = method("__smelt_get_value");
    let setter = method("__smelt_set_value");
    let class = mir
        .classes
        .iter_mut()
        .find(|class| class.name == class_name)
        .expect("mutable Model class");
    class.descriptors.push(smelt_mir::MirDescriptor {
        name: value_field.0,
        read_ty: value_field.1,
        write_ty: Some(value_field.1),
        getter: Some(getter),
        setter: Some(setter),
        data_descriptor: true,
        is_static: false,
        value_fields: Vec::new(),
    });

    smelt_mir::opt::optimize(&mut mir);
    let emitted = emit_source(&mir).expect("Rust source");
    // The accessors are SOURCE methods here (the fixture spells what the
    // specialization would mint), so their names are moved out of the
    // emitter's reserved `__smelt` namespace; the dispatch is unchanged.
    assert_eq!(
        rust_items(&emitted, &["fn update"]),
        r"fn update(mut model: Model) -> i64 {
model.src_smelt_set_value(7);
return model.src_smelt_get_value();
}"
    );

    mir.classes
        .iter_mut()
        .find(|model_class| model_class.name == class_name)
        .and_then(|model_class| model_class.descriptors.first_mut())
        .expect("Model descriptor")
        .data_descriptor = false;
    let shadowing = emit_source(&mir).expect("non-data descriptor Rust source");
    // A non-data descriptor is shadowed by instance storage, written through the
    // shared cell of this reference class.
    assert_eq!(
        rust_items(&shadowing, &["fn update"]),
        r"fn update(mut model: Model) -> i64 {
model.0.borrow_mut().value = 7;
return model.0.borrow().value.clone();
}"
    );
}

#[test]
fn emits_abort_controller_concrete_cancellation_model() {
    // `new AbortController()` emits two marker-bearing records sharing a mutable
    // `aborted` flag; `controller.abort()` routes through the runtime helper that
    // flips the flag and fires listeners; `instanceof` checks the markers.
    let source = source_for(
        r#"
const controller = new AbortController();
const signal = controller.signal;
signal.addEventListener("abort", () => {});
controller.abort();
const aborted = signal.aborted;
const isController = controller instanceof AbortController;
const isSignal = signal instanceof AbortSignal;
"#,
    );

    // The controller is a modeled host record; whether its fields fire and report
    // is observed in `emission_rules_behaviour`.
    assert!(
        source.contains(
            "matches!(controller.clone(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key(\"__smelt_abortcontroller\"))"
        ),
        "instanceof AbortController must check the controller marker\n{source}"
    );
    assert!(
        source.contains(
            "matches!(signal.clone(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key(\"__smelt_abortsignal\"))"
        ),
        "instanceof AbortSignal must check the signal marker\n{source}"
    );
}

#[test]
fn folds_number_constant_in_exported_const() {
    // An exported const aliasing a well-known numeric member constant folds to a
    // numeric literal instead of rejecting the initializer as non-foldable.
    let source = source_for(
        r"
export const SAFE = Number.MAX_SAFE_INTEGER;
export function ceiling(): number {
  return SAFE;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn ceiling"]),
        r"fn ceiling() -> f64 {
return 9007199254740991.0;
}"
    );
}

#[test]
fn lowers_truthy_guard_on_param_with_dependent_default() {
    // A default-parameter initializer (`end = array ? array.length : 0`) can
    // register a name in the lexical `locals` map whose `LocalId` is not
    // materialized in the function body. A later bare-identifier truthy guard
    // (`if (!end) { ... }`) falls back to "no narrowing".
    let source = source_for(
        r"
export function fillRange<T>(
  array: T[] | null | undefined,
  value: T,
  start = 0,
  end = array ? array.length : 0
): T[] {
  start = Math.floor(start);
  end = Math.floor(end);
  if (!start) {
    start = 0;
  }
  if (!end) {
    end = 0;
  }
  return [];
}
",
    );

    assert!(source.contains("fill_range"), "{source}");
}

#[test]
fn property_equality_after_in_guard_projects_concrete_arm() {
    // Property-equality discriminant comparison works once the value
    // has been narrowed to a concrete arm. The `in` guard narrows `shape` to
    // `Circle`, then `shape.tag === "c"` reads the concrete arm's field.
    let source = source_for(
        r#"
class Circle { tag: string = "c"; radius: number = 1; }
class Square { side: number = 2; }
function describe(shape: Circle | Square): number {
  if ("tag" in shape && shape.tag === "c") {
    return shape.radius;
  }
  return 0;
}
"#,
    );

    assert_eq!(
        rust_items(&source, &["enum SmeltUnion", "fn describe"]),
        r#"pub enum SmeltUnion2 {
M0(Circle),
M1(Square),
}

fn describe(shape: SmeltUnion2) -> f64 {
let mut _smelt_tmp_2: bool;
let _smelt_tmp_3: Circle;
let _smelt_tmp_4: bool;
let _smelt_tmp_5: Circle;
let _smelt_tmp_1: bool = matches!(shape.clone(), SmeltUnion2::M0(_));
let mut _smelt_tmp_2: bool = false;
if _smelt_tmp_1 {
_smelt_tmp_3 = match shape.clone() { SmeltUnion2::M0(value) => value, _ => unreachable!("union guard selected an excluded member") };
_smelt_tmp_4 = _smelt_tmp_3.tag == "c";
_smelt_tmp_2 = _smelt_tmp_4;
} else {
_smelt_tmp_2 = false;
}
if _smelt_tmp_2 {
_smelt_tmp_5 = match shape.clone() { SmeltUnion2::M0(value) => value, _ => unreachable!("union guard selected an excluded member") };
return _smelt_tmp_5.radius;
} else {
return 0.0;
}
}"#
    );
}

/// A TypeScript `static` class constant lowers to a materialized
/// static field and a qualified read resolves to its concrete literal value.
#[test]
fn emits_typescript_static_const_read() {
    let source = source_for(
        r#"
class Config {
  static readonly LIMIT: number = 42;
  static readonly NAME: string = "smelt";
}
export function limit(): number {
  return Config.LIMIT;
}
export function label(): string {
  return Config.NAME;
}
"#,
    );

    assert_eq!(
        rust_items(
            &source,
            &[
                "fn __smelt_static_limit",
                "fn __smelt_static_name",
                "fn limit",
                "fn label"
            ]
        ),
        r#"fn __smelt_static_limit() -> f64 {
42.0
}

fn __smelt_static_name() -> JsString {
JsString::from("smelt")
}

fn limit() -> f64 {
return 42.0;
}

fn label() -> JsString {
return JsString::from("smelt");
}"#
    );
}

/// `reduceRight` renders the same typed `fold` as `reduce`, over the
/// `enumerate()`d elements reversed: the callback sees the last element first
/// with its original index, and a seedless fold starts from the last element.
#[test]
fn reduce_right_folds_the_reversed_enumeration() {
    let source = source_for(
        r"
export function order(values: string[]): string {
  return values.reduceRight((acc, value, index) => acc + value + index, '');
}
export function lastMinusRest(values: number[]): number {
  return values.reduceRight((acc, value) => acc - value);
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn order", "fn last_minus_rest"]),
        r#"fn order(values: SmeltList<JsString>) -> JsString {
let _smelt_tmp_2: JsString = values.borrow().iter().enumerate().rev().fold(JsString::new(), |smelt_acc, (smelt_index, smelt_item)| { let smelt_item = (*smelt_item).clone(); let smelt_index = smelt_index as f64; let smelt_array = &values; { let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: JsString, closure_arg_1: JsString, closure_arg_2: i64, _arg0: &SmeltList<JsString>| {
let _smelt_tmp_3: JsString = closure_arg_0.clone() + &closure_arg_1.clone();
let _smelt_tmp_4: JsString = _smelt_tmp_3.clone() + &JsString::from(smelt_number_to_string(closure_arg_2 as f64));
_smelt_tmp_4.clone()
}); (smelt_callback)(smelt_acc, smelt_item, ((smelt_index as f64).trunc() as i64), smelt_array) } });
return _smelt_tmp_2;
}

fn last_minus_rest(values: SmeltList<f64>) -> f64 {
let _smelt_tmp_2: f64 = { let smelt_reduce_items = values.borrow(); let mut smelt_reduce_items = smelt_reduce_items.iter().enumerate().rev(); let (_, smelt_first) = smelt_reduce_items.next().expect("Reduce of empty array with no initial value"); smelt_reduce_items.fold(smelt_first.clone(), |smelt_acc, (smelt_index, smelt_item)| { let smelt_item = (*smelt_item).clone(); let smelt_index = smelt_index as f64; let smelt_array = &values; { let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: f64, closure_arg_1: f64, _arg0: i64, _arg1: &SmeltList<f64>| {
let _smelt_tmp_2: f64 = closure_arg_0 - closure_arg_1;
_smelt_tmp_2
}); (smelt_callback)(smelt_acc, smelt_item, ((smelt_index as f64).trunc() as i64), smelt_array) } }) };
return _smelt_tmp_2;
}"#
    );
}

#[test]
fn optional_chain_named_group_read_uses_named_group_owned() {
    // A named capture group read reached through an optional chain
    // (`m?.groups.result`) keeps the typed match accessor inside the
    // `.as_ref().map(..)` closure.
    let source = source_for(
        r"
export function firstGroup(input: string): string | undefined {
  const m = /(?<result>\w+)/.exec(input);
  return m?.groups.result;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn first_group"]),
        r#"fn first_group(input: JsString) -> Option<JsString> {
let _smelt_tmp_2: SmeltRegExp = SmeltRegExp::new(JsString::from("(?<result>\\w+)"), JsString::new());
let m: Option<SmeltMatch> = _smelt_tmp_2.exec(&input.clone());
let _smelt_tmp_4: Option<SmeltMatch> = m.as_ref().map(|_smelt_value| _smelt_value.clone());
let _smelt_tmp_5: Option<JsString> = _smelt_tmp_4.as_ref().and_then(|_smelt_value| _smelt_value.named_group_owned("result"));
return _smelt_tmp_5;
}"#
    );
}

#[test]
fn loop_body_if_guard_condition_renders_at_bool_type() {
    // An `if`-guard nested inside a for-of loop
    // is emitted through the structured while-header path when its then-branch
    // can reach the header again via the enclosing loop's back edge. The header
    // condition (a truthiness `PrimitiveCast`/`ToBool` of an optional) is
    // rendered at the switch local's boolean type.
    let source = source_for(
        r#"
export function joinTruthy(parts: (string | undefined)[]): string {
  let out = "";
  for (const part of parts) {
    if (part) {
      out += part;
    }
  }
  return out;
}
"#,
    );

    assert_eq!(
        rust_items(&source, &["fn join_truthy"]),
        r#"fn join_truthy(parts: SmeltList<Option<JsString>>) -> JsString {
let mut part: Option<JsString>;
let mut _smelt_tmp_4: usize;
let mut _smelt_tmp_5: bool;
let mut _smelt_tmp_6: bool;
let mut _smelt_tmp_7: JsString;
let mut _smelt_tmp_8: JsString;
let mut out: JsString = JsString::new();
let mut _smelt_tmp_3: usize = 0;
loop {
_smelt_tmp_4 = parts.len();
_smelt_tmp_5 = _smelt_tmp_3 < _smelt_tmp_4;
if !(_smelt_tmp_5) { break; }
part = parts.borrow().get(_smelt_tmp_3).cloned().unwrap_or_else(|| None::<JsString>);
_smelt_tmp_6 = part.clone().clone().is_some_and(|value| !value.is_empty());
if _smelt_tmp_6 {
_smelt_tmp_7 = part.clone().expect("optional value was absent after narrowing");
_smelt_tmp_8 = out + &_smelt_tmp_7;
out = _smelt_tmp_8;
_smelt_tmp_3 = _smelt_tmp_3 + 1;
continue;
} else {
_smelt_tmp_3 = _smelt_tmp_3 + 1;
continue;
}
}
return out;
}"#
    );
}

/// A fixed-arity user callback (`(item, item2, item3) => ...`) supplied where
/// a variadic `(...args: T[]) => R` is expected must type each fixed parameter
/// as the rest *element* `T`, not the rest list `T[]`. Otherwise `item` is a
/// `SmeltList`. Mirrors es-toolkit's `unzipWith`.
#[test]
fn fixed_params_against_variadic_hint_take_element_type() {
    let source = source_for(
        r"
function unzipWith<T, R>(target: readonly T[][], iteratee: (...args: T[]) => R): R[] {
  return target.map(group => iteratee(...group));
}

export function run(zipped: Array<[number, number, number]>): number[] {
  return unzipWith(zipped, (item, item2, item3) => item + item2 + item3);
}
",
    );

    // The three-parameter user iteratee must not receive a list as its first
    // fixed parameter; each fixed param takes the rest element type.
    assert!(
        !source.contains(
            "|closure_arg_0: SmeltList<SmeltUnknown>, closure_arg_1: SmeltUnknown, closure_arg_2: SmeltUnknown|"
        ),
        "a fixed callback param against a variadic hint must not be a list\n{source}"
    );
}

/// A predeclared (hoisted) local that is
/// assigned inside a loop body keeps its `mut` binding. Rust's
/// definite-assignment rules reject reassigning an immutable hoisted local from
/// a loop body, so the repeating-region rule must still fire here even after it
/// was scoped to predeclared bindings.
#[test]
fn hoisted_local_assigned_in_loop_keeps_mut() {
    let source = source_for(
        r"
export function lastVal(items: number[]): number {
  let found: number = 0;
  for (const x of items) {
    found = x;
  }
  return found;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn last_val"]),
        r"fn last_val(items: SmeltList<f64>) -> f64 {
let mut x: f64;
let mut _smelt_tmp_4: usize;
let mut _smelt_tmp_5: bool;
let mut found: f64 = 0.0;
let mut _smelt_tmp_3: usize = 0;
loop {
_smelt_tmp_4 = items.len();
_smelt_tmp_5 = _smelt_tmp_3 < _smelt_tmp_4;
if !(_smelt_tmp_5) { break; }
x = items.borrow().get(_smelt_tmp_3).cloned().unwrap_or_else(|| 0.0);
found = x;
_smelt_tmp_3 = _smelt_tmp_3 + 1;
}
return found;
}"
    );
}

/// A parameter property with a default stores the applied default, not the
/// `Option` ABI slot.
///
/// The parameter-property assignment is emitted after the default prelude, so
/// the field keeps its declared concrete type.
#[test]
fn defaulted_parameter_property_stores_the_applied_default() {
    let source = source_for(
        r"
class Sized {
  constructor(readonly size = 10) {}
}
const sized = new Sized();
",
    );

    assert_eq!(
        rust_items(&source, &["struct Sized_", "impl Sized_ {"]),
        r"struct Sized_ {
size: f64,
}

impl Sized_ {
fn new(size: Option<f64>) -> Self {
let mut this: Self = Sized_ { size: 0.0 };
let size_1: f64 = size.clone().unwrap_or(10.0);
this.size = size_1;
return this;
}
}"
    );
}

#[test]
fn strict_null_comparison_distinguishes_a_stored_null_from_an_absent_slot() {
    // JavaScript keeps `null` and `undefined` distinct under `===`, and an
    // erased `Option` payload has room for both, so an assigned `null` is
    // stored as `Some(SmeltUnknown::Null)`.
    let source = source_for(
        r"
export function clearSlot(handle: unknown): number {
  let slot: unknown | null = null;
  slot = handle;
  slot = null;
  if (slot !== null) {
    return 1;
  }
  return 0;
}
",
    );

    assert_eq!(
        rust_items(&source, &["fn clear_slot"]),
        r"fn clear_slot(handle: SmeltUnknown) -> f64 {
let mut slot: Option<SmeltUnknown> = Some(SmeltUnknown::Null);
slot = Some(handle.clone());
slot = Some(SmeltUnknown::Null);
let _smelt_tmp_2: bool = !(slot.as_ref().is_some_and(|smelt_value| matches!(smelt_value, SmeltUnknown::Null)));
if _smelt_tmp_2 {
return 1.0;
} else {
return 0.0;
}
}"
    );
}

#[test]
fn a_data_view_accessor_is_a_fallible_call_with_the_specs_to_index() {
    // Round 28's throwing rvalue. `DataView.prototype.getX/setX` throws a
    // catchable `RangeError` when the window it addresses leaves the view, and
    // MIR's exception edges live on TERMINATORS — `Terminator::Call` carries
    // `unwind`, a `Statement::Assign` carries nothing — so an accessor lowered
    // as an rvalue could not reach a `try` at all. It is therefore a call to
    // `Rvalue::DataViewAccess` behind the generated throwing adapter, the
    // same route `JSON.parse` takes. See
    // `blocker-logs/standards-throwing-rvalue.md`; executed end to end by
    // `examples/typescript/end-to-end/88_data_view_range_error`.
    let read = source_for("export function f(view: DataView) { return view.getInt16(1); }");
    assert!(
        read.contains("smelt_data_view_get_throwing("),
        "a read must go through the throwing adapter:\n{read}"
    );
    // The `?` is what carries the failure to the enclosing handler (or out of
    // the function, when the source wrote no `try`).
    assert!(
        read.contains(", SmeltTypedArrayKind::Int16, (1.0) as f64, false)?"),
        "the adapter call must pass kind, an f64 offset and the byte order, and end in `?`:\n{read}"
    );
    // The offset stays an `f64` so the adapter can run `ToIndex` on it: a
    // `max(0.0) as usize` at the emit site would answer byte 0 for
    // `getInt16(-1)`, which Node throws on.
    assert!(
        !read.contains("SmeltTypedArrayKind::Int16, ((1.0) as f64).max(0.0) as usize"),
        "the accessor's offset must not be clamped at the emit site:\n{read}"
    );
    assert!(
        read.contains("fn smelt_data_view_byte_index(offset: f64) -> Option<usize>")
            && read.contains("if integer < 0.0 || integer > 9007199254740991.0"),
        "`ToIndex` must reject a negative or non-representable index:\n{read}"
    );
    // The payload is the branded record a source `catch` inspects, carrying
    // Node's own message because a program can read `error.message`.
    assert!(
        read.contains("\"RangeError\"")
            && read.contains("Offset is outside the bounds of the DataView"),
        "the throw must be a branded `RangeError` with Node's message:\n{read}"
    );
    // The window bound is stated ONCE, in the checked pair, so the checked and
    // unchecked halves cannot disagree about where the view ends.
    assert!(
        read.contains("fn in_bounds(&self, kind: SmeltTypedArrayKind, offset: usize) -> bool"),
        "the bound belongs to the view type, not to the adapter:\n{read}"
    );

    // Pay-for-use has a FLOOR: the `RangeError` payload is a branded
    // `SmeltUnknown::Object`, so a program whose only erased value is that
    // throw still needs the carrier. Without it the adapters landed in a
    // prelude block the program never entered and the crate did not compile
    // (E0425) — the same clause `needs_uri_decode_runtime` and
    // `needs_base64_runtime` already have in `needs_unknown_type`.
    assert!(
        read.contains("enum SmeltUnknown"),
        "an accessor-only program must still carry the erased carrier:\n{read}"
    );

    // A write takes its value between the offset and the byte-order flag, and
    // is fallible on the same channel.
    let write = source_for("export function f(view: DataView) { view.setFloat64(0, 1.5, true); }");
    assert!(
        write.contains("smelt_data_view_set_throwing(")
            && write.contains(", SmeltTypedArrayKind::Float64, (0.0) as f64, 1.5, true)?"),
        "a write must go through the throwing adapter with the value in the middle:\n{write}"
    );

    // Pay-for-use: the adapters are gated on an accessor CALL, which the
    // rvalue-based dependency scan cannot see, so a byte-family program with no
    // accessor must carry neither of them.
    let no_accessor =
        source_for("export function f() { return new DataView(new ArrayBuffer(8)); }");
    assert!(
        !no_accessor.contains("smelt_data_view_get_throwing")
            && !no_accessor.contains("smelt_data_view_set_throwing"),
        "a program with no accessor must not pay for the throwing adapters:\n{no_accessor}"
    );
}

#[test]
fn every_byte_buffer_host_constructs_its_concrete_value() {
    // The whole byte family is concrete now, and each half in the shape its
    // surface asks for: `SharedArrayBuffer` is the storage type with its
    // species flag set (the two constructors differ in a tag, an `instanceof`
    // answer and the growth members, and in nothing about the bytes), while
    // `DataView` is its own type because its element width is an argument of
    // every accessor rather than a field of the value.
    //
    // The erased byte-backed record is what each reaches through its own
    // boundary adapter, and it is built by the SAME record builder
    // `smelt_reflected_construct` uses — which is what keeps a directly built
    // record indistinguishable from a reflectively built one. es-toolkit's
    // `clone` takes the reflected form where its `cloneDeepWith` takes the
    // direct one, and its specs compare the two results against each other.
    for (source, expected) in [
        (
            "export function f() { return new SharedArrayBuffer(8); }",
            "SmeltArrayBuffer::new_shared(",
        ),
        (
            "export function f() { return new DataView(new ArrayBuffer(8), 1, 2); }",
            "SmeltDataView::over_buffer(",
        ),
        (
            "export function f() { return new ArrayBuffer(8); }",
            "SmeltArrayBuffer::new(",
        ),
        (
            "export function f() { return new Uint8Array(8); }",
            "SmeltTypedArray::with_length(",
        ),
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains(expected),
            "expected `{source}` to construct `{expected}`:\n{generated}"
        );
        // Not through the erased host constructor: the assertion names the
        // CALL spelling, since the prelude's own definition of that helper is
        // still emitted for the reflected path.
        for kind in ["sharedarraybuffer", "dataview", "arraybuffer", "uint8array"] {
            assert!(
                !generated.contains(&format!("smelt_reflected_construct(\"{kind}\"")),
                "a concrete construction must not route through the erased `{kind}` constructor:\n{generated}"
            );
        }
    }
}

#[test]
fn math_round_uses_the_javascript_tie_rule() {
    // JavaScript rounds a tie toward +∞; Rust's `f64::round` rounds a tie away from
    // zero. They disagree for every negative value whose fraction is exactly 0.5 —
    // `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in Rust — which is what
    // made es-toolkit's `round` specs disagree. `floor`/`ceil`/`trunc` mean the same
    // thing in both languages and must keep mapping straight to their `f64` methods.
    let generated = source_for("export function f(x: number): number { return Math.round(x); }");
    assert!(
        generated.contains("fn smelt_math_round(value: f64) -> f64"),
        "the JavaScript rounding helper must be emitted:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("smelt_math_round("),
        "`Math.round` must route through the helper:\n{generated}"
    );
    assert!(
        !program.contains(".round()"),
        "`Math.round` must not use Rust's tie-away-from-zero rounding:\n{generated}"
    );
    let floor = source_for("export function f(x: number): number { return Math.floor(x); }");
    assert!(
        floor.contains(".floor()") && !floor.contains("fn smelt_math_round"),
        "`Math.floor` agrees between the languages and must not pull the helper in:\n{floor}"
    );
}

#[test]
fn an_assertion_overload_still_emits_its_call() {
    // An assertion overload (`asserts condition`) returns void at runtime, and
    // `function_declaration` records exactly that for the implementation signature.
    // `overload_signature` lowered the annotation structurally instead and got
    // `Bool` (a `TSTypePredicate` is boolean-shaped). The selected overload's return
    // type is what types the call's destination, so the call site ended up with a
    // `Bool` destination for a `None`-returning function and the emitter DROPPED the
    // call, leaving only its arguments evaluated. es-toolkit `invariant` is that
    // shape — two `asserts condition` overloads plus an implementation — and all
    // four of its specs asserted against a call that never happened.
    let generated = source_for(
        r"
export function invariant(condition: unknown, message: string): asserts condition;
export function invariant(condition: unknown, error: Error): asserts condition;
export function invariant(condition: unknown, message: string | Error): asserts condition {
  if (condition) {
    return;
  }
  throw new Error('boom');
}

export function run(): void {
  invariant(false, 'boom');
}
",
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    let run_body = program
        .split_once("fn run()")
        .map(|(_, rest)| rest)
        .and_then(|rest| {
            rest.split_once("\nfn ")
                .map_or(Some(rest), |(body, _)| Some(body))
        })
        .unwrap_or_default();
    assert!(
        run_body.contains("invariant("),
        "the assertion call must be emitted, not folded away:\n{generated}"
    );
}

/// A `return` inside a `try` that has a `finally` must still run the finalizer.
///
/// MIR made the finalizer the *fall-through* exit of the `try` body, so a
/// `Return` terminator bypassed it and the cleanup vanished from the generated
/// Rust altogether. es-toolkit's `areObjectsEqual` clears its recursion `Map` in
/// exactly that shape, and the leaked entries made
/// `isEqualWith({ constructor: [1] }, { constructor: ['1'] })` answer `true`.
/// `lower_return` now re-lowers the finalizer inline ahead of the return, so the
/// cleanup has to appear on the return path.
#[test]
fn finally_body_is_emitted_on_the_return_path() {
    let source = source_for(
        r#"
export function cleanupOnReturn(seen: Map<string, number>): number {
  seen.set("a", 1);
  try {
    return 7;
  } finally {
    seen.delete("a");
  }
}
const out = cleanupOnReturn(new Map<string, number>());
console.log(out);
"#,
    );

    let start = source
        .find("fn cleanup_on_return")
        .expect("cleanupOnReturn present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("cleanupOnReturn closing brace");
    let body = &after[..end];

    // The `seen.delete("a")` cleanup must survive on the return path. Before the
    // fix the function body contained no removal at all.
    assert!(
        body.contains(".remove("),
        "the finalizer's Map delete must be emitted, got:\n{body}"
    );
    assert!(
        body.contains('7'),
        "the try body's return value must survive the finalizer, got:\n{body}"
    );
}

/// Nested finalizers unwind inner-to-outer ahead of the return.
///
/// The inline duplication has to walk the whole lexical finalizer stack in
/// JavaScript's unwind order, not just the innermost clause.
#[test]
fn nested_finally_bodies_are_emitted_inner_to_outer() {
    let source = source_for(
        r#"
export function nestedCleanup(log: string[]): string {
  try {
    try {
      return "value";
    } finally {
      log.push("inner");
    }
  } finally {
    log.push("outer");
  }
}
const out = nestedCleanup([]);
console.log(out);
"#,
    );

    let start = source
        .find("fn nested_cleanup")
        .expect("nestedCleanup present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("nestedCleanup closing brace");
    let body = &after[..end];

    let inner = body.find("\"inner\"").expect("inner finalizer emitted");
    let outer = body.find("\"outer\"").expect("outer finalizer emitted");
    assert!(
        inner < outer,
        "the inner finalizer must be emitted before the outer one on the return \
         path (inner={inner}, outer={outer}):\n{body}"
    );
}

/// `Object.getOwnPropertySymbols` yields symbol VALUES, not descriptions.
///
/// A symbol-keyed property is stored under `"__smelt_symbol:<description>"`, and a
/// symbol value is `SmeltUnknown::Symbol(description)`. The projection used to
/// strip the prefix and hand back a bare `String`, which broke both directions of
/// the round trip: `source[symbols[i]]` looked up the *unprefixed* string key and
/// missed, and `target[symbols[i]] = v` created a plain string property that no
/// symbol lookup or symbol enumeration could see. Re-tagging the description keeps
/// the property-key mapping able to rebuild the internal key.
#[test]
fn reflected_symbol_keys_keep_their_symbol_tag() {
    let source = source_for(
        r"
export function copySymbols(source: any, target: any): void {
  const symbols = Object.getOwnPropertySymbols(source);
  for (let i = 0; i < symbols.length; i++) {
    target[symbols[i]] = source[symbols[i]];
  }
}
const out: any = {};
copySymbols({}, out);
console.log(out);
",
    );

    assert!(
        source.contains("SmeltUnknown::Symbol(description.into())"),
        "the symbols projection must re-tag the stripped description as a symbol \
         value:\n{source}"
    );
    assert!(
        !source.contains("strip_prefix(\"__smelt_symbol:\").map(str::to_owned)"),
        "the symbols projection must not hand back bare descriptions:\n{source}"
    );
}

/// A call through a function *value* pads the parameters the source omitted.
///
/// A callee's optional trailing parameters are part of the Rust `dyn Fn` type
/// the value lowered to, so JavaScript's "omitted arguments are `undefined`"
/// has to be materialized at the call site. `indirect_call_args_expr` rendered
/// only the arguments the source supplied, so a `try`-wrapped `work()` against
/// an `Rc<dyn Fn(Option<String>) -> f64>` emitted `(work)()` and rustc rejected
/// it with E0057 ("this function takes 1 argument but 0 arguments were
/// supplied") — the radash `async.test.ts` blocker, where a captured
/// `const fakeWork = (name?: string) => …` was invoked as `fakeWork()` inside a
/// `try`.
///
/// The `try` matters: it routes the call through the throwing-call terminator,
/// which renders its callee with `rendered_call` instead of the statement path that
/// receives an already-padded argument list from MIR. The load-bearing
/// assertion is the emitted `None` padding inside `catch_unwind`: the
/// static-call ladder has always padded missing trailing parameters, and this
/// proves the value-callable form now agrees with it instead of dropping the
/// arity.
#[test]
fn a_value_call_pads_the_optional_parameters_the_source_omitted() {
    let source = source_for(
        r"
export function outer(): number {
  const work = (name?: string): number => 7;
  const runA = (): number => {
    try {
      return work();
    } catch (e) {
      return 0;
    }
  };
  const runB = (): number => {
    try {
      return work();
    } catch (e) {
      return 1;
    }
  };
  return runA() + runB();
}
",
    );

    assert!(
        source.contains("|closure_arg_0: Option<JsString>|"),
        "the callback's optional parameter must survive into its Rust type:\n{source}"
    );
    // The padding is a bare `None`: `work` is a closure VALUE, whose own
    // signature types the `Option` (`closure_value_padding_expr`). The
    // requirement is unchanged — the argument is present, not dropped; only
    // the turbofish the caller's view used to spell is gone, because that view
    // can be less precise than the callee's real signature.
    assert!(
        source.contains("(work)(None)"),
        "the omitted optional argument must be padded, not dropped (E0057):\n{source}"
    );
    assert!(
        !source.contains("(work)()"),
        "a zero-argument call against a one-parameter `Fn` does not compile:\n{source}"
    );
}

#[test]
fn an_element_read_in_an_optional_slot_stays_fallible() {
    // `arr[i]` has TypeScript type `T` (there is no
    // `noUncheckedIndexedAccess` in play), so `last<T>(arr: T[]): T | undefined`
    // used to lower to an INFALLIBLE read that was then re-wrapped:
    // `Some(arr.get(..).cloned().unwrap_or(Default::default()))`. That makes
    // `last([])` answer `Some(0.0)` where JavaScript answers `undefined`. The
    // read is the natural `Option` producer, so a coercion into an `Option<T>`
    // slot must keep the miss a miss.
    let source = source_for(
        r"
export function last<T>(arr: readonly T[]): T | undefined {
  return arr[arr.length - 1];
}

export function head<T>(arr: readonly T[]): T | undefined {
  return arr[0];
}
",
    );

    assert!(
        !source.contains("Some(arr.get("),
        "the read must not be made total and re-wrapped in `Some(..)`:\n{source}"
    );
    assert!(
        !source.contains("unwrap_or(Default::default())"),
        "an out-of-range element must be `None`, not the element default:\n{source}"
    );
    // Counted in the PROGRAM half only: the shared runtime prelude has its own
    // `.cloned()` calls (the erased promise's continuation members clone their
    // handler arguments), and this assertion is about the two lowered functions.
    assert_eq!(
        program_body(&source).matches(".cloned();").count(),
        2,
        "both functions must return the bare `get(..).cloned()` option:\n{source}"
    );
}

#[test]
fn a_typescript_element_read_does_not_wrap_a_negative_index() {
    // `arr[-1]` is `undefined` in JavaScript. It is a PROPERTY key, not a
    // position: only `Array.prototype.at` counts back from the end. The shared
    // index normalizer applied Python's `len + index` wrap to both frontends, so
    // a generated TypeScript `arr[-1]` silently answered `arr[arr.length - 1]` —
    // a wrong VALUE, with no panic and no diagnostic to notice it by. This is
    // what moved es-toolkit's `at` rows: `at(['a','b','c'], [-4])` normalizes to
    // `-1` in source and must then miss, not wrap around to `'c'`.
    let source = source_for(
        r"
export function pick(arr: number[], index: number): number {
  return arr[index];
}
",
    );

    assert!(
        !source.contains("if index < 0 { len + index }"),
        "a TypeScript element read must not count a negative index from the end:\n{source}"
    );
    assert!(
        source.contains("let smelt_normalized = index as i64;"),
        "the index must reach the out-of-range machinery unchanged:\n{source}"
    );
}

#[test]
fn a_python_element_read_still_wraps_a_negative_index() {
    // The contrast that makes the rule above a per-site policy rather than a
    // global one: Python's `xs[-1]` IS the last element. One crate can mix
    // TypeScript and Python modules, so the wrap cannot be switched off in the
    // emitter — it is carried on the lowered place, from the source language of
    // the file the expression was written in.
    let source = source_for_py(
        r"
values: list[int] = [1, 2, 3]
last_value: int = values[-1]
",
    );

    assert!(
        source.contains("smelt_py_list_read(&")
            && source.contains("if index < 0 { index.saturating_add("),
        "a Python element read must still count a negative index from the end:\n{source}"
    );
}

#[test]
fn an_element_write_still_rejects_a_negative_normalized_index() {
    // The read relaxation must not leak into the WRITE path. A store to a slot
    // that does not exist cannot silently pick a different slot, and
    // `usize::MAX` would ask the resize helper for an impossible allocation, so
    // the write keeps the panic.
    let source = source_for(
        r"
export function put(arr: number[], index: number, value: number): void {
  arr[index] = value;
}
",
    );

    assert!(
        source
            .contains("usize::try_from(smelt_normalized).expect(\"negative index out of bounds\")"),
        "a write to a negative normalized index must still fail loudly:\n{source}"
    );
}

#[test]
fn an_element_read_in_a_concrete_slot_stays_total() {
    // The erased-target rule must not leak into a CONCRETE destination. There
    // is no `undefined` to put in a `Vec<f64>` or a `Vec<String>`, so a store
    // into a concrete list keeps the existing total read and its element
    // missing value. Making that read fallible would not type-check, and
    // widening the slot to hold a hole is a storage question, not a
    // read-coercion one.
    let source = source_for(
        r"
export function copy(b: string[], n: number[], i: number): void {
  const s: string[] = [''];
  const m: number[] = [0];
  s[0] = b[i];
  m[0] = n[i];
}
",
    );

    // The subject is that the read stays TOTAL (`.cloned().unwrap_or(default)`,
    // not a fallible read). The trailing `.clone()` these once carried was the
    // redundant second copy of an already-owned index read, removed by
    // `index_place_read_is_owned`; the `;` that ends each expected read below
    // asserts it stays absent.
    assert!(
        source.contains(".cloned().unwrap_or_else(|| JsString::new());"),
        "a concrete `string` slot keeps the total read:\n{source}"
    );
    assert!(
        source.contains(".cloned().unwrap_or_else(|| 0.0);"),
        "a concrete `number` slot keeps the total read:\n{source}"
    );
    assert!(
        !source.contains(".cloned().map(|value| "),
        "no fallible erased read belongs in a concrete slot:\n{source}"
    );
}

/// JavaScript `&&` and `||` select an OPERAND; the result type is the union of
/// the operand types, not `boolean`.
///
/// Modelling them as boolean operators discards the value. es-toolkit's
/// `expect(error instanceof Error && error.message).toBe('test')` became a
/// `bool` compared against a string — statically false, so the assertion folded
/// to `!(false)` and tested nothing. Here the guard's right operand is a
/// concrete `String`, so the selected value must reach the caller as a string.
#[test]
fn a_logical_and_emits_the_selected_operand_not_a_boolean() {
    let source = source_for(
        r"
export function pick(flag: boolean, value: string): string | boolean {
  return flag && value;
}
",
    );

    assert!(
        source.contains("fn pick(flag: bool, value: JsString) -> SmeltUnion"),
        "`boolean && string` is the union of its operand types:\n{source}"
    );
    assert!(
        source.contains("{ SmeltUnion2::M1(value.clone()) } else { SmeltUnion2::M0(flag) }"),
        "each arm must carry the operand JavaScript selects, not a boolean:\n{source}"
    );
}

/// The operand-selecting rule must leave the common case alone.
///
/// Both operands boolean means the union of the operand types IS `bool`, so the
/// existing boolean lowering is exactly right. Widening it would route ordinary
/// guards through a union — and one step further through `SmeltUnknown` — for
/// no gain.
#[test]
fn a_boolean_logical_and_stays_a_plain_boolean() {
    let source = source_for(
        r"
export function both(a: boolean, b: boolean): boolean {
  return a && b;
}
",
    );

    assert!(
        source.contains("fn both(a: bool, b: bool) -> bool"),
        "a boolean `&&` keeps its boolean signature:\n{source}"
    );
    assert!(
        !source.contains("SmeltUnknown"),
        "a boolean `&&` must not erase anything:\n{source}"
    );
}

/// A condition only observes truthiness, so it keeps the short-circuiting
/// branch shape rather than materializing the selected operand.
///
/// `truthy(a && b) == truthy(a) && truthy(b)`, so `if (a && b)` has no reason to
/// build a union value and test it. The branch form also keeps the right
/// operand's own statements inside the branch: flattening the condition to a
/// single `&&` over two already-computed temporaries evaluates the right-hand
/// call unconditionally.
#[test]
fn a_logical_condition_short_circuits_the_right_operand() {
    let source = source_for(
        r"
function valid(value: number): boolean {
  return value > 0;
}

export function count(a: number, b: number): number {
  if (valid(a) && valid(b)) {
    return 1;
  }
  return 0;
}
",
    );

    let first = source
        .find("valid(a)")
        .expect("the left guard should be called");
    let second = source
        .find("valid(b)")
        .expect("the right guard should be called");
    let branch = source
        .find("if _smelt_tmp")
        .expect("the guard should branch");
    assert!(
        first < branch && branch < second,
        "the right operand must be evaluated inside the branch, not before it:\n{source}"
    );
}

/// A Python `__add__` method becomes a concrete Rust `Add<Rhs>` implementation.
#[test]
fn python_add_dunder_emits_a_typed_rust_trait_impl() {
    let source = source_for_py(
        r#"
class Vector:
    def __init__(self, x: int) -> None:
        self.x = x
    def __add__(self, other: "Vector") -> "Vector":
        return Vector(self.x + other.x)

def combine(left: Vector, right: Vector) -> Vector:
    return left + right
"#,
    );

    assert!(
        source.contains("impl ::std::ops::Add<Vector> for Vector"),
        "Python __add__ must map to Rust's typed Add trait:\n{source}"
    );
    assert!(
        source.contains("type Output = Vector;")
            && source.contains("fn add(self, rhs: Vector) -> Self::Output { self.__add__(rhs) }"),
        "the Rust adapter must preserve the concrete operand and output types:\n{source}"
    );
    assert!(
        source.contains("left.clone() + right.clone()"),
        "Python + must use the typed Rust operator without consuming source bindings:\n{source}"
    );
}

/// A call with more arguments than any declared overload accepts must still
/// pass every argument.
///
/// JavaScript forwards the whole argument list regardless of declared arity,
/// and an overloaded callable interface stores its implementation in one erased
/// variadic `__smelt_call` slot, so the call is executable. Falling back to the
/// first declared signature truncated the argument list at the adapter and
/// called the callee with nothing.
///
/// `ArityReporter`'s overloads return different types on purpose: an overload
/// set that shares ONE result is stored as its merged concrete signature
/// instead (see `overload_set_sharing_a_result_is_one_concrete_slot`), so only
/// a result-dependent set keeps the erased slot this test guards.
#[test]
fn call_beyond_every_declared_overload_arity_keeps_its_arguments() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const reporter = arityReporter();
// @ts-ignore
const seen = reporter(1, 2, 3);
"
    ));

    assert!(
        source.contains(
            "smelt_callback.call(vec![smelt_arg0.clone(), smelt_arg1.clone(), smelt_arg2.clone()])"
        ),
        "all three arguments must reach the erased callable:\n{source}"
    );
}

/// A type the module declares shadows the global lib type of the same name.
///
/// TypeScript resolves `Set<E>` lexically, so under a module-scope
/// `interface Set<E> { (key: string, value: number): void }` it is that
/// callable interface, not the builtin collection. Matching the lib table by
/// spelling alone turned the class field into a `SmeltJsSet<_>` collection,
/// and `ctx.set('k', 1)` then failed as an unknown class method.
#[test]
fn module_declared_type_shadows_the_global_lib_type() {
    let source = source_for(
        "interface Set<E> {
  (key: string, value: number): void;
}
class Ctx {
  total = 0;
  set: Set<string> = (key: string, value: number) => {
    this.total += key.length + value;
  };
}
const ctx = new Ctx();
ctx.set('ab', 3);
",
    );

    assert!(
        source.contains("    set: Set,") && !source.contains("set: SmeltJsSet<"),
        "the module's `Set` must not lower to the builtin collection:\n{source}"
    );
    assert!(
        source.contains("__smelt_call: ::std::rc::Rc<dyn Fn(JsString, f64) -> ()>"),
        "the field keeps the module's own callable `Set` interface:\n{source}"
    );
}

/// Same-arity overloads that differ only by parameter type are selected by the
/// argument's type, and the interface's call slot is erased so one field can
/// store either of the two incompatible Rust `Fn` shapes.
#[test]
fn same_arity_call_signatures_are_selected_by_argument_type() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const pick = byArgumentType();
const numeric = pick(41);
const text = pick('a');
"
    ));

    assert!(
        source.contains("__smelt_call: SmeltErasedFunction"),
        "an overload set that differs by parameter type needs the erased slot:\n{source}"
    );
    assert!(
        source.contains("numeric: f64") && source.contains("text: JsString"),
        "each call must take the overload its argument type matches:\n{source}"
    );
}

#[test]
fn guarded_default_insert_becomes_a_single_entry_probe() {
    // `smelt_mir::opt::DictDefaultInsertElision` deletes the
    // `if (!Object.hasOwn(m, k)) { m[k] = []; }` guard, because the entry
    // mutation that follows inserts the same empty list through
    // `entry_or_insert`. The emitted group loop must therefore hash the key
    // once, not three times (probe, guarded insert, entry).
    let source = source_for(
        r"
export function groupBy<T, K extends PropertyKey>(
  arr: readonly T[],
  getKeyFromItem: (item: T, index: number) => K
): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  for (let i = 0; i < arr.length; i++) {
    const item = arr[i];
    const key = getKeyFromItem(item, i);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    result[key].push(item);
  }
  return result;
}
",
    );
    let body = emitted_function_body(&source, "fn group_by(");

    assert!(
        !body.contains("contains_key"),
        "the membership probe is gone: {body}"
    );
    assert_eq!(
        body.matches("entry_or_insert").count(),
        1,
        "exactly one entry probe remains: {body}"
    );
}

#[test]
fn count_by_accumulator_becomes_a_single_entry_probe() {
    // `smelt_mir::opt::DictEntryUpdate` fuses the
    // `result[key] = (result[key] ?? 0) + 1` read/compute/write-back triple, so
    // the emitted loop hashes the key ONCE (one entry accessor) instead of
    // twice (a `get` plus an `insert`).
    let source = source_for(
        r"
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K
): Record<K, number> {
  const result = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    result[key] = (result[key] ?? 0) + 1;
  }
  return result;
}
",
    );
    let body = emitted_function_body(&source, "fn count_by<");

    assert_eq!(
        body.matches("entry_or_insert").count(),
        1,
        "exactly one entry probe remains: {body}"
    );
    assert!(
        !body.contains(".insert("),
        "the write-back probe is gone: {body}"
    );
    assert!(!body.contains(".get(&"), "the read probe is gone: {body}");
}

/// A write through a receiver that is still optional-typed is an lvalue (H23).
///
/// `tsc` proved the receiver present (the assignment above narrows it), so the
/// write unwraps in place — `as_mut()`, never a copy, which would drop the
/// write — and a REFERENCE class keeps its handle (`.0.borrow_mut()`). Before
/// this the lvalue fell through to the field READ expression, which is not an
/// lvalue at all (E0070), and the read spelled a field of the handle rather than
/// of its interior (E0609).
#[test]
fn a_field_write_through_an_optional_receiver_unwraps_in_place() {
    let source = source_for(
        r"
class Slot {
  label = '';
}
export function label(slots: Record<string, Slot>, key: string, text: string): string {
  let slot: Slot | undefined = slots[key];
  if (!slot) {
    slot = new Slot();
    slots[key] = slot;
  }
  slot.label = text;
  return slot.label;
}
console.log(label({}, 'a', 'x'));
",
    );

    let body = emitted_function_body(&source, "fn label(");
    assert!(
        body.contains("expect(\"optional value was absent after narrowing\")"),
        "the receiver is unwrapped where `tsc` proved it present: {body}"
    );
    assert!(
        body.contains(".0.borrow_mut().label ="),
        "the write goes through the reference class's handle: {body}"
    );
    assert!(
        body.contains("_smelt_value.0.borrow().label.clone()"),
        "so does the read through the optional receiver: {body}"
    );
    assert!(
        !body.contains("_smelt_value.label.clone()"),
        "no field of the handle itself is ever named: {body}"
    );
}

/// A mutable global's assignment STORES at the global's declared type but
/// yields the assignment expression's own type, which need not be the same.
///
/// In JavaScript `x = v` evaluates to `v`, so an assignment can be consumed
/// somewhere that erased it. Hono's `wildcardRegExpCache = createNullObject()`
/// assigns a declared `unknown` into a `Record<string, RegExp>` global: the
/// store recovers the concrete record, but the expression is typed `unknown`,
/// and the block handed the concrete record back into a `SmeltUnknown` slot --
/// 2 errors in the hono slice.
#[test]
fn a_global_assignment_yields_the_expressions_own_type() {
    let source = source_for(
        r"
declare function createNullObject(): unknown;
let cache: Record<string, number> = {};
export function reset(key: string): number {
  cache = createNullObject();
  return cache[key];
}
",
    );

    // The enabling condition: the store really does recover the concrete
    // record, so the trailing value starts out at the global's type.
    let store = "*smelt_value.borrow_mut() = smelt_global_value.clone());";
    let at = source
        .find(store)
        .unwrap_or_else(|| panic!("no mutable-global store was emitted: {source}"));
    let tail = &source[at + store.len()..];

    // What the block hands back is erased, not the bare record.
    assert!(
        tail.trim_start()
            .starts_with("{ let smelt_record = smelt_global_value.clone();"),
        "a global assignment consumed as `unknown` must erase its result: {tail:.200}"
    );
    assert!(
        !tail.trim_start().starts_with("smelt_global_value }"),
        "the concrete record must not be yielded into an erased slot: {tail:.200}"
    );
}
