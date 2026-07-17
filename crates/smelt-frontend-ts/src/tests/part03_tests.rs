use super::*;

#[test]
fn lowers_math_rounding_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const value = 5.5;
const floor = Math.floor(value);
const ceil = Math.ceil(value);
const round = Math.round(value);
const trunc = Math.trunc(value);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        NumericRoundOp::Floor,
        NumericRoundOp::Ceil,
        NumericRoundOp::Round,
        NumericRoundOp::Trunc,
    ] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericRound { op, .. } if op == expected)
            )
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_ceil_after_sorted_array_destructuring() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function overlap(right: number, left: number, millisecondsInDay: number): number {
  const [start, end] = [right, left].sort((a, b) => a - b);
  return Math.ceil((end - start) / millisecondsInDay);
}
"),
        &mut ctx,
    )?;

    let float_ty = ctx.krate.types.intern(Type::Float);
    ensure!(
        ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::NumericRound {
                        op: NumericRoundOp::Ceil,
                        operand
                    } if body
                        .exprs
                        .get(operand.0 as usize)
                        .is_some_and(|operand| operand.ty == float_ty)
                )
            })
        }),
        "expected Math.ceil operand to stay a concrete number after array destructuring"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_ceil_after_datearg_array_destructuring() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
type DateArg<DateType extends Date> = DateType | number | string;
interface Interval<DateType extends Date = Date> {
  start: DateArg<DateType>;
  end: DateArg<DateType>;
}
function toDate<DateType extends Date>(date: DateArg<DateType>): DateType {
  return date as DateType;
}
function offset(value: number): number {
  return value;
}
export function overlap(intervalLeft: Interval, intervalRight: Interval, millisecondsInDay: number): number {
  const [leftStart, leftEnd] = [
    +toDate(intervalLeft.start),
    +toDate(intervalLeft.end),
  ].sort((a, b) => a - b);
  const [rightStart, rightEnd] = [
    +toDate(intervalRight.start),
    +toDate(intervalRight.end),
  ].sort((a, b) => a - b);
  const overlapLeft = rightStart < leftStart ? leftStart : rightStart;
  const left = overlapLeft - offset(overlapLeft);
  const overlapRight = rightEnd > leftEnd ? leftEnd : rightEnd;
  const right = overlapRight - offset(overlapRight);
  return Math.ceil((right - left) / millisecondsInDay);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_number_arithmetic_as_number() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function read(values: Array<number | undefined>): number {
  const left = values[0];
  const right = values[1];
  return (right - left) / 10;
}
"),
        &mut ctx,
    )?;
    let float_ty = ctx.krate.types.intern(Type::Float);
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::BinOp {
                        op: BinOp::Div,
                        ..
                    } if expr.ty == float_ty
                )
            })),
        "expected arithmetic with optional numeric operands to lower as a concrete number"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn narrows_unannotated_local_after_numeric_assignment() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function years(value: number): number {
  let months;
  if (value < 2) {
    months = Math.round(value);
    return months;
  }
  months = value;
  return Math.trunc(months / 12);
}
"),
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
                ExprKind::NumericRound {
                    op: NumericRoundOp::Trunc,
                    ..
                }
            )),
        "expected Math.trunc to accept the assigned numeric local",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_extrema_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const first = 1;
const second = 2;
const highest = Math.max(first, second, 3);
const lowest = Math.min(first, second, -1);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [NumericExtremaOp::Max, NumericExtremaOp::Min] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericExtrema { op, .. } if op == expected)
        ));
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_extrema_spread_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const highest = Math.max(...values);
const bounded = Math.min(0, ...values);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    // The spread list is carried by the `spread` operand rather than being
    // treated as a single scalar `TypeAssert`-to-float argument.
    ensure!(body.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::NumericExtrema {
            op: NumericExtremaOp::Max,
            args,
            spread: Some(_),
        } if args.is_empty()
    )));
    ensure!(body.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::NumericExtrema {
            op: NumericExtremaOp::Min,
            args,
            spread: Some(_),
        } if args.len() == 1
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_case_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const lower = word.toLowerCase();
const upper = word.toUpperCase();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Lower,
            ..
        }
    )));
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Upper,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_case_methods_on_erased_receivers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
declare const value: unknown;
const lower = value.toLowerCase();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Lower,
            ..
        }
    )));
    Ok(())
}

