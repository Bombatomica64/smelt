use super::*;

#[test]
fn lowers_array_shift_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: string[] = ["a", "b"];
values.shift();
const item = values.shift();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let shifts = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListShift { .. }))
        .count();
    ensure_eq!(shifts, 2);
    Ok(())
}

#[test]
fn rejects_unsupported_array_push_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let wrong_type = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.push("x");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&wrong_type, "argument must match")?;

    let mut ctx = HirCtx::new();
    let too_many = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.push(3, 4);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many, "exactly one item argument")
}

#[test]
fn rejects_unsupported_array_unshift_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let wrong_type = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.unshift("x");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&wrong_type, "arguments must match")?;

    let mut ctx = HirCtx::new();
    let non_local = lowering_errors(
        ts!(r#"
function values(): number[] { return [1, 2]; }
values().unshift(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&non_local, "local array receiver")
}

#[test]
fn rejects_unsupported_array_pop_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.pop(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires no arguments")
}

#[test]
fn rejects_unsupported_array_shift_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.shift(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires no arguments")
}

#[test]
fn rejects_unsupported_slice_argument_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let too_many = lowering_errors(
        ts!(r#"
const values: number[] = [1, 2, 3];
const bad = values.slice(0, 1, 2);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many, "omitted, start, and end arguments")
}

#[test]
fn lowers_array_is_array_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const yes = Array.isArray(values);
const no = Array.isArray(1);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(true))))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(false))))
    );
    Ok(())
}

#[test]
fn lowers_math_sqrt_pow_sign() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 4;
const root = Math.sqrt(value);
const cubeRoot = Math.cbrt(value);
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
const raised = Math.pow(value, 2);
const distance = Math.hypot(value, 3);
const sample = Math.random();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        NumericUnaryFuncOp::Sqrt,
        NumericUnaryFuncOp::Cbrt,
        NumericUnaryFuncOp::Sign,
        NumericUnaryFuncOp::Sin,
        NumericUnaryFuncOp::Cos,
        NumericUnaryFuncOp::Tan,
        NumericUnaryFuncOp::Asin,
        NumericUnaryFuncOp::Acos,
        NumericUnaryFuncOp::Atan,
        NumericUnaryFuncOp::Log,
        NumericUnaryFuncOp::Log10,
        NumericUnaryFuncOp::Log2,
        NumericUnaryFuncOp::Exp,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericUnaryFunc { op, .. } if op == expected)
        ));
    }
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericPow { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAtan2 { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericHypot { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericRandom))
    );
    Ok(())
}

#[test]
fn lowers_number_predicate_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 4;
const finite = Number.isFinite(value);
const integer = Number.isInteger(value);
const nan = Number.isNaN(value);
const globalNan = isNaN(value);
const missing = undefined;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        NumericPredicateOp::IsFinite,
        NumericPredicateOp::IsInteger,
        NumericPredicateOp::IsNaN,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericPredicate { op, .. } if op == expected)
        ));
    }
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(
                expr.kind,
                ExprKind::NumericPredicate {
                    op: NumericPredicateOp::IsNaN,
                    ..
                }
            ))
            .count(),
        2
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::None)))
    );
    Ok(())
}

#[test]
fn lowers_object_projection_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = Object.keys(mapping);
const values = Object.values(mapping);
const entries = Object.entries(mapping);
const rebuilt = Object.fromEntries([["a", 1], ["b", 2]]);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        DictProjectionOp::Keys,
        DictProjectionOp::Values,
        DictProjectionOp::Entries,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::DictProjection { op, .. } if op == expected)
        ));
    }
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_)))
    );
    Ok(())
}

#[test]
fn lowers_object_assign_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const source: Record<string, number> = { a: 1 };
const merged = Object.assign({}, source, { b: 2 });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictAssign { .. }))
    );
    Ok(())
}

#[test]
fn lowers_object_assign_with_optional_interface_source() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options {
  addSuffix?: boolean;
}

