//! A module namespace used as a value (`lowering::namespace_value`).

use super::*;

/// The struct a namespace value lowers to, found among the crate's interfaces.
fn namespace_struct<'a>(ctx: &'a HirCtx, name: &str) -> Result<&'a smelt_hir::Interface, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Interface(interface) if ctx.krate.names.get(interface.name) == Some(name) => {
                Some(interface)
            }
            _ => None,
        })
        .ok_or_else(|| format!("missing namespace struct `{name}`"))
}

/// Requirement: `NS` passed as a value is a struct of the module's VALUE
/// exports, one field per export in sorted order, each typed by the export
/// itself (no `Unknown`); a type export is no field.
#[test]
fn a_namespace_value_is_a_struct_of_the_value_exports_in_sorted_order() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
export interface Shape { side: number }
export const unit = 2;
export function area(side: number): number { return side * side; }
export const label = 'sq';
"),
        "shapes.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r"
import * as Shapes from './shapes';
function describe(ns: typeof Shapes): string { return ns.label; }
export const out = describe(Shapes);
"),
        "main.ts",
        &mut ctx,
    )?;
    let shapes = namespace_struct(&ctx, "ShapesNamespace")?;
    let names = shapes
        .fields
        .iter()
        .map(|field| ctx.krate.symbols.get(field.name).unwrap_or("?"))
        .collect::<Vec<_>>();
    ensure_eq!(names, vec!["area", "label", "unit"]);
    ensure!(
        shapes
            .fields
            .iter()
            .all(|field| !matches!(ctx.krate.types.get(field.ty), Some(Type::Unknown))),
        "a namespace field erased: {:?}",
        shapes.fields
    );
    Ok(())
}

/// Requirement: a namespace value of a module exporting a reassigned `let`
/// is refused, naming the binding: a copy would not see the reassignment.
#[test]
fn a_namespace_value_over_a_reassigned_export_let_is_refused_by_name() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
export let counter = 0;
export function bump(): void { counter = counter + 1; }
"),
        "counter.ts",
        &mut ctx,
    )?;
    let errors = match to_hir_with_path(
        ts!(r"
import * as Counter from './counter';
export const keys = Object.keys(Counter);
"),
        FileId(1),
        "main.ts",
        &mut ctx,
    ) {
        Ok(module) => return Err(format!("expected a refusal, got module {module:?}")),
        Err(errors) => errors,
    };
    ensure!(
        errors
            .iter()
            .any(|error| format!("{error:?}").contains("`counter`")),
        "the refusal must name the binding: {errors:?}"
    );
    Ok(())
}

/// Requirement: a member read of a namespace whose module exports a
/// reassigned `let` stays a live read (only the VALUE use is refused).
#[test]
fn a_member_read_of_an_export_let_through_a_namespace_still_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
export let counter = 0;
export function bump(): void { counter = counter + 1; }
"),
        "counter.ts",
        &mut ctx,
    )?;
    lower_path_ok(
        ts!(r"
import * as Counter from './counter';
Counter.bump();
export const now = Counter.counter;
"),
        "main.ts",
        &mut ctx,
    )?;
    Ok(())
}