#[test]
fn lowers_string_trim_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = " Smelt ";
const trimmed = word.trim();
const left = word.trimStart();
const right = word.trimEnd();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        StringTrimSide::Both,
        StringTrimSide::Start,
        StringTrimSide::End,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::StringTrim { side, .. } if side == expected)
        ));
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_prefix_suffix_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const starts = word.startsWith("Sm");
const ends = word.endsWith("lt");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [StringAffixOp::StartsWith, StringAffixOp::EndsWith] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringAffix { op, .. } if op == expected)
            )
        );
    }
    Ok(())
}

#[test]
fn lowers_string_search_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const first = word.indexOf("m");
const last = word.lastIndexOf("t");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [StringSearchOp::Find, StringSearchOp::RFind] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringSearch { op, .. } if op == expected)
            )
        );
    }
    Ok(())
}

#[test]
fn lowers_string_replace_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "hello hello";
const replaced = word.replace("hello", "hi");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringReplace {
            op: StringReplaceOp::First,
            ..
        }
    )));
    Ok(())
}

#[test]
fn lowers_global_regex_replace_literal() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = "a1b2";
const replaced = value.replace(/\d/g, "x");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::RegexReplace {
            op: StringReplaceOp::All,
            ..
        }
    )));
    Ok(())
}

#[test]
fn lowers_global_regex_replace_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const digits: Record<string, string> = { "1": "one" };
const replaced = "1".replace(/\d/g, function (match) {
  return digits[match];
});
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::RegexReplaceCallback {
            op: StringReplaceOp::All,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_regex_replace_uppercase_inside_template_literal() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function format(units: string[]): string[] {
  return units.map((unit) => `x${unit.replace(/(^.)/, (m) => m.toUpperCase())}` as string);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_string_key_record_access() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function values(record: Record<string, number>, keys: string[]): number[] {
  return keys.map((key) => record[key]);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn falls_back_for_array_callback_reading_module_const() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const TOKENS = ["MMM", "MMMM"];

export function hasLong(parts: { value: string }[]): boolean {
  return parts.some((part) => TOKENS.includes(part.value));
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_includes_inside_find_callback_after_array_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Parser = { incompatibleTokens: string[] | "*" };
type UsedToken = { token: string };

export function findIncompatible(parser: Parser, usedTokens: UsedToken[]) {
  if (Array.isArray(parser.incompatibleTokens)) {
    return usedTokens.find(
      (usedToken) =>
        parser.incompatibleTokens.includes(usedToken.token) ||
        usedToken.token === "x",
    );
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn records_mixed_object_const_as_unknown_module_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function week(value: boolean) {
  return (date: Date) => "week";
}

const formatRelativeLocale = {
  lastWeek: week(false),
  today: "'today'",
};

export function formatRelative(token: string): unknown {
  return formatRelativeLocale[token];
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_expression_array_callback_with_outer_capture() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function extract(token: string): string | undefined {
  const result = ["lessThan", "about"].filter(function (preposition) {
    return !!token.match(new RegExp("^" + preposition));
  });
  return result[0];
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn types_addition_with_string_operand_as_string() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function format(count: number, translated: string): string {
  const result = count + translated;
  return true ? translated : result;
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn allows_split_on_unknown_string_flow() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function words(value: unknown): string[] {
  return value.split(" ");
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn allows_last_switch_case_without_break() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function suffix(value: string): string {
  let result = "";
  switch (value) {
    case "a":
      result += "a";
      break;
    default:
      result += value;
  }
  return result;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_repeat_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "ha";
const repeated = word.repeat(3);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringRepeat { .. }))
    );
    Ok(())
}

#[test]
fn lowers_string_padding_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "7";
const paddedStart = word.padStart(3, "0");
const paddedEnd = word.padEnd(3);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [StringPadOp::Start, StringPadOp::End] {
        ensure!(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::StringPad { op, .. } if op == expected))
        );
    }
    Ok(())
}

#[test]
fn lowers_string_char_at_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const char = word.charAt(1);
const code = word.charCodeAt(2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringCharAt { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringCharCodeAt { .. }))
    );
    Ok(())
}

#[test]
fn lowers_array_join_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const words: string[] = ["a", "b", "c"];
const joined = words.join("-");
const comma = words.join();
const values: readonly unknown[] = [1, "b"];
const unknownJoined = values.join("-");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let join_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringJoin { .. }))
        .count();
    ensure_eq!(join_count, 3);
    Ok(())
}

