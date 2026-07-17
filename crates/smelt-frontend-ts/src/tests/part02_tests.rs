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
    // `clock` is a module-level `let` mutated inside `fakeDate`, so it lifts to
    // a mutable-global item alongside the function item and the test item. The
    // replayed setup reads and writes the same global (its declaration is not
    // re-declared as a shadowing local), so the test observes `fakeDate`'s
    // write.
    ensure_eq!(module.items.len(), 3);
    ensure!(
        ctx.krate
            .items
            .iter()
            .any(|item| matches!(item, Item::MutableGlobal(_))),
        "mutated module let should lift to a mutable global",
    );
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
    let source = ts!(r"
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
");
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
            .any(|expr| matches!(expr.kind, ExprKind::OptionalIndex { .. })),
        "string .at should lower to optional indexing"
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
fn distinguishes_array_at_from_negative_bracket_index() -> Result<(), String> {
    // `.at(-1)` wraps to count from the end and lowers to optional indexing,
    // whereas a negative bracket index `[-1]` is a JavaScript property lookup
    // that yields `undefined`. The two must stay distinct: bracket access must
    // not become `.at`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const last = values.at(-1);
"),
        &mut ctx,
    )?;
    let at_module = module(&ctx, module_id)?;
    let at_body = module_body(&ctx, at_module)?;

    ensure!(
        at_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::OptionalIndex { .. })),
        "array .at should lower to optional indexing"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());

    // A negative bracket index now lowers successfully to an optional `None`
    // (undefined) rather than being rejected, and it does not emit an element
    // `Index` access (i.e. it is not rewritten into `.at`).
    let mut bracket_ctx = HirCtx::new();
    let bracket_module_id = lower_ok(
        ts!(r"
const values: number[] = [1, 2, 3];
const missing = values[-1];
"),
        &mut bracket_ctx,
    )?;
    let bracket_module = module(&bracket_ctx, bracket_module_id)?;
    let bracket_body = module_body(&bracket_ctx, bracket_module)?;
    ensure!(
        bracket_body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::Literal(Literal::None))
                && matches!(bracket_ctx.krate.types.get(expr.ty), Some(Type::Optional(_)))
        }),
        "negative bracket index should lower to an optional None literal",
    );
    ensure!(
        !bracket_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. })),
        "negative bracket index must not emit an element Index access",
    );
    ensure!(smelt_hir::validate(&bracket_ctx.krate).is_empty());
    Ok(())
}