function merge(options?: Options): Record<string, unknown> {
  return Object.assign({}, options, {
    addSuffix: options?.addSuffix,
    comparison: 1,
  });
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_module_global_array_with_null_elements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const daysInMonths = [31, null, 31];

function days(month: number): number {
  return daysInMonths[month] || 28;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_parse_iso_string_and_regexp_helpers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function parseDateUnit(value: string): number {
  return value ? parseInt(value) : 1;
}

function parseYear(dateString: string, additionalDigits: number): string[] | undefined {
  const regex = new RegExp("^(\\d{" + (4 + additionalDigits) + "})");
  const captures = dateString.substr(1, dateString.length).match(regex);
  const token = regex.exec(dateString);
  if (!captures) return undefined;
  return token || dateString.slice((captures[1] || captures[2]).length).match(regex);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_string_length_after_truthy_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface DateString {
  date?: string;
}

function read(dateStrings: DateString): number {
  if (dateStrings.date) {
    return dateStrings.date.length;
  }
  return 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_global_is_nan_with_coercible_unknown() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(): boolean {
  let offset;
  offset = 1;
  return isNaN(offset);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_console_warn_and_error_like_console_log() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function warn(message: string): void {
  console.warn(message);
  console.error(message);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_error_constructor_with_unknown_message() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function fail(message: unknown): void {
  throw new RangeError(message);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_timezone_offset_as_number() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function offset(date: Date): number {
  return Math.abs(date.getTimezoneOffset());
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_qualified_external_type_reference_as_opaque_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function fakeDate(): void {
  let clock: sinon.SinonFakeTimers | undefined;
  clock = undefined;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nested_function_declaration_as_local_closure() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function outer(value: number): { inner: (next: number) => void } {
  let current = value;
  function inner(next: number) {
    current = next;
  }
  return { inner };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_sinon_fake_timers_helper_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function fakeDate(date: number | Date): { fakeNow: (date: number | Date) => void } {
  let clock: sinon.SinonFakeTimers | undefined;
  function fakeNow(date: number | Date) {
    clock?.restore();
    clock = sinon.useFakeTimers(+date);
  }
  return { fakeNow };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_do_while_statement() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function countDown(value: number): number {
  let current = value;
  do {
    current = current - 1;
  } while (current > 0);
  return current;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_static_field_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(value: unknown): unknown {
  return value.date;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_index_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(values: unknown, index: number): unknown {
  return values[index];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn tolerates_describe_scope_setup_and_dynamic_test_alias() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { describe, expect, it } from "vitest";

describe("group", () => {
  const enabled = true;
  const alias = enabled ? it : it.skip;
  alias("dynamic", () => {});

  describe("nested", () => {
    it("static", () => {
      expect(enabled).toBe(true);
    });
  });
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_values_through_partial_record_alias() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Boxed<T> = Partial<Record<string, T[]>>;

function values(result: Boxed<number>): number {
  return Object.values(result).length;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_literal_types_inside_tuples() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Items = [{ a: "cat" }, { a: string }?];

function first(items: Items): string {
  return items[0].a;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_constructor_identifier_as_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

it("checks date", () => {
  const result = Date.now();
  expect(result).toBeInstanceOf(Date);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_intl_timezone_probe_for_test_labels() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const tzName = Intl.DateTimeFormat().resolvedOptions().timeZone || process.env.tz;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_intl_date_time_format_format_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function formatDate(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { year: "numeric" }).format(date);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateToIsoString { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_intl_relative_time_format_format_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options extends Intl.RelativeTimeFormatOptions {
  unit?: string;
  locale?: string;
}

function formatDistance(value: number, unit: string, options?: Options): string {
  const rtf = new Intl.RelativeTimeFormat(options?.locale, {
    numeric: "auto",
    ...options,
  });
  return rtf.format(value, unit);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, function)?;
    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::String(ref value)) if value.is_empty())
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_guarded_dynamic_date_constructor_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function transpose(constructor: unknown): Date {
  return new constructor(0);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(0.0))))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_local_arrow_defaults_referencing_prior_params() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const override = (
  base: Date,
  year = base.getFullYear(),
  month = base.getMonth(),
) => new Date(year, month);
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_callback_as_loop() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function maxValue(values: number[]): number {
  let result = 0;
  values.forEach((value, index) => {
    if (index < 0) return;
    if (result < value) result = value;
  });
  return result;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_function_callback_as_list_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function call(data: readonly number[], callbackfn: (value: number, index: number, data: readonly number[]) => void): void {
  data.forEach(callbackfn);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::ListCallback {
            op: ListCallbackOp::ForEach,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn packs_normal_and_spread_arguments_into_rest_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function collect(first: number, ...rest: number[]): number {
  return rest.length;
}

function call(values: number[]): number {
  return collect(1, 2, ...values);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn splits_spread_arguments_across_fixed_and_rest_parameters() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function collect(first: unknown, second: unknown, ...rest: unknown[]): unknown {
  return second;
}

function call(values: readonly unknown[]): unknown {
  return collect("prefix", ...values);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 1)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. })),
        "missing fixed parameter read from spread list"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSlice { .. })),
        "missing rest slice from spread list"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_tuple_rest_destructuring_as_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pick(index: number): number {
  const [first, ...rest] = [1, 2, 3] as [number, number, number];
  return rest[index];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_sort_with_function_reference_comparator() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function compare(left: number, right: number): number {
  return left - right;
}

function sortValues(values: number[]): number[] {
  return values.sort(compare);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn preserves_set_item_type_through_spread_sort() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const sampleIndices = new Set<number>();
const sorted = [...sampleIndices].sort((a, b) => a - b);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_dynamic_index_with_non_null_assertion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function sample<T>(data: readonly T[]): T[] {
  const sampleIndices = new Set<number>();
  return [...sampleIndices].sort((a, b) => a - b).map((index) => data[index]!);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_sort_with_comparator_function_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const sortByImplementation = <T>(
  data: readonly T[],
  compareFn: (left: T, right: T) => number,
): T[] => [...data].sort(compareFn);
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_instanceof_inside_expect_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

it("checks instance", () => {
  const value = new Date(0);
  expect(value instanceof Date).toBe(true);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_instanceof_on_catch_like_unknown_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { expect, test } from "vitest";

test("range error", () => {
  try {
    throw new RangeError("bad");
  } catch (e) {
    expect(e instanceof RangeError).toBe(true);
  }
});
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_constructor_field_on_date_like_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = new Date(0);
const ctor = value.constructor;
class CustomDate extends Date {}
const custom = new CustomDate(0);
const customCtor = custom.constructor;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_getters_as_readonly_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class User {
  public get name(): string {
    return "Ada";
  }
}
const user = new User();
const name = user.name;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(ctx.krate.types.all().iter().any(|ty| {
        matches!(
            ty,
            Type::Class { name, .. } if ctx.krate.symbols.get(*name) == Some("User")
        )
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_block_scoped_class_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function make(): void {
  class CustomDate extends Date {}
  function acceptDate(value: CustomDate): void {}
  const value = new CustomDate(0);
  acceptDate(new CustomDate(0));
  const ctor = CustomDate;
  value instanceof CustomDate;
  const base = new Date(0);
  base instanceof CustomDate;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_block_scoped_type_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function check(): void {
  interface AB {
    a: number;
    b: number;
  }
  type Boxed = { value: AB };
  const item: Boxed = { value: { a: 1, b: 2 } };
  item.value.a;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_imported_constructor_as_opaque_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { UTCDate } from "@date-fns/utc";
import { expect, it } from "vitest";

it("checks extension date", () => {
  const result = new UTCDate();
  expect(result).toBeInstanceOf(UTCDate);
  expect(result instanceof UTCDate).toBe(true);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn allows_map_get_with_union_member_key_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function lookup<T, S>(map: Map<S | T, number>, value: T): number | undefined {
  return map.get(value);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_keys_after_object_string_nullish_guards() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function empty(data: object | string | undefined): boolean {
  if (data === "" || data === undefined) {
    return true;
  }
  if (Array.isArray(data)) {
    return data.length === 0;
  }
  return Object.keys(data).length === 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_in_after_typeof_object_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function hasEnumerable(data: unknown): boolean {
  if (typeof data !== "object") {
    return false;
  }
  for (const key in data) {
    return true;
  }
  return false;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_get_own_property_symbols_length() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function symbolCount(data: unknown): number {
  return Object.getOwnPropertySymbols(data).length;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_uninitialized_let_as_unknown_for_date_coercion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function parseDate(): Date {
  return new Date(0);
}

function read(): number {
  let date;
  date = parseDate();
  return +date;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_assign_call_on_callable_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const fnValue = (value: number): number => value;
const assigned = Object.assign(fnValue, { lazy: fnValue });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::CallableObjectAssign { ref props, .. } if props.len() == 1)
    ));
    Ok(())
}

#[test]
fn lowers_object_assign_call_on_inline_callable_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const assigned = Object.assign(
  (value: number): number => value,
  { flush: (): number => 1 },
);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::CallableObjectAssign { ref props, .. } if props.len() == 1)
    ));
    Ok(())
}

#[test]
fn local_function_implementations_shadow_cross_module_overloads() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Debouncer<F> = { readonly call: () => void };
function debounce<F>(func: F): Debouncer<F>;
function debounce<F>(func: F): Debouncer<F> {
  return { call: () => {} };
}
"#),
        &mut ctx,
    )?;
    let module_id = lower_ok(
        ts!(r#"
function debounce(func: () => void) {
  return Object.assign(func, { cancel: () => {} });
}

const debounced = debounce(() => {});
debounced();
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn infers_async_arrow_const_return_type_from_await_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
async function sleep(ms: number): Promise<void> {
  await new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function run(): Promise<void> {
  await yieldExecution();
}

const yieldExecution = async () => await sleep(0);
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_ignored_promise_then_catch_chain() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
async function load(values: number[]): Promise<number[]> {
  return values;
}

function run(values: number[]): void {
  load(values)
    .then((response) => {
      response.length;
    })
    .catch((_error) => {});
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_spread_from_generic_accumulator_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function append<T>(items: T | undefined, item: number): number[] {
  return [...(items ?? []), item];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_vitest_mock_with_implementation_callable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

async function run(): Promise<void> {
  const mockApi = vi.fn<(words: readonly string[]) => Promise<Record<string, number>>>(
    async (words) => ({ count: words.length }),
  );
  await mockApi(["a"]);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_captured_vitest_mock_callable_inside_async_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { vi } from "vitest";

function batch<Params extends any[], BatchResponse>(
  callback: (requests: readonly Params[]) => Promise<BatchResponse>,
): void {
  callback([]);
}

function run(): void {
  const mockApi = vi.fn<(words: readonly string[]) => Promise<Record<string, number>>>(
    async (words) => ({ count: words.length }),
  );
  batch(async (requests: readonly [word: string][]) => await mockApi(requests.flat()));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_promise_resolve_and_exported_object_values_const() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const TYPED_ARRAY = new Uint8Array(1);
export const DATA = {
  promise: Promise.resolve(5),
  string: "text",
  typedArray: TYPED_ARRAY,
} as const;
export const VALUES = Object.values(DATA);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    ensure!(ctx.krate.types.all().iter().any(|ty| {
        matches!(
            ty,
            Type::Future(inner) if matches!(ctx.krate.types.get(*inner), Some(Type::Float))
        )
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_generic_promise_constructor_executor_as_future() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function makeValue(): Promise<number> {
  return new Promise<number>((resolve) => {
    resolve(1);
  });
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_length_to_static_arity() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const fnValue = (left: number, right: number): number => left + right;
const arity = fnValue.length;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == 2.0)
        )
    );
    Ok(())
}

#[test]
fn lowers_function_bind_result_as_array_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function add(left: number, right: number): number {
  return left + right;
}

function shift(values: number[]): number[] {
  const addOne = add.bind(null, 1);
  return values.map(addOne);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                expr.kind,
                ExprKind::Closure(smelt_hir::ClosureExpr {
                    callback_body: None,
                    ..
                })
            )),
        "expected bind to lower to a first-class closure body"
    );
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ListCallback { .. })),
        "expected bound function local to be accepted as an array callback"
    );
    Ok(())
}

#[test]
fn lowers_bind_captures_inside_for_each_callback_blocks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function constructFrom(context: unknown, value: unknown): unknown {
  return value;
}

function max(dates: unknown[]): unknown {
  let context: ((value: unknown) => unknown) | undefined;
  dates.forEach((date) => {
    if (!context && typeof date === "object") {
      context = constructFrom.bind(null, date) as (value: unknown) => unknown;
    }
  });
  return context;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let mut saw_nested_bind_capture = false;
    let mut saw_root_bind_arg_capture = false;
    for body in &ctx.krate.bodies {
        for block in &body.blocks {
            for stmt_id in &block.stmts {
                let stmt = &body.stmts[stmt_id.0 as usize];
                let Stmt::Let { pat, .. } = stmt else {
                    continue;
                };
                let smelt_hir::Pattern::Binding(local) = body.patterns[pat.0 as usize] else {
                    continue;
                };
                let Some(name) = body.locals[local.0 as usize]
                    .name
                    .and_then(|symbol| ctx.krate.symbols.get(symbol))
                else {
                    continue;
                };
                if name == "__smelt_bind_arg_0" {
                    if block.stmts == body.blocks[body.root.0 as usize].stmts {
                        saw_root_bind_arg_capture = true;
                    } else {
                        saw_nested_bind_capture = true;
                    }
                }
            }
        }
    }

    ensure!(
        saw_nested_bind_capture,
        "expected bound callback argument capture to be emitted inside the callback block"
    );
    ensure!(
        !saw_root_bind_arg_capture,
        "expected callback-local bind argument capture not to leak to the function root"
    );
    Ok(())
}

#[test]
fn selects_tuple_rest_overload_from_source_arguments() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pair(...values: [number, number]): [number, number];
function pair(...values: number[]): number[] {
  return values;
}

const selected = pair(1, 2);

function pairWithSeed(seed: number, ...values: [number, number]): [number, number];
function pairWithSeed(seed: number, ...values: number[]): number[] {
  return values;
}

const selectedWithSeed = pairWithSeed(0, 1, 2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.locals
            .iter()
            .any(|local| matches!(ctx.krate.types.get(local.ty), Some(Type::Tuple(items)) if items.len() == 2)),
        "expected tuple rest overload return type to be selected"
    );
    Ok(())
}

#[test]
fn extracts_structural_fields_from_referenced_generic_interfaces_and_pick() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface LocaleOptions {
  weekStartsOn?: number;
}

interface Locale {
  options?: LocaleOptions;
  code: string;
}

interface LocalizedOptions<LocaleFields extends keyof Locale> {
  locale?: Pick<Locale, LocaleFields>;
}

interface WeekOptions {
  weekStartsOn?: number;
}

type DefaultOptions = LocalizedOptions<"options"> & WeekOptions;

function read(options?: DefaultOptions): number {
  const direct = options?.weekStartsOn;
  const locale = options?.locale?.options?.weekStartsOn;
  return 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    Ok(())
}

#[test]
fn lowers_never_rest_strict_function_spread_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;

function callStrict(fn: StrictFunction, args: readonly unknown[]): unknown {
  return fn(...args);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
    );
    Ok(())
}

#[test]
fn allows_strict_function_as_function_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;

function dataLast(fn: StrictFunction, args: readonly unknown[]): unknown {
  return fn(...args);
}

function purry(fn: StrictFunction, args: readonly unknown[]): unknown {
  return dataLast(fn, args);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. }))
    );
    Ok(())
}

#[test]
fn lowers_parenthesized_callable_intersection_type_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LazyFn = (value: unknown) => unknown;
type LazyMeta = { readonly single?: boolean };
export type LazyDefinition = {
  readonly lazy: LazyMeta & ((...args: any) => LazyFn);
  readonly lazyArgs: readonly unknown[];
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Function(function) if function.params.len() == 1))
    );
    Ok(())
}

#[test]
fn keeps_callable_alias_intersections_callable_after_reference() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LazyEvaluator<T = unknown, R = T> = (
  item: T,
  index: number,
  data: readonly T[],
) => R;

type PreparedLazyFunction<T> = LazyEvaluator<T> & {
  index: number;
  items: T[];
};

function processItem(lazyFn: PreparedLazyFunction<number>): number {
  const { index, items } = lazyFn;
  return lazyFn(1, index, items);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "expected callable intersection alias references to lower as closure calls",
    );
    Ok(())
}

#[test]
fn narrows_typeof_function_out_of_callable_tuple_union() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const labels = { asc: true, desc: false } as const;

type Projection<T> = (value: T) => string;
type OrderRule<T> =
  | Projection<T>
  | readonly [projection: Projection<T>, direction: keyof typeof labels];

function projector<T>(primaryRule: OrderRule<T>): Projection<T> {
  return typeof primaryRule === "function" ? primaryRule : primaryRule[0];
}

function direction<T>(primaryRule: OrderRule<T>): string {
  return "function" !== typeof primaryRule ? primaryRule[1] : "asc";
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn calls_callable_branch_of_union_local_and_nested_result() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Curried = ((value: number) => Curried | string) | string;

function run(fn: Curried): string {
  const first = fn(3);
  return first(2) as string;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let closure_calls = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
        .count();
    ensure_eq!(closure_calls, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn calls_overloaded_interface_call_signature_by_argument_count() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Step1 {
  (): Step1;
  (value: number): string;
}

interface Step2 {
  (): Step2;
  (value: number): Step1;
  (value: number, next: number): string;
}

function run(fn: Step2): string {
  const first = fn(2);
  return first(1);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let closure_calls = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
        .count();
    ensure_eq!(closure_calls, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unhinted_function_expression_object_property_as_unknown_callable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function build(args: { callback?: unknown }): unknown {
  return args.callback;
}

const result = build({
  callback: function (value) {
    return Number(value) - 1;
  },
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::Closure { .. })),
        "expected unhinted function expression property to lower as a closure",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_annotated_arrow_const_with_callable_alias_hint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocalizeFn<Value> = (value: Value, options?: { unit?: string }) => string;

const ordinalNumber: LocalizeFn<number> = (dirtyNumber, options) => {
  const number = Number(dirtyNumber);
  const unit = options?.unit;
  return unit ? String(number) : "0";
};
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Function(function) if function.params.len() == 2))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn ignores_browser_guarded_describe_branch_and_skipped_test() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { describe, it, expect } from "vitest";

describe("browser guard", () => {
  if (typeof window !== "undefined") {
    it("browser only", () => {
      document.body.append("x");
    });
  } else {
    it.skip("browser only", () => {});
  }

  it("native", () => {
    expect(1).toBe(1);
  });
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let tests = ctx
        .krate
        .items
        .iter()
        .filter(|item| matches!(item, Item::Function(function) if function.is_test))
        .count();
    ensure_eq!(tests, 1);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_for_each_statement_with_tuple_destructuring_param() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { describe, it, expect } from "vitest";

describe("forEach", () => {
  it("destructures tuple cases", () => {
    [
      ["do", "1er"],
      ["do M", "1er 1"],
    ].forEach(([formatString, expectedResult]) => {
      expect(formatString).toBe(formatString);
      expect(expectedResult).toBe(expectedResult);
    });
  });
});
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_module_symbol_const_used_by_arrow_closure() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const Marker = Symbol("marker");
const read = <T>(): T => Marker as T;
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn captures_type_assertion_wrapped_closure_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function read(value: unknown): () => string {
  const local = "value";
  return () => local as string;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_async_arrow_expression_object_property() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type AsyncCaller = {
  readonly call: (...params: number[]) => Promise<void>;
};

export function makeCaller(): AsyncCaller {
  return {
    call: async (...params: number[]): Promise<void> =>
      new Promise<void>((resolve) => setTimeout(resolve, 1)),
  };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    let async_closure_body = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            let ExprKind::Closure(closure) = &expr.kind else {
                return false;
            };
            let Some(Type::Function(function)) = ctx.krate.types.get(expr.ty) else {
                return false;
            };
            function.is_async
                && matches!(
                    ctx.krate.types.get(closure.return_ty),
                    Some(Type::Future(_))
                )
                && ctx
                    .krate
                    .bodies
                    .get(closure.body.0 as usize)
                    .is_some_and(|body| body.async_state_machine.is_some())
        })
    });
    ensure!(
        async_closure_body,
        "expected object-property async arrow to lower as an async closure"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_local_arrow_forward_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function run(): void {
  const first = (): void => {
    second();
  };
  const second = (): void => {};
  first();
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nullish_assignment_on_optional_locals() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function read(value: number | undefined): number | undefined {
  const now = 1;
  value ??= now;
  return value;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn destructures_fields_from_union_intersection_aliases() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Timing =
  | ({ readonly triggerAt?: "end" } & (
      | { readonly minGapMs: number }
      | {
          readonly minQuietPeriodMs?: number;
          readonly maxBurstDurationMs?: number;
          readonly minGapMs?: never;
        }
    ))
  | {
      readonly triggerAt: "start" | "both";
      readonly minQuietPeriodMs?: number;
      readonly maxBurstDurationMs?: number;
      readonly minGapMs?: number;
    };

type Options<R> = {
  readonly reducer?: (accumulator: R | undefined) => R;
} & Timing;

export function read<R>({ minQuietPeriodMs }: Options<R>): number {
  return minQuietPeriodMs ?? 0;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn infers_function_parameter_types_from_defaults() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function delay(wait = 0): number {
  return wait + 1;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_console_members_inside_test_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { it } from "vitest";

it("logs diagnostic output", () => {
  console.log("starting");
  console.warn("fallback");
  console.error("failed");
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_vitest_spy_on_console_mock_lifecycle() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { beforeEach, afterEach, describe, it, vi } from "vitest";
import type { MockInstance } from "vitest";

describe("console.warn", () => {
  let warn: MockInstance;

  beforeEach(() => {
    warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    warn.mockRestore();
  });

  it("runs", () => {
    console.warn("hidden");
  });
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_has_own_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const first = Object.hasOwn(mapping, "a");
const second = mapping.hasOwnProperty("b");
function generic<T>(value: T, key: string): boolean {
  return Object.hasOwn(value, key);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
            .count()
            == 2
    );
    let generic_body = function_body(&ctx, function_item(&ctx, module, 0)?)?;
    ensure!(
        generic_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
    );
    Ok(())
}

#[test]
fn lowers_computed_destructuring_key_with_type_assertion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function pick<T>(value: T, key: string): unknown {
  const { [key as keyof T]: picked } = value;
  return picked;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_generic_record_key_aliases_for_later_instantiation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type UpsertProp<T, K extends PropertyKey, V> = T & Record<K, V>;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    ensure!(
        ctx.krate.types.all().iter().any(|ty| matches!(
            ty,
            Type::Dict(key, _) if matches!(ctx.krate.types.get(*key), Some(Type::TypeParam { .. }))
        )),
        "expected generic Record<K, V> to preserve K for later substitution",
    );
    Ok(())
}

#[test]
fn normalizes_record_property_key_surfaces() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type NumberRecord = Record<number, string>;
type LiteralRecord = Record<123 | "name", boolean>;
type PropertyKeyRecord = Record<PropertyKey, unknown>;
type ConditionalRecord<T extends boolean> = Record<T extends true ? number : string, string>;
type UnionRecord = Record<number, string> | Record<string, number>;
"#),
        &mut ctx,
    )?;

    let has_string_keyed_record = ctx.krate.types.all().iter().any(|ty| {
        matches!(
            ty,
            Type::Dict(key, _) if matches!(ctx.krate.types.get(*key), Some(Type::String))
        )
    });
    ensure!(
        has_string_keyed_record,
        "expected concrete Record key surfaces to normalize to string-key dictionaries",
    );
    Ok(())
}

#[test]
fn lowers_template_literal_tuple_element_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Entry = readonly [`testing_${string}`, boolean];
type Entries = readonly Entry[];
type BigIntLiterals = 1n | 2n | 3n;
"#),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Tuple(items) if items.iter().any(|item| matches!(ctx.krate.types.get(*item), Some(Type::String))))),
        "expected template literal tuple keys to lower as strings",
    );
    Ok(())
}

#[test]
fn lowers_top_level_arrow_const_used_by_later_function() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const compare = (left: number, right: number): number => left - right;

function sortValues(values: number[]): number[] {
  return values.toSorted(compare);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 1)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSort { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_static_function_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function useUnary(fn: (value: unknown) => unknown): unknown {
  return fn([]);
}

function useBinary(fn: (left: unknown, right: unknown) => boolean): boolean {
  return fn(1, 1);
}

const entries = useUnary(Object.entries);
const values = useUnary(Object.values);
const keys = useUnary(Object.keys);
const rebuilt = useUnary(Object.fromEntries);
const same = useBinary(Object.is);
const owned = useBinary(Object.hasOwn);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let closure_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Closure(_)))
        .count();
    ensure!(
        closure_count >= 6,
        "expected Object static member references to lower as callables",
    );
    Ok(())
}

#[test]
fn lowers_callback_typeof_expression_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values = ["a", "b"].map((item) => typeof item);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListCallback {
                op: ListCallbackOp::Map,
                ..
            }
        )),
        "expected callback typeof expression to lower inside array map",
    );
    Ok(())
}

#[test]
fn lowers_object_spread_literals_as_ordered_assignments() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const base: Record<string, number> = { a: 1, b: 2 };
const merged: Record<string, number> = { ...base, b: 3, c: 4 };
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictAssign { .. })),
        "expected object spread to lower to an ordered dictionary assignment",
    );
    Ok(())
}

#[test]
fn lowers_generic_object_spread_with_computed_key() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type UpsertProp<T, K extends PropertyKey, V> = T & Record<K, V>;
export const addPropImplementation = <T, K extends PropertyKey, V>(
  obj: T,
  prop: K,
  value: V,
): UpsertProp<T, K, V> => ({ ...obj, [prop]: value });
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    let function = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "missing lowered function".to_owned())?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictAssign { .. })),
        "expected generic spread and computed key to lower through DictAssign",
    );
    Ok(())
}

#[test]
fn lowers_optional_object_spread_for_option_bags() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options {
  value?: number;
}

function merge(options?: Options): Record<string, number> {
  return { ...options, value: 1 };
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_conditional_object_spread_sources() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function merge(maxWait: number | undefined): Record<string, number> {
  return {
    minQuietPeriodMs: 0,
    ...(maxWait !== undefined && { maxBurstDurationMs: maxWait }),
  };
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "missing lowered function".to_owned())?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "expected conditional object spread source to lower as a conditional record",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_ternary_object_spread_sources_with_record_context() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function merge(trailing: boolean, leading: boolean): Record<string, string> {
  return {
    mode: "wait",
    ...(trailing
      ? leading
        ? { triggerAt: "both" }
        : { triggerAt: "end" }
      : { triggerAt: "start" }),
  };
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_type_assertion_call_arguments_with_asserted_object_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function readA(value: Record<string, string>): string {
  return value.a;
}
const result = readA({} as { a: string });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::DictLit(_)
                if matches!(ctx.krate.types.get(expr.ty), Some(Type::Dict(_, value)) if ctx.krate.types.get(*value) == Some(&Type::String))
        )),
        "expected asserted object call argument to use the asserted object type",
    );
    Ok(())
}

#[test]
fn lowers_vitest_expect_type_of_as_type_only_noop() -> Result<(), String> {
    let source = ts!(r#"
import { expectTypeOf, test } from "vitest";

test("type assertion", () => {
  const result = {} as { a: string };
  expectTypeOf(result).toEqualTypeOf<{ a: string }>();
  expectTypeOf(result).toEqualTypeOf<{ [Symbol.iterator]: string }>();
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/type.test-d.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::None))),
        "expected type-test assertion to lower to a no-op expression",
    );
    Ok(())
}

#[test]
fn lowers_json_stringify_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2];
const text = JSON.stringify(values);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonStringify { .. })),
        "expected JSON.stringify lowering",
    );
    Ok(())
}