#[test]
fn lowers_array_concat_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const left: number[] = [1, 2];
const right: number[] = [3, 4];
const merged = left.concat(right);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListConcat { .. }))
    );
    Ok(())
}

#[test]
fn lowers_array_search_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3, 2];
const first = values.indexOf(2);
const last = values.lastIndexOf(2);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [ListSearchOp::Find, ListSearchOp::RFind] {
        ensure!(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::ListSearch { op, .. } if op == expected))
        );
    }
    Ok(())
}

#[test]
fn lowers_array_search_methods_with_from_index() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function findFrom(values: readonly number[], target: number, from: number): number {
  return values.indexOf(target, from);
}
export function findLastFrom(values: readonly number[], target: number, from: number): number {
  return values.lastIndexOf(target, from);
}
"),
        &mut ctx,
    )?;
    let _ = module_id;

    // Both search directions must carry the optional `fromIndex` operand. The
    // function bodies live in the crate body list, not the module top-level body.
    for expected in [ListSearchOp::Find, ListSearchOp::RFind] {
        let found_with_index = ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::ListSearch {
                        op,
                        from_index: Some(_),
                        ..
                    } if op == expected
                )
            })
        });
        ensure!(found_with_index);
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_callback_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const mapped = values.map(value => value + 1);
const filtered = values.filter(value => value > 1);
const found = values.find(value => value > 1);
const foundIndex = values.findIndex(value => value > 1);
const hasAny = values.some(value => value > 1);
const hasEvery = values.every(value => value > 0);
values.forEach(value => value + 1);
const total = values.reduce((acc, value) => acc + value, 0);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        ListCallbackOp::Map,
        ListCallbackOp::Filter,
        ListCallbackOp::Find,
        ListCallbackOp::FindIndex,
        ListCallbackOp::Some,
        ListCallbackOp::Every,
    ] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::ListCallback { op, .. } if op == expected)
            ),
            "missing callback op {expected:?}"
        );
    }
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListReduce { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. })),
        "missing forEach statement loop"
    );
    Ok(())
}

#[test]
fn lowers_array_callback_index_parameters() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const mapped = values.map((value, index) => value + index);
const filtered = values.filter((value, index) => value > index);
const found = values.find((value, index) => value === index);
const foundIndex = values.findIndex((value, index) => value === index);
const hasAny = values.some((value, index) => value > index);
const hasEvery = values.every((value, index) => value >= index);
values.forEach((value, index) => value + index);
const total = values.reduce((acc, value, index) => acc + value + index);
const arrays = values.map((value, index, array) => array);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        ListCallbackOp::Map,
        ListCallbackOp::Filter,
        ListCallbackOp::Find,
        ListCallbackOp::FindIndex,
        ListCallbackOp::Some,
        ListCallbackOp::Every,
    ] {
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::ListCallback { op, callback, .. }
                    if *op == expected && closure_callback_has_param(&ctx, body, *callback, 1)
            )),
            "missing callback index param for {expected:?}"
        );
    }
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. })),
        "missing forEach statement loop using callback index param"
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListReduce {
                initial: None,
                callback,
                ..
            } if matches!(
                body.exprs.get(callback.0 as usize).map(|expr| &expr.kind),
                Some(ExprKind::Closure(closure)) if closure.params.len() > 2
            )
        )),
        "missing reduce without initial value using callback index param"
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListCallback {
                op: ListCallbackOp::Map,
                callback,
                ..
            } if closure_callback_has_param(&ctx, body, *callback, 2)
        )),
        "missing callback array param"
    );
    Ok(())
}

