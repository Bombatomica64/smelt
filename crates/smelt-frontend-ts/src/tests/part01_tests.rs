use super::*;

#[test]
fn vitest_public_api_imports_lower_as_test_builtins() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, test, beforeAll, afterAll, beforeEach, afterEach } from "vitest";

describe("group", () => {});
it("case", () => {});
test("case 2", () => {});
beforeAll(() => {});
afterAll(() => {});
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
export type NTuple<T, Result extends unknown[] = []> = [...Result, T];
export type NonEmptyArray<T> = [T, ...T[]];
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
        [param] if matches!(ctx.krate.types.get(*param), Some(Type::List(item)) if matches!(ctx.krate.types.get(*item), Some(Type::Never)))
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

    let generic_tuple = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::TypeAlias(alias) if ctx.krate.symbols.get(alias.name) == Some("NTuple") => {
                Some(alias)
            }
            _ => None,
        })
        .ok_or_else(|| "missing NTuple alias".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(generic_tuple.ty),
        Some(Type::Tuple(items)) if items.len() == 1
    ));

    let non_empty_array = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::TypeAlias(alias)
                if ctx.krate.symbols.get(alias.name) == Some("NonEmptyArray") =>
            {
                Some(alias)
            }
            _ => None,
        })
        .ok_or_else(|| "missing NonEmptyArray alias".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(non_empty_array.ty),
        Some(Type::List(_))
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
fn allows_never_inside_uninhabited_union_assertion_branch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocaleWidth = "narrow" | "wide";
type LocaleValues<Value extends string> = Value extends "known" ? string[] : never;

function values<Value extends string>(input: string[]): string[] {
  return input as LocaleValues<Value>;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
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

/// Return the const item with the given source name from a lowered module.
fn named_const_item<'a>(
    ctx: &'a HirCtx,
    module: &smelt_hir::Module,
    name: &str,
) -> Result<&'a smelt_hir::ConstItem, String> {
    module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize) {
            Some(Item::Const(const_item)) if ctx.krate.names.get(const_item.name) == Some(name) => {
                Some(const_item)
            }
            _ => None,
        })
        .ok_or_else(|| format!("expected const item `{name}` in lowered module"))
}

/// An exported const initialized from another module-level const identifier
/// (es-toolkit's `WORD_SEPARATORS` shape) inlines the referenced const array
/// through the general expression path instead of erroring as an unresolved
/// literal.
#[test]
fn exported_const_from_module_const_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const separators = ["-", "_", " "];
export const WORD_SEPARATORS: ReadonlyArray<string> = separators;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let const_item = named_const_item(&ctx, module, "WORD_SEPARATORS")?;
    let body = ctx
        .krate
        .bodies
        .get(const_item.body.0 as usize)
        .ok_or("missing const item body")?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::ListLit(items) if items.len() == 3)),
        "expected the referenced const array to inline as a three-element list literal"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// An exported const initialized from a member of another module-level const
/// object resolves through the referenced const instead of being rejected by
/// the well-known Number/Math member folder.
#[test]
fn exported_const_from_module_const_member() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const limits = { lower: 1, upper: 640 };
export const UPPER_LIMIT = limits.upper;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let const_item = named_const_item(&ctx, module, "UPPER_LIMIT")?;
    let body = ctx
        .krate
        .bodies
        .get(const_item.body.0 as usize)
        .ok_or("missing const item body")?;
    ensure!(
        body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == 640.0)
        ),
        "expected the const object member to resolve to its literal value"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Well-known Number/Math numeric constants keep folding to literal consts
/// (they must not be routed through the module-reference expression path).
#[test]
fn exported_const_number_math_constants_still_fold() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export const MAX_INTEGER = Number.MAX_VALUE;
export const PI_VALUE = Math.PI;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    for (name, expected) in [("MAX_INTEGER", f64::MAX), ("PI_VALUE", std::f64::consts::PI)] {
        let const_item = named_const_item(&ctx, module, name)?;
        let body = ctx
            .krate
            .bodies
            .get(const_item.body.0 as usize)
            .ok_or("missing const item body")?;
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                expr.kind,
                ExprKind::Literal(Literal::Float(value)) if value == expected
            )),
            "expected `{name}` to fold to its numeric literal"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// An exported const-from-const array is visible to later modules that import
