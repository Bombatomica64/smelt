//! Helpers for emitting Rust class storage, generics, and inheritance surfaces.
#![expect(
    clippy::redundant_pub_crate,
    reason = "class helpers are shared across sibling emitter modules"
)]

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{Mir, MirClass, MirField, MirFunction, MirInterface};

use crate::{EmitError, emitter::FunctionEmitter, generic_bindings, id_index, rust::RustIdent};

/// Return the sanitized Rust storage type name for a MIR class.
///
/// Class names can originate from TypeScript or Python identifiers, so this
/// function is the single place class storage emission applies Rust identifier
/// sanitization before combining names with generic arguments.
pub(crate) fn class_name_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    Ok(RustIdent::new(
        mir.symbols
            .get(class.name)
            .ok_or_else(|| EmitError::new("class has unknown symbol"))?,
    )
    .into_string())
}

/// Render the generic parameter declaration suffix for a class, such as `<T>`.
///
/// The returned text is empty for non-generic classes so callers can append it
/// directly after a struct, trait, or impl target name without extra branching.
pub(crate) fn class_type_params_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    if class.type_params.is_empty() {
        return Ok(String::new());
    }
    let params = class
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| RustIdent::new(name).into_string())
                .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("<{params}>"))
}

/// Render the generic parameter declaration suffix for an interface.
///
/// Interface type parameters must survive into Rust storage types because
/// function signatures can refer to instantiated interface names such as
/// `ContextOptions<SmeltUnknown>`. The returned suffix mirrors class generic
/// emission and is empty for non-generic interfaces.
pub(crate) fn interface_type_params_text(
    mir: &Mir,
    interface: &MirInterface,
) -> Result<String, EmitError> {
    if interface.type_params.is_empty() {
        return Ok(String::new());
    }
    let params = interface
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| RustIdent::new(name).into_string())
                .ok_or_else(|| EmitError::new("interface type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("<{}>", params.join(", ")))
}

/// Render bounded generic parameters for interface impl blocks.
pub(crate) fn interface_impl_generics_text(
    mir: &Mir,
    interface: &MirInterface,
) -> Result<String, EmitError> {
    if interface.type_params.is_empty() {
        return Ok(String::new());
    }
    let params = interface
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| {
                    format!(
                        "{}: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static",
                        RustIdent::new(name).into_string()
                    )
                })
                .ok_or_else(|| EmitError::new("interface type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("<{params}>"))
}

/// Render the generic argument suffix for a class, such as `<T>`.
///
/// This mirrors [`class_type_params_text`] for places where the generated Rust
/// references an already-declared class type rather than declaring parameters.
pub(crate) fn class_type_args_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    class_type_params_text(mir, class)
}

/// Return whether the class type parameter `name` is used as a map (`Dict`) key
/// in any of the class's stored fields.
///
/// A `SmeltJsMap<K, V>`'s methods (`get`, `insert`, `contains_key`, `remove`,
/// `len`, `clear`) require `K: SmeltJsKeyEq + Clone` because keys are compared
/// through the erased JS key-equality projection. When a class stores a field
/// of type `SmeltJsMap<T, ..>` keyed by its own generic parameter `T`, the impl
/// block that calls those methods must carry the `SmeltJsKeyEq` bound on `T` or
/// the generated code fails to type-check (`E0599`/`E0277` "trait bounds were
/// not satisfied"). This scans field types for `T` appearing in a `Dict` key
/// position so the bound can be inferred from field usage generally, rather
/// than special-casing any particular class.
fn class_type_param_used_as_map_key(mir: &Mir, class: &MirClass, name: Symbol) -> bool {
    class
        .fields
        .iter()
        .any(|field| type_param_in_dict_key(mir, field.ty, name))
}

/// Return whether `name` occurs in a `Dict` key position anywhere within `ty`.
///
/// Descends through value wrappers and nested shapes so a `T` used as the key
/// of a map nested inside a list/tuple/other map is still detected. Only the
/// key slot of a `Dict` counts; the value slot and non-`Dict` positions do not
/// require `SmeltJsKeyEq`.
fn type_param_in_dict_key(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            generic_bindings::type_param_occurs(mir, *key, name)
                || type_param_in_dict_key(mir, *value, name)
        }
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_param_in_dict_key(mir, *item, name)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => items
            .iter()
            .any(|item| type_param_in_dict_key(mir, *item, name)),
        Some(Type::Class { args, .. }) => args
            .iter()
            .any(|arg| type_param_in_dict_key(mir, *arg, name)),
        Some(Type::Function(function)) => {
            function
                .params
                .iter()
                .any(|param| type_param_in_dict_key(mir, *param, name))
                || type_param_in_dict_key(mir, function.return_ty, name)
        }
        Some(Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        }) => {
            type_param_in_dict_key(mir, *yield_ty, name)
                || type_param_in_dict_key(mir, *return_ty, name)
                || type_param_in_dict_key(mir, *next_ty, name)
        }
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => {
            type_param_in_dict_key(mir, *yield_ty, name)
                || type_param_in_dict_key(mir, *return_ty, name)
        }
        Some(
            Type::TypeParam { .. }
            | Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None,
        )
        | None => false,
    }
}

