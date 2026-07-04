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

/// A class that is purely a string index signature (issue #84) lowers instead
/// of being rejected, and its statically known value type is recorded so keyed
/// access resolves through it. The keyed read is the honest `Optional<T>`
/// (missing key -> undefined), not an erased `SmeltUnknown`.
#[test]
fn lowers_pure_class_index_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class StringBag {
  [key: string]: string;
}

export function readBag(bag: StringBag, key: string): string | undefined {
  return bag[key];
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    // The index signature's value type is recorded for the class.
    ensure!(!ctx.class_index_values.is_empty());
    // The class carries a synthesized private `__smelt_index_store` field typed
    // `Dict<String, String>` that backs the runtime keyed store (issue #84/#18).
    let string_bag = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.names.get(class.name) == Some("StringBag") => {
                Some(class)
            }
            _ => None,
        })
        .ok_or_else(|| "missing StringBag class".to_owned())?;
    ensure!(string_bag.fields.iter().any(|field| {
        ctx.krate.symbols.get(field.name) == Some(smelt_hir::CLASS_INDEX_STORE_FIELD)
            && matches!(field.visibility, smelt_hir::Visibility::Private)
            && matches!(
                ctx.krate.types.get(field.ty),
                Some(Type::Dict(key, value))
                    if matches!(ctx.krate.types.get(*key), Some(Type::String))
                        && matches!(ctx.krate.types.get(*value), Some(Type::String))
            )
    }));
    // `bag[key]` reads the index value type as an honest `Optional<String>`.
    let read_bag = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some("read_bag") =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing read_bag function".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(read_bag.return_ty),
        Some(Type::Optional(inner))
            if matches!(ctx.krate.types.get(*inner), Some(Type::String))
    ));
    Ok(())
}

/// A mixed class keeps its declared named field concretely typed alongside the
/// index signature: named access stays concrete (`bag.size` is a number) while
/// the index value type backs access to any other key.
#[test]
fn lowers_mixed_class_index_signature_preserving_named_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class MixedBag {
  size: number = 0;
  [key: string]: string | number;
}

export function bagSize(bag: MixedBag): number {
  return bag.size;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    ensure!(!ctx.class_index_values.is_empty());
    // The class item still carries the concrete named `size: Float` field.
    let mixed = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.names.get(class.name) == Some("MixedBag") => {
                Some(class)
            }
            _ => None,
        })
        .ok_or_else(|| "missing MixedBag class".to_owned())?;
    ensure!(
        mixed
            .fields
            .iter()
            .any(|field| ctx.krate.symbols.get(field.name) == Some("size")
                && matches!(ctx.krate.types.get(field.ty), Some(Type::Float)))
    );
    // Named-field access resolves to the concrete `Float`, not the index value.
    let bag_size = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some("bag_size") =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing bag_size function".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(bag_size.return_ty),
        Some(Type::Float)
    ));
    Ok(())
}

/// Reading an undeclared named member on an index-signature class resolves
/// through the index signature's value type instead of failing as an unknown
/// class field.
#[test]
fn resolves_unnamed_member_through_class_index_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Flags {
  [key: string]: boolean;
}

export function isEnabled(flags: Flags): boolean {
  return flags.enabled;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    // `flags.enabled` names no declared field; it resolves to the index value
    // type `boolean`, so the function's return type is `Bool`.
    let is_enabled = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some("is_enabled") =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing is_enabled function".to_owned())?;
    ensure!(matches!(
        ctx.krate.types.get(is_enabled.return_ty),
        Some(Type::Bool)
    ));
    Ok(())
}

/// A class string index signature also supports keyed writes (`bag[key] = v`):
/// the assignment lowers cleanly and the emitted program validates.
#[test]
fn lowers_class_index_signature_keyed_write() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class StringBag {
  [key: string]: string;
}

export function writeBag(bag: StringBag, key: string, value: string): void {
  bag[key] = value;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    ensure!(!ctx.class_index_values.is_empty());
    Ok(())
}

/// Issue #98: a `static` method lowers to a `ClassStaticMethod`-owned function
/// (no `this` receiver) kept in the class's `static_methods`, and a `static`
/// constant becomes a materialized static field with a concrete literal value.
#[test]
fn lowers_static_method_and_static_const() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class MathUtils {
  static readonly LIMIT: number = 7;
  static square(value: number): number {
    return value * value;
  }
}

export function area(radius: number): number {
  return MathUtils.square(radius) * MathUtils.LIMIT;
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());

    let class = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(class) if ctx.krate.symbols.get(class.name) == Some("MathUtils") => {
                Some(class)
            }
            _ => None,
        })
        .ok_or("missing MathUtils class item")?;

    // The static method lives in `static_methods`, not `methods`.
    ensure_eq!(class.static_methods.len(), 1);
    let static_item = class
        .static_methods
        .first()
        .copied()
        .ok_or("missing static method item")?;
    let static_index = usize::try_from(static_item.0)
        .map_err(|err| format!("item id does not fit usize: {err}"))?;
    let Some(Item::Function(function)) = ctx.krate.items.get(static_index) else {
        return Err("static method item is not a function".to_owned());
    };
    ensure!(matches!(
        function.owner,
        smelt_hir::FunctionOwner::ClassStaticMethod { .. }
    ));
    // A static method must not carry an implicit `this` receiver.
    ensure!(
        function
            .params
            .first()
            .is_none_or(|param| ctx.krate.symbols.get(param.name) != Some("this"))
    );

    // The static constant is materialized with its concrete literal value.
    ensure_eq!(class.static_fields.len(), 1);
    let static_field = class
        .static_fields
        .first()
        .ok_or("missing static field")?;
    ensure!(matches!(
        static_field.value,
        Some(Literal::Float(value)) if (value - 7.0).abs() < f64::EPSILON
    ));
    Ok(())
}