/// it, mirroring how exported foldable numeric constants travel across files.
#[test]
fn exported_const_from_const_is_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
const separators = ["-", "_", " "];
export const WORD_SEPARATORS: ReadonlyArray<string> = separators;
"#),
        "src/constants.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { WORD_SEPARATORS } from "./constants";

export function firstSeparator(): string {
  return WORD_SEPARATORS[0];
}
"#),
        "src/words.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::ListLit(items) if items.len() == 3)),
        "expected the imported exported const array to inline into the function body"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Member expressions on names Smelt has not lowered still fail with the
/// well-known-constants unsupported message rather than lowering incorrectly.
#[test]
fn exported_const_unresolved_member_still_errors() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
export const MYSTERY = missingNamespace.someMember;
"#),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| format!("{error:?}")
            .contains("exported const member expressions support well-known Number/Math")),
        "expected the well-known-constants unsupported error, got: {errors:?}"
    );
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
fn exported_literal_object_constants_are_visible_to_later_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export const SKIP_ITEM = { done: false, hasNext: false } as const;
"#),
        "src/utilityEvaluators.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { SKIP_ITEM } from "./utilityEvaluators";
const value = SKIP_ITEM;
"#),
        "src/main.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_))),
        "expected imported object const to lower as a Rust record literal"
    );
    Ok(())
}

#[test]
fn remeda_lazy_utility_evaluator_consts_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
type LazyResult<T> =
  | { done: true; hasNext: false }
  | { done: false; hasNext: false }
  | { done: false; hasNext: true; next: T };

const EMPTY_PIPE = { done: true, hasNext: false } as const;
export const SKIP_ITEM = { done: false, hasNext: false } as const;
export const lazyEmptyEvaluator = <T>(): LazyResult<T> => EMPTY_PIPE;
export const lazyIdentityEvaluator = <T>(value: T) =>
  ({
    hasNext: true,
    next: value,
    done: false,
  }) as const;
"#),
        "src/internal/utilityEvaluators.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 4);
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_))),
        "expected static and inferred object const paths to lower object literals"
    );
    Ok(())
}

#[test]
fn remeda_local_arrow_const_is_visible_before_declaration_order() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
type StrictFunction = (...args: never) => unknown;

function purry(fn: StrictFunction, args: readonly unknown[]): unknown {
  return 0;
}

export function add(...args: readonly unknown[]): unknown {
  return purry(addImplementation, args);
}

const addImplementation = (value: number, addend: number): number =>
  value + addend;
"#),
        "src/add.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(
        module.items.len() >= 3,
        "expected local arrow const to lower as a callable item"
    );
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(expr.kind, ExprKind::Closure(_))),
        "expected function item argument to lower through a first-class closure"
    );
    Ok(())
}

#[test]
fn remeda_overload_calls_select_public_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
export function add(value: number, addend: number): number;
export function add(addend: number): (value: number) => number;
export function add(...args: readonly unknown[]): unknown {
  return args;
}

const dataFirst = add(10, 5);
const dataLast = add(5)(10);
"#),
        "src/add.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "expected curried overload call to lower as a closure call"
    );
    ensure!(
        body.locals
            .iter()
            .any(|local| matches!(ctx.krate.types.get(local.ty), Some(Type::Float))),
        "expected data-first overload call to produce number"
    );
    Ok(())
}

#[test]
fn remeda_overload_signatures_are_visible_to_imports() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export function add(value: bigint, addend: bigint): bigint;
export function add(value: number, addend: number): number;
export function add(addend: number): (value: number) => number;
export function add(...args: readonly unknown[]): unknown {
  return args;
}
"#),
        "src/add.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { add } from "./add";
const dataFirst = add(10, 5);
const dataLast = add(5)(10);
"#),
        "src/add.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "expected imported curried overload call to lower"
    );
    Ok(())
}

#[test]
fn constrained_data_last_overload_does_not_capture_direct_string_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
type Options = { readonly preserve?: boolean };