/// Render the generic parameter suffix used on inherent impl blocks.
///
/// The helper is intentionally separate from struct rendering because impl
/// blocks are the first place bounds may be introduced as class codegen grows.
/// A type parameter used as a map key in any field additionally gains the
/// `SmeltJsKeyEq` bound the map methods require (see
/// [`class_type_param_used_as_map_key`]).
pub(crate) fn class_impl_generics_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    if class.type_params.is_empty() {
        return Ok(String::new());
    }
    let params = class
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| {
                    let mut bound = format!(
                        "{}: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static",
                        RustIdent::new(name).into_string()
                    );
                    if class_type_param_used_as_map_key(mir, class, param.name) {
                        bound.push_str(" + SmeltJsKeyEq");
                    }
                    bound
                })
                .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("<{params}>"))
}

/// Return whether a free function's signature should emit real Rust generics.
///
/// This increment lowers a free function to real Rust generics
/// (`fn identity<T>(x: T) -> T`) only when doing so is provably sound for Rust's
/// type inference at every call site. It is an all-or-nothing decision per
/// function: if any type parameter fails a safety check, the whole function
/// falls back to full erasure (all its type parameters render as
/// `SmeltUnknown`), matching pre-#99 behavior.
///
/// A function emits real generics only when EVERY declared type parameter is:
/// - **unconstrained** — a bounded parameter (`<D extends Date>`, `<O extends {
///   ... }>`) still needs method/property dispatch through the erased boundary
///   that this increment does not model;
/// - **inferable from a plain value parameter** — the parameter must appear in a
///   *direct* (non-callback) parameter position such as `T`, `T[]`, or
///   `Option<T>`, so Rust can infer it from the argument. A type parameter used
///   only in the return type (e.g. a type predicate `data is T`) or only inside
///   a callback parameter cannot be inferred and would produce `E0283`;
/// - **absent from every callback (function-typed) parameter** — codegen
///   synthesizes default callbacks as `move |arg: SmeltUnknown| ...`, which
///   cannot unify with a generic callback signature `Fn(T) -> _` (`E0631`), so a
///   type parameter appearing inside a function-typed parameter is unsafe.
///
/// These are the deferred "bounded / higher-order / return-only" slices of #99.
pub(crate) fn function_emits_rust_generics(mir: &Mir, function: &MirFunction) -> bool {
    if function.type_params.is_empty()
        || function
            .type_params
            .iter()
            .any(|param| param.constraint.is_some())
    {
        return false;
    }

    let param_types: Vec<TypeId> = function
        .params
        .iter()
        .filter_map(|param| {
            function
                .locals
                .get(id_index(param.0, "local index does not fit usize").ok()?)
                .map(|local| local.ty)
        })
        .collect();

    let signature_safe = function.type_params.iter().all(|type_param| {
        let name = type_param.name;
        // The parameter must be inferable from at least one direct value
        // parameter position and must never appear inside a callback parameter.
        let mut inferable = false;
        for &param_ty in &param_types {
            if type_param_in_callback(mir, param_ty, name) {
                return false;
            }
            if type_param_directly_inferable(mir, param_ty, name) {
                inferable = true;
            }
        }
        inferable
    });

    signature_safe && !called_with_erased_type_param_argument(mir, function)
}

