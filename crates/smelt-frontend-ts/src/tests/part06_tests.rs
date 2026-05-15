use super::*;

#[test]
fn lowers_interface_inheritance_into_shape_requirements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Entity { id: string; }
interface Named extends Entity { name: string; }
class User implements Named {
  id: string;
  name: string;
  constructor(id: string, name: string) {
    this.id = id;
    this.name = name;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_missing_inherited_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("interface Entity { id: string; }
interface Named extends Entity { name: string; }
class User implements Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "field `id`")
}

#[test]
fn lowers_literal_computed_property_names() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Entity { [\"id\"]: string; }
class User implements Entity {
  [\"id\"]: string;
  constructor(id: string) {
    this.id = id;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_dynamic_computed_property_names() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("const key = \"id\";
class User {
  [key]: string;
  constructor() {}
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "dynamic computed property")
}

#[test]
fn optional_interface_fields_may_be_absent() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Named { name?: string; }
class User implements Named {
  constructor() {}
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn required_fields_satisfy_optional_interface_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Named { name?: string; }
class User implements Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_optional_class_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("class User {
  name?: string;
  constructor() {}
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "optional class fields")
}

#[test]
fn rejects_generic_classes() -> Result<(), String> {
    let mut class_ctx = HirCtx::new();
    let class_errors = lowering_errors(
        ts!("class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
"),
        &mut class_ctx,
    )?;
    assert_unsupported_ts(&class_errors, "generic classes")?;

    Ok(())
}

#[test]
fn rejects_static_members() -> Result<(), String> {
    let mut field_ctx = HirCtx::new();
    let field_errors = lowering_errors(
        ts!("class User {
  static role: string;
  constructor() {}
}
"),
        &mut field_ctx,
    )?;
    assert_unsupported_ts(&field_errors, "static fields")?;

    let mut method_ctx = HirCtx::new();
    let method_errors = lowering_errors(
        ts!("class User {
  static role(): string { return \"admin\"; }
}
"),
        &mut method_ctx,
    )?;
    assert_unsupported_ts(&method_errors, "static methods")
}

#[test]
fn rejects_getters_setters_decorators_and_abstract_classes() -> Result<(), String> {
    let mut getter_ctx = HirCtx::new();
    let getter_errors = lowering_errors(
        ts!("class User {
  get name(): string { return \"Ada\"; }
}
"),
        &mut getter_ctx,
    )?;
    assert_unsupported_ts(&getter_errors, "getters and setters")?;

    let mut decorator_ctx = HirCtx::new();
    let decorator_errors = lowering_errors(
        ts!("@sealed
class User {
  constructor() {}
}
"),
        &mut decorator_ctx,
    )?;
    assert_unsupported_ts(&decorator_errors, "decorators")?;

    let mut abstract_ctx = HirCtx::new();
    let abstract_errors = lowering_errors(
        ts!("abstract class User {
  abstract name(): string;
}
"),
        &mut abstract_ctx,
    )?;
    assert_unsupported_ts(&abstract_errors, "abstract classes")
}

#[test]
fn lowers_literal_switch_to_hir_match() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(
            "function label(status: \"pending\" | \"approved\" | \"rejected\"): string {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
    case \"rejected\":
      return \"Rejected\";
  }
}
const result = label(\"approved\");
console.log(result);
"
        ),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    let Some(Stmt::Match { arms, default, .. }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Match { .. }))
    else {
        return Err("expected switch to lower to HIR match".to_owned());
    };
    ensure_eq!(arms.len(), 3);
    ensure!(default.is_none());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_grouped_empty_switch_cases_to_same_arm_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function label(status: \"a\" | \"aa\" | \"aaa\"): string {
  switch (status) {
    case \"a\":
    case \"aa\":
      return \"short\";
    case \"aaa\":
      return \"long\";
  }
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    let Some(Stmt::Match { arms, .. }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Match { .. }))
    else {
        return Err("expected grouped switch to lower to HIR match".to_owned());
    };
    ensure_eq!(arms.len(), 3);
    ensure_eq!(arms[0].body, arms[1].body);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_empty_case_grouped_with_default_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function label(status: \"a\" | \"aa\"): string {
  switch (status) {
    case \"a\":
    default:
      return \"fallback\";
  }
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    let Some(Stmt::Match { arms, default, .. }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Match { .. }))
    else {
        return Err("expected default-grouped switch to lower to HIR match".to_owned());
    };
    ensure_eq!(arms.len(), 1);
    ensure_eq!(Some(arms[0].body), *default);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_coercive_equality() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("function same(a: number, b: number): boolean {
  return a == b;
}
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "coercive equality")
}

#[test]
fn rejects_untyped_for_of_binding() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("let values = 1;
for (let item of values) {
  continue;
}
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "index access is only lowered")
}

#[test]
fn lowers_async_functions_and_await_to_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function load(value: number): Promise<number> {
  return value;
}

async function main(): Promise<number> {
  return await load(1);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;

    ensure_eq!(module.items.len(), 2);
    let load = function_item(&ctx, module, 0)?;
    ensure!(load.is_async);
    ensure!(matches!(
        ctx.krate.types.get(load.return_ty),
        Some(Type::Future(_))
    ));

    let main = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, main)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_)))
    );
    let machine = body
        .async_state_machine
        .as_ref()
        .ok_or_else(|| "async body should have state-machine metadata".to_owned())?;
    ensure_eq!(machine.states.len(), 2);
    ensure_eq!(machine.suspensions.len(), 1);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_promise_all_to_async_runtime_op() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function lift(value: number): Promise<number> {
  return value;
}

async function main(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let main = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, main)?;