export function convert<T extends string>(data: T): string;
export function convert<Config extends Options>(options?: Config): (data: string) => string;
export function convert(value?: Options | string): unknown {
  return value;
}
"#),
        "src/convert.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { convert } from "./convert";

const direct = convert("Ada");
const dataLast = convert({ preserve: true });
"#),
        "src/convert.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let local_ty = |name: &str| {
        body.locals
            .iter()
            .find(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some(name))
            .map(|local| local.ty)
            .ok_or_else(|| format!("missing `{name}` local"))
    };
    ensure!(
        matches!(ctx.krate.types.get(local_ty("direct")?), Some(Type::String)),
        "expected direct string argument to select the data-first overload"
    );
    ensure!(
        matches!(
            ctx.krate.types.get(local_ty("data_last")?),
            Some(Type::Function(_))
        ),
        "expected options object argument to select the data-last overload"
    );
    Ok(())
}

#[test]
fn dependent_data_last_constraint_waits_for_callback_context() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export function pick<T extends object, K extends KeysOf<T>>(
  key: K,
): (data: T) => unknown;
export function pick(value: unknown): unknown {
  return value;
}
"#),
        "src/pick.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { pick } from "./pick";
const getter = pick("active");
"#),
        "src/pick.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let getter = body
        .locals
        .iter()
        .find(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some("getter"))
        .ok_or_else(|| "missing `getter` local".to_owned())?;
    ensure!(
        matches!(ctx.krate.types.get(getter.ty), Some(Type::Function(_))),
        "expected a dependent data-last constraint to preserve its callable return"
    );
    Ok(())
}

#[test]
fn optional_overload_parameters_preserve_data_first_selection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export function flatten<T extends readonly unknown[]>(data: T, depth?: number): unknown[];
export function flatten(depth?: number): (data: readonly unknown[]) => unknown[];
export function flatten(value?: readonly unknown[] | number, depth?: number): unknown {
  return value;
}
"#),
        "src/flatten.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { flatten } from "./flatten";
const direct = flatten([1, [2]]);
const curried = flatten(2);
"#),
        "src/flatten.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let local_ty = |name: &str| {
        body.locals
            .iter()
            .find(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some(name))
            .map(|local| local.ty)
            .ok_or_else(|| format!("missing `{name}` local"))
    };
    ensure!(
        matches!(
            ctx.krate.types.get(local_ty("direct")?),
            Some(Type::List(_))
        ),
        "expected optional trailing depth to allow the direct overload"
    );
    ensure!(
        matches!(
            ctx.krate.types.get(local_ty("curried")?),
            Some(Type::Function(_))
        ),
        "expected numeric argument to retain the curried overload"
    );
    Ok(())
}

#[test]
fn union_overload_parameters_preserve_numeric_datalast_selection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
export function split(data: string, separator: RegExp, limit?: number): string[];
export function split(separator: RegExp, limit?: number): (data: string) => string[];
export function split<S extends string, Separator extends string, N extends number | undefined = undefined>(
  data: S,
  separator: Separator,
  limit?: N,
): string[];
export function split<S extends string, Separator extends string, N extends number | undefined = undefined>(
  separator: Separator,
  limit?: N,
): (data: S) => string[];
export function split(
  dataOrSeparator: RegExp | string,
  separatorOrLimit?: RegExp | number | string,
  limit?: number,
): unknown {
  return dataOrSeparator;
}
"#),
        "src/split.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { split } from "./split";
const direct = split("a,b", ",", 1);
const limited = split(",", 1);
"#),
        "src/split.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let local_ty = |name: &str| {
        body.locals
            .iter()
            .find(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some(name))
            .map(|local| local.ty)
            .ok_or_else(|| format!("missing `{name}` local"))
    };
    ensure!(
        matches!(
            ctx.krate.types.get(local_ty("direct")?),
            Some(Type::List(_))
        ),
        "expected three-argument split to select the data-first overload"
    );
    ensure!(
        matches!(
            ctx.krate.types.get(local_ty("limited")?),
            Some(Type::Function(_))
        ),
        "expected numeric second argument to select the data-last overload, got {:?}",
        ctx.krate.types.get(local_ty("limited")?)
    );
    Ok(())
}

