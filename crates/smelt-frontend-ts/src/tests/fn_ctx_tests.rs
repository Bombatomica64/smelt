//! Regression coverage for the per-body lowering context (`state::fn_ctx`).
//!
//! Every nested body must start with its own statement sink and postfix-update
//! mode. Before `FnCtx`, each body entry point saved and restored its own subset
//! of the context by hand, and the class-member entry points saved neither: a
//! class lowered while an enclosing statement was filling a block of the OUTER
//! body pushed the stores it synthesized (a parameter default's `a++`, a field
//! initializer's `this.a++`) at that outer `BlockId` — an out-of-bounds panic
//! in `Body::push_stmt_to_block`, or a store silently landing in the wrong
//! block. The end-to-end golden `186_nested_body_statement_sinks` checks the
//! same programs' runtime output against Node.

use super::*;

/// Lower `source`, requiring success and a structurally valid crate.
fn lower_valid(source: &str) -> Result<HirCtx, String> {
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    let problems = smelt_hir::validate(&ctx.krate);
    ensure!(problems.is_empty(), "HIR validation failed: {problems:?}");
    Ok(ctx)
}

/// An id's index into its body's arena (`usize::MAX`, so a lookup misses,
/// when it does not fit).
fn index(id: u32) -> usize {
    usize::try_from(id).unwrap_or(usize::MAX)
}

/// Whether some body assigns `name` (an update's store) in any of its blocks.
fn some_body_assigns_local_named(ctx: &HirCtx, name: &str) -> bool {
    ctx.krate.bodies.iter().any(|body| {
        body.blocks.iter().flat_map(|block| &block.stmts).any(|stmt| {
            let Some(Stmt::Assign { target, .. }) = body.stmts.get(index(stmt.0)) else {
                return false;
            };
            let Some(ExprKind::Local(local)) =
                body.exprs.get(index(target.0)).map(|expr| &expr.kind)
            else {
                return false;
            };
            body.locals
                .get(index(local.0))
                .and_then(|decl| decl.name)
                .and_then(|symbol| ctx.krate.symbols.get(symbol))
                == Some(name)
        })
    })
}

