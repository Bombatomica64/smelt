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
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(
        |expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value.is_infinite())
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
    let assign_errors = lowering_errors(
        ts!(r#"
const merged = Object.assign({}, { value: 1 });
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&assign_errors, "Object.assign")?;

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
fn lowers_global_numeric_parse_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const intValue = parseInt("42");
const floatValue = parseFloat("42.5");
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
