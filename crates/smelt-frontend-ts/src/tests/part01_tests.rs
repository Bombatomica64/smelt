use super::*;

#[test]
fn vitest_public_api_imports_lower_as_test_builtins() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, test, beforeEach, afterEach } from "vitest";

describe("group", () => {});
it("case", () => {});
test("case 2", () => {});
beforeEach(() => {});
afterEach(() => {});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/example.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure_eq!(body.stmts.len(), 0);
    Ok(())
}

#[test]
fn effect_vitest_concurrent_describe_lowers_as_test_builtin() -> Result<(), String> {
    let source = ts!(r#"
import { describe, test } from "@effect/vitest";

describe.concurrent("group", () => {});
test("case", () => {});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        source,
        "packages/typeclass/test/data/Number.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure_eq!(body.stmts.len(), 0);
    Ok(())
}

#[test]
fn generic_interfaces_substitute_defaults_and_inherited_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
interface ContextOptions<DateType extends Date = Date> {
  in?: DateType;
}

interface AddOptions<DateType extends Date = Date> extends ContextOptions<DateType> {
  amount: number;
}

export type DateArg<DateType extends Date> = DateType | number | string;
export type ContextFn<DateType extends Date> = (value: DateArg<Date> & {}) => DateType;

function sameDate<DateType extends Date>(
  date: DateType,
  options: AddOptions<DateType>,
  normalize: ContextFn<DateType>
): DateType {
  return date;
}
"#),
        "src/generic-options.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let interface = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Interface(interface)
                if ctx.krate.symbols.get(interface.name) == Some("AddOptions") =>
            {
                Some(interface)
            }
            _ => None,
        })
        .ok_or_else(|| "missing AddOptions interface".to_owned())?;

    ensure_eq!(interface.type_params.len(), 1);
    let inherited = interface
        .fields
        .iter()
        .find(|field| ctx.krate.symbols.get(field.name) == Some("in"))
        .ok_or_else(|| "missing inherited `in` field".to_owned())?;
    let Some(Type::Optional(inner)) = ctx.krate.types.get(inherited.ty) else {
        return Err("inherited field should be optional".to_owned());
    };
    ensure!(matches!(
        ctx.krate.types.get(*inner),
        Some(Type::TypeParam { name })
            if ctx.krate.symbols.get(*name) == Some("DateType")
    ));

    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) if ctx.krate.names.get(function.name) == Some("sameDate") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing sameDate function".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(function.return_ty),
        Some(Type::TypeParam { name })
            if ctx.krate.symbols.get(*name) == Some("DateType")
    ));
    ensure!(matches!(
        ctx.krate.types.get(function.params[2].ty),
        Some(Type::Function(function))
            if matches!(
                ctx.krate.types.get(function.return_ty),
                Some(Type::TypeParam { name })
                    if ctx.krate.symbols.get(*name) == Some("DateType")
            )
    ));
    Ok(())
}

#[test]
fn date_fns_shared_types_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r##"
export type DateArg<DateType extends Date> = DateType | number | string;

export interface ConstructableDate extends Date {
  [constructFromSymbol]: <DateType extends Date = Date>(
    value: DateArg<Date> & {},
  ) => DateType;
}

export interface GenericDateConstructor<DateType extends Date = Date> {
  new (): DateType;
  new (value: DateArg<Date> & {}): DateType;
  new (
    year: number,
    month: number,
    date?: number,
    hours?: number,
    minutes?: number,
    seconds?: number,
    ms?: number,
  ): DateType;
}

export interface Duration {
  years?: number;
  months?: number;
}

export type DurationUnit = keyof Duration;
export interface LocalizedOptions<LocaleFields extends keyof Locale> {
  locale?: Pick<Locale, LocaleFields>;
}
export type NearestMinutesOptions = NearestToUnitOptions<1 | 2>;
export interface ContextOptions<DateType extends Date> {
  in?: ContextFn<DateType> | undefined;
}
export type ResultType<DateType extends Date> = DateType extends Date ? DateType : Date;
"##),
        "src/types.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(
        module.items.len() >= 4,
        "expected date-fns shared type items to lower"
    );
    let duration_unit = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::TypeAlias(alias) if ctx.krate.symbols.get(alias.name) == Some("DurationUnit") => {
                Some(alias)
            }
            _ => None,
        })
        .ok_or_else(|| "missing DurationUnit alias".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(duration_unit.ty),
        Some(Type::String)
    ));
    Ok(())
}