#[test]
fn method_parameter_default_update_stays_in_the_method_body() -> Result<(), String> {
    let ctx = lower_valid(ts!(r"
function outer(n: number): number {
  if (n > 0) {
    class K {
      get(a: number, x: number = a++): number { return a + x; }
    }
    return new K().get(n);
  }
  return 0;
}
console.log(outer(3));
"))?;
    ensure!(
        some_body_assigns_local_named(&ctx, "a"),
        "the default's `a++` store must be emitted into the method body"
    );
    Ok(())
}

#[test]
fn constructor_parameter_default_update_stays_in_the_constructor_body() -> Result<(), String> {
    let ctx = lower_valid(ts!(r"
function outer(n: number): number {
  if (n > 0) {
    class Pair {
      v: number;
      constructor(a: number, x: number = a++) { this.v = a * 10 + x; }
    }
    return new Pair(n).v;
  }
  return 0;
}
console.log(outer(2));
"))?;
    ensure!(
        some_body_assigns_local_named(&ctx, "a"),
        "the default's `a++` store must be emitted into the constructor body"
    );
    Ok(())
}

#[test]
fn field_initializer_update_stays_in_the_synthesized_constructor() -> Result<(), String> {
    let ctx = lower_valid(ts!(r"
function outer(n: number): number {
  if (n > 0) {
    class Counter {
      a: number = 3;
      b: number = this.a++;
    }
    const counter = new Counter();
    return counter.a * 10 + counter.b;
  }
  return 0;
}
console.log(outer(1));
"))?;
    ensure!(
        crate_has_field(&ctx, "a") && crate_has_new(&ctx, "Counter"),
        "the initializer reads `this.a` inside the synthesized constructor"
    );
    Ok(())
}

#[test]
fn method_default_inside_a_loop_body_block() -> Result<(), String> {
    let ctx = lower_valid(ts!(r"
function outer(n: number): number {
  let total = 0;
  for (let r = 0; r < n; r++) {
    class Scaled {
      get(a: number, x: number = a++): number { return a * 10 + x; }
    }
    total += new Scaled().get(r);
  }
  return total;
}
console.log(outer(3));
"))?;
    ensure!(
        some_body_assigns_local_named(&ctx, "a"),
        "the default's `a++` store must be emitted into the method body"
    );
    Ok(())
}

#[test]
fn class_expression_in_a_deferring_declaration_keeps_its_own_updates() -> Result<(), String> {
    // `g = i++` opens a postfix deferral for the declaration; the class
    // expression's method body must not append its `a++` to it.
    let ctx = lower_valid(ts!(r"
function outer(n: number): number {
  let i = n;
  const g = i++, C = class { m(a: number, x: number = a++): number { return a * 10 + x; } };
  return g * 1000 + new C().m(i) + i;
}
console.log(outer(3));
"))?;
    ensure!(some_body_assigns_local_named(&ctx, "a"));
    ensure!(some_body_assigns_local_named(&ctx, "i"));
    Ok(())
}

#[test]
fn method_does_not_read_the_enclosing_functions_arguments() -> Result<(), String> {
    // A method has its own `arguments` object, which Smelt does not model, so
    // reading it is rejected instead of reinterpreting the method's own
    // parameters through the enclosing function's arity.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
function outer(n: number): number {
  class K {
    m(b: number): number { return arguments.length + b; }
  }
  return new K().m(n);
}
console.log(outer(1));
"),
        &mut ctx,
    )?;
    ensure!(
        errors
            .iter()
            .any(|error| format!("{error:?}").contains("`arguments` is only available")),
        "unexpected diagnostics: {errors:?}"
    );
    Ok(())
}

/// Whether every body that declares a local named `name` also assigns it.
///
/// A store lowered into the wrong body indexes that body's expressions with
/// another body's ids, so it never reads back as an assignment of `name` in the
/// body that owns the local.
fn every_declaring_body_assigns(ctx: &HirCtx, name: &str) -> bool {
    let declaring = ctx.krate.bodies.iter().filter(|body| {
        body.locals.iter().any(|decl| {
            decl.name.and_then(|symbol| ctx.krate.symbols.get(symbol)) == Some(name)
        })
    });
    let mut any = false;
    for body in declaring {
        any = true;
        let assigns = body.blocks.iter().flat_map(|block| &block.stmts).any(|stmt| {
            let Some(Stmt::Assign { target, .. }) = body.stmts.get(index(stmt.0)) else {
                return false;
            };
            let Some(ExprKind::Local(local)) =
                body.exprs.get(index(target.0)).map(|expr| &expr.kind)
            else {
                return false;
            };
            body.locals
                .get(index(local.0))
                .and_then(|decl| decl.name)
                .and_then(|symbol| ctx.krate.symbols.get(symbol))
                == Some(name)
        });
        if !assigns {
            return false;
        }
    }
    any
}

#[test]
fn closures_in_a_deferring_initializer_keep_their_own_updates() -> Result<(), String> {
    // `const v = i++ + …` defers `i++`'s store past the declaration. The
    // arrow's and the function expression's own `a++` / `b++` must store
    // inside their own bodies, not join that deferral list (which would flush
    // them into the enclosing body). Node prints `26 1`.
    let ctx = lower_valid(ts!(r"
function outer(): string {
  let i = 0;
  const v =
    i++ +
    ((a: number): number => { return a++ + a; })(5) +
    (function (b: number): number { return b++ + b; })(7);
  return `${v} ${i}`;
}
console.log(outer());
"))?;
    ensure!(every_declaring_body_assigns(&ctx, "a"), "`a++` left the arrow body");
    ensure!(every_declaring_body_assigns(&ctx, "b"), "`b++` left the function body");
    ensure!(every_declaring_body_assigns(&ctx, "i"));
    Ok(())
}
