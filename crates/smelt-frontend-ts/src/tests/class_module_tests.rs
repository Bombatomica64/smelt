//! Regression tests for class- and module-level language features:
//! private class fields, `this`-parameter function types, bare `asserts`
//! signatures, interface heritage over non-interface parents, and numeric
//! property keys.

use super::*;

#[test]
fn lowers_private_class_field_read_and_write() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
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
"),
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
        ts!(r"
type Handler = (this: number, value: number) => number;

function run(handler: Handler, value: number): number {
  return handler(value);
}
"),
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
        ts!(r"
export function invariant(condition: unknown, message: string): asserts condition {
  if (condition) {
    return;
  }
  throw new Error(message);
}
"),
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
        ts!(r"
export interface RecursiveArray<T> extends Array<T | RecursiveArray<T>> {}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_generic_class_with_type_param_methods() -> Result<(), String> {
    // Issue #99: a generic class carries its declared type parameters into HIR,
    // and a call to a method whose declared return type is the class parameter
    // resolves to the receiver's concrete argument (`Container<number>::get()`
    // is `number`), so the whole program lowers and validates.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class Container<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
  set(value: T): void { this.value = value; }
}

export function useContainer(): number {
  const b = new Container<number>(3);
  b.set(5);
  return b.get();
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());

    // The lowered class retains exactly one declared type parameter.
    let container = ctx.krate.items.iter().find_map(|item| match item {
        Item::Class(class)
            if ctx.krate.symbols.get(class.name) == Some("Container") =>
        {
            Some(class)
        }
        _ => None,
    });
    let container = container.ok_or_else(|| "Container class not lowered".to_owned())?;
    let type_param = container
        .type_params
        .first()
        .ok_or_else(|| "Container has no type parameter".to_owned())?;
    ensure_eq!(container.type_params.len(), 1);
    ensure_eq!(ctx.krate.symbols.get(type_param.name), Some("T"));
    Ok(())
}

#[test]
fn lowers_generic_free_function_with_type_params() -> Result<(), String> {
    // Issue #99: a generic free function retains its declared type parameters on
    // the HIR `Function` item (they were previously dropped at
    // `function_declaration_named`), so MIR and codegen can emit real Rust
    // generics instead of erasing `T` to `SmeltUnknown`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function identity<T>(x: T): T {
  return x;
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());

    let identity = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some("identity") =>
            {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "identity function not lowered".to_owned())?;
    ensure_eq!(identity.type_params.len(), 1);
    let type_param = identity
        .type_params
        .first()
        .ok_or_else(|| "identity has no type parameter".to_owned())?;
    ensure_eq!(ctx.krate.symbols.get(type_param.name), Some("T"));
    Ok(())
}

#[test]
fn lowers_numeric_property_keys() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export interface ByIndex {
  0: number;
  1: number;
}
"),
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
        ts!(r"
class StringBag {
  [key: string]: string;
}

export function readBag(bag: StringBag, key: string): string | undefined {
  return bag[key];
}
"),
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
        ts!(r"
class MixedBag {
  size: number = 0;
  [key: string]: string | number;
}

export function bagSize(bag: MixedBag): number {
  return bag.size;
}
"),
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
        ts!(r"
class Flags {
  [key: string]: boolean;
}

export function isEnabled(flags: Flags): boolean {
  return flags.enabled;
}
"),
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
        ts!(r"
export interface Seq {
  [Symbol.iterator](): number;
}
"),
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
        ts!(r"
class Dynamic {
  [Math.random()]: number = 0;
}
"),
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
        ts!(r"
class StringBag {
  [key: string]: string;
}

export function writeBag(bag: StringBag, key: string, value: string): void {
  bag[key] = value;
}
"),
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

/// Issue #114 (follow-up to #83/#84): dotted access to an *undeclared* member of
/// a class that carries a string index signature resolves through the index
/// signature's value type, not the erased `Unknown` boundary. `bag.anything`
/// is a keyed store read whose static type is the index value `string`, so the
/// function that returns it type-checks concretely. This is the statically
/// resolvable case the blocker used to reject; it must lower without error and
/// keep its concrete type.
#[test]
fn resolves_dot_access_on_class_index_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class StringBag {
  [key: string]: string;
}
export function readBag(bag: StringBag): string {
  return bag.anything;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    // The undeclared dotted read stays concretely typed as the index value
    // (`string`), never widened to `Unknown`.
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
        Some(Type::String)
    ));
    Ok(())
}

/// Issue #114: dotted access to an undeclared member of a class whose base type
/// carries an index signature resolves through the *inherited* index value
/// type. `derived.dynamic` finds no named field on `Derived` and no index
/// signature on `Derived` itself, but the base `Bag`'s string index signature
/// supplies the value type. The subclass access must not hard-error.
#[test]
fn resolves_dot_access_through_inherited_index_signature() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class Bag {
  [key: string]: string;
}
class Derived extends Bag {
  size: number = 0;
}
export function readDynamic(derived: Derived): string {
  return derived.dynamic;
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Issue #114: an undeclared dotted read on a concrete class with neither an
/// index signature nor a resolvable base no longer aborts lowering with the
/// `unknown class or interface field` blocker. Static resolution (declared
/// fields, methods, index signatures, base/interface heritage, builtins) is
/// exhausted, so the access routes to the explicit `Unknown` dynamic boundary
/// (mirroring how interface receivers and `id`-named reads were already handled
/// through ad-hoc escape hatches that this general rule replaces). In valid
/// TypeScript this only type-checks for widened/dynamic receivers or — as under
/// isolated per-file lowering — a receiver whose full member set is not visible
/// here; either way the honest lowering is the boundary, not a hard error. This
/// is not `SmeltUnknown` *widening* of statically-typed access: every static
/// resolver is tried first, so the declared field `x` keeps its concrete type.
#[test]
fn routes_unresolved_class_field_to_dynamic_boundary_without_error() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
class Point {
  x: number = 0;
}
export function readDynamic(p: Point): unknown {
  return p.y;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    // Lowering succeeds (no `unknown class or interface field` abort); the
    // declared field `x` is untouched and stays concrete `Float`.
    let point = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.names.get(class.name) == Some("Point") => Some(class),
            _ => None,
        })
        .ok_or_else(|| "missing Point class".to_owned())?;
    ensure!(point.fields.iter().any(|field| {
        ctx.krate.symbols.get(field.name) == Some("x")
            && matches!(ctx.krate.types.get(field.ty), Some(Type::Float))
    }));
    Ok(())
}

/// A well-known `[Symbol.asyncIterator]` interface method resolves to a stable
/// synthetic member spelling and lowers to a named method rather than hitting
/// the "property names must be static" gate (issue #115, neverthrow residual).
#[test]
fn lowers_symbol_async_iterator_computed_interface_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export interface Seq {
  [Symbol.asyncIterator](): number;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let seq = interface_named(&ctx, module, "Seq")?;
    ensure!(
        seq.methods.iter().any(|method| ctx.krate.symbols.get(method.name)
            == Some("__smelt_symbol_async_iterator")),
        "expected a `__smelt_symbol_async_iterator` method, got {:?}",
        seq.methods
    );
    Ok(())
}

/// A `Symbol.for(<literal>)`-aliased const used as a computed interface method
/// key (`const matcher = Symbol.for("k"); { [matcher](): T }`) folds to the
/// registry synthetic member spelling (issue #115, ts-pattern residual). This is
/// the const-alias shape ts-pattern uses for its `[matcher]()` protocol methods.
#[test]
fn lowers_symbol_for_const_keyed_interface_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const matcher = Symbol.for('@ts-pattern/matcher');

export interface Matcher {
  [matcher](): number;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let matcher = interface_named(&ctx, module, "Matcher")?;
    ensure!(
        matcher.methods.iter().any(|method| ctx.krate.symbols.get(method.name)
            == Some("__smelt_symbol_for_ts_pattern_matcher")),
        "expected a `__smelt_symbol_for_ts_pattern_matcher` method, got {:?}",
        matcher.methods
    );
    Ok(())
}

/// An inline `[Symbol.for("k")]` computed interface field folds to the same
/// registry synthetic member spelling as a `Symbol.for`-aliased const, since
/// registry symbols are interned by description (issue #115).
#[test]
fn lowers_inline_symbol_for_computed_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export interface Branded {
  [Symbol.for('@ts-pattern/override')]: number;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let branded = interface_named(&ctx, module, "Branded")?;
    ensure!(
        branded.fields.iter().any(|field| ctx.krate.symbols.get(field.name)
            == Some("__smelt_symbol_for_ts_pattern_override")),
        "expected a `__smelt_symbol_for_ts_pattern_override` field, got {:?}",
        branded.fields
    );
    Ok(())
}

/// A `Symbol.for(<literal>)`-aliased const referenced through a namespace import
/// (`import * as symbols; { [symbols.override]: T }`) folds to the registry
/// synthetic member spelling — the exact shape ts-pattern's `Pattern.ts` uses
/// (issue #115). The registry const is declared and re-exported by an earlier
/// module so it resolves through the cross-module const machinery.
#[test]
fn lowers_namespace_symbol_for_computed_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
export const override = Symbol.for('@ts-pattern/override');
"),
        "symbols.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r"
import * as symbols from './symbols';

export interface Override {
  [symbols.override]: number;
}
"),
        "Pattern.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let override_iface = interface_named(&ctx, module, "Override")?;
    ensure!(
        override_iface.fields.iter().any(|field| ctx.krate.symbols.get(field.name)
            == Some("__smelt_symbol_for_ts_pattern_override")),
        "expected a `__smelt_symbol_for_ts_pattern_override` field, got {:?}",
        override_iface.fields
    );
    Ok(())
}

/// A *unique* `Symbol("desc")` (no `.for`) aliased to a const has fresh identity
/// each evaluation and is not a stable static key, so using it as a computed
/// property name still reports the dynamic-key diagnostic (issue #115 folds only
/// globally-interned registry symbols, not unique brands).
#[test]
fn rejects_unique_symbol_const_computed_property_name() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
const brand = Symbol('brand');

class Branded {
  [brand]: number = 0;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(
        &errors,
        "dynamic computed property names are not lowered yet",
    )?;
    Ok(())
}

/// A class extending a modeled JavaScript host constructor (`Blob`, `File`, the
/// boxed primitive wrappers, …) resolves its base through the shared
/// `smelt_stdlib::host_object` registry, exactly as extending a builtin error or
/// collection type does. es-toolkit's `isBlob`/`isFile` specs declare
/// `class File extends Blob {}` to fake the host `File` global, which previously
/// failed with "base class `Blob` is not declared".
#[test]
fn lowers_class_extending_host_object_base() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export class MyFile extends Blob {
  name: string;
  constructor(chunks: unknown[], filename: string) {
    super(chunks);
    this.name = filename;
  }
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    // The lowered class records `Blob` as its resolved base symbol.
    let my_file = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.symbols.get(class.name) == Some("MyFile") => {
                Some(class)
            }
            _ => None,
        })
        .ok_or_else(|| "missing MyFile class".to_owned())?;
    let base = my_file
        .base
        .ok_or_else(|| "MyFile class has no resolved base".to_owned())?;
    ensure_eq!(ctx.krate.symbols.get(base), Some("Blob"));
    Ok(())
}

/// A class extending a genuinely unknown identifier still reports the honest
/// blocker, so the host-object allowance does not silently accept every base.
#[test]
fn rejects_class_extending_undeclared_base() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
export class Widget extends NotARealBaseClass {}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "base class `NotARealBaseClass` is not declared")?;
    Ok(())
}

