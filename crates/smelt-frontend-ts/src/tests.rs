//! Tests for TypeScript frontend lowering.

use super::*;
use smelt_hir::{ExprKind, FileId, Item, Stmt, StringCaseOp, Type};

#[test]
fn converts_top_level_let_and_console_log() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "let x = 6;
console.log(x);
",
        FileId(0),
        &mut ctx,
    )
    .expect("valid HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    assert_eq!(body.locals.len(), 1);
    assert_eq!(body.stmts.len(), 2);
    assert_eq!(body.exprs.len(), 4);
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn lowers_stdlib_length_properties() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        r#"
const values: number[] = [1, 2, 3];
const count = values.length;
const word = "smelt";
const letters = word.length;
"#,
        FileId(0),
        &mut ctx,
    )
    .expect("stdlib length properties should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    let len_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
        .count();
    assert_eq!(len_count, 2);
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn lowers_string_index_and_for_of() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        r#"
const word = "abc";
const first = word[0];
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#,
        FileId(0),
        &mut ctx,
    )
    .expect("string index and for...of should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    assert!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. }))
    );
    assert!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn lowers_string_case_methods() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        r#"
const word = "Smelt";
const lower = word.toLowerCase();
const upper = word.toUpperCase();
"#,
        FileId(0),
        &mut ctx,
    )
    .expect("string case methods should lower");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    assert!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Lower,
            ..
        }
    )));
    assert!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Upper,
            ..
        }
    )));
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn rejects_unknown_identifier() {
    let mut ctx = HirCtx::new();
    let errors = to_hir("console.log(x);", FileId(0), &mut ctx).expect_err("unknown x");
    assert_eq!(errors[0].code, "smelt::unsupported-ts");
    assert!(errors[0].message.contains("unresolved identifier"));
}

#[test]
fn formats_compact_hir() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "let count = 42;
console.log(count);
",
        FileId(0),
        &mut ctx,
    )
    .expect("valid HIR");

    let output = smelt_hir::format_compact(&ctx.krate, &[("sample.ts".to_owned(), module_id)]);

    assert_eq!(
        output,
        "module sample.ts (ModuleId(0))\n  body BodyId(0)\n  locals\n    %0 let count: Float\n  exprs\n    #0: Float = 42.0\n    #1: Float = %0\n    #2: None = @0(console_log)\n    #3: None = call #2(#1)\n  stmts\n    s0: let %0: Float = #0\n    s1: #3\n\ninterned types\n  t0 = Float\n  t1 = None\n"
    );
}

#[test]
fn normalizes_camel_case() {
    assert_eq!(camel_to_snake("myFunction"), "my_function");
    assert_eq!(camel_to_snake("URLParser"), "url_parser");
    assert_eq!(camel_to_snake("IPAddr"), "ip_addr");
    assert_eq!(camel_to_snake("_internal"), "_internal");
}