#[test]
fn coerces_optional_numeric_at_index() -> Result<(), String> {
    // `.at(index)` where `index` is `number | undefined` is statically
    // numeric-compatible: JavaScript runs `ToInteger` on the argument, treating
    // `undefined` as `0`. The frontend must coerce such an optional-numeric index
    // to the runtime `Float` the optional-indexing path expects (via a
    // `Number(...)` primitive cast) instead of rejecting it.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function pick(values: number[], index: number | undefined): number | undefined {
  return values.at(index);
}
"),
        &mut ctx,
    )?;
    // The `.at` call lives in the `pick` function body, so scan every body.
    let _ = module_id;
    let has_optional_index = ctx
        .krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::OptionalIndex { .. })
        }));
    ensure!(
        has_optional_index,
        "array .at with optional-numeric index should lower to optional indexing"
    );
    let has_number_cast = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    ..
                }
            )
        })
    });
    ensure!(
        has_number_cast,
        "optional-numeric .at index should be coerced with a Number(...) cast"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn coerces_numeric_type_param_at_index() -> Result<(), String> {
    // A generic index constrained to `number` is numeric-like through its type
    // parameter constraint, so `.at` must coerce rather than reject it.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function pickAt<T extends number>(values: string[], index: T): string | undefined {
  return values.at(index);
}
"),
        &mut ctx,
    )?;
    // Lowering succeeded (`lower_ok`); confirm the `.at` call became optional
    // indexing across the lowered function bodies.
    let _ = module_id;
    let has_optional_index = ctx
        .krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::OptionalIndex { .. })
        }));
    ensure!(
        has_optional_index,
        "array .at with a numeric type-param index should lower to optional indexing"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn coerces_erased_at_index() -> Result<(), String> {
    // An erased index — e.g. a value flowing through an `unknown`/opaque surface,
    // mirroring the cross-module `toInteger(n)` return in es-toolkit `nthArg` —
    // is coerced with a `Number(...)` cast rather than rejected, since `.at` runs
    // `ToInteger` on any argument at runtime.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function pick(values: string[], index: unknown): string | undefined {
  return values.at(index as any);
}
"),
        &mut ctx,
    )?;
    let _ = module_id;
    let has_optional_index = ctx
        .krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::OptionalIndex { .. })
        }));
    ensure!(
        has_optional_index,
        "array .at with an erased index should lower to optional indexing"
    );
    let has_number_cast = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    ..
                }
            )
        })
    });
    ensure!(
        has_number_cast,
        "erased .at index should be coerced with a Number(...) cast"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_non_numeric_at_index() -> Result<(), String> {
    // A genuinely non-numeric index (here a string) is not coercible and must
    // stay an explicit unsupported diagnostic rather than being silently coerced.
    let errors = lowering_errors(
        ts!(r"
function pick(values: number[], key: string): number | undefined {
  return values.at(key);
}
"),
        &mut HirCtx::new(),
    )?;
    assert_unsupported_ts(&errors, "array/string at index must be a number")
}

#[test]
fn lowers_math_abs_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const value = -5;
const positive = Math.abs(value);
"),
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
        PrimitiveCastOp::ToJsNumber,
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
        ts!(r"
type Day = 0 | 1 | 2 | 3 | 4 | 5 | 6;

function label(day: Day): string {
  return String(day);
}
"),
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
        ts!(r"
const value = 42;
const text = value.toString();
"),
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
            op: PrimitiveCastOp::ParseFloat,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_parse_float_erased_inputs_through_string_coercion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function parseValues(value: any): number[] {
  return [parseFloat(value), Number.parseFloat(value)];
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;
    let coerced_parse_count = body
        .exprs
        .iter()
        .filter(|expr| {
            let ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ParseFloat,
                operand,
            } = expr.kind
            else {
                return false;
            };
            usize::try_from(operand.0).ok().is_some_and(|index| {
                matches!(
                    body.exprs.get(index).map(|operand| &operand.kind),
                    Some(ExprKind::PrimitiveCast {
                        op: PrimitiveCastOp::ToString,
                        ..
                    })
                )
            })
        })
        .count();
    ensure_eq!(coerced_parse_count, 2);
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

    // `parseInt("42")` casts to int; `parseInt("42", 10)` honors the radix via
    // the dedicated `ParseIntRadix` op.
    let to_int_count = body
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
    let radix_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ParseIntRadix { .. }))
        .count();
    ensure_eq!(to_int_count, 1);
    ensure_eq!(radix_count, 1);
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

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ParseIntRadix { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_infinity_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const upper = Infinity;
const lower = -Infinity;
const missing = NaN;
"),
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
        ts!(r"
const now = Date.now();
const current = new Date();
const iso = new Date(now).toISOString();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::DateNow))
            .count()
            >= 2
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
fn lowers_conditional_numeric_and_optional_numeric_branches() -> Result<(), String> {
    // A ternary whose branches are a bare numeric literal and an optional-numeric
    // flow-typed local (mirroring es-toolkit `clamp`'s `isNaN(x) ? 0 : x` where
    // `x` stays `number | undefined`) must merge to `Optional<Float>` instead of
    // aborting because a `Float` and `Optional<Float>` have different shapes.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function pick(bound?: number): number | undefined {
  return bound === undefined ? 0 : bound;
}
"),
        &mut ctx,
    )?;
    let _ = module_id;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body
                .exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. }))),
        "missing conditional expression"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
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
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(1.0_f64)))),
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
            op: PrimitiveCastOp::ToJsNumber,
            ..
        }
    )));
    Ok(())
}