#[test]
fn remeda_test_overload_fallback_rejects_impossible_argument_shapes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
type Case<In, Out, When extends (x: In) => boolean = (x: In) => boolean> =
  readonly [when: When, then: (x: In) => Out];
type DefaultCase<In, Out> = (x: In) => Out;

export function conditional<T, Fn0 extends (x: T) => boolean, Return0, Fallback = never>(
  case0: Case<T, Return0, Fn0>,
  fallback?: DefaultCase<T, Fallback>,
): (data: T) => Return0 | Fallback;
export function conditional<T, Fn0 extends (x: T) => boolean, Return0, Fallback = never>(
  data: T,
  case0: Case<T, Return0, Fn0>,
  fallback?: DefaultCase<T, Fallback>,
): Return0 | Fallback;
export function conditional(...args: readonly unknown[]): unknown {
  return args;
}
"#),
        "src/conditional.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r#"
import { conditional } from "./conditional";

function constant<T>(value: T): () => T {
  return () => value;
}

const dataFirst = conditional("Jokic", [constant(false), constant("hello")], constant(undefined));
const dataLast = conditional([constant(false), constant("hello")], constant(undefined));
"#),
        "src/conditional.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let local_ty = |name: &str| {
        body.locals
            .iter()
            .find(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some(name))
            .map(|local| local.ty)
            .ok_or_else(|| {
                let names = body
                    .locals
                    .iter()
                    .filter_map(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)))
                    .collect::<Vec<_>>();
                format!("missing `{name}` local; locals: {names:?}")
            })
    };
    ensure!(
        !matches!(
            ctx.krate.types.get(local_ty("data_first")?),
            Some(Type::Function(_))
        ),
        "expected data-first conditional call not to select the data-last closure overload"
    );
    ensure!(
        matches!(
            ctx.krate.types.get(local_ty("data_last")?),
            Some(Type::Function(_))
        ),
        "expected data-last conditional call to select the closure overload"
    );
    Ok(())
}

#[test]
fn remeda_pipe_overload_infers_curried_generic_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
type UpsertProp<T, K, V> = T & Record<K, V>;

export function addProp<T, K extends PropertyKey, V>(
  obj: T,
  prop: K,
  value: V,
): UpsertProp<T, K, V>;
export function addProp<T, K extends PropertyKey, V>(
  prop: K,
  value: V,
): (obj: T) => UpsertProp<T, K, V>;
export function addProp(...args: readonly unknown[]): unknown {
  return args;
}

export function pipe<A>(data: A): A;
export function pipe<A, B>(data: A, funcA: (input: A) => B): B;
export function pipe(...args: readonly unknown[]): unknown {
  return args[0];
}

const actual = pipe({ a: 1 }, addProp("b", 2));
"#),
        "src/addProp.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.locals
            .iter()
            .any(|local| local.name.and_then(|name| ctx.krate.names.get(name)) == Some("actual")),
        "expected Remeda-style pipe/addProp call to lower"
    );
    Ok(())
}

#[test]
fn remeda_predicate_array_arrows_lower_as_closure_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
export function allPass<T>(
  data: T,
  fns: readonly ((data: T) => boolean)[],
): boolean;
export function allPass<T>(
  fns: readonly ((data: T) => boolean)[],
): (data: T) => boolean;
export function allPass(...args: readonly unknown[]): unknown {
  return args[0];
}

const fns = [(x: number) => x % 3 === 0, (x: number) => x % 4 === 0] as const;
const dataFirst = allPass(12, fns);
const dataLast = allPass(fns)(12);
"#),
        "src/allPass.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.locals
            .iter()
            .any(|local| local.name.and_then(|name| ctx.krate.names.get(name)) == Some("fns")),
        "expected predicate array binding to lower"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "expected data-last allPass call to lower as a closure call"
    );
    Ok(())
}

#[test]
fn vitest_body_can_read_inferred_module_const_function_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
import { describe, expect, test } from "vitest";

function allPass<T>(data: T, fns: readonly ((data: T) => boolean)[]): boolean {
  return true;
}

