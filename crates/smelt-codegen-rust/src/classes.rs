//! Helpers for emitting Rust class storage, generics, and inheritance surfaces.
#![expect(
    clippy::redundant_pub_crate,
    reason = "class helpers are shared across sibling emitter modules"
)]

use std::collections::HashSet;

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{FuncId, LocalId, Mir, MirClass, MirField, MirFunction, MirInterface};

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
///   a callback parameter cannot be inferred and would produce `E0283`.
///
/// A type parameter that appears inside a callback parameter is *no longer*
/// rejected outright. That gate (Increment 1 of
/// `blocker-logs/estk-callback-generics-plan.md`) existed because
/// `param_type_text` rendered a callback's parameter halves in an empty
/// type-parameter scope, so a callee whose signature said `Fn(T)` was handed
/// closures declared `|arg: SmeltUnknown|`. The scope, not Rust inference, was
/// the defect; both halves of a callback type now render in the callee's own
/// lexical scope, and call-site adapters render the callee's declared callback
/// under the bindings that call site pinned.
///
/// That relaxation is bounded by §4.4 of the plan: only a **direct, required,
/// borrowed, non-rest** `Type::Function` parameter can carry a type parameter
/// into the emitted signature and still have every call site render an argument
/// that matches it. See [`callback_occurrences_are_liftable`] for the rule and
/// for why each excluded shape is excluded.
///
/// `owned_callback_params` is the emitter's own ownership fixpoint
/// (`emitter::compute_owned_callback_params`), threaded in rather than
/// recomputed so the gate and the renderer cannot grow two different notions of
/// "owned callback".
///
/// These are the deferred "bounded / higher-order / return-only" slices of #99.
pub(crate) fn function_emits_rust_generics(
    mir: &Mir,
    function: &MirFunction,
    owned_callback_params: &HashSet<(FuncId, LocalId)>,
) -> bool {
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
        // parameter position. Increment 3 of the callback-generics plan is what
        // relaxes this to inference *through* a callback; until then a type
        // parameter reachable only through a callback still demotes the
        // function, because a `dyn Fn` argument position is an unsize coercion
        // rather than an inference source.
        param_types
            .iter()
            .any(|&param_ty| type_param_directly_inferable(mir, param_ty, name))
            // ... and every callback position it *also* occupies must be one the
            // renderer can express under Increment 1's `&dyn Fn(T, ..)`
            // representation (§4.4).
            && callback_occurrences_are_liftable(mir, function, owned_callback_params, name)
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
    let own_params: HashSet<Symbol> =
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
/// The walk descends through value wrappers to find a nested `Type::Function`,
/// then checks whether `name` occurs anywhere within that function type using
/// the shared [`generic_bindings::type_param_occurs`] walk.
///
/// This used to be the callback gate itself — any occurrence demoted the
/// function. Increment 1 of the callback-generics plan deleted that
/// all-or-nothing early return; the walk stayed, and is now the occurrence half
/// of the §4.4 eligibility rule implemented by
/// [`callback_occurrences_are_liftable`]: it answers "does `name` reach a
/// function-typed sub-shape of `ty` at all", and the caller decides whether the
/// position that occurrence sits in is one the renderer can express.
///
/// Increment 3 additionally turns this into
/// `type_param_inferable_through_callback` by restricting it to positions Rust
/// can *infer* from — a callback parameter or return, but not a `T` buried in a
/// union inside a callback parameter, since a union erases and never appears in
/// the emitted `Fn` bound. That restriction is deferred: it only matters once a
/// callback position becomes an inference source. Here the walk is used as a
/// *demotion* trigger, so being deliberately wide (it descends unions, tuples,
/// class arguments and generator shapes) is the conservative direction.
pub(crate) fn type_param_in_callback(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
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

/// Return whether every callback occurrence of `name` in `function`'s parameter
/// list sits in a position the current callback representation can express.
///
/// §4.4 of `blocker-logs/estk-callback-generics-plan.md` scopes callback-borne
/// generics to a **direct, required, borrowed, non-rest** `Type::Function`
/// parameter. A parameter that mentions `name` inside a function type in any
/// other shape demotes the whole function to erasure, because the callee
/// signature and the call-site argument would be rendered by two different
/// paths that cannot agree:
///
/// * **optional** (`Type::Optional(Function)`) — lowers to
///   `Option<Rc<dyn Fn(T) -> _>>`. An omitted argument is `None`, which carries
///   no closure and therefore no `T`; the call site must name a concrete
///   element type for the `None` and picks the erased one (`E0308`, and `E0631`
///   on the synthesized default closure). This is also exactly the shape
///   `emitter::callee_param_is_owned_callback_sink` classifies as an owned sink.
/// * **owned or escaping** — the emitter's ownership fixpoint decided this
///   parameter is stored, returned or captured, so it lowers to an owned
///   `Rc<dyn Fn(..)>` with a `'static` bound rather than a borrowed `&dyn Fn`.
///   The borrowed-callback argument ladder in `emitter::call` — the one that
///   renders the substituted adapter — declines those positions, so the
///   substituted signature would be met with an erased argument.
/// * **nested inside a container, or inside another function type** — the
///   occurrence is behind a wrapper the adapter renderers do not walk into, so
///   the callee would advertise `T` in a position no call site substitutes.
/// * **rest** (`function.rest == Some(index)`) — a packed rest parameter is
///   emitted as an erased sequence of callables; there is no single declared
///   callback whose type the call site can be substituted against.
///
/// `owned` is the emitter's `compute_owned_callback_params` fixpoint, threaded
/// in by the caller. It is deliberately *the same* set the renderer consults
/// (`FunctionEmitter::is_owned_callback_param`) so the gate cannot disagree with
/// the representation it is predicting.
///
/// Only parameter positions are examined. A callback in the *return* type is
/// produced by the callee's own body rather than supplied by a call site, so it
/// has no argument to disagree with.
fn callback_occurrences_are_liftable(
    mir: &Mir,
    function: &MirFunction,
    owned: &HashSet<(FuncId, LocalId)>,
    name: Symbol,
) -> bool {
    for (index, param) in function.params.iter().enumerate() {
        let Ok(local_index) = id_index(param.0, "local index does not fit usize") else {
            return false;
        };
        let Some(local) = function.locals.get(local_index) else {
            return false;
        };
        if !type_param_in_callback(mir, local.ty, name) {
            // No callback occurrence in this parameter at all; a direct value
            // position such as `T` or `T[]` is handled by
            // `type_param_directly_inferable`.
            continue;
        }
        // There IS a callback occurrence here, so this parameter must be the one
        // eligible shape.
        let Some(Type::Function(callback)) = mir.types.get(local.ty) else {
            // Wrapped in `Option<..>`, a list, a dict, a class argument, ...
            return false;
        };
        if function.rest == Some(index) || owned.contains(&(function.id, *param)) {
            return false;
        }
        // Direct callback parameter — but the occurrence must be at its own
        // top level, not inside a further nested function type.
        if callback
            .params
            .iter()
            .any(|nested| type_param_in_callback(mir, *nested, name))
            || type_param_in_callback(mir, callback.return_ty, name)
        {
            return false;
        }
    }
    true
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
    owned_callback_params: &HashSet<(FuncId, LocalId)>,
) -> Result<String, EmitError> {
    if !function_emits_rust_generics(mir, function, owned_callback_params) {
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