#[test]
fn lowers_postfix_update_expression_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values = [10, 20];
let index = 0;
const value = values[index++];
"),
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
fn postfix_update_in_index_assignment_snapshots_old_value() -> Result<(), String> {
    // Regression: `arr[k++] = v` must index with the *old* value of `k` and then
    // increment. Because the increment store runs inside the current statement
    // block, the enclosing index expression cannot lazily re-read `k` (it would
    // observe the already-incremented value); the old value is snapshotted into a
    // synthetic `__smelt_update_tmp` local instead.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const arr = [0, 0, 0];
let k = 0;
arr[k++] = 42;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.locals.iter().any(|local| local
            .name
            .is_some_and(|name| ctx.krate.symbols.get(name) == Some("__smelt_update_tmp"))),
        "expected postfix update in value position to snapshot the old value into a temp: {:?}",
        body.locals,
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn postfix_update_value_binding_snapshots_old_value() -> Result<(), String> {
    // Regression: `y = x++` binds `y` to the pre-increment value of `x`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let x = 0;
let y = 0;
y = x++;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.locals.iter().any(|local| local
            .name
            .is_some_and(|name| ctx.krate.symbols.get(name) == Some("__smelt_update_tmp"))),
        "expected postfix update value use to snapshot the old value into a temp: {:?}",
        body.locals,
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_empty_statements_as_noops() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function answer(): number {
  ;
  {
    ;
  }
  return 42;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;
    ensure!(
        matches!(body.stmts.as_slice(), [Stmt::Return(Some(_))]),
        "expected empty statements to emit nothing before the return: {:?}",
        body.stmts,
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_postfix_update_expression_side_effect_inside_loop_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const values = [10, 20];
let index = 0;
while (index < values.length) {
  const value = values[index++];
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let Some(Stmt::While {
        body: loop_body, ..
    }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::While { .. }))
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
        ts!(r"
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
"),
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
    let unique_priority_setters = body
        .locals
        .iter()
        .find(|local| {
            local.name.and_then(|symbol| ctx.krate.symbols.get(symbol))
                == Some("unique_priority_setters")
        })
        .ok_or_else(|| "missing uniquePrioritySetters binding".to_owned())?;
    let setter_item_ty = match ctx.krate.types.get(unique_priority_setters.ty) {
        Some(Type::List(item_ty)) => *item_ty,
        other => {
            return Err(format!(
                "expected uniquePrioritySetters to lower as a list, got {other:?}"
            ));
        }
    };
    ensure!(
        !matches!(ctx.krate.types.get(setter_item_ty), Some(Type::Unknown)),
        "expected nested filter/sort callback chain to preserve Setter element type",
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
fn lowers_typeof_optional_number_as_a_runtime_presence_check() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function isNumber(value?: number): boolean {
  return typeof value === "number";
}

function isUndefined(value?: number): boolean {
  return typeof value === "undefined";
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let first_body = function_body(&ctx, function_item(&ctx, module, 0)?)?;
    let second_body = function_body(&ctx, function_item(&ctx, module, 1)?)?;
    ensure!(first_body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::UnknownIs {
            kind: smelt_hir::UnknownKind::Null,
            ..
        }
    )));
    ensure!(second_body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::UnknownIs {
            kind: smelt_hir::UnknownKind::Null,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn narrows_union_param_in_switch_typeof_cases() -> Result<(), String> {
    // `switch (typeof x)` narrows the `string | string[]` union per arm the way
    // a chain of `if (typeof x === 'k')` guards would: the `'string'` arm proves
    // `chars` is a `string` (so `.length` and `=== chars` type-check) and the
    // `'object'` arm proves it is the `string[]` member (so `.includes` does).
    // Without per-arm `typeof` switch narrowing these member accesses hit the
    // union receiver and fail to lower. Mirrors es-toolkit `string/trimStart.ts`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function trimStartLike(str: string, chars: string | string[]): number {
  let startIndex = 0;
  switch (typeof chars) {
    case 'string': {
      if (chars.length !== 1) {
        return -1;
      }
      while (startIndex < str.length && str[startIndex] === chars) {
        startIndex++;
      }
      break;
    }
    case 'object': {
      while (startIndex < str.length && chars.includes(str[startIndex])) {
        startIndex++;
      }
    }
  }
  return startIndex;
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(0.0_f64))))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_coercive_nullish_equality_idiom() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function emptyish(data: object | string | undefined): boolean {
  return data == undefined || data != null;
}
"),
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
        ts!(r"
const pair: number[] = [1, 2];
const [first, second] = pair;
const data: Record<string, number> = { count: 3 };
const { count } = data;
"),
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
        ts!(r"
interface Duration {
  years?: number;
  months?: number;
}

export function monthsInDuration(duration: Duration): number {
  const { years = 0, months = 0 } = duration;
  return months + years * 12;
}
"),
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
        ts!(r"
interface Duration {
  years?: unknown;
  months?: unknown;
}

export function monthsInDuration(duration: Duration): number {
  const { years = 0, months = 0 } = duration;
  return months + years * 12;
}
"),
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
        ts!(r"
const options = { weekStartsOn: 1 };
const value = options.weekStartsOn;
"),
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
        ts!(r"
interface LocalizedOptions {
  locale?: string;
}
interface WeekOptions {
  weekStartsOn?: number;
}
type DefaultOptions = LocalizedOptions & WeekOptions;
let defaultOptions: DefaultOptions = {};
"),
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
    // `defaultOptions` is a module-level `let` mutated inside a function, so it
    // classifies as a mutable global; its object initializer is outside the V1
    // literal constraint, producing the named frontend blocker. Before the
    // mutable-global lift this shape HIR-lowered but the function-body write
    // had no assignable place, so MIR lowering always aborted with the generic
    // "only local, field, and index expressions can be assigned" — the named
    // blocker surfaces the same gap earlier and more precisely.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
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
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(
        &errors,
        "module-level mutable binding initializer must be a literal for now",
    )
}

#[test]
fn lowers_date_fns_date_parts() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
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
"),
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
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToFloat,
                ..
            }
        )),
        "expected getTime() to lower as numeric output without Date identity",
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
        ts!(r"
const values: number[] = [3, 1, 2];
const sorted = values.sort();
const sortedByCompare = values.sort((left, right) => left - right);
"),
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
        } if closure_callback_has_param(&ctx, body, *callback, 1)
    )));
    Ok(())
}

#[test]
fn lowers_instanceof_for_class_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class Box {}
const value = new Box();
const result = value instanceof Box;
"),
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
fn lowers_instanceof_for_record_left_operand() -> Result<(), String> {
    // A plain object/record value (`transform(obj): Record<string, unknown>`)
    // carries no nominal class identity in Smelt's record model, so
    // `value instanceof UserClass` lowers to a concrete `InstanceOf` (which the
    // codegen resolves to `false`) instead of aborting the build.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class Foo {}
function make(): Record<string, unknown> {
  return {};
}
const result = make() instanceof Foo;
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::InstanceOf { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_zero_argument_primitive_coercions() -> Result<(), String> {
    // Zero-argument primitive conversions are legal JavaScript and return the
    // type's default primitive: `Boolean()` -> `false`, `Number()` -> `0`,
    // `String()` -> `""`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const b = Boolean();
const n = Number();
const s = String();
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(false))))
    );
    // `Number()` -> numeric zero (the exact `0.0` value is verified end to end
    // in the generated-crate fixtures; assert the literal shape here).
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::Literal(Literal::Float(value)) if value.abs() < f64::EPSILON))
    );
    ensure!(body.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::Literal(Literal::String(value)) if value.is_empty()
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_date_instanceof_for_union_that_can_contain_date() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
type DateArg = number | string | Date;
const value: DateArg = 1;
const result = value instanceof Date;
"),
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
fn folds_date_instanceof_true_for_constrained_timestamp_date_result() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function identity<ResultDate extends Date>(date: ResultDate): ResultDate {
  return date;
}
const value = identity(new Date(0));
const result = value instanceof Date;
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(true))))
    );
    Ok(())
}