#[test]
fn lowers_modern_array_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let values: number[] = [1, 2, 3, 4];
let nested: number[][] = [[1], [2, 3]];
let erasedNested: unknown[] = [[1], [[2]]];
const spliced = values.splice(1, 2, 9);
const copiedSplice = values.toSpliced(1, 1, 8);
const spreadSplice = values.toSpliced(1, 1, ...[10, 11]);
const filled = values.fill(0, 1, 3);
const copiedWithin = values.copyWithin(0, 1, 3);
const replaced = values.with(1, 7);
const flat = nested.flat();
const deepFlat = erasedNested.flat(2);
const flatMapped = values.flatMap((value, index) => [value + index]);
const sorted = values.toSorted((left, right) => right - left);
const reversed = values.toReversed();
const last = values.findLast((value, index) => value > index);
const lastIndex = values.findLastIndex((value, index) => value > index);
const keys = values.keys();
const vals = values.values();
const entries = values.entries();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::ListSplice { .. }))
            .count(),
        3
    );
    ensure!(body.exprs.iter().any(|expr| {
        matches!(
            &expr.kind,
            ExprKind::ListSplice { items, .. } if items.iter().any(|item| item.spread)
        )
    }));
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListFill { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListCopyWithin { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListWith { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListFlat { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListFlat { depth: Some(_), .. })),
        "expected explicit array flat depth to remain in HIR"
    );
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::ListCallback {
            op: ListCallbackOp::FlatMap,
            ..
        }
    )));
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::ListSort {
            comparator: Some(_),
            ..
        }
    )));
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListReversed { .. }))
    );
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::ListCallback {
            op: ListCallbackOp::FindLast,
            ..
        }
    )));
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::ListCallback {
            op: ListCallbackOp::FindLastIndex,
            ..
        }
    )));
    for expected in [
        ListProjectionOp::Keys,
        ListProjectionOp::Values,
        ListProjectionOp::Entries,
    ] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::ListProjection { op, .. } if op == expected)
            ),
            "missing list projection {expected:?}"
        );
    }
    Ok(())
}

#[test]
fn lowers_array_callback_captures() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const minimum = values.length;
const filtered = values.filter(value => value > minimum);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            let ExprKind::ListCallback {
                op: ListCallbackOp::Filter,
                callback,
                ..
            } = &expr.kind
            else {
                return false;
            };
            matches!(
                body.exprs.get(callback.0 as usize).map(|expr| &expr.kind),
                Some(ExprKind::Closure(closure))
                    if closure_has_cfg_body(&ctx, closure)
                        && closure.captures.iter().all(|capture| capture.body_local.is_some())
                        && !closure.captures.is_empty()
            )
        }),
        "missing CFG-backed captured filter callback"
    );
    Ok(())
}

#[test]
fn lowers_static_callback_indexes_into_closure_bodies() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: string[][] = [["a"], ["b"]];
const first = values.map(value => value[0]);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListCallback { callback, .. }
                if matches!(
                    body.exprs.get(callback.0 as usize).map(|expr| &expr.kind),
                    Some(ExprKind::Closure(closure)) if closure_has_cfg_body(&ctx, closure)
                )
        )),
        "missing CFG-backed static-index callback"
    );
    Ok(())
}

