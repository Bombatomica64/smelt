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
fn lowers_optional_class_fields() -> Result<(), String> {
    // An optional class field (`name?: string`) records `optional: true` and
    // interns its type as `Type::Optional`, while a required field stays
    // concrete. Rust codegen relies on the `Type::Optional` wrapper to emit
    // `Option<T>` for the optional slot only.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("class User {
  id: string;
  name?: string;
  constructor(id: string) {
    this.id = id;
  }
}
"),
        &mut ctx,
    )?;
    let class = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(class) => Some(class),
            _ => None,
        })
        .ok_or_else(|| "missing class item".to_owned())?;
    let id_field = class
        .fields
        .iter()
        .find(|field| ctx.krate.symbols.get(field.name) == Some("id"))
        .ok_or_else(|| "missing field id".to_owned())?;
    let name_field = class
        .fields
        .iter()
        .find(|field| ctx.krate.symbols.get(field.name) == Some("name"))
        .ok_or_else(|| "missing field name".to_owned())?;
    ensure!(!id_field.optional);
    ensure!(!matches!(
        ctx.krate.types.get(id_field.ty),
        Some(Type::Optional(_))
    ));
    ensure!(name_field.optional);
    ensure!(matches!(
        ctx.krate.types.get(name_field.ty),
        Some(Type::Optional(_))
    ));
    Ok(())
}

#[test]
fn lowers_optional_constructor_parameters_as_optional_values() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("class ValueSetter {
  constructor(subPriority?: number) {}
}
"),
        &mut ctx,
    )?;
    let constructor = ctx.krate.items.iter().find_map(|item| match item {
        Item::Function(function)
            if matches!(function.owner, smelt_hir::FunctionOwner::Constructor { .. }) =>
        {
            Some(function)
        }
        _ => None,
    });
    let constructor = constructor.ok_or_else(|| "missing constructor function".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(constructor.params[0].ty),
        Some(Type::Optional(_))
    ));
    Ok(())
}