/// Return whether any call site in the crate passes an *erased* argument into a
/// position typed as one of `function`'s own type parameters.
///
/// Rust must be able to infer a unique concrete type argument at every call
/// site. When a generic free function is invoked with an argument whose static
/// type is already erased — `SmeltUnknown`, a union (also emitted as
/// `SmeltUnknown`), or a type parameter from a *different* scope — the emitted
/// argument does not pin the callee's type parameter, so monomorphization fails
/// with `E0283` ("type annotations needed"). Such a function cannot safely emit
/// real generics; it must fall back to the fully erased signature so its
/// parameter accepts the erased value directly.
///
/// This scans every function's `Call` terminators for calls that resolve to
/// `function` and checks the argument at each of its type-parameter positions.
fn called_with_erased_type_param_argument(mir: &Mir, function: &MirFunction) -> bool {
    // Parameter positions whose declared type is exactly one of the function's
    // own type parameters (`x: T`). Nested shapes (`T[]`) still bind `T` from
    // the argument's element type, so only bare-parameter positions are checked.
    let own_params: std::collections::HashSet<Symbol> =
        function.type_params.iter().map(|param| param.name).collect();
    let bare_type_param_positions: Vec<usize> = function
        .params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let ty = function
                .locals
                .get(id_index(param.0, "local index does not fit usize").ok()?)?
                .ty;
            matches!(mir.types.get(ty), Some(Type::TypeParam { name }) if own_params.contains(name))
                .then_some(index)
        })
        .collect();
    if bare_type_param_positions.is_empty() {
        return false;
    }

    for caller in &mir.functions {
        for block in &caller.blocks {
            let Some(smelt_mir::Terminator::Call {
                callee: smelt_mir::Callee::Static(target),
                args,
                ..
            }) = &block.terminator
            else {
                continue;
            };
            if *target != function.id {
                continue;
            }
            for &position in &bare_type_param_positions {
                let Some(arg) = args.get(position) else {
                    continue;
                };
                if operand_type_is_erased(mir, caller, arg) {
                    return true;
                }
            }
        }
    }
    false
}

/// Return whether an operand's static type is erased for codegen purposes
/// (`SmeltUnknown`, `Never`, a union — all emitted as `SmeltUnknown` — or a type
/// parameter that is not resolvable to a concrete type at the call site).
///
/// The place arm and the erasure test are shared with
/// [`crate::generic_bindings`] so this scan and the call-site binding matcher
/// cannot grow different notions of "erased argument". The three-valued binding
/// result collapses onto this boolean as follows:
///
/// | shared result | here | why |
/// | --- | --- | --- |
/// | resolved type | [`generic_bindings::actual_type_is_erased`] | the shared classifier |
/// | `Err(ProjectedOperand)` | erased | a field/index projection carries no type of its own |
/// | `Err(ShapeMismatch)` | erased | the caller local is missing, so nothing pins the parameter |
/// | `Err(MissingLiteralType)` | unreachable | constants short-circuit below |
///
/// The `Operand::Const` arm is deliberately *not* delegated. This scan treats
/// every literal as pinning, while `generic_bindings` resolves a real `TypeId`
/// and therefore classifies a JS symbol literal (`Type::Unknown`) as erased.
/// Widening this arm would demote functions that are generic today, so it stays
/// frozen until the increment that rewrites this predicate's caller.
fn operand_type_is_erased(mir: &Mir, caller: &MirFunction, operand: &smelt_mir::Operand) -> bool {
    // Literal constants (numbers, strings, booleans) are concrete and pin
    // the type parameter, so they never force erasure.
    if matches!(operand, smelt_mir::Operand::Const(_)) {
        return false;
    }
    match generic_bindings::operand_type(mir, caller, operand) {
        Ok(ty) => generic_bindings::actual_type_is_erased(mir, ty),
        // A missing local or a field/index projection is conservatively erased.
        Err(
            generic_bindings::BindingUnsupportedReason::ProjectedOperand
            | generic_bindings::BindingUnsupportedReason::ShapeMismatch,
        ) => true,
        Err(_) => false,
    }
}

/// Return whether `name` appears in a *direct* (non-callback) position of `ty`.
///
/// Direct positions are the value shapes Rust can infer a type argument from:
/// the bare parameter (`T`), collections and wrappers over it (`T[]`,
/// `Set<T>`, `Option<T>`, `T[]` inside a tuple, a union member, dict key/value).
/// This walk must mirror what codegen actually EMITS for a parameter type
/// (see `FunctionEmitter::rust_type`): it only descends through shapes
/// that preserve the type parameter in the emitted Rust. It intentionally does
/// NOT descend into:
/// - **`Union`** — a union parameter erases to `SmeltUnknown` in emission, so a
///   `T` inside `T | string` disappears from the emitted signature and cannot be
///   inferred (`E0283`);
/// - **`Function`** — a callback boundary, handled by [`type_param_in_callback`].
///
/// This is deliberately *narrower* than
/// [`generic_bindings::match_types`](crate::generic_bindings), which does bind
/// through `Type::Function`. That difference is the callback gate itself, so
/// this predicate is not routed through the shared walk: doing so would promote
/// every function whose only occurrence of the parameter is inside a callback.
fn type_param_directly_inferable(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::TypeParam { name: param_name }) => *param_name == name,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_param_directly_inferable(mir, *item, name)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_param_directly_inferable(mir, *key, name)
                || type_param_directly_inferable(mir, *value, name)
        }
        Some(Type::Tuple(items)) => items
            .iter()
            .any(|item| type_param_directly_inferable(mir, *item, name)),
        Some(Type::Class { args, .. }) => args
            .iter()
            .any(|arg| type_param_directly_inferable(mir, *arg, name)),
        Some(Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        }) => {
            type_param_directly_inferable(mir, *yield_ty, name)
                || type_param_directly_inferable(mir, *return_ty, name)
                || type_param_directly_inferable(mir, *next_ty, name)
        }
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => {
            type_param_directly_inferable(mir, *yield_ty, name)
                || type_param_directly_inferable(mir, *return_ty, name)
        }
        // Unions erase to `SmeltUnknown` and functions are a callback boundary,
        // so neither preserves the type parameter in a directly-inferable value
        // position.
        Some(
            Type::Union(_)
            | Type::Function(_)
            | Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None,
        )
        | None => false,
    }
}