#[test]
fn lowers_json_parse_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const text = "[1,2]";
const values = JSON.parse<number[]>(text);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::JsonParse { .. })),
        "expected JSON.parse lowering",
    );
    Ok(())
}

#[test]
fn lowers_regexp_test_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const text = "abc123";
const pattern = "\\d+";
const hasDigits = new RegExp(pattern).test(text);
const alsoHasDigits = RegExp(pattern).test(text);
const literalHasDigits = /\d+/.test(text);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(
                expr.kind,
                ExprKind::RegexIsMatch {
                    op: RegexMatchOp::Search,
                    ..
                }
            ))
            .count()
            == 3,
        "expected RegExp.test lowering",
    );
    Ok(())
}

#[test]
fn rejects_unsupported_regexp_test_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const text = "abc123";
const hasDigits = new RegExp("\\d+", "g").test(text);
"#),
        &mut ctx,
    )?;

    let mut ctx = HirCtx::new();
    let non_string = lowering_errors(
        ts!(r#"
const text = "abc123";
    const hasDigits = new RegExp(1).test(text);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&non_string, "string pattern")?;
    Ok(())
}

#[test]
fn rejects_unsupported_json_stringify_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let extra_arg = lowering_errors(
        ts!(r#"
const values: number[] = [1, 2];
const text = JSON.stringify(values, null);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&extra_arg, "exactly one value")?;

    let mut ctx = HirCtx::new();
    let unsupported_type = lowering_errors(
        ts!(r#"
class User {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
const user = new User("Ada");
const text = JSON.stringify(user);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&unsupported_type, "JSON-serializable")
}

#[test]
fn rejects_unsupported_json_parse_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let missing_type = lowering_errors(
        ts!(r#"
const text = "[1,2]";
const values = JSON.parse(text);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&missing_type, "explicit type argument")
}

#[test]
fn lowers_string_includes_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const has = word.includes("mel");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringContains { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_includes_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const has = values.includes(2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListContains { .. })),
        "array includes did not lower to ListContains"
    );
    Ok(())
}