#[test]
fn lowers_generic_classes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_uninitialized_static_field_but_lowers_static_members() -> Result<(), String> {
    // A `static` field without a concrete literal initializer is still rejected:
    // there is no materializable value to resolve `Class.role` to (issue #98
    // lowers only literal-initialized static constants).
    let mut field_ctx = HirCtx::new();
    let field_errors = lowering_errors(
        ts!("class User {
  static role: string;
  constructor() {}
}
"),
        &mut field_ctx,
    )?;
    assert_unsupported_ts(&field_errors, "static fields require a concrete literal initializer")?;

    // A literal-initialized `static` constant and a `static` method now lower
    // successfully (issue #98): the constant becomes a materialized static field
    // and the method a receiver-free associated function.
    let mut ok_ctx = HirCtx::new();
    lower_ok(
        ts!("class User {
  static readonly ROLE: string = \"admin\";
  static greeting(): string { return \"hi\"; }
}
"),
        &mut ok_ctx,
    )?;
    ensure!(smelt_hir::validate(&ok_ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_setters_and_decorators_and_lowers_abstract_classes() -> Result<(), String> {
    let mut setter_ctx = HirCtx::new();
    let setter_errors = lowering_errors(
        ts!("class User {
  set name(value: string) {}
}
"),
        &mut setter_ctx,
    )?;
    assert_unsupported_ts(&setter_errors, "getters and setters")?;

    let mut decorator_ctx = HirCtx::new();
    let decorator_errors = lowering_errors(
        ts!("@sealed
class User {
  constructor() {}
}
"),
        &mut decorator_ctx,
    )?;
    let decorator_error = decorator_errors
        .first()
        .ok_or_else(|| "missing decorator specialization diagnostic".to_owned())?;
    ensure_eq!(decorator_error.code, "smelt::specialization-required");

    let mut abstract_ctx = HirCtx::new();
    lower_ok(
        ts!("abstract class User {
  abstract name(): string;
}
"),
        &mut abstract_ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_direct_abstract_class_construction() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("abstract class User {
  abstract name(): string;
}
const user = new User();
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "cannot be constructed")
}

#[test]
fn lowers_same_class_abstract_method_calls_and_method_references() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("type ParseResult<T> = { value: T; rest: string } | null;
class ValueSetter<T> {
  value: T;
  validateValue: (value: T) => boolean;
  constructor(value: T, validateValue: (value: T) => boolean) {
    this.value = value;
    this.validateValue = validateValue;
  }
}
abstract class Parser<Value> {
  public run(): { setter: ValueSetter<Value>; rest: string } | null {
    const result = this.parse();
    if (!result) {
      return null;
    }
    return {
      setter: new ValueSetter<Value>(result.value, this.validate),
      rest: result.rest,
    };
  }
  protected validate(value: Value): boolean {
    return true;
  }
  protected abstract parse(): ParseResult<Value>;
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_extends_inherited_fields_and_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("class Base<T> {
  x: T;
  constructor(x: T) {
    this.x = x;
  }
  pick(): T {
    return this.x;
  }
}
class Child<T> extends Base<T> {
  y: T;
  constructor(x: T, y: T) {
    super(x);
    this.y = y;
  }
  value(): T {
    return this.pick();
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_class_extends_imported_opaque_base() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("import { AbstractRouteValidator } from '@strapi/utils';

class CoreRouteValidator extends AbstractRouteValidator {
  value: string;
  constructor(value: string) {
    super();
    this.value = value;
  }
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_unannotated_abstract_method_as_unknown_return() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("abstract class Validator {
  public abstract fieldRecord(type: unknown);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_abstract_class_method_default_params() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export abstract class CoreService {
  getFetchParams(params = {}): any {
    return {
      status: 'published',
      ...params,
    };
  }
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_unannotated_async_class_methods_as_unknown_future() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
class Service {
  async find(params = {}) {
    return params;
  }
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn ignores_qualified_external_implements_clauses() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import type { Core } from '@strapi/types';

class SingleTypeService implements Core.CoreAPI.Service.SingleType {}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_unknown_missing_fields_on_derived_classes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
class Base {}

class Derived extends Base {
  read() {
    return this.externalField;
  }
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_optional_receiver_field_access_as_optional() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Config {
  pagination?: { withCount?: boolean };
}

function read(config?: Config) {
  return config?.pagination?.withCount;
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_unannotated_class_method_as_unknown_return() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("class Validator {
  public fieldRecord(type: unknown) {
    return {};
  }
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
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
fn lowers_coercive_equality_as_strict_equality() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function same(a: number, b: number): boolean {
  return a == b;
}
function different(a: number, b: number): boolean {
  return a != b;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
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
fn lowers_fetch_with_options_and_url_objects() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
async function load(url: RequestInfo | URL, options?: RequestInit): Promise<string> {
  return await fetch(url, options);
}
"#),
        &mut ctx,
    )?;
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
fn lowers_switch_fallthrough_as_single_pass_loop() -> Result<(), String> {
    // Genuine fallthrough (a case body reaching the next case) lowers through
    // the single-iteration-loop chain instead of a HIR `Match`.
    let mut ctx = HirCtx::new();
    let _module_id = lower_ok(
        ts!(
            "export function label(status: string): string {
  let result = \"\";
  switch (status) {
    case \"pending\":
      result = \"waiting\";
    case \"approved\":
      return \"Approved\";
    default:
      result = \"unknown\";
  }
  return result;
}
"
        ),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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

#[test]
fn lowers_rejects_to_throw_over_async_call_actual() -> Result<(), String> {
    // An `async` helper resolves to `Promise<T>`; `.rejects.toThrow` awaits the
    // actual (flattening through native exception flow) to assert it rejects.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

async function run(): Promise<number> {
  throw new Error("boom");
}

it("rejects", async () => {
  await expect(run()).rejects.toThrow("boom");
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_resolves_over_erased_promise_actual() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";
import { run } from "./run";

it("resolves", async () => {
  await expect(run()).resolves.toBeUndefined();
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_to_have_property_over_erased_object_actual() -> Result<(), String> {
    // The actual is the erased return of an imported helper; `toHaveProperty`
    // lowers to a runtime key-containment check on the live `SmeltUnknown`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";
import { transform } from "./transform";

it("has property", () => {
  const result = transform({ user_id: 1 });
  expect(result).toHaveProperty("userId", 1);
  expect(result).toHaveProperty("toString");
});
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_mock_return_value_with_argument() -> Result<(), String> {
    // `vi.fn().mockReturnValue(x)` configures a plain mock's return value; the
    // configured value is accepted and the chainable mock handle is returned.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it, vi } from "vitest";

it("configures return value", () => {
  const mockFn = vi.fn();
  mockFn.mockReturnValue(3);
  expect(mockFn).toBe(mockFn);
});
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn captures_enclosing_locals_in_for_loop_closure_body() -> Result<(), String> {
    // A returned `function (...)` whose body iterates over a captured enclosing
    // rest parameter with a C-style `for (let i = 0; i < xs.length; ++i)` loop
    // must capture `xs` (the overEvery/overSome/bind pattern). Before
    // `collect_statement_capture_names` traversed `ForStatement`, the loop test,
    // update, and body were skipped and `xs` failed with `unresolved identifier`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function overEvery(...xs: number[]): (v: number) => boolean {
  return function (v: number): boolean {
    let total = 0;
    for (let i = 0; i < xs.length; ++i) {
      total = total + xs[i];
    }
    return total > v;
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
fn captures_mutated_counter_across_arrow_closure() -> Result<(), String> {
    // `after`-style closure: the returned arrow mutates a captured enclosing
    // `let counter` and reads the captured parameter `n`. The closure-body
    // capture machinery must record both across the C-style/conditional body.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function after(n: number, func: () => number): () => number {
  let counter = 0;
  return (): number => {
    counter = counter + 1;
    if (counter >= n) {
      return func();
    }
    return 0;
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
fn capturing_const_function_binding_is_not_synthesized_as_class() -> Result<(), String> {
    // `const bound = function (...) { … }` that captures enclosing locals is a
    // constructable function *value*, not a static class. The binding must
    // remain a closure value so its captures (`xs`) are preserved; synthesizing
    // a top-level class would drop them and break the closure body. This must
    // lower without an `unresolved identifier`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function bind(...xs: number[]): (v: number) => number {
  const bound = function (v: number): number {
    let total = v;
    for (let i = 0; i < xs.length; i++) {
      total = total + xs[i];
    }
    return total;
  };
  return bound;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_interface_method_signature_as_callable_field() -> Result<(), String> {
    // A method signature on an interface is a callable member. It is stored as
    // a function-typed field of the same name so an interface-typed value can be
    // invoked through the field-call machinery, mirroring class virtual-method
    // fields. The `methods` list is retained for `implements` validation.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Counter { count(): number; }
"),
        &mut ctx,
    )?;
    let Some(Item::Interface(interface)) = ctx
        .krate
        .items
        .iter()
        .find(|item| matches!(item, Item::Interface(_)))
    else {
        return Err("expected an interface item".to_owned());
    };
    let count = interface
        .fields
        .iter()
        .find(|field| ctx.krate.symbols.get(field.name) == Some("count"))
        .ok_or_else(|| "expected a `count` callable field on the interface".to_owned())?;
    let Some(Type::Function(function)) = ctx.krate.types.get(count.ty) else {
        return Err("expected the `count` field to be function-typed".to_owned());
    };
    ensure!(function.params.is_empty());
    ensure_eq!(ctx.krate.types.get(function.return_ty), Some(&Type::Float));
    ensure!(
        interface
            .methods
            .iter()
            .any(|method| ctx.krate.symbols.get(method.name) == Some("count"))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_method_call_through_interface_typed_parameter() -> Result<(), String> {
    // A parameter typed as a method-bearing interface can be called through: the
    // call lowers to a closure call on the interface's callable field and takes
    // the method's declared return type (`number`), not the interface itself.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("interface Counter { count(): number; }
export function total(counter: Counter): number {
  return counter.count();
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "total")?;
    ensure_eq!(ctx.krate.types.get(function.return_ty), Some(&Type::Float));
    let body = function_body(&ctx, function)?;
    let call = body
        .exprs
        .iter()
        .find(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
        .ok_or_else(|| "expected the interface method call to lower to a closure call".to_owned())?;
    ensure_eq!(ctx.krate.types.get(call.ty), Some(&Type::Float));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_interface_method_call_with_arguments() -> Result<(), String> {
    // Method signatures with parameters carry the parameter types into the
    // callable field's function type, so the call type-checks the argument list.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("interface Adder { add(a: number, b: number): number; }
export function run(x: Adder): number {
  return x.add(1, 2);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "run")?;
    ensure_eq!(ctx.krate.types.get(function.return_ty), Some(&Type::Float));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn class_method_satisfies_implemented_interface_method() -> Result<(), String> {
    // Turning interface methods into callable fields must not break class
    // `implements` validation: a real class method still satisfies the required
    // interface method rather than demanding a matching data field.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Counter { count(): number; }
class MyCounter implements Counter {
  count(): number {
    return 7;
  }
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn object_literal_satisfies_interface_method_field() -> Result<(), String> {
    // An object literal supplying the method as a property satisfies a
    // method-bearing interface, because the method is stored as a field.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("interface Counter { count(): number; }
export function total(counter: Counter): number {
  return counter.count();
}
export function useit(): number {
  const c: Counter = { count: () => 5 };
  return total(c);
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