#[test]
fn lowers_never_type_surface_without_runtime_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export type StrictFunction = (...args: never) => unknown;
export type Value = string | never;
export type ImpossibleTuple = [never, ...never[]];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;

    let strict_function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::TypeAlias(alias)
                if ctx.krate.symbols.get(alias.name) == Some("StrictFunction") =>
            {
                Some(alias)
            }
            _ => None,
        })
        .ok_or_else(|| "missing StrictFunction alias".to_owned())?;
    let Some(Type::Function(function)) = ctx.krate.types.get(strict_function.ty) else {
        return Err("StrictFunction did not lower to a function type".to_owned());
    };
    ensure!(matches!(
        function.params.as_slice(),
        [param] if matches!(ctx.krate.types.get(*param), Some(Type::Never))
    ));
    ensure!(matches!(
        ctx.krate.types.get(function.return_ty),
        Some(Type::Unknown)
    ));

    let value = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::TypeAlias(alias) if ctx.krate.symbols.get(alias.name) == Some("Value") => {
                Some(alias)
            }
            _ => None,
        })
        .ok_or_else(|| "missing Value alias".to_owned())?;
    ensure!(matches!(ctx.krate.types.get(value.ty), Some(Type::String)));

    let tuple = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::TypeAlias(alias)
                if ctx.krate.symbols.get(alias.name) == Some("ImpossibleTuple") =>
            {
                Some(alias)
            }
            _ => None,
        })
        .ok_or_else(|| "missing ImpossibleTuple alias".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(tuple.ty),
        Some(Type::Tuple(items))
            if matches!(items.as_slice(), [item] if matches!(ctx.krate.types.get(*item), Some(Type::Never)))
    ));
    Ok(())
}

#[test]
fn rejects_runtime_never_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const value: never = 1;"), &mut ctx)?;
    assert_unsupported_ts(&errors, "never")?;

    let errors = lowering_errors(ts!("const value: [never] = [1];"), &mut HirCtx::new())?;
    assert_unsupported_ts(&errors, "never")
}

#[test]
fn exported_literal_constants_are_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!("export const monthsInQuarter = 3;\n"),
        "src/constants/index.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { monthsInQuarter } from "../constants/index.ts";

export function quartersToMonths(quarters: number): number {
  return Math.trunc(quarters * monthsInQuarter);
}
"#),
        "src/quartersToMonths/index.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == 3.0)
        )
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn exported_foldable_constants_are_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export const base = Math.pow(10, 2);
export const maxTime = base * 5;
export const minTime = -maxTime;
export const rounded = Math.trunc((+minTime) / 3);
"#),
        "src/constants.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { minTime, rounded } from "./constants";
const value = minTime + rounded;
"#),
        "src/main.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == -500.0)
    ));
    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == -166.0)
    ));
    Ok(())
}

#[test]
fn date_fns_constant_slice_folds_importable_numeric_consts() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export const daysInWeek = 7;
export const daysInYear = 365.2425;
export const maxTime = Math.pow(10, 8) * 24 * 60 * 60 * 1000;
export const minTime = -maxTime;
export const secondsInHour = 3600;
export const secondsInDay = secondsInHour * 24;
export const secondsInWeek = secondsInDay * +daysInWeek;
export const secondsInYear = secondsInDay * (daysInYear);
export const secondsInMonth = secondsInYear / 12;
export const secondsInQuarter = Math.trunc(secondsInMonth * 3);
export const monthsInQuarter = 3;
export const constructFromSymbol = Symbol.for("constructDateFrom");
"#),
        "src/constants/index.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { monthsInQuarter, secondsInQuarter, minTime } from "../constants/index.ts";

export function quartersToMonths(quarters: number): number {
  return Math.trunc(quarters * monthsInQuarter + secondsInQuarter * 0 + minTime * 0);
}
"#),
        "src/quartersToMonths/index.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == 3.0)
        )
    );
    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == -8640000000000000.0)
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn reexported_named_items_are_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!("export function add(a: number, b: number): number { return a + b; }\n"),
        "src/math.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!("export { add as plus } from \"./math\";\n"),
        "src/index.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { plus } from "./index";
const value = plus(2, 3);
"#),
        "src/main.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. }))
    );
    Ok(())
}

#[test]
fn namespace_import_members_are_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!("export function double(value: number): number { return value * 2; }\n"),
        "src/number.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import * as NumberInstances from "./number";
const value = NumberInstances.double(4);
"#),
        "src/main.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. }))
    );
    Ok(())
}

#[test]
fn exported_arrow_function_constants_are_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!("export const double = (value: number): number => value * 2;\n"),
        "src/number.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { double } from "./number";
const value = double(4);
"#),
        "src/main.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. }))
    );
    Ok(())
}

#[test]
fn exported_object_constants_can_act_as_namespace_apis() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export function double(value: number): number { return value * 2; }
export const NumberInstances = { double };
"#),
        "src/number.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { NumberInstances } from "./number";
const value = NumberInstances.double(4);
"#),
        "src/main.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Call { .. }))
    );
    Ok(())
}