/// Return whether `name` appears anywhere inside a function-typed sub-shape of
/// `ty` (a callback parameter or return).
///
/// Such positions are unsafe for real generics because codegen synthesizes a
/// default callback as `move |arg: SmeltUnknown| ...`, which cannot unify with a
/// generic `Fn(T) -> _` signature. The walk descends through value wrappers to
/// find a nested `Type::Function`, then checks whether `name` occurs anywhere
/// within that function type using the shared
/// [`generic_bindings::type_param_occurs`] walk.
fn type_param_in_callback(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::Function(function)) => {
            function
                .params
                .iter()
                .any(|param| generic_bindings::type_param_occurs(mir, *param, name))
                || generic_bindings::type_param_occurs(mir, function.return_ty, name)
        }
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_param_in_callback(mir, *item, name)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_param_in_callback(mir, *key, name)
                || type_param_in_callback(mir, *value, name)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => items
            .iter()
            .any(|item| type_param_in_callback(mir, *item, name)),
        Some(Type::Class { args, .. }) => args
            .iter()
            .any(|arg| type_param_in_callback(mir, *arg, name)),
        Some(Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        }) => {
            type_param_in_callback(mir, *yield_ty, name)
                || type_param_in_callback(mir, *return_ty, name)
                || type_param_in_callback(mir, *next_ty, name)
        }
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => {
            type_param_in_callback(mir, *yield_ty, name)
                || type_param_in_callback(mir, *return_ty, name)
        }
        Some(
            Type::TypeParam { .. }
            | Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None,
        )
        | None => false,
    }
}

/// Render the bounded generic-parameter suffix for a generic free function.
///
/// A generic free function such as `function identity<T>(x: T): T` is emitted
/// as `fn identity<T: ..>(x: T) -> T`. The bounds mirror generic class impl
/// blocks (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static`)
/// so a `T`-typed value can be cloned, defaulted, and cross the erased boundary
/// when it must. The returned text is empty for non-generic functions (and for
/// functions with bounded parameters, which stay erased) so callers can splice
/// it directly after the function name without branching.
pub(crate) fn function_impl_generics_text(
    mir: &Mir,
    function: &MirFunction,
) -> Result<String, EmitError> {
    if !function_emits_rust_generics(mir, function) {
        return Ok(String::new());
    }
    let params = function
        .type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| {
                    format!(
                        "{}: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static",
                        RustIdent::new(name).into_string()
                    )
                })
                .ok_or_else(|| EmitError::new("function type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("<{params}>"))
}

/// Render the Rust trait name for a class inheritance surface.
///
/// Trait names currently match the source class name because Rust permits a
/// trait and struct with the same identifier in the type namespace only when
/// separated by context is not possible, so callers should avoid emitting both
/// for the same concrete class until trait emission is expanded.
pub(crate) fn class_trait_name_text(mir: &Mir, class: &MirClass) -> Result<String, EmitError> {
    class_name_text(mir, class)
}

/// Render an owned trait-object type for a class surface.
///
/// This is reserved for base-typed polymorphic storage and parameters. Current
/// emission remains concrete unless a later lowering stage marks trait objects
/// as required.
#[expect(
    dead_code,
    reason = "trait-object lowering is only emitted for later polymorphic cases"
)]
pub(crate) fn class_trait_object_type_text(
    mir: &Mir,
    class: &MirClass,
) -> Result<String, EmitError> {
    Ok(format!(
        "Box<dyn {}{}>",
        class_trait_name_text(mir, class)?,
        class_type_args_text(mir, class)?
    ))
}

