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
fn lowers_object_has_own_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const first = Object.hasOwn(mapping, "a");
const second = mapping.hasOwnProperty("b");
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
    let flags = lowering_errors(
        ts!(r#"
const text = "abc123";
const hasDigits = new RegExp("\\d+", "g").test(text);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&flags, "exactly one string pattern")?;

    let mut ctx = HirCtx::new();
    let non_string = lowering_errors(
        ts!(r#"
const text = "abc123";
const hasDigits = new RegExp(1).test(text);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&non_string, "string pattern and haystack")?;

    let mut ctx = HirCtx::new();
    let literal_flags = lowering_errors(
        ts!(r#"
const text = "abc123";
const hasDigits = /\d+/g.test(text);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&literal_flags, "flags")?;
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
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetLit(_))),
        "Set constructor did not lower to SetLit"
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetContains { .. })),
        "Set.has did not lower to SetContains"
    );
    Ok(())
}
