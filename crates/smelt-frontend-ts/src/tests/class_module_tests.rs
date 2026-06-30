//! Regression tests for class- and module-level language features:
//! private class fields, `this`-parameter function types, bare `asserts`
//! signatures, interface heritage over non-interface parents, and numeric
//! property keys.

use super::*;

#[test]
fn lowers_private_class_field_read_and_write() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Counter {
  #count: number;
  constructor(start: number) {
    this.#count = start;
  }
  next(): number {
    this.#count = this.#count + 1;
    return this.#count;
  }
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_this_parameter_function_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
type Handler = (this: number, value: number) => number;

function run(handler: Handler, value: number): number {
  return handler(value);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_bare_asserts_condition_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function invariant(condition: unknown, message: string): asserts condition {
  if (condition) {
    return;
  }
  throw new Error(message);
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_interface_extending_global_lib_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // `Array`/`ArrayLike` are ambient global lib types with no user interface
    // declaration; the heritage must not block lowering.
    let module_id = lower_ok(
        ts!(r#"
export interface RecursiveArray<T> extends Array<T | RecursiveArray<T>> {}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_numeric_property_keys() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export interface ByIndex {
  0: number;
  1: number;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