#[test]
fn lowers_set_constructor_and_has_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: Set<number> = new Set([1, 2, 3]);
const has = values.has(2);
const empty: Set<string> = new Set();
const genericEmpty = new Set<number>();
const genericEmptyLiteral = new Set<string>([]);
const source: readonly number[] = [1, 2, 3];
const fromSource = new Set(source);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::SetLit(_)))
            .count()
            >= 4,
        "Set constructor did not lower to SetLit"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetContains { .. })),
        "Set.has did not lower to SetContains"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListToSet { .. })),
        "Set constructor from array did not lower to ListToSet"
    );
    Ok(())
}

#[test]
fn lowers_rest_parameters_with_type_level_tuple_alias_constraints() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;
type IterableContainer = readonly unknown[];
type TuplePrefix<T extends IterableContainer> = readonly unknown[];
type TupleSuffix<T extends IterableContainer> = readonly unknown[];
type RemovePrefix<
  T extends IterableContainer,
  Prefix extends TuplePrefix<T>,
> = readonly unknown[];
type RemoveSuffix<
  T extends IterableContainer,
  Suffix extends TupleSuffix<T>,
> = readonly unknown[];

export function partialBind<
  F extends StrictFunction,
  PrefixArgs extends TuplePrefix<Parameters<F>>,
  RemovedPrefix extends RemovePrefix<Parameters<F>, PrefixArgs>,