/// `class X extends Object {}` names the universal root constructor as its base.
/// Since every class already descends from `Object`, an explicit `extends Object`
/// contributes nothing and must lower as a base-less class rather than demanding
/// an `Object` class item Smelt does not synthesize. es-toolkit's
/// `isPlainObject` spec reaches `new (class extends Object {})()`.
#[test]
fn lowers_class_extending_object_as_empty_base() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export class Tagged extends Object {
  label = "x";
  count = 3;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let tagged = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.symbols.get(class.name) == Some("Tagged") => {
                Some(class)
            }
            _ => None,
        })
        .ok_or_else(|| "missing Tagged class".to_owned())?;
    ensure!(
        tagged.base.is_none(),
        "`extends Object` should lower to no declared base, got {:?}",
        tagged.base
    );
    Ok(())
}

/// A class declared inside a function body (e.g. a test's `describe` callback)
/// is lowered inline without a forward-declaration pass, so it is not yet
/// registered in the class table while its own method bodies are lowered. A
/// `this`-typed return/annotation on one of those methods must still resolve to
/// the enclosing class type instead of failing with "this class type is not
/// resolvable yet". es-toolkit's `memoize` spec declares such a class with an
/// `override clear(): this` returning `new ImmutableCache() as this`.
#[test]
fn lowers_this_type_in_class_declared_in_function_body() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function build(): number {
  class Node {
    value: number;
    constructor(v: number) {
      this.value = v;
    }
    self(): this {
      return this;
    }
    rebuild(): this {
      return new Node(this.value) as this;
    }
  }
  const node = new Node(4);
  node.self();
  node.rebuild();
  return node.value;
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A static field initialized to a function or arrow expression
/// (`static c = function () {};`) is a static callable member, not a data
/// constant. Materializing it as an associated function needs the static-method
/// lowering path, which is not wired to property initializers yet; until then
/// the field is skipped rather than blocking the whole class, so a class that
/// merely carries such a member still lowers (es-toolkit's `cloneDeep` spec
/// declares `class Foo { static c = function () {}; }`).
#[test]
fn lowers_class_carrying_static_function_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export class Widget {
  a = 1;
  b = 2;
  static make = function () {};
  static build = () => 42;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let widget = module
        .items
        .iter()
        .find_map(|item| match ctx.krate.items.get(item.0 as usize)? {
            Item::Class(class) if ctx.krate.symbols.get(class.name) == Some("Widget") => {
                Some(class)
            }
            _ => None,
        })
        .ok_or_else(|| "missing Widget class".to_owned())?;
    // The instance data fields survive; the function-valued statics are not
    // materialized as static fields (they are skipped, not stored as data).
    ensure!(
        widget.fields.len() == 2,
        "expected two instance fields, got {}",
        widget.fields.len()
    );
    ensure!(
        widget.static_fields.is_empty(),
        "function-valued static fields should not be materialized as data static fields, got {}",
        widget.static_fields.len()
    );
    Ok(())
}

/// TypeScript class overload declarations contribute no runtime body; after
/// TypeScript validates calls, only the concrete implementation becomes HIR.
#[test]
fn lowers_class_method_overloads_through_their_implementation() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export class Formatter {
  static format(value: string): string;
  static format(value: number): string;
  static format(value: string | number): string {
    return String(value);
  }

  parse(value: string): string;
  parse(value: number): string;
  parse(value: string | number): string {
    return String(value);
  }
}
"),
        &mut ctx,
    )?;

    let format_count = ctx
        .krate
        .items
        .iter()
        .filter(|item| {
            matches!(
                item,
                Item::Function(function)
                    if ctx.krate.symbols.get(function.name) == Some("format")
            )
        })
        .count();
    let parse_count = ctx
        .krate
        .items
        .iter()
        .filter(|item| {
            matches!(
                item,
                Item::Function(function)
                    if ctx.krate.symbols.get(function.name) == Some("parse")
            )
        })
        .count();

    ensure!(
        format_count == 1,
        "expected one static implementation, got {format_count}"
    );
    ensure!(
        parse_count == 1,
        "expected one instance implementation, got {parse_count}"
    );
    Ok(())
}

