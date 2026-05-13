use super::*;

#[test]
fn vitest_describe_each_expands_literal_rows() -> Result<(), String> {
    let source = ts!(r#"
import { describe, test, expect } from "vitest";

describe.each([[1], [2]])("group", (value) => {
  test("case", () => {
    expect(value).toBe(value);
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/describe-each.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    Ok(())
}

#[test]
fn converts_top_level_let_and_console_log() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("let x = 6;
console.log(x);
"),
        &mut ctx,
    )?;
    let url_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, url_module)?;

    ensure_eq!(body.locals.len(), 1);
    ensure_eq!(body.stmts.len(), 2);
    ensure_eq!(body.exprs.len(), 4);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_stdlib_length_properties() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const count = values.length;
const word = "smelt";
const letters = word.length;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let len_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
        .count();
    ensure_eq!(len_count, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_index_and_for_of() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "abc";
const first = word[0];
const last = word.at(-1);
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_at_and_rejects_negative_bracket_index() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const last = values.at(-1);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());

    let errors = lowering_errors(
        ts!(r#"
const values: number[] = [1, 2, 3];
const invalid = values[-1];
"#),
        &mut HirCtx::new(),
    )?;
    assert_unsupported_ts(&errors, "negative array/string bracket indexes")
}

#[test]
fn lowers_math_abs_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = -5;
const positive = Math.abs(value);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAbs { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_primitive_conversion_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 42;
const asText = String(value);
const asNumber = Number("42");
const asBool = Boolean("");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        PrimitiveCastOp::ToString,
        PrimitiveCastOp::ToFloat,
        PrimitiveCastOp::ToBool,
    ] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::PrimitiveCast { op, .. } if op == expected)
            )
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_number_to_string_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 42;
const text = value.toString();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ToString,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_number_parse_float_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = Number.parseFloat("42.5");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ToFloat,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_number_parse_int_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = Number.parseInt("42");
const decimalValue = Number.parseInt("42", 10);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let parse_count = body
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToInt,
                    ..
                }
            )
        })
        .count();
    ensure_eq!(parse_count, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_infinity_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const upper = Infinity;
const lower = -Infinity;
const missing = NaN;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value.is_infinite())
    ));
    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value.is_nan())
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_now_and_to_iso_string() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const now = Date.now();
const iso = new Date(now).toISOString();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateNow))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateToIsoString { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_conditional_expression() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const value = true ? \"yes\" : \"no\";"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "missing conditional expression"
    );
    Ok(())
}

#[test]
fn rejects_conditional_expression_with_mismatched_branches() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const value = true ? 1 : \"no\";"), &mut ctx)?;
    assert_unsupported_ts(&errors, "branches must have the same lowered type")
}

#[test]
fn lowers_unary_plus_expression() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const value = +1;"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(1.0)))),
        "missing unary plus numeric value"
    );
    Ok(())
}

#[test]
fn lowers_destructuring_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const pair: number[] = [1, 2];
const [first, second] = pair;
const data: Record<string, number> = { count: 3 };
const { count } = data;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.locals.len() >= 5,
        "expected destructuring declarations to create locals"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. })),
        "missing array destructuring index"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Field { .. })),
        "missing object destructuring field"
    );
    Ok(())
}

#[test]
fn infers_object_literal_record_type_without_annotation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const options = { weekStartsOn: 1 };
const value = options.weekStartsOn;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_))),
        "missing inferred object literal"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Field { .. })),
        "missing inferred object field access"
    );
    Ok(())
}

#[test]
fn lowers_empty_object_for_date_fns_default_options_alias() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface LocalizedOptions {
  locale?: string;
}
interface WeekOptions {
  weekStartsOn?: number;
}
type DefaultOptions = LocalizedOptions & WeekOptions;
let defaultOptions: DefaultOptions = {};
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::DictLit(entries) if entries.is_empty())),
        "missing empty default options object"
    );
    Ok(())
}