const fns = [(x: number) => x % 3 === 0, (x: number) => x % 4 === 0] as const;

describe("data first", () => {
  test("allPass", () => {
    expect(allPass(12, fns)).toBe(true);
  });
});
"#),
        "src/allPass.test.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let test_body = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) if function.is_test => function
                .body
                .and_then(|body| ctx.krate.bodies.get(body.0 as usize)),
            _ => None,
        })
        .ok_or_else(|| {
            "expected Vitest body using module const function array to lower".to_owned()
        })?;
    ensure!(
        test_body
            .exprs
            .iter()
            .any(|expr| { matches!(&expr.kind, ExprKind::ListLit(items) if items.len() == 2) }),
        "expected top-level predicate array setup to be replayed into the generated test body"
    );
    Ok(())
}

#[test]
fn function_can_read_module_const_object_function_table() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
const COMPARATORS = {
  asc: (x: number, y: number) => x > y,
  desc: (x: number, y: number) => x < y,
} as const;

export function compare(direction: string, x: number, y: number): boolean {
  const { [direction]: comparator } = COMPARATORS;
  return comparator(x, y);
}
"#),
        "src/comparators.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let compare_body = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("compare") => {
                function
                    .body
                    .and_then(|body| ctx.krate.bodies.get(body.0 as usize))
            }
            _ => None,
        })
        .ok_or_else(|| "expected compare function to lower".to_owned())?;
    ensure!(
        compare_body
            .exprs
            .iter()
            .any(|expr| { matches!(&expr.kind, ExprKind::DictLit(entries) if entries.len() == 2) }),
        "expected module const function table read to recreate both object entries"
    );
    Ok(())
}

#[test]
fn spread_call_uses_optional_index_for_optional_fixed_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
function choose(primary: string, secondary?: string, ...rest: string[]): string {
  return secondary ?? primary;
}

export function run(values: string[]): string {
  return choose(...values);
}
"#),
        "src/spread-optional.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let run_body = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("run") => {
                function
                    .body
                    .and_then(|body| ctx.krate.bodies.get(body.0 as usize))
            }
            _ => None,
        })
        .ok_or_else(|| "expected run function to lower".to_owned())?;
    ensure!(
        run_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::OptionalIndex { .. })),
        "expected spread expansion to optional-index the optional fixed parameter"
    );
    Ok(())
}

#[test]
fn remeda_capitalize_generic_string_constraint_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
export function capitalize<T extends string>(data: T): Capitalize<T>;
export function capitalize(): <T extends string>(data: T) => Capitalize<T>;
export function capitalize(...args: readonly unknown[]): unknown {
  return args[0];
}

const capitalizeImplementation = <T extends string>(data: T): Capitalize<T> =>
  `${data[0]?.toUpperCase() ?? ""}${data.slice(1)}` as Capitalize<T>;

const templated = capitalize("prefix_123" as `prefix_${number}`);
"#),
        "src/capitalize.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.locals.iter().any(|local| {
            local.name.and_then(|name| ctx.krate.names.get(name))
                == Some("capitalizeImplementation")
        }),
        "expected generic string-constrained implementation const to lower"
    );
    Ok(())
}

#[test]
fn remeda_with_precision_block_bodied_arrow_expression_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
const MAX_PRECISION = 15;

export const withPrecision =
  (roundingFn: (value: number) => number) =>
  (value: number, precision: number): number => {
    if (precision === 0) {
      return roundingFn(value);
    }

    if (precision > MAX_PRECISION || precision < -MAX_PRECISION) {
      throw new RangeError("precision must be between -15 and 15");
    }

    const shiftedValue = value + precision;
    const rounded = roundingFn(shiftedValue);
    return rounded - precision;
  };
