//! Tests for JavaScript constructor-function (`function Foo(){}` +
//! `Foo.prototype.x = …` + `new Foo()` / `instanceof Foo`) lowering and for the
//! default-parameter scoping fix that lets a later parameter default reference
//! an earlier parameter.

use super::*;
use smelt_hir::Class;

/// Find a synthesized class by its source name in the lowered crate.
fn class_by_name<'a>(ctx: &'a HirCtx, name: &str) -> Option<&'a Class> {
    ctx.krate.items.iter().find_map(|item| match item {
        Item::Class(class) if ctx.krate.symbols.get(class.name) == Some(name) => Some(class),
        _ => None,
    })
}

#[test]
fn function_constructor_with_new_synthesizes_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('ctor', () => {
  it('constructs', () => {
    function Foo(this: any) {
      this.a = 1;
    }
    Foo.prototype.b = function () {
      return 2;
    };
    const value = new Foo();
    expect(value).toBeDefined();
  });
});
"),
        &mut ctx,
    )?;
    let class = class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo`")?;
    ensure!(
        class.fields.iter().any(|field| {
            ctx.krate.symbols.get(field.name) == Some("a")
        }),
        "constructor `this.a = 1` should become an own field `a`",
    );
    ensure!(
        class.constructor.is_some(),
        "synthesized class should have a constructor",
    );
    ensure!(
        !class.methods.is_empty(),
        "prototype method `b` should become a class method",
    );
    Ok(())
}

#[test]
fn function_constructor_instanceof_resolves() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('ctor', () => {
  it('checks instanceof', () => {
    function Foo(this: any) {
      this.a = 1;
    }
    const value = new Foo();
    expect(value instanceof Foo).toBe(true);
  });
});
"),
        &mut ctx,
    )?;
    let class = class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo`")?;
    ensure!(
        ctx.krate.symbols.get(class.name) == Some("Foo"),
        "instanceof target should resolve to the synthesized class",
    );
    Ok(())
}

#[test]
fn const_function_expression_constructor_synthesizes_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('ctor', () => {
  it('constructs from a const function expression', () => {
    const Foo = function (this: any) {
      this.a = 1;
    } as unknown as { new (): any };
    Foo.prototype.b = function () {
      return 2;
    };
    const value = new Foo();
    expect(value instanceof Foo).toBe(true);
  });
});
"),
        &mut ctx,
    )?;
    let class = class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo`")?;
    ensure!(
        class.fields.iter().any(|field| {
            ctx.krate.symbols.get(field.name) == Some("a")
        }),
        "const-function constructor `this.a = 1` should become an own field `a`",
    );
    Ok(())
}

#[test]
fn describe_scoped_constructor_used_in_nested_it() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // The constructor function and its prototype assignment live in the
    // `describe` body, while `new Foo()` lives in a nested `it` callback.
    lower_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('ctor', () => {
  function Foo(this: any) {
    this.a = 1;
  }
  Foo.prototype.b = function () {
    return 2;
  };

  it('constructs in a nested callback', () => {
    const value = new Foo();
    expect(value).toBeDefined();
  });
});
"),
        &mut ctx,
    )?;
    class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo` from describe scope")?;
    Ok(())
}

#[test]
fn plain_function_without_new_is_not_a_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // A `function` that is only ever called normally must stay a function, not
    // be promoted to a class.
    lower_ok(
        ts!(r"
export function helper(value: number): number {
  return value + 1;
}
"),
        &mut ctx,
    )?;
    ensure!(
        class_by_name(&ctx, "helper").is_none(),
        "a normally-called function must not become a class",
    );
    Ok(())
}

