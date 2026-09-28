//! Regression coverage for `this`-parameter functions used as methods, the
//! callback-table shadowing rule, and `toThrow`'s expected-error argument.

use super::*;

/// Whether any body in the crate contains an expression matching `pred`.
fn any_expr(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

/// Whether some `try/catch` binds the caught value.
fn any_bound_catch(ctx: &HirCtx) -> bool {
    ctx.krate.bodies.iter().any(|body| {
        body.stmts.iter().any(|stmt| {
            matches!(
                stmt,
                Stmt::TryCatch {
                    catch_binding: Some(_),
                    ..
                }
            )
        })
    })
}

#[test]
fn callable_field_satisfies_interface_method() -> Result<(), String> {
    // TypeScript accepts a field holding a function where an implemented
    // interface declares a method; Smelt used to reject the class outright.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Router {
  match(path: string): string;
}
function match<R extends Router>(this: R, path: string): string {
  return path + '!';
}
class R1 implements Router {
  match: typeof match = match;
}
console.log(new R1().match('a'));
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn non_function_binding_shadows_another_bodys_callback() -> Result<(), String> {
    // `m` in `g` is the destructured string, not `f`'s arrow: `g`'s body must
    // not rebuild that arrow as a closure where it reads `m`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function f(): string {
  const m = (a: string): string => a + '!';
  return m('x');
}
function g(xs: [string, string][]): string {
  let out = '';
  for (const [m, p] of xs) {
    out = out + m + p;
  }
  return out;
}
console.log(f(), g([['a', 'b']]));
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let g = named_function_item(&ctx, module, "g")?;
    let body = function_body(&ctx, g)?;
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Closure(_))),
        "`g` must read its own `m`, not rebuild `f`'s arrow"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn to_throw_string_compares_the_thrown_message() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

function fail(): void {
  throw new Error("boom here");
}

test("throws", () => {
  expect(() => fail()).toThrow("boom");
});
"#);
    let mut ctx = HirCtx::new();
    lower_path_ok(source, "src/to-throw-message.test.ts", &mut ctx)?;
    ensure!(any_bound_catch(&ctx), "the caught value must be bound");
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::StringContains { .. })),
        "a string argument is a substring test on the thrown message"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn to_throw_regexp_tests_the_thrown_message() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

function fail(): void {
  throw new Error("boom here");
}

test("throws", () => {
  expect(() => fail()).toThrowError(/bo+m/);
});
"#);
    let mut ctx = HirCtx::new();
    lower_path_ok(source, "src/to-throw-regexp.test.ts", &mut ctx)?;
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::RegexTest { .. })),
        "a RegExp argument tests the thrown message"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn to_throw_class_is_an_instanceof_test() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

class MyError extends Error {}

function fail(): void {
  throw new MyError("boom");
}

test("throws", () => {
  expect(() => fail()).toThrow(MyError);
});
"#);
    let mut ctx = HirCtx::new();
    lower_path_ok(source, "src/to-throw-class.test.ts", &mut ctx)?;
    let my_error = ctx.krate.symbols.intern("MyError");
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::InstanceOf { class, .. } if *class == my_error)),
        "a class argument is an instanceof test on the thrown value"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn to_throw_without_argument_accepts_any_throw() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

function fail(): void {
  throw new Error("boom");
}

test("throws", () => {
  expect(() => fail()).toThrow();
});
"#);
    let mut ctx = HirCtx::new();
    lower_path_ok(source, "src/to-throw-any.test.ts", &mut ctx)?;
    ensure!(
        !any_expr(&ctx, |kind| matches!(
            kind,
            ExprKind::StringContains { .. } | ExprKind::RegexTest { .. } | ExprKind::InstanceOf { .. }
        )),
        "no argument means no comparison"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn push_on_guarded_optional_field_is_kept() -> Result<(), String> {
    // `if (!this.items) throw ..; this.items.push(x)`: the receiver keeps its
    // declared optional type at the push, which used to lower to `none` and
    // drop the mutation.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Bag {
  items?: string[] = [];
  add(x: string): void {
    if (!this.items) {
      throw new Error('sealed');
    }
    this.items.push(x);
  }
}
const b = new Bag();
b.add('x');
console.log(b.items?.length);
"),
        &mut ctx,
    )?;
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::ListPush { .. })),
        "the push must survive as a list mutation"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
