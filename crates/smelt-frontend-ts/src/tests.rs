//! Tests for TypeScript frontend lowering.

use super::*;
use smelt_hir::{FileId, Stmt};

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
    let smelt_hir::Item::Function(function) = &ctx.krate.items[module.items[0].0 as usize] else {
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
fn rejects_async_functions_until_async_lowering_exists() {
    let mut ctx = HirCtx::new();
    let errors = to_hir(
        "async function load(): string {
  return \"done\";
}
",
        FileId(0),
        &mut ctx,
    )
    .expect_err("async functions are unsupported");

    assert!(errors[0].message.contains("async functions"));
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
