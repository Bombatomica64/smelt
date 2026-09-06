use super::*;

/// Generic implementation arguments instantiate both interface fields and
/// method signatures before structural class validation.
#[test]
fn lowers_generic_implements_with_concrete_members() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Value<T> {
  value: T;
  map(input: T): T;
}
class StringValue implements Value<string> {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
  map(input: string): string {
    return input;
  }
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// TypeScript overload declarations may share one explicit-`any` runtime
/// implementation, which structurally satisfies every instantiated signature.
#[test]
fn lowers_generic_implements_with_overloaded_any_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Mapper<T> {
  map(value: T): T;
  map<U>(value: T): U;
}
class StringMapper implements Mapper<string> {
  map(value: string): string;
  map<U>(value: string): U;
  map(value: any): any {
    return value;
  }
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A concrete generic implementation still rejects members that satisfy the
/// uninstantiated shape but not the supplied type argument.
#[test]
fn rejects_mismatched_generic_implements_member() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
interface Value<T> {
  value: T;
}
class NumberValue implements Value<string> {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "mismatched type")
}

/// Imported generic interfaces have no local structural definition for Smelt
/// to validate and remain an opaque boundary after TypeScript validation.
#[test]
fn ignores_imported_generic_implements_clause() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import type { Contract } from "external-contracts";
class Service implements Contract<string> {}
"#),
        &mut ctx,
    )?;
    Ok(())
}

/// An imported interface must not bind to an unrelated same-name interface
/// that happened to be lowered from an earlier module.
#[test]
fn imported_generic_implements_ignores_cross_module_name_collision() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
export interface Contract<T> {
  value: T;
}
"),
        "local-contract.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r#"
import type { Contract } from "external-contracts";
class Service implements Contract<string> {}
"#),
        "service.ts",
        &mut ctx,
    )?;
    Ok(())
}

/// Interface declarations are module-scoped and may follow the class that
/// implements them.
#[test]
fn lowers_forward_declared_generic_implements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class StringValue implements Value<string> {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
}
interface Value<T> {
  value: T;
}
"),
        &mut ctx,
    )?;
    Ok(())
}

/// Deferred validation still rejects a forward-declared generic interface
/// whose instantiated member type does not match the class.
#[test]
fn rejects_mismatched_forward_declared_generic_implements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
class NumberValue implements Value<string> {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}
interface Value<T> {
  value: T;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "mismatched type")
}

/// Eager validation of a forward declaration must not borrow a same-name
/// interface shape from an earlier module before the local declaration lowers.
#[test]
fn forward_generic_implements_ignores_prior_module_name_collision() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
export interface Value<T> {
  priorOnly: number;
}
"),
        "prior.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r"
class StringValue implements Value<string> {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
}
interface Value<T> {
  value: T;
}
"),
        "current.ts",
        &mut ctx,
    )?;
    Ok(())
}

/// Generic substitution applies to method parameters and returns as well as
/// data fields.
#[test]
fn rejects_mismatched_generic_implements_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
interface Mapper<T> {
  map(value: T): T;
}
class NumberMapper implements Mapper<string> {
  map(value: number): number {
    return value;
  }
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "mismatched signature")
}

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

