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

/// Find a class item by source name in a lowered module.
fn class_named<'a>(
    ctx: &'a HirCtx,
    module: &'a smelt_hir::Module,
    name: &str,
) -> Result<&'a smelt_hir::Class, String> {
    module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.names.get(class.name) == Some(name) => Some(class),
            _ => None,
        })
        .ok_or_else(|| format!("missing class `{name}`"))
}

/// Find an interface item by source name in a lowered module.
fn interface_named<'a>(
    ctx: &'a HirCtx,
    module: &'a smelt_hir::Module,
    name: &str,
) -> Result<&'a smelt_hir::Interface, String> {
    module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Interface(interface) if ctx.krate.names.get(interface.name) == Some(name) => {
                Some(interface)
            }
            _ => None,
        })
        .ok_or_else(|| format!("missing interface `{name}`"))
}

/// A `const`-keyed computed interface field (`{ [KEY]: T }`) resolves to the
/// const's string value as a static named member (issue #96), instead of being
/// rejected as a dynamic property name.
#[test]
fn lowers_const_keyed_computed_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const KEY = "id";

export interface Keyed {
  [KEY]: number;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let keyed = interface_named(&ctx, module, "Keyed")?;
    ensure!(
        keyed
            .fields
            .iter()
            .any(|field| ctx.krate.symbols.get(field.name) == Some("id")
                && matches!(ctx.krate.types.get(field.ty), Some(Type::Float))),
        "expected const-keyed field `id: Float`, got {:?}",
        keyed.fields
    );
    Ok(())
}

/// An enum-member-keyed computed interface field (`{ [E.A]: T }`) folds to the
/// enum member's string value as a static named member (issue #96).
#[test]
fn lowers_enum_keyed_computed_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
enum Kind {
  First = "first",
}

export interface ByKind {
  [Kind.First]: number;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let by_kind = interface_named(&ctx, module, "ByKind")?;
    ensure!(
        by_kind
            .fields
            .iter()
            .any(|field| ctx.krate.symbols.get(field.name) == Some("first")
                && matches!(ctx.krate.types.get(field.ty), Some(Type::Float))),
        "expected enum-keyed field `first: Float`, got {:?}",
        by_kind.fields
    );
    Ok(())
}

/// A `const`-keyed computed class field resolves to the const's string value as
/// a static named class field (issue #96).
#[test]
fn lowers_const_keyed_computed_class_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const TAG = "tag";

class Node {
  [TAG]: string = "leaf";
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let node = class_named(&ctx, module, "Node")?;
    ensure!(
        node.fields
            .iter()
            .any(|field| ctx.krate.symbols.get(field.name) == Some("tag")
                && matches!(ctx.krate.types.get(field.ty), Some(Type::String))),
        "expected const-keyed class field `tag: String`, got {:?}",
        node.fields
    );
    Ok(())
}

/// A well-known `[Symbol.iterator]` interface method resolves to the stable
/// synthetic member spelling so it lowers to a named method (issue #96) rather
/// than being silently dropped or rejected as a dynamic key.
#[test]
fn lowers_symbol_iterator_computed_interface_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export interface Seq {
  [Symbol.iterator](): number;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let seq = interface_named(&ctx, module, "Seq")?;
    ensure!(
        seq.methods
            .iter()
            .any(|method| ctx.krate.symbols.get(method.name)
                == Some("__smelt_symbol_iterator")),
        "expected a `__smelt_symbol_iterator` method, got {:?}",
        seq.methods
    );
    Ok(())
}

/// A genuinely dynamic computed class property name (a runtime call) is not a
/// statically-resolvable key, so it still reports the dynamic-property-name
/// diagnostic (issue #96 folds only static keys; it does not silence honest
/// dynamic ones).
#[test]
fn rejects_dynamic_computed_class_property_name() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let class_errors = lowering_errors(
        ts!(r#"
class Dynamic {
  [Math.random()]: number = 0;
}
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(
        &class_errors,
        "dynamic computed property names are not lowered yet",
    )?;
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