/// Return the flattened field layout for a class.
///
/// Smelt stores subclass values with inherited fields first and own fields
/// after them. This helper follows the single-inheritance chain and leaves type
/// substitution to earlier lowering phases until MIR grows canonical layout
/// substitution metadata. When a subclass redeclares a field from its base
/// class, the subclass field replaces the inherited slot so Rust struct storage
/// stays valid and matches the effective source member surface.
pub(crate) fn effective_class_fields(mir: &Mir, class: &MirClass) -> Vec<MirField> {
    let mut fields = class
        .base
        .and_then(|base| mir.classes.iter().find(|candidate| candidate.name == base))
        .map(|base| effective_class_fields(mir, base))
        .unwrap_or_default();
    for field in &class.fields {
        if let Some(existing) = fields
            .iter_mut()
            .find(|candidate| candidate.name == field.name)
        {
            *existing = field.clone();
        } else {
            fields.push(field.clone());
        }
    }
    fields
}

/// Render a primitive class-level value captured by specialization.
///
/// Static members are emitted as associated getter functions because owned
/// values such as `String` cannot be Rust `const` items.
pub(crate) fn materialized_static_value_text(literal: Option<&smelt_hir::Literal>) -> String {
    match literal {
        Some(smelt_hir::Literal::Bool(value)) => value.to_string(),
        Some(smelt_hir::Literal::Int(value)) => value.to_string(),
        Some(smelt_hir::Literal::Float(value)) if value.is_nan() => "f64::NAN".to_owned(),
        Some(smelt_hir::Literal::Float(value))
            if value.is_infinite() && value.is_sign_positive() =>
        {
            "f64::INFINITY".to_owned()
        }
        Some(smelt_hir::Literal::Float(value)) if value.is_infinite() => {
            "f64::NEG_INFINITY".to_owned()
        }
        Some(smelt_hir::Literal::Float(value)) => format!("{value:?}"),
        Some(smelt_hir::Literal::String(value)) => format!("{value:?}.to_owned()"),
        Some(
            smelt_hir::Literal::Symbol(_)
            | smelt_hir::Literal::Undefined
            | smelt_hir::Literal::None,
        )
        | None => "Default::default()".to_owned(),
    }
}

/// Return the Rust-valid field layout for an interface.
///
/// TypeScript interface inheritance and utility-type expansion can present the
/// same source property more than once. Rust structs cannot contain duplicate
/// field identifiers, so codegen keeps the last field for each sanitized Rust
/// name. This mirrors source member lookup where later, more specific
/// declarations describe the effective surface while keeping generated storage
/// valid.
pub(crate) fn effective_interface_fields(mir: &Mir, interface: &MirInterface) -> Vec<MirField> {
    let mut fields = Vec::new();
    for field in &interface.fields {
        let field_name = mir
            .symbols
            .get(field.name)
            .map(RustIdent::new)
            .map_or_else(|| "field".to_owned(), RustIdent::into_string);
        if let Some(existing) = fields.iter_mut().find(|candidate: &&mut MirField| {
            mir.symbols
                .get(candidate.name)
                .map(RustIdent::new)
                .map_or_else(|| "field".to_owned(), RustIdent::into_string)
                == field_name
        }) {
            *existing = field.clone();
        } else {
            fields.push(field.clone());
        }
    }
    fields
}

/// Return inherited abstract method signatures required by a class.
///
/// The list walks base classes first so generated trait surfaces have stable,
/// deterministic ordering that matches the flattened field layout.
pub(crate) fn inherited_trait_methods(mir: &Mir, class: &MirClass) -> Vec<smelt_hir::MethodSig> {
    let mut methods = class
        .base
        .and_then(|base| mir.classes.iter().find(|candidate| candidate.name == base))
        .map(|base| inherited_trait_methods(mir, base))
        .unwrap_or_default();
    methods.extend(class.abstract_methods.clone());
    methods
}

/// Render a HIR class type with its Rust generic arguments.
#[expect(
    dead_code,
    reason = "standalone class type rendering is reserved for trait objects"
)]
pub(crate) fn class_type_text(
    mir: &Mir,
    class_name: Symbol,
    class_args: &[TypeId],
) -> Result<String, EmitError> {
    let name_text = RustIdent::new(
        mir.symbols
            .get(class_name)
            .ok_or_else(|| EmitError::new("class type has unknown symbol"))?,
    )
    .into_string();
    if class_args.is_empty() {
        return Ok(name_text);
    }
    let args_text = class_args
        .iter()
        .map(|arg| FunctionEmitter::type_text_for(mir, *arg))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("{name_text}<{args_text}>"))
}