// NOTE: the former `rejects_dynamic_computed_property_names` test asserted that a
// `const`-keyed computed class field (`const key = "id"; [key]: string`) was
// rejected. Issue #96 intentionally made statically-resolvable computed keys fold
// to named members, so that case now lowers. Coverage moved to
// `class_module_tests`: `lowers_const_keyed_computed_class_field` (resolution) and
// `rejects_dynamic_computed_class_property_name` (`[Math.random()]`, still rejected).

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
    assert_unsupported_ts(&setter_errors, "setters are not lowered yet")?;

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
        ts!(r"
export abstract class CoreService {
  getFetchParams(params = {}): any {
    return {
      status: 'published',
      ...params,
    };
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_unannotated_async_class_methods_as_unknown_future() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Service {
  async find(params = {}) {
    return params;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn ignores_qualified_external_implements_clauses() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import type { Core } from '@strapi/types';

class SingleTypeService implements Core.CoreAPI.Service.SingleType {}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_unknown_missing_fields_on_derived_classes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Base {}

class Derived extends Base {
  read() {
    return this.externalField;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_optional_receiver_field_access_as_optional() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Config {
  pagination?: { withCount?: boolean };
}

function read(config?: Config) {
  return config?.pagination?.withCount;
}
"),
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
fn async_arrow_array_body_keeps_awaited_value_type() -> Result<(), String> {
    // Regression: an async expression-bodied arrow whose body is an array
    // literal (`async n => [n, n * 2]`) must type that body at its own
    // `List` value type. The closure's declared return type is
    // `Promise<number[]>` (`Type::Future`), but the async wrapper is what adds
    // the promise, so the returned expression itself is the awaited `List`.
    // Hinting the body at the raw future type instead used to type the array
    // literal as `Future<List<..>>`, which then emitted a
    // `Pin<Box<dyn Future>>`-annotated `vec![..]` local (E0308) in generated
    // Rust.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function flatMapAsync<T, U>(arr: T[], fn: (item: T) => Promise<U[]>): Promise<U[]> {
  const out: U[] = [];
  for (const item of arr) {
    out.push(...(await fn(item)));
  }
  return out;
}

async function main(): Promise<number[]> {
  return await flatMapAsync([1, 2, 3], async (n) => [n, n * 2]);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let main = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, main)?;

    // Locate the async callback closure and inspect its lowered body.
    let closure = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::Closure(closure) => Some(closure),
            _ => None,
        })
        .ok_or_else(|| "expected an async arrow closure in main".to_owned())?;
    // The closure's declared return type is the promise wrapper.
    ensure!(matches!(
        ctx.krate.types.get(closure.return_ty),
        Some(Type::Future(_))
    ));
    let closure_body = ctx
        .krate
        .bodies
        .get(closure.body.0 as usize)
        .ok_or_else(|| "closure body missing".to_owned())?;
    let returned = closure_body
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Return(Some(expr)) => Some(*expr),
            _ => None,
        })
        .ok_or_else(|| "closure body has no return".to_owned())?;
    let returned_ty = closure_body
        .exprs
        .get(returned.0 as usize)
        .map(|expr| expr.ty)
        .ok_or_else(|| "return expression missing".to_owned())?;
    // The body value keeps its awaited `List` type; it is not a future.
    ensure!(matches!(
        ctx.krate.types.get(returned_ty),
        Some(Type::List(_))
    ));
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

/// `expect(promise).resolves.M(...)` and `.rejects.M(...)` must emit the real
/// matcher, not an inert placeholder.
///
/// Regression guard for a measurement-integrity bug: every matcher other than
/// `rejects.toThrow` used to lower to a bare `Promise<void>` literal, so both
/// the assertion *and* the awaited call it asserted on were dropped from the
/// generated Rust and the test passed unconditionally. Under that lowering the
/// body below contained no `Await` of an actual, no `TryCatch`, and no failure
/// `Throw` at all, so the lower bounds asserted here are what separates a real
/// assertion from a deleted one. They are bounds rather than exact counts
/// because the surrounding `await` of the chain's own placeholder value is an
/// implementation detail that MIR folds away.
#[test]
fn vitest_resolves_and_rejects_matchers_emit_real_assertions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function unit(): Promise<void> {
}

async function text(): Promise<string> {
  return \"Hello\";
}

import { expect } from \"vitest\";

async function run(): Promise<void> {
  await expect(unit()).resolves.toBeUndefined();
  await expect(text()).resolves.toBe(\"Hello\");
  await expect(text()).rejects.toEqual(\"Hello\");
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let test_case = named_function_item(&ctx, module, "run")?;
    let body = function_body(&ctx, test_case)?;

    // One `await` per chain, at least: the actual has to be awaited before the
    // matcher can see the settled value.
    let awaits = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Await(_)))
        .count();
    ensure!(awaits >= 3);
    // Exactly one `try`/`catch`: the single `.rejects` chain.
    let try_catches = body
        .stmts
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::TryCatch { .. }))
        .count();
    ensure!(try_catches == 1);
    // One failure `Throw` per matcher, plus the "did not reject" guard.
    let throws = body
        .stmts
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::Throw(_)))
        .count();
    ensure!(throws >= 4);
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

/// `fetch(url)` resolves to a `Response`, not to the body text.
///
/// This test used to declare `Promise<string>` and assert `HttpGetText`, which
/// is not what `fetch` returns in any runtime — `tsc` rejects that signature.
/// A caller reads `status`, `ok`, `headers` and the body separately, so the
/// fused text operation threw away everything but one field. `HttpGetText`
/// stays in the op set for Python's `requests.get(url).text`, which really is
/// the fused operation.
#[test]
fn lowers_fetch_to_an_async_response() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function load(): Promise<Response> {
  return await fetch(\"https://example.com\");
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let load = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, load)?;

    let fetch_ty = body
        .exprs
        .iter()
        .find_map(|expr| {
            matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: smelt_hir::AsyncOp::HttpFetch,
                    ..
                }
            )
            .then_some(expr.ty)
        })
        .ok_or_else(|| "no fetch op lowered".to_owned())?;
    let Some(smelt_hir::Type::Future(inner)) = ctx.krate.types.get(fetch_ty) else {
        return Err("fetch must answer a future".to_owned());
    };
    ensure!(
        matches!(
            ctx.krate.types.get(*inner),
            Some(smelt_hir::Type::Class { .. })
        ),
        "fetch must resolve to the concrete `Response` class",
    );
    Ok(())
}