>(
  func: F,
  ...partial: PrefixArgs
): (
  ...rest: RemovedPrefix extends IterableContainer ? RemovedPrefix : never
) => ReturnType<F> {
  return (...rest) => func(...partial, ...rest);
}

export function partialLastBind<
  F extends StrictFunction,
  SuffixArgs extends TupleSuffix<Parameters<F>>,
  RemovedSuffix extends RemoveSuffix<Parameters<F>, SuffixArgs>,
>(
  func: F,
  ...partial: SuffixArgs
): (
  ...rest: RemovedSuffix extends IterableContainer ? RemovedSuffix : never
) => ReturnType<F> {
  return (...rest) => func(...rest, ...partial);
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_all_rest_tuple_spread_return_type_as_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type IterableContainer = readonly unknown[];

export const concatImplementation = <
  T1 extends IterableContainer,
  T2 extends IterableContainer,
>(
  arr1: T1,
  arr2: T2,
): [...T1, ...T2] => [...arr1, ...arr2];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if matches!(
                    ctx.krate.symbols.get(function.name),
                    Some("concatImplementation" | "concat_implementation")
                ) =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing concatImplementation".to_owned())?;
    ensure!(
        matches!(ctx.krate.types.get(function.return_ty), Some(Type::List(_))),
        "expected all-rest tuple spread return type to lower as a list",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_random_bigint_stdlib_surface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function asBigInt(bytes: Iterable<number>): bigint {
  let result = 0n;
  for (const byte of bytes) {
    result = (result << 8n) + BigInt(byte);
  }
  return result >> 1n;
}

function random(numBytes: number): Uint8Array {
  const output = new Uint8Array(numBytes);
  if (typeof crypto === "undefined") {
    for (let index = 0; index < numBytes; index += 1) {
      output[index] = Math.floor(Math.random() * 256);
    }
  } else {
    crypto.getRandomValues(output);
  }
  return output;
}

const text = (10n).toString(2);
const bits = text.length;
const pivot = (4 + 10) >>> 1;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::BinOp {
                        op: BinOp::Shl | BinOp::Shr | BinOp::UShr,
                        ..
                    }
                )
            })
        }),
        "bitwise shift operators did not lower"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericToStringRadix { .. })),
        "number.toString(radix) did not lower"
    );
    Ok(())
}