#[test]
fn lowers_read_only_mutable_filter_callback_captures() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
let minimum = 1;
const filtered = values.filter(value => value > minimum);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListCallback {
                op: ListCallbackOp::Filter,
                callback,
                ..
            } if matches!(
                body.exprs.get(callback.0 as usize).map(|expr| &expr.kind),
                Some(ExprKind::Closure(closure))
                    if closure_has_cfg_body(&ctx, closure)
                        && closure.captures.iter().all(|capture| capture.body_local.is_some())
                        && !closure.captures.is_empty()
            )
        )),
        "missing CFG-backed read-only mutable binding capture"
    );
    Ok(())
}

#[test]
fn lowers_local_closure_callback_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const offset = 10;
const scale = (value: number): number => value + offset;
const mapped = values.map(scale);
"),
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
        "missing local closure callback map"
    );
    Ok(())
}

#[test]
fn lowers_mutable_array_callback_captures() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
let total = 0;
values.forEach(value => total += value);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. })),
        "missing mutable captured forEach loop"
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Assign { .. })),
        "missing mutable captured forEach assignment"
    );
    Ok(())
}

#[test]
fn rejects_const_array_callback_assignment() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
const values: number[] = [1, 2, 3];
const total = 0;
values.forEach(value => total += value);
"),
        &mut ctx,
    )?;

    ensure!(
        errors.iter().any(|err| err
            .message
            .contains("callback assignment to captured const local")),
        "missing const capture assignment diagnostic: {errors:?}"
    );
    Ok(())
}

#[test]
fn lowers_array_and_string_slice_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3, 4];
const allValues = values.slice();
const tailValues = values.slice(1);
const midValues = values.slice(1, 3);
const lastValues = values.slice(-2);
const word = "smelting";
const allText = word.slice();
const tailText = word.slice(1);
const midText = word.slice(1, 4);
const lastText = word.slice(-3);
const subText = word.substring(1, 4);
function sliceOptional(start?: number, end?: number): string {
  return word.slice(start, end);
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let list_slices = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListSlice { .. }))
        .count();
    let string_slices = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringSlice { .. }))
        .count();
    ensure_eq!(list_slices, 4);
    ensure_eq!(string_slices, 5);
    let all_string_slices = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| &body.exprs)
        .filter(|expr| matches!(expr.kind, ExprKind::StringSlice { .. }))
        .count();
    ensure!(
        all_string_slices >= 6,
        "optional string slice inside function body did not lower"
    );
    Ok(())
}

#[test]
fn lowers_array_concat_element_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const values: string[] = [];
const next = values.concat("a");
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_push_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let values: number[] = [1, 2];
values.push(3);
const length = values.push(4);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let pushes = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListPush { .. }))
        .count();
    ensure_eq!(pushes, 2);
    Ok(())
}