#[test]
fn lowers_function_declaration_and_direct_call() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
",
        FileId(0),
        &mut ctx,
    )
    .expect("valid HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];

    assert_eq!(module.items.len(), 1);
    assert_eq!(ctx.krate.items.len(), 2);
    assert_eq!(ctx.krate.bodies.len(), 2);
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn lowers_if_else_while_and_for_of_to_hir() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "let count = 0;
if (count < 10) {
  console.log(count);
} else {
  console.log(count);
}
while (count < 10) {
  break;
}
for (let item: number of count) {
  continue;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("valid HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    assert!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. }))
    );
    assert!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::While { .. }))
    );
    assert!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn lowers_try_catch_finally_to_hir() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "try {
  throw 'x';
} catch (error) {
  console.log(error);
} finally {
  console.log('done');
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("valid HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

    let Some(Stmt::TryCatch {
        body: try_body,
        catch_binding: Some(_),
        catch_body: Some(catch_body),
        finally_body: Some(finally_body),
    }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::TryCatch { .. }))
    else {
        panic!("expected try/catch/finally to lower to HIR");
    };
    assert!(
        body.blocks[try_body.0 as usize]
            .stmts
            .iter()
            .any(|stmt| matches!(body.stmts[stmt.0 as usize], Stmt::Throw(_)))
    );
    assert!(!body.blocks[catch_body.0 as usize].stmts.is_empty());
    assert!(!body.blocks[finally_body.0 as usize].stmts.is_empty());
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn rejects_missing_implemented_interface_field() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "interface Named { name: string; }
class User implements Named {
  constructor() {}
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("missing field");
    assert_eq!(errors[0].code, "smelt::unsupported-ts");
    assert!(errors[0].span.end >= errors[0].span.start);
    assert!(errors[0].message.contains("field `name`"));
}

#[test]
fn rejects_implemented_method_signature_mismatch() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "interface Named { label(prefix: string): string; }
class User implements Named {
  label(prefix: number): string { return \"x\"; }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("mismatch");
    assert_eq!(errors[0].code, "smelt::unsupported-ts");
    assert!(errors[0].span.end >= errors[0].span.start);
    assert!(errors[0].message.contains("mismatched signature"));
}

#[test]
fn lowers_interface_inheritance_into_shape_requirements() {
    let mut ctx = HirCtx::new();
    to_hir(
        "interface Entity { id: string; }
interface Named extends Entity { name: string; }
class User implements Named {
  id: string;
  name: string;
  constructor(id: string, name: string) {
    this.id = id;
    this.name = name;
  }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("interface inheritance should flatten into implements checks");
}

#[test]
fn rejects_missing_inherited_interface_field() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "interface Entity { id: string; }
interface Named extends Entity { name: string; }
class User implements Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("missing inherited field should be rejected");
    assert_eq!(errors[0].code, "smelt::unsupported-ts");
    assert!(errors[0].message.contains("field `id`"));
}

#[test]
fn lowers_literal_computed_property_names() {
    let mut ctx = HirCtx::new();
    to_hir(
        "interface Entity { [\"id\"]: string; }
class User implements Entity {
  [\"id\"]: string;
  constructor(id: string) {
    this.id = id;
  }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("literal computed property names should lower as static fields");
}

#[test]
fn rejects_dynamic_computed_property_names() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "const key = \"id\";
class User {
  [key]: string;
  constructor() {}
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("dynamic computed property names should be rejected");
    assert_eq!(errors[0].code, "smelt::unsupported-ts");
    assert!(errors[0].message.contains("dynamic computed property"));
}

#[test]
fn optional_interface_fields_may_be_absent() {
    let mut ctx = HirCtx::new();
    to_hir(
        "interface Named { name?: string; }
class User implements Named {
  constructor() {}
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("optional interface field may be absent on implementing class");
}

#[test]
fn required_fields_satisfy_optional_interface_fields() {
    let mut ctx = HirCtx::new();
    to_hir(
        "interface Named { name?: string; }
class User implements Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("required class field should satisfy optional interface field");
}

#[test]
fn rejects_optional_class_fields() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "class User {
  name?: string;
  constructor() {}
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("optional class fields require construction semantics");
    assert_eq!(errors[0].code, "smelt::unsupported-ts");
    assert!(errors[0].message.contains("optional class fields"));
}

#[test]
fn rejects_generic_classes_and_interfaces() {
    let mut ctx = HirCtx::new();
    let class_errors = to_hir(
        "class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("generic classes are deferred");
    assert_eq!(class_errors[0].code, "smelt::unsupported-ts");
    assert!(class_errors[0].message.contains("generic classes"));

    let mut ctx = HirCtx::new();
    let interface_errors = to_hir(
        "interface Box<T> {
  value: T;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("generic interfaces are deferred");
    assert_eq!(interface_errors[0].code, "smelt::unsupported-ts");
    assert!(interface_errors[0].message.contains("generic interfaces"));
}

#[test]
fn rejects_static_members() {
    let mut ctx = HirCtx::new();
    let field_errors = to_hir(
        "class User {
  static role: string;
  constructor() {}
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("static fields are unsupported");
    assert_eq!(field_errors[0].code, "smelt::unsupported-ts");
    assert!(field_errors[0].message.contains("static fields"));

    let mut ctx = HirCtx::new();
    let method_errors = to_hir(
        "class User {
  static role(): string { return \"admin\"; }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("static methods are unsupported");
    assert_eq!(method_errors[0].code, "smelt::unsupported-ts");
    assert!(method_errors[0].message.contains("static methods"));
}

#[test]
fn rejects_getters_setters_decorators_and_abstract_classes() {
    let mut ctx = HirCtx::new();
    let getter_errors = to_hir(
        "class User {
  get name(): string { return \"Ada\"; }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("getters are unsupported");
    assert_eq!(getter_errors[0].code, "smelt::unsupported-ts");
    assert!(getter_errors[0].message.contains("getters and setters"));

    let mut ctx = HirCtx::new();
    let decorator_errors = to_hir(
        "@sealed
class User {
  constructor() {}
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("decorators are unsupported");
    assert_eq!(decorator_errors[0].code, "smelt::unsupported-ts");
    assert!(decorator_errors[0].message.contains("decorators"));

    let mut ctx = HirCtx::new();
    let abstract_errors = to_hir(
        "abstract class User {
  abstract name(): string;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("abstract classes are unsupported");
    assert_eq!(abstract_errors[0].code, "smelt::unsupported-ts");
    assert!(abstract_errors[0].message.contains("abstract classes"));
}

#[test]
fn lowers_literal_switch_to_hir_match() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
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
",
        FileId(0),
        &mut ctx,
    )
    .expect("valid HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let Item::Function(function) = &ctx.krate.items[module.items[0].0 as usize] else {
        panic!("expected function item");
    };
    let body = &ctx.krate.bodies[function.body.expect("function body").0 as usize];

    let Some(Stmt::Match { arms, default, .. }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Match { .. }))
    else {
        panic!("expected switch to lower to HIR match");
    };
    assert_eq!(arms.len(), 3);
    assert!(default.is_none());
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn rejects_coercive_equality() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "function same(a: number, b: number): boolean {
  return a == b;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("coercive equality is unsupported");

    assert!(errors[0].message.contains("coercive equality"));
}

#[test]
fn rejects_untyped_for_of_binding() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "let values = 1;
for (let item of values) {
  continue;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("for-of binding must be typed");

    assert!(errors[0].message.contains("explicit type annotations"));
}

#[test]
fn lowers_async_functions_and_await_to_hir() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "async function load(value: number): Promise<number> {
  return value;
}

async function main(): Promise<number> {
  return await load(1);
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("async functions should lower to HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];

    assert_eq!(module.items.len(), 2);
    let Item::Function(load) = &ctx.krate.items[module.items[0].0 as usize] else {
        panic!("expected function item");
    };
    assert!(load.is_async);
    assert!(matches!(
        ctx.krate.types.get(load.return_ty),
        Some(Type::Future(_))
    ));

    let Item::Function(main) = &ctx.krate.items[module.items[1].0 as usize] else {
        panic!("expected function item");
    };
    let body = &ctx.krate.bodies[main.body.expect("main body").0 as usize];
    assert!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_)))
    );
    let machine = body
        .async_state_machine
        .as_ref()
        .expect("async body should have state-machine metadata");
    assert_eq!(machine.states.len(), 2);
    assert_eq!(machine.suspensions.len(), 1);
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn lowers_promise_all_to_async_runtime_op() {
    let mut ctx = HirCtx::new();
    let module_id = to_hir(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function main(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("Promise.all should lower to HIR");
    let module = &ctx.krate.modules[module_id.0 as usize];
    let Item::Function(main) = &ctx.krate.items[module.items[1].0 as usize] else {
        panic!("expected function item");
    };
    let body = &ctx.krate.bodies[main.body.expect("main body").0 as usize];

    assert!(body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: smelt_hir::AsyncOp::All,
                ..
            }
        )
    }));
}

#[test]
fn lowers_promise_race_all_settled_and_timer_shim() {
    let mut ctx = HirCtx::new();
    to_hir(
        "async function lift(value: number): Promise<number> {
  await setTimeout(0);
  return value;
}

async function race(): Promise<number> {
  return await Promise.race([lift(1), lift(2)]);
}

async function settled(): Promise<[number, number]> {
  return await Promise.allSettled([lift(1), lift(2)]);
}
",
        FileId(0),
        &mut ctx,
    )
    .expect("Promise race/allSettled and timer shim should lower");
}

#[test]
fn rejects_await_outside_async_function() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "function read(): number {
  return await 1;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("await outside async should fail");

    assert!(errors[0].message.contains("await"));
}

#[test]
fn rejects_async_functions_without_promise_return_type() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "async function load(): number {
  return 1;
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("async functions should require Promise<T> return types");

    assert!(errors[0].message.contains("Promise<T>"));
}

#[test]
fn rejects_switch_fallthrough_until_it_is_modeled() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "function label(status: \"pending\" | \"approved\"): string {
  switch (status) {
    case \"pending\":
      const waiting = \"waiting\";
    case \"approved\":
      return \"Approved\";
  }
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("switch fallthrough is unsupported");

    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("switch fallthrough")),
        "expected switch fallthrough error, got {errors:?}"
    );
}

#[test]
fn lowers_template_literal_to_string_concat() {
    let mut ctx = HirCtx::new();
    let _module_id = to_hir(
        "const name: string = \"world\";\nconst msg: string = `Hello ${name}!`;",
        FileId(0),
        &mut ctx,
    )
    .expect("template literal should lower");
    assert!(smelt_hir::validate(&ctx.krate).is_empty());
}

#[test]
fn accepts_import_and_export_declarations() {
    let mut ctx = HirCtx::new();
    let _module_id = to_hir(
        "import { foo } from './foo';\nexport function bar(): number { return 1; }",
        FileId(0),
        &mut ctx,
    )
    .expect("import and export should not crash");
}
