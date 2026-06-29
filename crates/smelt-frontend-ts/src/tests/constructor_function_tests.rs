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
        ts!(r#"
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
"#),
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
        ts!(r#"
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
"#),
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
        ts!(r#"
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
"#),
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
        ts!(r#"
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
"#),
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
        ts!(r#"
export function helper(value: number): number {
  return value + 1;
}
"#),
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
        ts!(r#"
export function fillRange<T>(array: T[], value: T, start = 0, end = array.length): T[] {
  for (let i = start; i < end; i++) {
    array[i] = value;
  }
  return array;
}
"#),
        &mut ctx,
    )?;
    Ok(())
}
