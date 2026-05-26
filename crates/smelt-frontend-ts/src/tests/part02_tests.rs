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
fn vitest_describe_each_supports_nested_describe_and_hooks() -> Result<(), String> {
    let source = ts!(r#"
import { describe, test, expect, beforeAll, afterEach } from "vitest";

describe.each([[1], [2]])("outer", (value) => {
  const expected = value;
  beforeAll(() => {
    expect(expected).toBe(value);
  });
  describe("inner", () => {
    afterEach(() => {
      expect(expected).toBe(value);
    });
    test("case", () => {
      expect(value).toBe(expected);
    });
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/nested-describe-each.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_describe_expression_setup_is_replayed_into_nested_tests() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, expect } from "vitest";

let clock = 0;
function fakeDate(value: number): void {
  clock = value;
}

describe("outer", () => {
  fakeDate(42);

  describe("inner", () => {
    it("sees setup", () => {
      expect(clock).toBe(42);
    });
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/setup-expression.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_it_accepts_function_expression_callbacks() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, expect } from "vitest";

describe("suite", () => {
  it("case", function () {
    expect(1).toBe(1);
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/function-callback.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_dynamic_function_table_lookup() -> Result<(), String> {
    let source = ts!(r#"
type Formatter = (value: string) => string;

const lower: Formatter = (value) => value.toLowerCase();
const upper: Formatter = (value) => value.toUpperCase();

export const table: Record<string, Formatter> = {
  a: lower,
  b: upper,
};

export function apply(values: string[]): string[] {
  return values.map((value) => {
    const key = value[0];
    const formatter = table[key];
    return formatter(value);
  });
}
"#);
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_callback_structural_object_literal_branch() -> Result<(), String> {
    let source = ts!(r#"
interface Part {
  isToken: boolean;
  value: string;
}

export function normalize(parts: Part[]): Part[] {
  return parts.map((part) =>
    part.isToken && part.value === "do"
      ? { isToken: true, value: "d" }
      : part,
  );
}
"#);
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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
fn lowers_math_member_reference_as_callback_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const roundingFn: (value: number) => number = Math.ceil;"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Closure(_))),
        "expected Math.ceil member reference to lower as a closure"
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
function truthy<T>(value: T): boolean {
  return Boolean(value);
}
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
fn lowers_string_conversion_from_numeric_literal_union() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Day = 0 | 1 | 2 | 3 | 4 | 5 | 6;

function label(day: Day): string {
  return String(day);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
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
fn lowers_number_parse_int_optional_string_operand() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const parts = "1e2".split("e");
const [, exponent] = parts;
const shifted = exponent === undefined ? 0 : Number.parseInt(exponent, 10);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ToInt,
            ..
        }
    )));
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
fn lowers_to_iso_string_on_generic_date_like_receiver() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function parseJSON<ResultDate extends Date = Date>(value: string): ResultDate {
  return new Date(value) as ResultDate;
}

const parsedDate = parseJSON("2000-03-15T05:20:10.123Z");
const iso = parsedDate.toISOString();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

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
fn lowers_unary_plus_bool_to_float() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const value = +true;"), &mut ctx)?;
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
fn lowers_postfix_update_expression_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values = [10, 20];
let index = 0;
const value = values[index++];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Assign { .. })),
        "expected postfix update to emit assignment side effect",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_postfix_update_expression_side_effect_inside_loop_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values = [10, 20];
let index = 0;
while (index < values.length) {
  const value = values[index++];
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let Some(Stmt::While {
        body: loop_body, ..
    }) = body.stmts.iter().find(|stmt| matches!(stmt, Stmt::While { .. }))
    else {
        return Err("expected while statement".to_owned());
    };
    let loop_block = body
        .blocks
        .get(usize::try_from(loop_body.0).map_err(|err| err.to_string())?)
        .ok_or_else(|| "expected loop body block".to_owned())?;

    let value_position = loop_block
        .stmts
        .iter()
        .position(|stmt| {
            usize::try_from(stmt.0)
                .is_ok_and(|index| matches!(body.stmts.get(index), Some(Stmt::Let { .. })))
        })
        .ok_or_else(|| "expected indexed value binding in loop body".to_owned())?;
    let update_position = loop_block
        .stmts
        .iter()
        .position(|stmt| {
            usize::try_from(stmt.0)
                .is_ok_and(|index| matches!(body.stmts.get(index), Some(Stmt::Assign { .. })))
        })
        .ok_or_else(|| "expected postfix update side effect in loop body".to_owned())?;
    ensure!(
        update_position > value_position,
        "expected postfix side effect after initializer binding",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_non_null_string_match_array_callbacks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const parts = "abc"
  .match(/a/)!
  .map((substring) => substring)
  .join("")
  .match(/b/)!
  .map((substring) => substring);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListCallback { .. })),
        "expected match(...)! to preserve array type for callback methods",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_block_bodied_array_callback_control_flow() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const formatters: Record<string, boolean> = { a: true };
const re = /x/;
const parts = "ab"
  .match(/./)!
  .map((substring) => {
    if (substring === "a") {
      return { isToken: false, value: "'" };
    }
    const firstCharacter = substring[0];
    if (formatters[firstCharacter]) {
      return { isToken: true, value: substring };
    }
    if (firstCharacter.match(re)) {
      throw new RangeError("bad " + firstCharacter);
    }
    return { isToken: false, value: substring };
  });
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListCallback { .. })),
        "expected block-bodied callback to lower into list callback",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nested_array_callback_inside_map_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Setter {
  priority: number;
  subPriority: number;
}

const setters: Setter[] = [];
const uniquePrioritySetters = setters
  .map((setter) => setter.priority)
  .sort((a, b) => b - a)
  .filter((priority, index, array) => array.indexOf(priority) === index)
  .map((priority) =>
    setters
      .filter((setter) => setter.priority === priority)
      .sort((a, b) => b.subPriority - a.subPriority),
  )
  .map((setterArray) => setterArray[0]);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListCallback { .. })),
        "expected nested date-fns parse callbacks to lower",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_typeof_expression_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const kind = typeof "value";
const numeric = typeof 1;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(body.exprs.iter().any(
        |expr| matches!(&expr.kind, ExprKind::Literal(Literal::String(value)) if value == "string")
    ));
    ensure!(body.exprs.iter().any(
        |expr| matches!(&expr.kind, ExprKind::Literal(Literal::String(value)) if value == "number")
    ));
    Ok(())
}

#[test]
fn lowers_typeof_bigint_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function isBig(value: unknown): boolean {
  return typeof value === "bigint";
}

const zero = 0n;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(0.0))))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_coercive_nullish_equality_idiom() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function emptyish(data: object | string | undefined): boolean {
  return data == undefined || data != null;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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
fn lowers_defaulted_object_destructuring_as_non_optional_binding() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Duration {
  years?: number;
  months?: number;
}

export function monthsInDuration(duration: Duration): number {
  const { years = 0, months = 0 } = duration;
  return months + years * 12;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some("months_in_duration") =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing lowered function".to_owned())?;
    let body = function
        .body
        .and_then(|body| ctx.krate.bodies.get(body.0 as usize))
        .ok_or_else(|| "missing lowered function body".to_owned())?;
    let float_ty = ctx.krate.types.intern(Type::Float);
    let years = body
        .locals
        .iter()
        .find(|local| local.name.and_then(|name| ctx.krate.symbols.get(name)) == Some("years"))
        .ok_or_else(|| "missing years binding".to_owned())?;
    let months = body
        .locals
        .iter()
        .find(|local| local.name.and_then(|name| ctx.krate.symbols.get(name)) == Some("months"))
        .ok_or_else(|| "missing months binding".to_owned())?;
    ensure_eq!(years.ty, float_ty);
    ensure_eq!(months.ty, float_ty);
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::OptionalCoalesce { .. })),
        "expected defaulted destructuring to lower through optional coalescing"
    );
    Ok(())
}

#[test]
fn lowers_defaulted_unknown_object_destructuring_to_fallback_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Duration {
  years?: unknown;
  months?: unknown;
}

export function monthsInDuration(duration: Duration): number {
  const { years = 0, months = 0 } = duration;
  return months + years * 12;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some("months_in_duration") =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing lowered function".to_owned())?;
    let body = function
        .body
        .and_then(|body| ctx.krate.bodies.get(body.0 as usize))
        .ok_or_else(|| "missing lowered function body".to_owned())?;
    for name in ["years", "months"] {
        let local = body
            .locals
            .iter()
            .find(|local| local.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some(name))
            .ok_or_else(|| format!("missing {name} binding"))?;
        ensure!(
            matches!(ctx.krate.types.get(local.ty), Some(Type::Float)),
            "expected {name} to lower as float, got {:?}",
            ctx.krate.types.get(local.ty)
        );
    }
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
date.setTime(timestamp + 1);
date.setFullYear(year, month, day + 1);
date.setMonth(0, 1);
date.setDate(2);
date.setHours(hours, minutes, seconds, milliseconds);
date.setMinutes(minutes, seconds, milliseconds);
date.setSeconds(seconds, milliseconds);
date.setMilliseconds(milliseconds);
interface DateValues {
  year?: number;
  date?: number;
  hours?: number;
}
const values: DateValues = { year: 2020, date: 3, hours: 4 };
if (values.year != null) date.setFullYear(values.year);
if (values.date != null) date.setDate(values.date);
if (values.hours != null) date.setHours(values.hours);
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
        10
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
const origin = new URL("https://example.com/path?q=1").origin;
const href = new URL("https://example.com/path?q=1").toString();
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
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::UrlField { .. }))
            .count(),
        3
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
fn lowers_error_instanceof_for_generic_union_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function isError<T>(data: Error | T): boolean {
  return data instanceof Error;
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
            .any(|expr| matches!(expr.kind, ExprKind::InstanceOf { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_promise_instanceof_for_generic_union_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function isPromise<T>(data: PromiseLike<unknown> | T): boolean {
  return data instanceof Promise;
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
            .any(|expr| matches!(expr.kind, ExprKind::InstanceOf { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateFromValue { .. }))
    );
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
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::OptionalCoalesce { .. })),
        "expected optional numeric fallback to preserve the selected runtime value"
    );
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
fn lowers_date_fns_fp_exported_convert_to_fp_wrapper() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function difference(dateLeft: number, dateRight: number): number {
  return dateLeft - dateRight;
}

export const differenceFp = convertToFP(difference, 2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let wrapper = function_item(&ctx, module, 1)?;
    ensure_eq!(ctx.krate.symbols.get(wrapper.name), Some("difference_fp"));
    ensure_eq!(wrapper.params.len(), 2);
    ensure_eq!(
        ctx.krate.symbols.get(wrapper.params[0].name),
        Some("date_right")
    );
    ensure_eq!(
        ctx.krate.symbols.get(wrapper.params[1].name),
        Some("date_left")
    );

    let body = function_body(&ctx, wrapper)?;
    let return_call = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Return(Some(expr)) => body.exprs.get(expr.0 as usize),
            _ => None,
        })
        .ok_or_else(|| "expected FP wrapper to return a call".to_owned())?;
    let ExprKind::Call { args, .. } = &return_call.kind else {
        return Err(format!("expected wrapper return call, got {return_call:?}"));
    };
    ensure_eq!(args.len(), 2);
    ensure!(matches!(
        body.exprs.get(args[0].0 as usize).map(|expr| &expr.kind),
        Some(ExprKind::Local(local)) if *local == wrapper.params[1].local
    ));
    ensure!(matches!(
        body.exprs.get(args[1].0 as usize).map(|expr| &expr.kind),
        Some(ExprKind::Local(local)) if *local == wrapper.params[0].local
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_fp_wrapper_with_erased_optional_tail_param() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function addDays(date: number, amount: number): number {
  return date + amount;
}

export const addDaysWithOptions = convertToFP(addDays, 3);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let target = function_item(&ctx, module, 0)?;
    let wrapper = function_item(&ctx, module, 1)?;
    ensure_eq!(target.params.len(), 3);
    ensure_eq!(wrapper.params.len(), 3);
    ensure_eq!(
        ctx.krate.symbols.get(wrapper.params[0].name),
        Some("__fp_arg2")
    );
    ensure_eq!(
        ctx.krate.symbols.get(wrapper.params[1].name),
        Some("amount")
    );
    ensure_eq!(ctx.krate.symbols.get(wrapper.params[2].name), Some("date"));

    let body = function_body(&ctx, wrapper)?;
    let return_call = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Return(Some(expr)) => body.exprs.get(expr.0 as usize),
            _ => None,
        })
        .ok_or_else(|| "expected FP wrapper to return a call".to_owned())?;
    let ExprKind::Call { args, .. } = &return_call.kind else {
        return Err(format!("expected wrapper return call, got {return_call:?}"));
    };
    ensure_eq!(args.len(), 3);
    ensure!(matches!(
        body.exprs.get(args[2].0 as usize).map(|expr| &expr.kind),
        Some(ExprKind::Local(local)) if *local == wrapper.params[0].local
    ));
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
fn lowers_date_fns_locale_string_replace_on_union_object_branch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type FormatDistanceTokenValue =
  | string
  | {
      one: string;
      other: string;
    };

function format(tokenValue: FormatDistanceTokenValue, count: number): string {
  let result;

  if (typeof tokenValue === "string") {
    result = tokenValue;
  } else if (count === 1) {
    result = tokenValue.one;
  } else {
    result = tokenValue.other.replace("{{count}}", count.toString());
  }

  return result;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_locale_mapped_lookup_string_replace() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type FormatDistanceToken = "xSeconds" | "xMinutes";
type FormatDistanceLocale<Template> = {
  [Token in FormatDistanceToken]: Template;
};
type FormatDistanceTokenValue =
  | string
  | {
      one: string;
      other: string;
    };

const formatDistanceLocale: FormatDistanceLocale<FormatDistanceTokenValue> = {
  xSeconds: {
    one: "1 second",
    other: "{{count}} seconds",
  },
  xMinutes: {
    one: "1 minute",
    other: "{{count}} minutes",
  },
};

function format(token: FormatDistanceToken, count: number): string {
  let result;

  const tokenValue = formatDistanceLocale[token];
  if (typeof tokenValue === "string") {
    result = tokenValue;
  } else if (count === 1) {
    result = tokenValue.one;
  } else {
    result = tokenValue.other.replace("{{count}}", count.toString());
  }

  return result;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_returned_arrow_with_contextual_default_parameter_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type FormatLongWidth = "full" | "long" | "medium" | "short" | "any";
interface FormatLongFnOptions {
  width?: FormatLongWidth;
}
type FormatLongFn = (options: FormatLongFnOptions) => string;

function buildFormatLongFn(defaultWidth: FormatLongWidth): FormatLongFn {
  return (options = {}) => {
    const width = options.width ? String(options.width) as FormatLongWidth : defaultWidth;
    return width;
  };
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_exported_object_const_with_helper_call_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type FormatLongWidth = "full" | "long" | "medium" | "short";
interface FormatLongFnOptions {
  width?: FormatLongWidth;
}
type FormatLongFn = (options: FormatLongFnOptions) => string;
interface FormatLong {
  date: FormatLongFn;
  time: FormatLongFn;
  dateTime: FormatLongFn;
}

const dateFormats = {
  full: "EEEE, MMMM do, y",
  long: "MMMM do, y",
  medium: "MMM d, y",
  short: "MM/dd/yyyy",
};

function buildFormatLongFn(defaultWidth: FormatLongWidth): FormatLongFn {
  return (options = {}) => {
    const width = options.width ? String(options.width) as FormatLongWidth : defaultWidth;
    return width;
  };
}

export const formatLong: FormatLong = {
  date: buildFormatLongFn("full"),
  time: buildFormatLongFn("full"),
  dateTime: buildFormatLongFn("full"),
};
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(
        module.items.iter().any(|item| {
            matches!(
                ctx.krate.items.get(item.0 as usize),
                Some(Item::Const(const_item))
                    if ctx.krate.names.get(const_item.name) == Some("formatLong")
            )
        }),
        "expected exported object const with helper call fields to lower"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_exported_object_namespace_method_members() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export const lightFormatters = {
  y(date: Date, token: string): string {
    return token + String(date.getFullYear());
  },
};

function read(date: Date): string {
  return lightFormatters.y(date, "y");
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_valued_object_properties_with_contextual_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Formatter = (date: Date, token: string) => string;

export const formatters: { [token: string]: Formatter } = {
  y: function (date, token) {
    return token + String(date.getFullYear());
  },
};
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn object_function_table_preserves_case_distinct_keys() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type Formatter = (date: Date, token: string) => string;

export const formatters: { [token: string]: Formatter } = {
  M: function (date, token) {
    return "month";
  },
  m: function (date, token) {
    return "minute";
  },
};
"#),
        &mut ctx,
    )?;
    let namespace = ctx
        .object_namespaces
        .get("formatters")
        .ok_or_else(|| "expected formatters namespace metadata".to_owned())?;
    let month = namespace
        .get("M")
        .and_then(|item| ctx.krate.items.get(item.0 as usize))
        .and_then(|item| match item {
            Item::Function(function) => ctx.krate.symbols.get(function.name),
            _ => None,
        })
        .ok_or_else(|| "expected M formatter function".to_owned())?;
    let minute = namespace
        .get("m")
        .and_then(|item| ctx.krate.items.get(item.0 as usize))
        .and_then(|item| match item {
            Item::Function(function) => ctx.krate.symbols.get(function.name),
            _ => None,
        })
        .ok_or_else(|| "expected m formatter function".to_owned())?;
    ensure!(month != minute);
    ensure_eq!(month, "formatters_M");
    ensure_eq!(minute, "formatters_m");
    Ok(())
}

#[test]
fn lowers_arrow_valued_record_properties_with_contextual_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Formatter = (date: Date, token: string) => string;

export const formatters: { [token: string]: Formatter } = {
  y: (date, token) => token + String(date.getFullYear()),
};
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unary_plus_inside_function_valued_object_property() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Formatter = (date: Date, token: string) => number;

export const formatters: { [token: string]: Formatter } = {
  t: function (date, token) {
    return +date;
  },
};
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_function_expression_call_argument_with_contextual_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function accepts(predicate: (pattern: string) => boolean): boolean {
  return predicate("abc");
}

function check(): boolean {
  return accepts(function (pattern) {
    return pattern.length > 0;
  });
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_regex_literal_call_arguments() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function parts(pattern: string): string[] | null {
  return pattern.match(/(P+)(p+)?/);
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn infers_function_declaration_return_type_from_final_return() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function getRoundingMethod(method: "ceil" | "floor" | "round" | "trunc" | undefined) {
  return (number: number) => {
    const result = number;
    return result === 0 ? 0 : result;
  };
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;

    ensure!(
        !matches!(ctx.krate.types.get(function.return_ty), Some(Type::None)),
        "expected final return expression to provide the function return type"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_dynamic_math_rounding_member_reference() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function roundWith(method: "ceil" | "floor" | "round" | "trunc", value: number): number {
  const round = Math[method];
  return round(value);
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
            .any(|expr| matches!(expr.kind, ExprKind::Closure(_))),
        "expected Math[method] to lower as a captured closure"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_string_logical_or_as_value_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocaleWidth = "full" | "long" | "medium" | "short";
interface Args {
  defaultWidth: LocaleWidth;
  defaultFormattingWidth?: LocaleWidth;
}

function width(args: Args): string {
  const defaultWidth = args.defaultFormattingWidth || args.defaultWidth;
  return defaultWidth;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_callable_logical_or_as_selected_runtime_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function select(
  value: unknown,
  context?: (value: unknown) => unknown,
): unknown {
  return context || value;
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
            .any(|expr| matches!(expr.kind, ExprKind::OptionalCoalesce { .. })),
        "expected optional callback fallback to preserve the selected runtime value"
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::BinOp { op: BinOp::Or, .. })),
        "optional callback fallback must not collapse to a boolean expression"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nullish_coalescing_with_structural_object_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
interface Locale {
  code: string;
}
interface Options {
  locale?: Locale;
}

const defaultLocale = { code: "en-US" };

function locale(options?: Options): Locale {
  return options?.locale ?? defaultLocale;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 2)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::TypeAssert { .. })),
        "expected concrete object fallback to be asserted to the optional object surface"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nullish_coalescing_when_fallback_matches_union_member() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Delay = number | (() => void) | undefined;

function delay(value: Delay): number | (() => void) {
  return value ?? 0;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::OptionalCoalesce { .. })),
        "expected union-member fallback to lower as nullish coalescing"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_numeric_logical_or_as_value_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function localDay(day: number, weekStartsOn: number): string {
  const localDayOfWeek = (day - weekStartsOn + 8) % 7 || 7;
  return String(localDayOfWeek);
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
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_build_localize_width_fallback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocaleWidth = "narrow" | "abbreviated" | "wide";
type LocaleUnitValue = string;
type LocalizeFn<Value extends LocaleUnitValue> = (
  value: Value,
  options?: { context?: string; width?: LocaleWidth },
) => string;
type LocalizeFnArgCallback<Value extends LocaleUnitValue | number> = (
  value: Value,
) => number;
type LocalizePeriodValuesMap<Value extends LocaleUnitValue> = {
  [Pattern in LocaleWidth]?: string[];
};
type BuildLocalizeFnArgs<
  Value extends LocaleUnitValue,
  ArgCallback extends LocalizeFnArgCallback<Value> | undefined,
> = {
  values: LocalizePeriodValuesMap<Value>;
  defaultWidth: LocaleWidth;
  formattingValues?: LocalizePeriodValuesMap<Value>;
  defaultFormattingWidth?: LocaleWidth;
} & (ArgCallback extends undefined
  ? { argumentCallback?: undefined }
  : { argumentCallback: LocalizeFnArgCallback<Value> });

function buildLocalizeFn<
  Value extends LocaleUnitValue,
  ArgCallback extends LocalizeFnArgCallback<Value> | undefined,
>(args: BuildLocalizeFnArgs<Value, ArgCallback>): LocalizeFn<Value> {
  return (value, options) => {
    if (options?.context && args.formattingValues) {
      const defaultWidth = args.defaultFormattingWidth || args.defaultWidth;
      const width = options?.width ? String(options.width) : defaultWidth;
      return width;
    }
    return args.defaultWidth;
  };
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_nullishable_callback_union_in_condition() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Callback = (value: string) => string;
type Args<ArgCallback extends Callback | undefined> =
  ArgCallback extends undefined
    ? { argumentCallback?: undefined }
    : { argumentCallback: Callback };

function call<ArgCallback extends Callback | undefined>(
  args: Args<ArgCallback>,
  value: string,
): string {
  return args.argumentCallback ? args.argumentCallback(value) : value;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_captured_callback_in_array_callback_condition() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Mapper = ((value: number) => number) | undefined;

function applyMaybe(mapper: Mapper, value: number): number {
  const values = [value].map((item) => mapper ? mapper(item) : item);
  return values[0];
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_chain_condition_in_array_callback_conditional() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Options = { width?: string };

function widths(options: Options | undefined): string[] {
  return ["wide"].map((fallback) =>
    options?.width ? String(options.width) : fallback
  );
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_build_localize_argument_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocaleWidth = "narrow" | "wide";
type Era = 0 | 1;
type Quarter = 1 | 2 | 3 | 4;
type Month = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11;
type Day = 0 | 1 | 2 | 3 | 4 | 5 | 6;
type LocaleDayPeriod =
  | "am"
  | "pm"
  | "midnight"
  | "noon"
  | "morning"
  | "afternoon"
  | "evening"
  | "night";
type LocaleUnitValue = Era | Quarter | Month | Day | LocaleDayPeriod;
type LocalizeValues<Value extends LocaleUnitValue> =
  Value extends LocaleDayPeriod
    ? Record<LocaleDayPeriod, string>
    : Value extends Era
      ? readonly [string, string]
      : Value extends Quarter
        ? readonly [string, string, string, string]
        : Value extends Day
          ? readonly [string, string, string, string, string, string, string]
          : Value extends Month
            ? readonly [
                string,
                string,
                string,
                string,
                string,
                string,
                string,
                string,
                string,
                string,
                string,
                string,
              ]
            : never;
type LocalizeUnitIndex<Value extends LocaleUnitValue | number> =
  Value extends LocaleUnitValue ? keyof LocalizeValues<Value> : number;
type LocalizeFnArgCallback<Value extends LocaleUnitValue | number> = (
  value: Value,
) => LocalizeUnitIndex<Value>;
type BuildLocalizeFnArgs<
  Value extends LocaleUnitValue,
  ArgCallback extends LocalizeFnArgCallback<Value> | undefined,
> = {
  values: { [Pattern in LocaleWidth]?: LocalizeValues<Value> };
  defaultWidth: LocaleWidth;
} & (ArgCallback extends undefined
  ? { argumentCallback?: undefined }
  : { argumentCallback: LocalizeFnArgCallback<Value> });

function buildLocalizeFn<
  Value extends LocaleUnitValue,
  ArgCallback extends LocalizeFnArgCallback<Value> | undefined,
>(args: BuildLocalizeFnArgs<Value, ArgCallback>, value: Value): LocalizeUnitIndex<Value> {
  return (
    args.argumentCallback ? args.argumentCallback(value as Value) : value
  ) as LocalizeUnitIndex<Value>;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_module_arrow_const_used_in_exported_object() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type LocalizeFn<Value> = (value: Value, options?: { width?: string }) => string;
type Localize = {
  ordinalNumber: LocalizeFn<number>;
  era: string;
};

function buildLocalizeFn(args: {
  values: Record<string, readonly string[]>;
  defaultWidth: string;
  argumentCallback?: (value: number) => number;
}): string {
  return args.defaultWidth;
}

const eraValues = {
  narrow: ["B", "A"] as const,
  wide: ["Before Christ", "Anno Domini"] as const,
};

const ordinalNumber: LocalizeFn<number> = (dirtyNumber, _options) => {
  const number = Number(dirtyNumber);
  return String(number);
};

export const localize: Localize = {
  ordinalNumber,
  era: buildLocalizeFn({
    values: eraValues,
    defaultWidth: "wide",
    argumentCallback: (quarter) => quarter - 1,
  }),
};
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_fns_find_key_for_in_loop() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function findKey<Value, Obj extends { [key in string | number]: Value }>(
  object: Obj,
  predicate: (value: Value) => boolean,
): string | undefined {
  for (const key in object) {
    if (
      Object.prototype.hasOwnProperty.call(object, key) &&
      predicate(object[key])
    ) {
      return key;
    }
  }
  return undefined;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body.exprs.iter().any(|expr| matches!(
                expr.kind,
                ExprKind::DictProjection {
                    op: DictProjectionOp::Keys,
                    ..
                }
            )))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_match_as_optional_match_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function firstMatch(text: string, pattern: string): string | undefined {
  const matchResult = text.match(pattern);
  if (!matchResult) {
    return undefined;
  }
  return matchResult[0];
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::RegexFind { .. }))
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_arrow_function_call_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function accepts(predicate: (pattern: string) => boolean): boolean {
  return predicate("abc");
}

function check(matchedString: string): boolean {
  return accepts((pattern) => pattern.test(matchedString));
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Closure(_)))
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_assignment_conditional_with_target_type_hint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function select<Value>(value: Value, callback: ((value: Value) => Value) | undefined): Value {
  value = callback ? ("fallback" as any) : value;
  return value;
}
"#),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. }))
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn narrows_optional_after_negated_truthy_guard_exits() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function rest(matchResult: string[] | undefined, text: string): string | undefined {
  if (!matchResult) {
    return undefined;
  }
  const matchedString = matchResult[0];
  if (!matchedString) {
    return undefined;
  }
  return text.slice(matchedString.length);
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
