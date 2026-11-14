//! Object-shaped union arms discriminated by literal-typed fields, and the
//! remeda lazy-pipeline shapes that reach them.
//!
//! Requirements, independent of the implementation:
//!
//! * Behaviour: a value of `LazyEmpty | LazyMany<T> | LazyNext<T>` lands in
//!   the arm TypeScript's discriminated-union check picks (`hasNext: false`
//!   vs `true`, `hasMany: true` present or not), whether it arrives as an
//!   object literal, a record constant, an erased value, or the arm of
//!   another union.
//! * Type preservation: the arm's own struct is built wherever the source
//!   proves the arm, and the recovery from an erased value tests the same
//!   literal fields instead of taking the first object arm.

use super::*;

/// The remeda `LazyResult` shapes.
const LAZY: &str = r"
type LazyEmpty = { done: boolean; hasNext: false; hasMany?: false | undefined; next?: undefined };
type LazyNext<T> = { done: boolean; hasNext: true; hasMany?: false | undefined; next: T };
type LazyMany<T> = { done: boolean; hasNext: true; hasMany: true; next: readonly T[] };
export type LazyResult<T> = LazyEmpty | LazyMany<T> | LazyNext<T>;
";

/// The text of the generated `from_smelt_unknown` of the union enum that
/// declares `first_member`.
fn recovery_of<'a>(source: &'a str, first_member: &str) -> &'a str {
    let member = source
        .find(first_member)
        .unwrap_or_else(|| panic!("a union declares `{first_member}`:\n{source}"));
    let rest = source.get(member..).unwrap_or_default();
    let start = rest.find("fn from_smelt_unknown").unwrap_or(0);
    let body = rest.get(start..).unwrap_or_default();
    body.get(..body.find("\n    }\n").unwrap_or(body.len()))
        .unwrap_or(body)
}