#[test]
fn lowers_array_concat_on_erased_value_import_receiver() -> Result<(), String> {
    // A named value import that resolves to an erased array (`falsey` here) used
    // as a `concat` receiver is real `Array.prototype.concat`: the member object
    // is the array and every argument is concatenated onto it. It must not be
    // mistaken for the lodash `ns.concat(collection, ...values)` free-function
    // form (which only applies to namespace/utility objects), and the erased
    // receiver must lower rather than being rejected as "not an array receiver".
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { falsey } from "./falsey";

export function values(): unknown[] {
  return falsey.concat(true, 1, "a");
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body
                .exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::ListConcat { .. })))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_concat_on_union_receiver() -> Result<(), String> {
    // `number[] | string[]` receiver: the union has array members, so concat
    // coerces it to a concrete erased list and appends the arguments instead of
    // rejecting the non-statically-array receiver.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function widen(a: number[] | string[]): unknown[] {
  return a.concat(1, "x");
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body
                .exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::ListConcat { .. })))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_push_tuple_literal_into_tuple_element_array() -> Result<(), String> {
    // Pushing a bare `[value, key]` literal into an `Array<[number, string]>`
    // must adopt the element's concrete tuple shape at construction time. The
    // literal lowers to a `(number, string)` tuple, so no re-typing of a
    // `List<Unknown>` into the tuple destination is required.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function collect(value: number, key: string): Array<[number, string]> {
  const result: Array<[number, string]> = [];
  result.push([value, key]);
  return result;
}
"),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    let function_body = ctx
        .krate
        .bodies
        .iter()
        .find(|body| {
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::ListPush { .. }))
        })
        .ok_or_else(|| "expected a ListPush in the lowered crate".to_owned())?;
    ensure!(
        function_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::TupleLit(_))),
        "pushed array literal should lower as a tuple literal matching the element type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_push_mixed_literals_into_union_element_array() -> Result<(), String> {
    // Mixed-literal pushes into a `Array<number | string>` coerce each argument
    // into the union element type instead of requiring an exact match.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function mixed(): Array<number | string> {
  const xs: Array<number | string> = [];
  xs.push(1);
  xs.push("a");
  return xs;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    let pushes = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::ListPush { .. }))
        .count();
    ensure_eq!(pushes, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_unshift_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let values: number[] = [2, 3];
const sameLength = values.unshift();
const oneMore = values.unshift(1);
const threeMore = values.unshift(-1, 0);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let unshifts = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListUnshift { .. }))
        .count();
    ensure_eq!(unshifts, 3);
    Ok(())
}

#[test]
fn lowers_array_reverse_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let values: number[] = [1, 2];
values.reverse();
const reversed = values.reverse();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let reverses = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListReverse { .. }))
        .count();
    ensure_eq!(reverses, 2);
    Ok(())
}

#[test]
fn lowers_array_pop_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let values: number[] = [1, 2];
values.pop();
const item = values.pop();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let pops = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListPop { .. }))
        .count();
    ensure_eq!(pops, 2);
    Ok(())
}

/// Find a function item by name in a lowered module.
fn function_by_name<'a>(
    ctx: &'a HirCtx,
    module: &'a smelt_hir::Module,
    name: &str,
) -> Result<&'a Function, String> {
    module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) if ctx.krate.names.get(function.name) == Some(name) => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| format!("missing function {name}"))
}

#[test]
fn lowers_erased_spread_into_fresh_list() -> Result<(), String> {
    // `[...items]` where `items` is an erased type parameter constrained to a
    // list (Remeda's `T extends IterableContainer`) must construct a FRESH
    // `List`-typed value, not return the erased alias. Otherwise a following
    // `.sort(cmp)` stays dynamic and its result is discarded (the sort bug).
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function sortImpl<T extends readonly unknown[]>(
  items: T,
  cmp: (a: T[number], b: T[number]) => number,
): T[number][] {
  const ret = [...items];
  ret.sort(cmp);
  return ret;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_by_name(&ctx, module, "sortImpl")?;
    let body = function_body(&ctx, function)?;

    // The fresh-list idiom is `ListConcat(items, [])`.
    let concat = body
        .exprs
        .iter()
        .find(|expr| matches!(expr.kind, ExprKind::ListConcat { .. }))
        .ok_or_else(|| "erased spread should lower to a fresh-list ListConcat".to_owned())?;
    // The concat result must be typed `List`, so the `ret` binding is a real
    // list and the typed in-place `sort` path fires downstream.
    ensure!(matches!(
        ctx.krate.types.get(concat.ty),
        Some(Type::List(_))
    ));
    Ok(())
}

#[test]
fn lowers_typed_spread_to_fresh_list() -> Result<(), String> {
    // A spread `[...items]` is a NEW array in JS (`[...items] !== items`), so even
    // a genuinely `List`-typed operand lowers through the fresh-list idiom
    // `ListConcat(items, [])` rather than aliasing the source.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function copy(items: readonly number[]): number[] {
  const ret = [...items];
  return ret;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_by_name(&ctx, module, "copy")?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListConcat { .. }))
    );
    Ok(())
}