#[test]
fn folds_date_instanceof_true_inside_native_test_function() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

function identity<ResultDate extends Date>(date: ResultDate): ResultDate {
  return date;
}

it("preserves Date identity", () => {
  const value = identity(new Date(NaN));
  expect(value instanceof Date).toBe(true);
});
"#),
        &mut ctx,
    )?;
    ensure!(ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(true))))
    }));
    Ok(())
}

#[test]
fn folds_date_instanceof_true_for_declared_date_timestamp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const value: Date = new Date(0);
const result = value instanceof Date;
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(true))))
    );
    Ok(())
}

#[test]
fn folds_date_now_instanceof_false_for_numeric_timestamp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const result = Date.now() instanceof Date;
"),
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
fn lowers_date_instanceof_for_unknown_runtime_date_identity() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function isDate(value: unknown): boolean {
  return value instanceof Date;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .filter_map(|item_id| ctx.krate.items.get(item_id.0 as usize))
        .find_map(|item| match item {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "expected contextual arrow to lower into a function item".to_owned())?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::InstanceOf { .. }))
    );
    Ok(())
}

#[test]
fn lowers_error_instanceof_for_generic_union_guard() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function isError<T>(data: Error | T): boolean {
  return data instanceof Error;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .filter_map(|item_id| ctx.krate.items.get(item_id.0 as usize))
        .find_map(|item| match item {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "expected contextual arrow to lower into a function item".to_owned())?;
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
        ts!(r"
function isPromise<T>(data: PromiseLike<unknown> | T): boolean {
  return data instanceof Promise;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .filter_map(|item_id| ctx.krate.items.get(item_id.0 as usize))
        .find_map(|item| match item {
            Item::Function(function) => Some(function),
            _ => None,
        })
        .ok_or_else(|| "expected contextual arrow to lower into a function item".to_owned())?;
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
fn lowers_date_constructor_member_as_date_from_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const date: Date = new Date(1);
const value = 2;
const result = new (date.constructor as unknown)(value);
"),
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
fn lowers_asserted_generic_constructor_member_without_date_interception() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function clone<T extends object>(value: T): T {
  return new ((value as object).constructor as { new (): T })();
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(body
        .exprs
        .iter()
        .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })));
    ensure!(!body
        .exprs
        .iter()
        .any(|expr| matches!(expr.kind, ExprKind::DateFromValue { .. })));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_new_date_from_datearg_union_to_timestamp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
type DateArg = number | string | Date;
const value: DateArg = 1;
const result = new Date(value);
"),
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
        ts!(r"
type DateArg = number | string | Date;
function timestamp(value: DateArg): number {
  return +value;
}
"),
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
                    op: PrimitiveCastOp::ToJsNumber,
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
        ts!(r"
interface Options {
  in?: number;
}
function read(options: Options, date: number): number {
  return options?.in || date;
}
"),
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
        ts!(r"
interface Options {
  in?: number;
}
function useContext(date: number, context?: number): number {
  return context || date;
}
function read(options: Options, date: number): number {
  return useContext(date, options?.in);
}
"),
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
        ts!(r"
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
"),
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
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DateToString { .. })),
        "expected Date .toString() to retain Date string semantics",
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
        ts!(r"
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
"),
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
        ts!(r"
export function difference(dateLeft: number, dateRight: number): number {
  return dateLeft - dateRight;
}

export const differenceFp = convertToFP(difference, 2);
"),
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
        ts!(r"
export function addDays(date: number, amount: number): number {
  return date + amount;
}

export const addDaysWithOptions = convertToFP(addDays, 3);
"),
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
        ts!(r"
export interface AddOptions<DateType extends Date = Date> extends ContextOptions<DateType> {}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn ignores_safe_function_overload_signatures() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function double(value: number): number;
function double(value: number): number {
  return value * 2;
}

export function identity(value: string): string;
export function identity(value: string): string {
  return value;
}
"),
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
        ts!(r"
function missing(value: number): number;
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "declare functions are not lowered yet")
}

#[test]
fn ignores_exported_ambient_declare_functions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export declare function addLeadingZeros(number: number, targetLength: number): string;
"),
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
        ts!(r"
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
"),
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
        ts!(r"
type Formatter = (date: Date, token: string) => string;

export const formatters: { [token: string]: Formatter } = {
  y: function (date, token) {
    return token + String(date.getFullYear());
  },
};
"),
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
        ts!(r"
type Formatter = (date: Date, token: string) => string;

export const formatters: { [token: string]: Formatter } = {
  y: (date, token) => token + String(date.getFullYear()),
};
"),
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
        ts!(r"
type Formatter = (date: Date, token: string) => number;

export const formatters: { [token: string]: Formatter } = {
  t: function (date, token) {
    return +date;
  },
};
"),
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
        ts!(r"
function parts(pattern: string): string[] | null {
  return pattern.match(/(P+)(p+)?/);
}
"),
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
fn lowers_optional_object_logical_or_as_selected_runtime_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
type Result<T> = { value: T; rest: string } | null;

function select(left: Result<number>, right: Result<number>): unknown {
  return left || right;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    ..
                }
            )
        }),
        "optional object logical fallback should branch on runtime truthiness"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "optional object logical fallback should preserve the selected object"
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| ctx.krate.types.get(expr.ty) == Some(&Type::String)),
        "optional object logical fallback must not lower through string selection"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_callable_logical_or_as_selected_runtime_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function select(
  value: unknown,
  context?: (value: unknown) => unknown,
): unknown {
  return context || value;
}
"),
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
fn lowers_unknown_logical_or_as_selected_runtime_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function select(value: unknown, fallback: unknown): unknown {
  return value || fallback;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    ..
                }
            )
        }),
        "unknown logical fallback should branch on runtime truthiness"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "unknown logical fallback should preserve one selected operand"
    );
    ensure!(
        !body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::BinOp {
                    op: BinOp::NotEq,
                    ..
                }
            )
        }),
        "unknown logical fallback must not use string inequality selection"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_optional_date_type_parameter_logical_or_nan_as_selected_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function select<ResultDate extends Date>(
  result: ResultDate | undefined,
): unknown {
  return result || NaN;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::OptionalCoalesce { .. })),
        "expected an optional Date value or NaN fallback to preserve the selected runtime value"
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::BinOp { op: BinOp::Or, .. })),
        "optional Date fallback must not collapse to a boolean expression"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_return_addition_with_branch_assigned_suffix() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function ordinal(dirtyNumber: number, feminine: boolean): string {
  const number = Number(dirtyNumber);
  let suffix;
  if (number === 1) {
    suffix = feminine ? "ère" : "er";
  } else {
    suffix = "ème";
  }
  return number + suffix;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::BinOp { op: BinOp::Add, .. })
                && ctx.krate.types.get(expr.ty) == Some(&Type::String)
        ),
        "string-return additions with erased branch locals must keep string result type"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_contextual_string_arrow_addition_with_unknown_suffix() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let _module_id = lower_ok(
        ts!(r#"
type LocalizeFn<Value> = (value: Value, options?: { unit?: string }) => string;
type Localize = {
  ordinalNumber: LocalizeFn<number>;
};

const feminineUnits = ["second", "minute"];

const ordinalNumber: LocalizeFn<number> = (dirtyNumber, options) => {
  const number = Number(dirtyNumber);
  const unit = options?.unit;
  if (number === 0) return "0";
  let suffix;
  if (number === 1) {
    suffix = unit && feminineUnits.includes(unit) ? "ère" : "er";
  } else {
    suffix = "ème";
  }
  return number + suffix;
};

export const localize: Localize = {
  ordinalNumber,
};
"#),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body.exprs.iter().any(|expr| matches!(
                expr.kind,
                ExprKind::BinOp { op: BinOp::Add, .. }
            ) && ctx.krate.types.get(expr.ty)
                == Some(&Type::String))),
        "contextual string-return arrows must keep number-plus-suffix as string addition"
    );
    ensure!(
        !ctx.krate
            .bodies
            .iter()
            .any(|body| body.exprs.iter().any(|expr| matches!(
                expr.kind,
                ExprKind::BinOp { op: BinOp::Add, .. }
            ) && ctx.krate.types.get(expr.ty)
                == Some(&Type::Float))),
        "contextual string-return arrows must not lower number-plus-suffix as numeric addition"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_negated_optional_date_type_parameter_as_presence_check() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function absent<ResultDate extends Date>(
  result: ResultDate | undefined,
): boolean {
  return !result;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::None))),
        "expected optional Date truthiness to compare presence rather than inspect its timestamp"
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
        ts!(r"
type Delay = number | (() => void) | undefined;

function delay(value: Delay): number | (() => void) {
  return value ?? 0;
}
"),
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
        ts!(r"
function localDay(day: number, weekStartsOn: number): string {
  const localDayOfWeek = (day - weekStartsOn + 8) % 7 || 7;
  return String(localDayOfWeek);
}
"),
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
fn preserves_mixed_string_numeric_logical_fallback_for_numeric_coercion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function offset(values: string[]): number {
  return +(values[0] || 0);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = function_body(&ctx, function_item(&ctx, module, 0)?)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })
                && ctx.krate.types.get(expr.ty) == Some(&Type::Unknown)),
        "expected string-or-number selection to remain erased until unary coercion"
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToJsNumber,
                ..
            }
        )),
        "expected unary plus to coerce the selected JavaScript value"
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
        ts!(r"
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
"),
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
        ts!(r"
type Mapper = ((value: number) => number) | undefined;

function applyMaybe(mapper: Mapper, value: number): number {
  const values = [value].map((item) => mapper ? mapper(item) : item);
  return values[0];
}
"),
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
        ts!(r"
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
"),
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
                    op: DictProjectionOp::ForInKeys,
                    ..
                }
            )))
    );
    ensure!(ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_match_as_optional_match_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function firstMatch(text: string, pattern: string): string | undefined {
  const matchResult = text.match(pattern);
  if (!matchResult) {
    return undefined;
  }
  return matchResult[0];
}
"),
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
        ts!(r"
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
"),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_conditional_assignment_to_uninitialized_value_nullishable_after_branch()
-> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function read(available: boolean): Date {
  let date;
  if (available) {
    date = new Date(0);
  }
  if (!date) {
    return new Date(NaN);
  }
  return date;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    ensure!(
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::UnaryOp { op: smelt_hir::UnaryOp::Not, operand }
                if matches!(body.exprs.get(usize::try_from(operand.0).unwrap_or(usize::MAX)).map(|operand| &operand.ty).and_then(|ty| ctx.krate.types.get(*ty)), Some(Type::Unknown)))
        }),
        "possibly unassigned values must preserve runtime truthiness after a conditional assignment"
    );
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

    // `parseInt("42")` casts to int; `parseInt("42", 10)` honors the radix via
    // `ParseIntRadix`; `parseFloat` casts to float.
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
    let radix_parse_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ParseIntRadix { .. }))
        .count();
    ensure_eq!(int_parse_count, 1);
    ensure_eq!(radix_parse_count, 1);
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ParseFloat,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_computed_class_record_reads_as_optional_runtime_lookups() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class Parser {
  run(): number { return 1; }
}
function read(parsers: Record<string, Parser>, key: string): number {
  const parser = parsers[key];
  if (parser) {
    return parser.run();
  }
  return 0;
}
"),
        &mut ctx,
    )?;
    let _ = module(&ctx, module_id)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .any(|body| body.exprs.iter().any(|expr| {
                matches!(
                    expr.kind,
                    ExprKind::Index { .. }
                        if matches!(ctx.krate.types.get(expr.ty), Some(Type::Optional(_)))
                )
            }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Collect the source-level names of every test function item in a module.
fn test_function_names(ctx: &HirCtx, module: &smelt_hir::Module) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..module.items.len() {
        if let Ok(function) = function_item(ctx, module, index)
            && function.is_test
            && let Some(name) = ctx.krate.symbols.get(function.name)
        {
            names.push(name.to_owned());
        }
    }
    names
}

#[test]
fn vitest_describe_foreach_literal_unrolls_one_test_per_element() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, expect } from "vitest";