    ensure!(body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: smelt_hir::AsyncOp::All,
                ..
            }
        )
    }));
    Ok(())
}

#[test]
fn lowers_promise_race_all_settled_and_timer_shim() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("async function lift(value: number): Promise<number> {
  await setTimeout(0);
  return value;
}

async function race(): Promise<number> {
  return await Promise.race([lift(1), lift(2)]);
}

async function settled(): Promise<[number, number]> {
  return await Promise.allSettled([lift(1), lift(2)]);
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_promise_all_over_local_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function lift(value: number): Promise<number> {
  return value;
}

async function main(): Promise<number[]> {
  const prepared = [lift(1), lift(2), lift(3)];
  return await Promise.all(prepared);
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_vitest_async_expect_matchers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function value(): Promise<void> {
}

import { expect } from \"vitest\";

async function testCase(): Promise<void> {
  await expect(value()).resolves.toBeUndefined();
  await expect(Promise.all([value(), value()])).rejects.toThrow(\"boom\");
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn skips_fast_check_vitest_property_registration() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { fc, test } from "@fast-check/vitest";

test.prop([fc.array(fc.anything()), fc.func(fc.string()).map((fn) => fn)])(
  "property",
  (data, grouper) => {
    expect(data.map(grouper)).toHaveLength(data.length);
  },
);
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;

    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_fetch_to_async_http_get_text() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function load(): Promise<string> {
  return await fetch(\"https://example.com\");
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let load = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, load)?;

    ensure!(body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: smelt_hir::AsyncOp::HttpGetText,
                ..
            }
        )
    }));
    Ok(())
}

#[test]
fn rejects_await_outside_async_function() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("function read(): number {
  return await 1;
}
"),
        &mut ctx,
    )?;

    let error = errors
        .first()
        .ok_or_else(|| "expected at least one lowering error".to_owned())?;
    ensure_eq!(error.code, "smelt::parse-error");
    ensure!(error.message.contains("await"));
    Ok(())
}

#[test]
fn rejects_async_functions_without_promise_return_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("async function load(): number {
  return 1;
}
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "Promise<T>")
}

#[test]
fn rejects_switch_fallthrough_until_it_is_modeled() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(
            "function label(status: \"pending\" | \"approved\"): string {
  switch (status) {
    case \"pending\":
      const waiting = \"waiting\";
    case \"approved\":
      return \"Approved\";
  }
}
"
        ),
        &mut ctx,
    )?;

    ensure!(
        errors
            .iter()
            .any(|error| error.message.contains("switch fallthrough")),
        "expected switch fallthrough error, got {errors:?}"
    );
    Ok(())
}

#[test]
fn lowers_template_literal_to_string_concat() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let _module_id = lower_ok(
        ts!("const name: string = \"world\";\nconst msg: string = `Hello ${name}!`;"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn accepts_import_and_export_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let _module_id = lower_ok(
        ts!("import { foo } from './foo';\nexport function bar(): number { return 1; }"),
        &mut ctx,
    )?;
    Ok(())
}