#[test]
fn unknown_and_readonly_unknown_array_types_lower() -> Result<(), String> {
    let source = ts!(r#"
export function identity(value: unknown): unknown {
  return value;
}

export function passthrough(values: readonly unknown[]): readonly unknown[] {
  return values;
}
"#);
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Unknown)),
        "expected unknown to lower as a distinct HIR type",
    );
    ensure!(
        ctx.krate
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::List(item) if matches!(ctx.krate.types.get(*item), Some(Type::Unknown)))),
        "expected readonly unknown[] to lower as List<Unknown>",
    );
    Ok(())
}

#[test]
fn unknown_narrowing_guards_and_assertions_lower() -> Result<(), String> {
    let source = ts!(r#"
function assertString(value: unknown): asserts value is string {
  if (typeof value !== "string") {
    throw "not string";
  }
}

export function read(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  assertString(value);
  return value;
}

export function isList(value: unknown): boolean {
  return Array.isArray(value);
}

export function isNull(value: unknown): boolean {
  return value === null;
}
"#);
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::UnknownIs { .. })),
        "expected unknown runtime tag checks",
    );
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::UnknownCast { .. })),
        "expected narrowed unknown extraction",
    );
    Ok(())
}

#[test]
fn vitest_common_positive_matchers_lower_to_assertion_checks() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

test("common matchers", () => {
  expect(1 + 1).toEqual(2);
  expect([1, 2, 3]).toContain(2);
  expect([1, 2, 3]).toHaveLength(3);
  expect(["a"]).toStrictEqual(["a"]);
  expect([1, 2, 3]).not.toContain(4);
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/example.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let test_fn = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, test_fn)?;

    ensure!(test_fn.is_test);
    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(
                expr.kind,
                ExprKind::BinOp {
                    op: BinOp::NotEq,
                    ..
                }
            ))
            .count()
            >= 3
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListContains { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::UnaryOp { .. }))
            .count()
            >= 2
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Len { .. }))
    );
    Ok(())
}

#[test]
fn vitest_lifecycle_hooks_are_inlined_into_tests() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect, beforeEach, afterEach } from "vitest";

beforeEach(() => {
  expect(true).toBe(true);
});
afterEach(() => {
  expect(true).toBe(true);
});
test("uses hooks", () => {
  expect(true).toBe(true);
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/hooks.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    let function = function_item(&ctx, module, 0)?;
    ensure!(function.is_test);
    let body = function_body(&ctx, function)?;
    ensure!(
        body.stmts.len() >= 3,
        "expected beforeEach, test body, and afterEach statements"
    );
    Ok(())
}

#[test]
fn vitest_nested_describe_inherits_lifecycle_hooks() -> Result<(), String> {
    let source = ts!(r#"
import { describe, test, expect, beforeEach, afterEach } from "vitest";

describe("outer", () => {
  beforeEach(() => {
    expect(true).toBe(true);
  });
  describe("inner", () => {
    afterEach(() => {
      expect(true).toBe(true);
    });
    test("case", () => {
      expect(true).toBe(true);
    });
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/nested-hooks.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    let function = function_item(&ctx, module, 0)?;
    ensure!(function.is_test);
    ensure_eq!(
        ctx.krate
            .symbols
            .get(function.name)
            .ok_or_else(|| "missing function symbol".to_owned())?,
        "test_outer_inner_case"
    );
    let body = function_body(&ctx, function)?;
    ensure!(
        body.stmts.len() >= 3,
        "expected inherited beforeEach, test body, and nested afterEach statements"
    );
    Ok(())
}

#[test]
fn node_assert_deep_strict_equal_identifier_lowers() -> Result<(), String> {
    let source = ts!(r#"
import { test } from "vitest";
import { deepStrictEqual } from "node:assert";

test("assert helper", () => {
  deepStrictEqual(1 + 1, 2);
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/assert.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::BinOp {
                op: BinOp::NotEq,
                ..
            }
        )),
        "expected deepStrictEqual identifier to lower to an assertion check",
    );
    Ok(())
}

#[test]
fn node_assert_default_member_calls_lower_as_statements() -> Result<(), String> {
    let source = ts!(r#"
import assert from "node:assert";

assert.strictEqual(1, 1);
assert.deepStrictEqual("same", "same");
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/assert-basic.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. })),
        "expected node assert calls to lower into assertion checks",
    );
    Ok(())
}

#[test]
fn vitest_test_each_expands_literal_rows() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

test.each([[1, 2, 3], [2, 3, 5]])("adds", (a, b, expected) => {
  expect(a + b).toBe(expected);
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/table.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    for index in 0..module.items.len() {
        let function = function_item(&ctx, module, index)?;
        ensure!(function.is_test);
    }
    Ok(())
}