describe("escape", () => {
  ["a", "b", "c"].forEach((chr) => {
    it(`escapes ${chr}`, () => {
      expect(chr).toBe(chr);
    });
  });
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/foreach.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let names = test_function_names(&ctx, module);
    ensure_eq!(names.len(), 3);
    // Distinct elements that sanitize identically still get unique names.
    ensure!(names.iter().any(|name| name.contains("case_0")));
    ensure!(names.iter().any(|name| name.contains("case_2")));
    // The template loop variable folds into the resolved test title.
    ensure!(names.iter().any(|name| name.contains("escapes_a")));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_describe_for_of_literal_unrolls_one_test_per_element() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, expect } from "vitest";

describe("group", () => {
  for (const value of [1, 2]) {
    it(`handles ${value}`, () => {
      expect(value).toBe(value);
    });
  }
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/for-of.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let names = test_function_names(&ctx, module);
    ensure_eq!(names.len(), 2);
    ensure!(names.iter().any(|name| name.contains("handles_1")));
    ensure!(names.iter().any(|name| name.contains("handles_2")));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn vitest_template_test_name_without_bound_expression_is_rejected() -> Result<(), String> {
    let source = ts!(r#"
import { describe, it, expect } from "vitest";

describe("group", () => {
  const value = compute();
  it(`handles ${value}`, () => {
    expect(value).toBe(value);
  });
});
"#);
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(source, &mut ctx)?;
    ensure!(
        errors
            .iter()
            .any(|error| error.message.contains("test case names must be string literals"))
    );
    Ok(())
}

#[test]
fn mapped_types_over_iterable_keys_preserve_list_shape() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Mapped<T extends readonly unknown[], U> = {
  -readonly [P in keyof T]: U;
};
const values: Mapped<readonly number[], string> = ["a", "b"];
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(body.locals.iter().any(|local| {
        local.name.and_then(|name| ctx.krate.symbols.get(name)) == Some("values")
            && matches!(ctx.krate.types.get(local.ty), Some(Type::List(item)) if ctx.krate.types.get(*item) == Some(&Type::String))
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn in_guard_narrows_class_union_to_concrete_arm() -> Result<(), String> {
    // Issue #55: a structural `"field" in value` guard narrows a class union so
    // the true-branch read is lowered at the single matching arm. Smelt records
    // this as a narrowing fact that re-types the local read with an
    // `UnknownCast` to the narrowed type.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Circle { radius: number = 1; }
class Square { side: number = 2; }
function describe(shape: Circle | Square): number {
  if ("radius" in shape) {
    return shape.radius;
  }
  return 0;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .filter_map(|item_id| ctx.krate.items.get(item_id.0 as usize))
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("describe") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "expected `describe` to lower into a function item".to_owned())?;
    let body = function_body(&ctx, function)?;
    // The narrowed read is re-typed to the single class arm via an UnknownCast.
    ensure!(body.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::UnknownCast { target, .. }
            if matches!(ctx.krate.types.get(*target), Some(Type::Class { .. }))
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn property_equality_narrows_union_by_field_presence() -> Result<(), String> {
    // Issue #55: `value.field === literal` narrows the union to arms that carry
    // `field` (Smelt erases literal types, so presence is what it can prove).
    // Chained after an `in` guard the discriminant read projects the concrete
    // arm and the whole function lowers to a valid HIR crate.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Circle { tag: string = "c"; radius: number = 1; }
class Square { side: number = 2; }
function describe(shape: Circle | Square): number {
  if ("tag" in shape && shape.tag === "c") {
    return shape.radius;
  }
  return 0;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = module
        .items
        .iter()
        .filter_map(|item_id| ctx.krate.items.get(item_id.0 as usize))
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("describe") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "expected `describe` to lower into a function item".to_owned())?;
    let body = function_body(&ctx, function)?;
    ensure!(body.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::UnknownCast { target, .. }
            if matches!(ctx.krate.types.get(*target), Some(Type::Class { .. }))
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn reassigning_narrowed_local_with_compatible_value_keeps_narrowing() -> Result<(), String> {
    // Issue #55 invalidation: writing a value still inside the narrowed set
    // refines the fact instead of dropping it. The reassigned `path` read stays
    // narrowed to `string`, so the function validates cleanly.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function resolve(path: string | (() => string)): string {
  if (typeof path === "string") {
    path = path + "x";
    return path;
  }
  return path();
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let _ = module_id;
    Ok(())
}