/// Async generator methods retain typed suspension points and must not require
/// a `Promise<T>` annotation.
#[test]
fn lowers_async_generator_class_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export class Values {
  async *items(): AsyncGenerator<number, void> {
    yield 1;
  }
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    ensure_eq!(module.items.len(), 1);
    let function = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some("items") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "missing async generator class method".to_owned())?;
    let body = function_body(&ctx, function)?;
    ensure!(body.async_state_machine.is_some());
    ensure!(body.is_generator);
    ensure!(matches!(
        ctx.krate.types.get(function.return_ty),
        Some(Type::Generator { is_async: true, .. })
    ));
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::GeneratorYield { .. })),
        "yield should remain a resumable suspension point"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Nested generator declarations retain suspension metadata independently of
/// the surrounding generator method.
#[test]
fn isolates_nested_generator_declaration_from_generator_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export class Values {
  *items(): Generator<number> {
    function* inner(): Generator<number> {
      yield 2;
    }
    yield 1;
  }
}
"),
        &mut ctx,
    )?;
    let generator_bodies = ctx
        .krate
        .bodies
        .iter()
        .filter(|body| {
            body.is_generator
                && body
                    .exprs
                    .iter()
                    .any(|expr| matches!(expr.kind, ExprKind::GeneratorYield { .. }))
        })
        .count();
    ensure_eq!(generator_bodies, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A class is in its own lexical scope while its method bodies are evaluated,
/// including as the right-hand constructor of `instanceof`.
#[test]
fn lowers_instanceof_against_class_under_construction() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export class ResultBox {
  isResultBox(value: unknown): boolean {
    return value instanceof ResultBox;
  }
}
"),
        &mut ctx,
    )?;

    ensure!(
        ctx.krate.bodies.iter().any(|body| body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::InstanceOf { .. }))),
        "self-referential instanceof should retain a nominal class check"
    );
    Ok(())
}