"#),
        "src/internal/withPrecision.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(
        module.items.iter().any(|item| {
            matches!(
                ctx.krate.items.get(item.0 as usize),
                Some(Item::Function(function))
                    if ctx.krate.names.get(function.name) == Some("withPrecision")
            )
        }),
        "expected block-bodied arrow expression export to lower"
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
fn vitest_setup_helpers_inline_registered_before_each_callbacks() -> Result<(), String> {
    let source = ts!(r#"
import { beforeEach, describe, test, vi } from "vitest";

function fakeClock(date: Date) {
  beforeEach(() => {
    vi.useFakeTimers({ now: date });
  });
}

describe("clock", () => {
  fakeClock(new Date(2020, 0, 1));
  test("reads the mocked clock", () => {
    Date.now();
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/setup-helper.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let helper_has_clock_setup = module
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, _)| function_item(&ctx, module, index).ok())
        .filter(|function| !function.is_test)
        .filter_map(|function| function_body(&ctx, function).ok())
        .any(|body| {
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::DateSetNow { .. }))
        });
    ensure!(helper_has_clock_setup);
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

#[test]
fn vitest_test_each_accepts_scalar_rows() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

test.each([Number.NaN, Number.POSITIVE_INFINITY])("value", (value) => {
  expect(value).toBe(value);
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/scalar-table.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_to_throw_lowers_arrow_callback_assertion() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

function fail(): void {
  throw "boom";
}

test("throws", () => {
  expect(() => fail()).toThrow("boom");
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/to-throw.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body_id = function
        .body
        .ok_or_else(|| "missing test body id".to_owned())?;
    let body = ctx
        .krate
        .bodies
        .get(body_id.0 as usize)
        .ok_or_else(|| "missing test body".to_owned())?;
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::TryCatch { .. })),
        "expected toThrow to lower through try/catch",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_to_throw_lowers_bound_function_callback_assertion() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

function noop(value?: number): void {}

test("does not throw", () => {
  const block = noop.bind(null);
  expect(block).not.toThrow();
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/to-throw-bound.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body_id = function
        .body
        .ok_or_else(|| "missing test body id".to_owned())?;
    let body = ctx
        .krate
        .bodies
        .get(body_id.0 as usize)
        .ok_or_else(|| "missing test body".to_owned())?;
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::TryCatch { .. })),
        "expected bound toThrow to lower through try/catch",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_to_throw_preserves_throwing_bound_function_type() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

function fail(value: string): never {
  throw new RangeError(value);
}

test("throws", () => {
  expect(fail.bind(null, "bad")).toThrow();
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/to-throw-bound-error.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body_id = function
        .body
        .ok_or_else(|| "missing test body id".to_owned())?;
    let body = ctx
        .krate
        .bodies
        .get(body_id.0 as usize)
        .ok_or_else(|| "missing test body".to_owned())?;
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::TryCatch { .. })),
        "expected throwing bound toThrow to lower through try/catch",
    );
    let throwing_assertion_callee = body
        .exprs
        .iter()
        .find_map(|expr| matches!(expr.kind, ExprKind::TypeAssert { .. }).then_some(expr.ty))
        .ok_or_else(|| "missing throwing assertion callable cast".to_owned())?;
    ensure!(
        matches!(
            ctx.krate.types.get(throwing_assertion_callee),
            Some(Type::Function(function)) if function.may_throw
        ),
        "toThrow call site must use a throwing callable ABI",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_expect_matchers_lower_inside_nested_test_blocks() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

test("loop", () => {
  for (const value of [1, 2]) {
    expect(value).toBe(value);
  }
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/nested-expect.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body_id = function
        .body
        .ok_or_else(|| "missing test body id".to_owned())?;
    let body = ctx
        .krate
        .bodies
        .get(body_id.0 as usize)
        .ok_or_else(|| "missing test body".to_owned())?;
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. })),
        "expected nested matcher to lower into an assertion branch",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_symbol_brand_and_type_query_metadata() -> Result<(), String> {
    let source = ts!(r#"
const Brand = Symbol("Brand");
export type Branded = typeof Brand;
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::Literal(Literal::Symbol(ref value)) if value.starts_with("Symbol(Brand)@")
        )),
        "expected Symbol(...) to lower to an opaque symbol literal",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unhinted_empty_array_as_unknown_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const values = [];"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            ctx.krate.types.get(expr.ty),
            Some(Type::List(item)) if matches!(ctx.krate.types.get(*item), Some(Type::Unknown))
        )),
        "expected unhinted empty array to lower as unknown[]",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_single_spread_array_literal_as_list_clone() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const data: number[] = [1, 2]; const cloned = [...data];"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(ctx.krate.types.get(expr.ty), Some(Type::List(_))))
            .count()
            >= 2,
        "expected spread clone to lower as a list expression",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_length_constructor_as_list_container() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const result = new Array<number[]>(3);"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::ListLit(ref items) if items.is_empty()
        )),
        "expected Array constructor to lower as an empty list container",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            ctx.krate.types.get(expr.ty),
            Some(Type::List(item))
                if matches!(ctx.krate.types.get(*item), Some(Type::List(_)))
        )),
        "expected Array constructor generic to become the list item type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_index_access_on_union_collections() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function read(value: number[] | [string]): number | string {
  return value[0];
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    ensure!(matches!(
        ctx.krate.types.get(function.return_ty),
        Some(Type::Union(items)) if items.len() == 2
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_destructuring_rest_binding() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const values: number[] = [1, 2, 3]; const [head, ...tail] = values;"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSlice { .. })),
        "expected array rest destructuring to lower as a list slice",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nested_array_destructuring_rest_binding() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(
            "const chunks: (string | number)[][] = [[\"abc\", 1, 2]]; const [[first, second, ...otherItems]] = chunks;"
        ),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSlice { .. })),
        "expected nested array rest destructuring to lower as a list slice",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unhinted_empty_array_call_argument_as_unknown_list() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function passthrough(values: unknown[]): unknown[] {
  return values;
}