#[test]
fn lowers_module_mutable_default_options_accessors() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface LocalizedOptions {
  locale?: string;
}
interface WeekOptions {
  weekStartsOn?: number;
}
type DefaultOptions = LocalizedOptions & WeekOptions;
let defaultOptions: DefaultOptions = {};
function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
function setDefaultOptions(newOptions: DefaultOptions): void {
  defaultOptions = newOptions;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_date_parts() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const date = new Date(2014, 8, 2, 11, 55, 0);
function timestamp(value: number): number {
  return value;
}
const callArg = timestamp(new Date(2014, 8, 1));
const timestamp = date.getTime();
const year = date.getFullYear();
const month = date.getMonth();
const day = date.getDate();
const remaining = day % 5;
const hours = date.getHours();
const minutes = date.getMinutes();
const seconds = date.getSeconds();
const milliseconds = date.getMilliseconds();
const utc = Date.UTC(year, month);
date.setFullYear(year, month, day + 1);
date.setMonth(0, 1);
date.setDate(2);
date.setHours(hours, minutes, seconds, milliseconds);
date.setMinutes(minutes, seconds, milliseconds);
date.setSeconds(seconds, milliseconds);
date.setMilliseconds(milliseconds);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateFromParts { .. }))
    );
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DateGetPart { .. }))
            .count(),
        7
    );
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DateSetPart { .. }))
            .count(),
        7
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_url_fields_and_rejects_deferred_object_apis() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const host = new URL("https://example.com/path?q=1").hostname;
"#),
        &mut ctx,
    )?;
    let url_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, url_module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UrlField { .. }))
    );

    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [3, 1, 2];
const sorted = values.sort();
const sortedByCompare = values.sort((left, right) => left - right);
"#),
        &mut ctx,
    )?;
    let sort_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, sort_module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSort { .. }))
    );
    ensure!(body.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::ListSort {
            comparator: Some(callback),
            ..
        } if callback_has_param(callback, 1)
    )));
    Ok(())
}

#[test]
fn lowers_instanceof_for_class_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Box {}
const value = new Box();
const result = value instanceof Box;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::InstanceOf { .. }))
    );
    Ok(())
}

#[test]
fn folds_date_instanceof_for_timestamp_model() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type DateArg = number | string | Date;
const value: DateArg = 1;
const result = value instanceof Date;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(false))))
    );
    Ok(())
}

#[test]
fn lowers_date_constructor_member_as_timestamp_passthrough() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const date: Date = new Date(1);
const value = 2;
const result = new (date.constructor as unknown)(value);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::Literal(Literal::Int(2) | Literal::Float(2.0))
    )));
    Ok(())
}

#[test]
fn lowers_new_date_from_datearg_union_to_timestamp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type DateArg = number | string | Date;
const value: DateArg = 1;
const result = new Date(value);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ToFloat,
            ..
        }
    )));
    Ok(())
}

#[test]
fn lowers_unary_plus_datearg_to_timestamp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type DateArg = number | string | Date;
function timestamp(value: DateArg): number {
  return +value;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(module.items.iter().any(|item| {
        let Some(Item::Function(function)) = ctx.krate.items.get(item.0 as usize) else {
            return false;
        };
        let Some(body_id) = function.body else {
            return false;
        };
        let Some(body) = ctx.krate.bodies.get(body_id.0 as usize) else {
            return false;
        };
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToFloat,
                    ..
                }
            )
        })
    }));
    Ok(())
}

#[test]
fn lowers_optional_chain_or_fallback_to_rhs_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options {
  in?: number;
}
function read(options: Options, date: number): number {
  return options?.in || date;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_chain_call_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Options {
  in?: number;
}
function useContext(date: number, context?: number): number {
  return context || date;
}
function read(options: Options, date: number): number {
  return useContext(date, options?.in);
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_chain_context_into_date_get_day() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface ContextOptions<DateType extends Date = Date> {
  in?: DateType;
}
type DateArg<DateType extends Date> = DateType | number | string;
function toDate<DateType extends Date>(
  date: DateArg<DateType>,
  context?: DateType,
): DateType {
  return date as DateType;
}
function isSaturday<DateType extends Date>(
  date: DateArg<DateType>,
  options: ContextOptions<DateType>,
): boolean {
  return toDate(date, options?.in).getDay() === 6;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(
        module.items.iter().any(|item| {
            let Some(Item::Function(function)) = ctx.krate.items.get(item.0 as usize) else {
                return false;
            };
            let Some(body_id) = function.body else {
                return false;
            };
            let body = &ctx.krate.bodies[body_id.0 as usize];
            body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::DateGetPart {
                        part: DatePart::Day,
                        ..
                    }
                )
            })
        }),
        "expected getDay() to lower as a Date day-of-week operation",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_node_probe_and_date_to_string() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
if (process.env.TZ !== "America/Santiago")
  throw new Error("bad timezone");

if (parseInt(process.version.match(/^v(\d+)\./)?.[1] || "0") < 10)
  throw new Error("bad version");

const rendered = new Date(2014, 8, 1).toString();
console.log(rendered);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToString,
                ..
            }
        )),
        "expected Date .toString() to lower through a string primitive cast",
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. })),
        "expected the Node environment probes to lower as conditional statements",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_locale_type_surfaces() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import type { DateArg, WeekOptions } from "../types";