#[test]
fn lowers_array_from_length_mapper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function range(start: number, length: number, step: number): number[] {
  return Array.from({ length }, (_, i) => (i === 0 ? start : start + i * step));
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(
        ctx.krate.bodies.iter().any(|body| {
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::ListFromLengthMap { .. }))
        }),
        "Array.from({{ length }}, mapper) did not lower"
    );
    Ok(())
}

#[test]
fn lowers_array_from_length_without_mapper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const sparse = Array.from({ length: 1000 });"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListFromLength { .. })),
        "Array.from({{ length }}) did not lower"
    );
    Ok(())
}

#[test]
fn accepts_assignable_arrow_return_annotations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function use<T, R>(items: readonly T[], fn: (item: T) => R): R {
  return fn(items[0]);
}

const stringValue = use(
  [
    { a: "cat", b: 123 },
    { a: "dog", b: 456 },
  ] as const,
  (x): string => x.a,
);

const numberValue = use(
  [
    { a: "cat", b: 123 },
    { a: "dog", b: 456 },
  ] as const,
  (x): number => x.b,
);

const unionValue = use(
  [
    { a: "cat", b: 123 },
    { a: "dog", b: 456 },
  ] as const,
  (x): number | string => x.b,
);
"#),
        &mut ctx,
    )?;
    Ok(())
}