/// Builtin method recognizers must inspect the receiver before enforcing the
/// builtin arity; a user class may legitimately define the same method name.
#[test]
fn lowers_class_match_method_without_string_builtin_dispatch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export class ResultBox {
  match(ok: (value: number) => number, err: (value: string) => number): number {
    return ok(1);
  }
}

export function run(box: ResultBox): number {
  return box.match((value: number) => value, (_error: string) => 0);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A union of user classes remains outside String builtin dispatch even when
/// every arm exposes a compatible `.match(...)` method.
#[test]
fn lowers_union_class_match_method_without_string_builtin_dispatch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export class OkBox {
  match(ok: (value: number) => number, _err: (value: string) => number): number {
    return ok(1);
  }
}

export class ErrBox {
  match(_ok: (value: number) => number, err: (value: string) => number): number {
    return err('bad');
  }
}

export function run(box: OkBox | ErrBox): number {
  return box.match((value: number) => value, (_error: string) => 0);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Constructor, factory, field, and instance-method class receivers must all
/// bypass String builtin dispatch regardless of their expression shape.
#[test]
fn lowers_expression_class_match_methods_without_string_builtin_dispatch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export class ResultBox {
  match(ok: (value: number) => number, _err: (value: string) => number): number {
    return ok(1);
  }
}

export class Holder {
  result: ResultBox;
  constructor(result: ResultBox) {
    this.result = result;
  }
  getResult(): ResultBox {
    return this.result;
  }
}

export function makeResult(): ResultBox {
  return new ResultBox();
}

export function run(holder: Holder): number {
  const fromConstructor = new ResultBox().match(
    (value: number) => value,
    (_error: string) => 0,
  );
  const fromFactory = makeResult().match(
    (value: number) => value + fromConstructor,
    (_error: string) => 0,
  );
  const fromField = holder.result.match(
    (value: number) => value + fromFactory,
    (_error: string) => 0,
  );
  return holder.getResult().match(
    (value: number) => value + fromField,
    (_error: string) => 0,
  );
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