/// The body of a fetched response is read through `text()`, and it is a string.
#[test]
fn a_fetched_response_body_is_read_through_text() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function load(): Promise<string> {
  const response = await fetch(\"https://example.com\");
  return await response.text();
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let load = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, load)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::Text,
                ..
            }
        )),
        "reading a fetched body must go through the modeled `text()`",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
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
        ts!(r"
async function load(url: RequestInfo | URL, options?: RequestInit): Promise<string> {
  return await fetch(url, options);
}
"),
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
fn lowers_vi_fn_to_stateful_vitest_mock() -> Result<(), String> {
    // `vi.fn()` lowers to `ExprKind::VitestMockFn` with the erased `Unknown`
    // type: the mock is a genuine dynamic boundary (no declared shape, behavior
    // reconfigured imperatively at runtime through `mock*` methods), so no
    // concrete function type or scoped generic can represent it — its outcome
    // set (return/resolve/reject/implementation) is chosen per call at runtime.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { it, vi } from "vitest";

it("creates a mock", () => {
  const mockFn = vi.fn();
  mockFn();
});
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    let mock = body
        .exprs
        .iter()
        .find(|expr| matches!(expr.kind, ExprKind::VitestMockFn { implementation: None }))
        .ok_or("expected a VitestMockFn expression")?;
    ensure!(matches!(ctx.krate.types.get(mock.ty), Some(Type::Unknown)));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_vi_fn_with_implementation_to_stateful_vitest_mock() -> Result<(), String> {
    // `vi.fn(impl)` wraps the implementation as the mock's default outcome so
    // calls are still recorded (unlike the old passthrough, which returned the
    // implementation and lost call tracking).
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { it, vi } from "vitest";

it("creates a wrapped mock", () => {
  const double = vi.fn((value: number) => value * 2);
  double(2);
});
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::VitestMockFn {
            implementation: Some(_)
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_mock_configuration_chain_through_dynamic_calls() -> Result<(), String> {
    // A `vi.fn().mockRejectedValueOnce(..).mockResolvedValue(..)` chain must
    // lower through the generic dynamic method-call path (the chain methods are
    // runtime fields on the mock object), constructing exactly ONE mock — the
    // old interception (`mockImplementation`/`mockReturnValue` receiver
    // passthrough) must not swallow the configuration.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { it, vi } from "vitest";

it("configures a chain", () => {
  const func = vi
    .fn()
    .mockRejectedValueOnce(new Error("failure"))
    .mockResolvedValue("success");
  const spy = vi.fn().mockReturnValue(3).mockImplementation(() => 4);
  func();
  spy();
});
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    // Interceptor probing re-lowers receiver subexpressions and discards the
    // results, so `body.exprs` holds dangling duplicates; only the exprs
    // referenced by statements reach MIR (single mock construction per chain
    // is pinned on the generated Rust by the codegen snapshot test).
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::VitestMockFn { .. }))
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
        ts!(r"
export function overEvery(...xs: number[]): (v: number) => boolean {
  return function (v: number): boolean {
    let total = 0;
    for (let i = 0; i < xs.length; ++i) {
      total = total + xs[i];
    }
    return total > v;
  };
}
"),
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
        ts!(r"
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
"),
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
        ts!(r"
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
"),
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

#[test]
fn lowers_to_have_property_over_erased_class_actual() -> Result<(), String> {
    // The actual is typed by an ambient class-shaped interface with no local
    // declaration (`IArguments`), which erases to a runtime `SmeltUnknown`
    // value; `toHaveProperty` lowers to the same live key-containment check
    // used for `unknown` actuals.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
import { expect, it } from "vitest";

function toArgs(array: unknown[]): IArguments {
  return (function (..._: unknown[]) {
    return arguments;
  })(...array);
}

it("has property", () => {
  const actual = toArgs([1, 2, 3]);
  expect(actual).toHaveProperty("length");
});
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictContainsKey { .. })),
        "erased-class actual should lower to a runtime key-containment check"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_to_have_property_over_primitive_actual() -> Result<(), String> {
    // A statically primitive actual has no runtime object shape to inspect, so
    // the matcher must keep rejecting it even though erased class-shaped
    // actuals are now accepted.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
import { expect, it } from "vitest";

it("has property", () => {
  const actual = 42;
  expect(actual).toHaveProperty("length");
});
"#),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| error
            .message
            .contains("toHaveProperty(...) requires an object or map actual value")),
        "primitive actual should still be rejected by toHaveProperty"
    );
    Ok(())
}