const result = passthrough([]);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::ListLit(ref items) if items.is_empty())
                && matches!(
                    ctx.krate.types.get(expr.ty),
                    Some(Type::List(item)) if ctx.krate.types.get(*item) == Some(&Type::Unknown)
                )
        }),
        "expected unhinted [] to lower as unknown[]",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_destructured_arrow_function_parameters() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Limits = { min?: number; max?: number };
const clampImplementation = (value: number, { min, max }: Limits): number =>
  min !== undefined && value < min ? min : max !== undefined && value > max ? max : value;
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_constraint_and_structured_clone() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function cloneObject<T extends object>(value: T): T {
  return structuredClone(value);
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_push_into_unknown_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: unknown[] = [];
const copied: Record<PropertyKey, unknown> = {};
values.push(copied);
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_prototype_and_entries_on_generic_object() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function inspect<T extends object>(value: T): unknown {
  const prototype = Object.getPrototypeOf(value);
  const same = prototype === Object.prototype;
  const entries = Object.entries(value);
  return entries[0];
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_array_cycle_lookup() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function lookup<T>(value: T, refFrom: unknown[] = [], refTo: unknown[] = []): T {
  const idx = refFrom.indexOf(value);
  return refTo[idx] as T;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unknown_array_index_assignment() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function copy(index: number, item: unknown): unknown[] {
  const copiedValue: unknown[] = [];
  copiedValue[index] = item;
  return copiedValue;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_entries_for_destructured_for_of() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function copy(value: readonly unknown[]): unknown[] {
  const copiedValue: unknown[] = [];
  for (const [index, item] of value.entries()) {
    copiedValue[index] = item;
  }
  return copiedValue;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_any_like_unknown_access_in_test_modules() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
const value: any = { a: [{ b: 1 }] };
const first = value.a[0].b;
"#),
        "src/clone.test.ts",
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_object_and_regexp_array_elements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values = [{ a: (x: number): number => x + x }, /x/u];
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nullish_expect_matchers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
import { expect, test } from "vitest";

test("nullish", () => {
  expect(undefined).toBeUndefined();
  expect(null).toBeNull();
});
"#),
        "src/nullish.test.ts",
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_type_test_index_access_on_unknown_metadata() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expectTypeOf, test } from "vitest";

test("metadata", () => {
  const value = 1 as unknown;
  expectTypeOf(value[0]).toEqualTypeOf<unknown>();
});
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_declaration_type_test_index_access_on_unknown_metadata() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        "const value = 1 as unknown; const item = value[0];",
        "src/value.test-d.ts",
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_runtime_index_access_on_unknown_metadata() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function read(value: unknown): unknown { return value[0]; }"),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