export interface LocaleOptions extends WeekOptions {}

export type FormatDistanceFn = (
  token: FormatDistanceToken,
  count: number,
  options?: FormatDistanceFnOptions,
) => string;

export interface FormatDistanceFnOptions {
  comparison?: -1 | 0 | 1;
}

export type FormatDistanceLocale<Template> = {
  [Token in FormatDistanceToken]: Template;
};

export type FormatDistanceToken = "xSeconds" | "xMinutes";

export type FormatRelativeFn = <DateType extends Date>(
  date: DateArg<DateType>,
  options?: { unit: LocaleUnit },
) => string;

export type LocaleUnit = "second" | "minute";
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_fp_type_surfaces() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export type FPFnInput = (...args: any[]) => any;
export type FPArity = 1 | 2 | 3 | 4;
export type FPFn<Fn extends FPFnInput> = FPFn2<
  ReturnType<Fn>,
  Parameters<Fn>[1],
  Parameters<Fn>[0]
>;
export interface FPFn1<Result, Arg> {
  (arg: Arg): Result;
}
export interface FPFn2<Result, Arg2, Arg1> {
  (arg2: Arg2): FPFn1<Result, Arg1>;
  (arg2: Arg2, arg1: Arg1): Result;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn skips_date_fns_context_options_type_only_heritage() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export interface AddOptions<DateType extends Date = Date> extends ContextOptions<DateType> {}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn ignores_safe_function_overload_signatures() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function double(value: number): number;
function double(value: number): number {
  return value * 2;
}

export function identity(value: string): string;
export function identity(value: string): string {
  return value;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);

    let first = function_item(&ctx, module, 0)?;
    let second = function_item(&ctx, module, 1)?;
    ensure!(first.body.is_some());
    ensure!(second.body.is_some());
    Ok(())
}

#[test]
fn rejects_unimplemented_function_overload_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
function missing(value: number): number;
"#),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "declare functions are not lowered yet")
}

#[test]
fn ignores_exported_ambient_declare_functions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export declare function addLeadingZeros(number: number, targetLength: number): string;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(module.items.is_empty());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_exported_arrow_const_from_function_type_annotation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type FormatDistanceFn = (
  token: string,
  count: number,
  options?: { addSuffix?: boolean },
) => string;

export const formatDistance: FormatDistanceFn = (token, count, options) => {
  if (options?.addSuffix) {
    return token + count.toString();
  }
  return token;
};
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize) {
            Some(Item::Function(function)) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "expected exported arrow const to lower as a function item".to_owned())?;
    ensure_eq!(function.params.len(), 3);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_field_access_on_object_branch_of_union() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type TokenValue = string | { one: string; other: string };

function resolve(value: TokenValue, count: number): string {
  if (typeof value === "string") {
    return value;
  }
  if (count === 1) {
    return value.one;
  }
  return value.other;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_replace_on_erased_type_surface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function format(value: ExternalTokenValue): string {
  return value.other.replace("{{count}}", "2");
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_global_numeric_parse_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const intValue = parseInt("42");
const decimalIntValue = parseInt("42", 10);
const floatValue = parseFloat("42.5");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let int_parse_count = body
        .exprs
        .iter()
        .filter(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToInt,
                    ..
                }
            )
        })
        .count();
    ensure_eq!(int_parse_count, 2);
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ToFloat,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