#[test]
fn later_parameter_default_references_earlier_parameter() -> Result<(), String> {
    // `end = array.length` references the earlier `array` parameter; the
    // declaration-hoisting prepass must register earlier parameters as locals
    // before inferring later parameter defaults.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function fillRange<T>(array: T[], value: T, start = 0, end = array.length): T[] {
  for (let i = start; i < end; i++) {
    array[i] = value;
  }
  return array;
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn unannotated_constructor_parameter_defaults_to_unknown() -> Result<(), String> {
    // `function Foo(object) { Object.assign(this, object); }` used with `new`
    // is a constructor function whose single parameter carries no annotation.
    // A synthesized constructor's own fields are all `unknown`, so the parameter
    // that flows into `this` is the same dynamic boundary and defaults to
    // `unknown` (es-toolkit's `merge` spec spells the identical shape
    // `object: any`). Ordinary untyped functions still require an annotation.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('nonplain', () => {
  it('constructs from an object', () => {
    function Foo(object) {
      Object.assign(this, object);
    }
    const object = new Foo({ a: new Foo({ b: 1, c: 2 }) });
    expect((object as any).a.b).toBe(1);
  });
});
"),
        &mut ctx,
    )?;
    let class = class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo`")?;
    ensure!(
        class.constructor.is_some(),
        "unannotated constructor function should still synthesize a class",
    );
    Ok(())
}

#[test]
fn plain_unannotated_function_parameter_still_rejected() -> Result<(), String> {
    // The unknown-parameter fallback is scoped to constructor functions. A plain
    // function declaration that is never used as a `new` target keeps requiring
    // explicit annotations, so this must still fail to lower.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
export function plain(object) {
  return object;
}
"),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| error
            .message
            .contains("explicit type annotations or default initializers")),
        "plain untyped function parameter should still be rejected: {errors:?}",
    );
    Ok(())
}

#[test]
fn sibling_it_blocks_synthesize_independent_constructor_classes() -> Result<(), String> {
    // A `function Foo` synthesized as a class in one `it` block must not leak
    // into a sibling block that declares a differently-shaped `function Foo`.
    // The class registry is scoped per test case, so the second block
    // re-synthesizes its own `Foo` (whose unannotated constructor parameter
    // would otherwise fall back to the plain-function path and fail to lower).
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('scoping', () => {
  it('inherited constructor', () => {
    function Foo() {}
    Foo.prototype.b = 2;
    const object = { a: new Foo() };
    expect((object as any).a).toBeDefined();
  });

  it('non-plain constructor', () => {
    function Foo(object) {
      Object.assign(this, object);
    }
    const object = new Foo({ a: new Foo({ b: 1, c: 2 }) });
    expect((object as any).a.b).toBe(1);
  });
});
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn module_scope_constructor_function_synthesizes_class() -> Result<(), String> {
    // Module top level is where real JavaScript writes the idiom, and it hides
    // the `new` site from a sibling-statement scan: the construction happens
    // inside *another* top-level function's body. Before module-scope detection
    // this module failed to lower with "unresolved class `Foo`" because `Foo` was
    // predeclared as a plain function item instead.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function Foo(this: any) {
  this.a = 1;
}
Foo.prototype.b = function () {
  return 2;
};
export function make(): unknown {
  return new Foo();
}
"),
        &mut ctx,
    )?;
    let class = class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo`")?;
    ensure!(
        class
            .fields
            .iter()
            .any(|field| ctx.krate.symbols.get(field.name) == Some("a")),
        "module-scope constructor `this.a = 1` should become an own field `a`",
    );
    ensure!(
        class.constructor.is_some(),
        "module-scope synthesized class should have a constructor",
    );
    ensure!(
        !class.methods.is_empty(),
        "module-scope prototype method `b` should become a class method",
    );
    Ok(())
}

#[test]
fn exported_module_scope_constructor_function_synthesizes_class() -> Result<(), String> {
    // The `export function Foo(){}` spelling reaches the declaration through an
    // `ExportNamedDeclaration` wrapper, which the shared statement scan does not
    // traverse. The synthesized class is what the module exports under the name.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function Foo(this: any) {
  this.a = 1;
}
export function make(): unknown {
  return new Foo();
}
"),
        &mut ctx,
    )?;
    let class = class_by_name(&ctx, "Foo").ok_or("expected synthesized class `Foo`")?;
    ensure!(
        class
            .fields
            .iter()
            .any(|field| ctx.krate.symbols.get(field.name) == Some("a")),
        "exported constructor `this.a = 1` should become an own field `a`",
    );
    Ok(())
}

#[test]
fn module_scope_plain_function_stays_a_function() -> Result<(), String> {
    // The detection prepass must not turn every module function into a class:
    // a function that is only ever *called* keeps its function item, so its
    // return value and call sites lower normally.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function helper(value: number): number {
  return value + 1;
}
export function use(): number {
  return helper(1);
}
"),
        &mut ctx,
    )?;
    ensure!(
        class_by_name(&ctx, "helper").is_none(),
        "a module function that is only called must not become a synthesized class",
    );
    Ok(())
}