/// The erased recovery of each arm tests its literal discriminants: an
/// object is `LazyEmpty` only when `hasNext` is `false`, `LazyMany` only
/// when `hasNext` is `true` and `hasMany` is `true`.
#[test]
fn erased_recovery_tests_literal_discriminants() {
    let source = source_for(&format!(
        "{LAZY}\nexport function read(value: unknown): LazyResult<number> {{ return value as LazyResult<number>; }}\n"
    ));
    let recovery = recovery_of(&source, "M0(LazyEmpty),");
    assert!(
        recovery.contains(r#"map.get("hasNext"), Some(SmeltUnknown::Bool(false))"#),
        "LazyEmpty is recognised by `hasNext: false`: {recovery}"
    );
    assert!(
        recovery.contains(r#"map.get("hasMany"), Some(SmeltUnknown::Bool(true))"#),
        "LazyMany is recognised by `hasMany: true`: {recovery}"
    );
}

/// An object literal at the expected union builds its arm's struct, chosen
/// by its literal discriminant.
#[test]
fn object_literal_builds_the_discriminated_arm() {
    let source = source_for(&format!(
        "{LAZY}\nexport function pick(flag: boolean): LazyResult<number> {{\n  if (flag) {{ return {{ done: false, hasNext: true, next: 1 }}; }}\n  return {{ done: false, hasNext: false }};\n}}\n"
    ));
    let pick = emitted_function_body(&source, "fn pick(");
    assert!(
        pick.contains("LazyNext {") && pick.contains("LazyEmpty {"),
        "each literal is its arm's struct: {pick}"
    );
    assert!(!pick.contains("SmeltRecord"), "no record detour: {pick}");
}

/// A record constant (no contextual type at its declaration) whose arm the
/// record type alone cannot pick (at a generic `T`, `{ hasNext: Bool }` also
/// fits `LazyNext<T>`) reaches the union through the discriminating recovery, and so does the record arm
/// of a conditional (`cond ? { .. } : SKIP_ITEM`) projected into it.
#[test]
fn record_values_join_the_union_through_discriminants() {
    let source = source_for(&format!(
        "{LAZY}\nconst SKIP_ITEM = {{ done: false, hasNext: false }} as const;\nexport function skip<T>(): LazyResult<T> {{ return SKIP_ITEM; }}\nexport function either(flag: boolean, value: number): LazyResult<number> {{\n  return flag ? {{ done: false, hasNext: true, next: value }} : SKIP_ITEM;\n}}\n"
    ));
    let skip = emitted_function_body(&source, "fn skip(");
    assert!(skip.contains("::from_smelt_unknown("), "{skip}");
    let either = emitted_function_body(&source, "fn either(");
    assert!(
        !either.contains("unreachable!"),
        "the record arm of the conditional is not excluded: {either}"
    );
}

/// The same generic record at another instantiation converts arm to arm
/// instead of being excluded (`LazyNext<SmeltUnknown>` from an erased
/// generic call into the caller's `LazyNext<f64>`).
#[test]
fn union_projection_converts_same_record_at_other_arguments() {
    let source = source_for(&format!(
        "{LAZY}\nexport function evaluator<T>(other: readonly T[]): (value: T) => LazyResult<T> {{\n  return (value) => ({{ done: other.length === 0, hasNext: true, next: value }});\n}}\nexport function use(): boolean {{\n  const result: LazyResult<number> = evaluator<number>([])(7);\n  return result.done;\n}}\n"
    ));
    let user = emitted_function_body(&source, "fn use_(");
    assert!(!user.contains("unreachable!"), "{user}");
}

/// A tuple's `.length` is its arity.
#[test]
fn tuple_length_is_its_arity() {
    let source = source_for(
        r"
export function apply<T>(value: T, fn: (value: T) => number): number { return fn(value); }
const DATA: readonly [number] = [1] as const;
export function run(): number { return apply(DATA, (data: readonly [number]) => data.length); }
",
    );
    assert!(!source.contains(".length.clone()"), "{source}");
    assert!(source.contains("1.0"), "{source}");
}

/// A list element entering a generated-union element slot injects into its
/// arm (`[flat, nested]` with `nested: number[][]` against
/// `(number | number[])[][]`): each `number[]` element is the `number[]` arm.
#[test]
fn list_element_injects_into_union_arm() {
    let source = source_for(
        r"
export function total(rows: (number | number[])[][]): number { return rows.length; }
export function run(): number {
  const flat = [1, 2];
  const nested = [[3]];
  const rows: (number | number[])[][] = [flat, nested];
  return total(rows);
}
",
    );
    let run = emitted_function_body(&source, "fn run(");
    assert!(!run.contains("value.into_smelt_unknown()"), "{run}");
}

/// The result of an erased-rest call bound to a function-typed local is
/// extracted to that function (remeda `const fn = lazy(...lazyArgs)`), and a
/// callable-object struct is recovered from a bare erased function.
#[test]
fn erased_call_result_and_callable_object_from_function() {
    let source = source_for(
        r"
type LazyFn = (value: unknown, index: number) => unknown;
type LazyMeta = { readonly single?: boolean };
type LazyDefinition = {
  readonly lazy: LazyMeta & ((...args: any) => LazyFn);
  readonly lazyArgs: readonly unknown[];
};
export function prepare(def: LazyDefinition): unknown {
  const { lazy, lazyArgs } = def;
  const fn = lazy(...lazyArgs);
  return Object.assign(fn, { isSingle: lazy.single ?? false });
}
export function fromUnknown(value: unknown): unknown {
  return prepare(value as LazyDefinition);
}
",
    );
    let prepare = emitted_function_body(&source, "fn prepare(");
    let call_line = prepare
        .lines()
        .find(|line| line.contains(".call("))
        .unwrap_or_else(|| panic!("prepare calls the erased lazy: {prepare}"));
    assert!(
        call_line.contains("SmeltUnknown::Function(smelt_function) => Some(smelt_function)"),
        "the erased result is extracted to the typed callback: {call_line}"
    );
    assert!(
        source.contains("smelt_function_value @ SmeltUnknown::Function(_) =>"),
        "a bare function recovers as the callable-object struct: {source}"
    );
}

/// `Number.isNaN` does not coerce: over `number | undefined` it is `false`
/// for an absent value and tests the value where present (remeda
/// `when((x) => Number.isNaN(x) || x === undefined, ..)` asserted presence
/// and panicked on `undefined`).
#[test]
fn number_is_nan_over_optional_number_tests_presence() {
    let source = source_for(
        r"
export function missing(x: number | undefined): boolean { return Number.isNaN(x) || x === undefined; }
",
    );
    let body = emitted_function_body(&source, "fn missing(");
    assert!(body.contains(".map_or(false, |smelt_value|"), "{body}");
    assert!(!body.contains("absent after narrowing"), "{body}");
}
