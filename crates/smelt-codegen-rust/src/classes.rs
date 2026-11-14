//! Helpers for emitting Rust class storage, generics, and inheritance surfaces.
#![expect(
    clippy::redundant_pub_crate,
    reason = "class helpers are shared across sibling emitter modules"
)]

use crate::rust::RustExpr;
use std::collections::{HashMap, HashSet};

use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{
    FuncId, LocalId, Mir, MirClass, MirField, MirFunction, MirInterface, Operand, Place, Terminator,
};

use crate::{EmitError, emitter::EmitContext, generic_bindings};

/// The trait bounds every generated type parameter carries.
///
/// ONE set, spelled once. A generated type parameter is reached through the
/// crate's own erasure round trip (`IntoSmeltUnknown` / `SmeltFromUnknown`), is
/// stored in `Rc`-shared cells (`Clone`, `'static`) and is materialized by
/// `Default::default()` in every generated `Default` body — so every generic
/// item the crate emits needs the same set, and a per-item subset is a bug
/// waiting on the first field whose type is another generated class.
///
/// Bounds that belong to ONE parameter's use rather than to all of them are
/// appended by the caller, not added here: `SmeltJsKeyEq` for a parameter used
/// as a map key (see [`class_type_param_used_as_map_key`]) and the generated
/// `F{n}: Fn(..) + ?Sized` callback bounds are both of that kind.
pub(crate) const GENERATED_TYPE_PARAM_BOUNDS: &str =
    "Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static";

/// Return the Rust storage type name of a MIR class, from the crate's name
/// plan (see [`crate::names`]).
pub(crate) fn class_name_text(
    context: &EmitContext,
    class: &MirClass,
) -> Result<String, EmitError> {
    Ok(context.names().type_name(class.name)?.as_str().to_owned())
}

/// Render the generic parameter declaration suffix for a class, such as `<T>`.
///
/// The returned text is empty for non-generic classes so callers can append it
/// directly after a struct, trait, or impl target name without extra branching.
pub(crate) fn class_type_params_text(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<String, EmitError> {
    let params = type_param_idents(mir, &class.type_params, "class")?
        .into_iter()
        .map(|(_, ident)| ident)
        .collect();
    Ok(carried_generics_text(context, class.name, params))
}

/// Pair every declared type parameter with its Rust identifier, in
/// declaration order.
///
/// `owner_kind` names the declaring item (`class`, `interface`) in the error a
/// parameter with no interned name produces.
fn type_param_idents(
    mir: &Mir,
    type_params: &[smelt_hir::TypeParamDef],
    owner_kind: &str,
) -> Result<Vec<(Symbol, String)>, EmitError> {
    type_params
        .iter()
        .map(|param| {
            mir.symbols
                .get(param.name)
                .map(|name| (param.name, crate::names::type_param(name).into_string()))
                .ok_or_else(|| {
                    EmitError::new(format!("{owner_kind} type parameter has unknown symbol"))
                })
        })
        .collect()
}

/// Wrap the rendered parameters `owner`'s emitted Rust still carries in `<..>`.
///
/// `params` holds one entry per declared type parameter, in declaration order.
/// The positions [`crate::generic_elision`] drops for `owner` are removed
/// first, so the declaration and every reference agree on arity. The result is
/// empty when no parameter is left, so a caller can append it directly after a
/// struct, trait, or impl target name.
fn carried_generics_text(context: &EmitContext, owner: Symbol, params: Vec<String>) -> String {
    let params = context.type_param_elision().retain_carried(owner, params);
    if params.is_empty() {
        return String::new();
    }
    format!("<{}>", params.join(", "))
}

/// Render the generic parameter declaration suffix for an interface.
///
/// Interface type parameters must survive into Rust storage types because
/// function signatures can refer to instantiated interface names such as
/// `ContextOptions<SmeltUnknown>`. The returned suffix mirrors class generic
/// emission and is empty for non-generic interfaces.
pub(crate) fn interface_type_params_text(
    mir: &Mir,
    context: &EmitContext,
    interface: &MirInterface,
) -> Result<String, EmitError> {
    let params = type_param_idents(mir, &interface.type_params, "interface")?
        .into_iter()
        .map(|(_, ident)| ident)
        .collect();
    Ok(carried_generics_text(context, interface.name, params))
}

/// Render bounded generic parameters for interface impl blocks.
pub(crate) fn interface_impl_generics_text(
    mir: &Mir,
    context: &EmitContext,
    interface: &MirInterface,
) -> Result<String, EmitError> {
    let params = type_param_idents(mir, &interface.type_params, "interface")?
        .into_iter()
        .map(|(name, ident)| {
            // The same field-driven key bound a class carries
            // (`class_type_param_used_as_map_key`): an object-shaped record with
            // a `Record<P, V>` field stores a `SmeltJsMap<P, V>`.
            let mut bound = format!("{ident}: {GENERATED_TYPE_PARAM_BOUNDS}");
            if interface
                .fields
                .iter()
                .any(|field| type_param_in_dict_key(mir, field.ty, name))
            {
                bound.push_str(" + SmeltJsKeyEq");
            }
            bound
        })
        .collect();
    Ok(carried_generics_text(context, interface.name, params))
}

/// Render the generic argument suffix for a class, such as `<T>`.
///
/// This mirrors [`class_type_params_text`] for places where the generated Rust
/// references an already-declared class type rather than declaring parameters.
pub(crate) fn class_type_args_text(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<String, EmitError> {
    class_type_params_text(mir, context, class)
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

/// The class declared under `name`.
pub(crate) fn find_class(mir: &Mir, name: Symbol) -> Option<&MirClass> {
    mir.class_named(name)
}

/// Return whether `name` occurs in a `Dict` key position anywhere within `ty`.
///
/// Descends through value wrappers and nested shapes so a `T` used as the key
/// of a map nested inside a list/tuple/other map is still detected. Only the
/// key slot of a `Dict` counts; the value slot and non-`Dict` positions do not
/// require `SmeltJsKeyEq`.
pub(crate) fn type_param_in_dict_key(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            generic_bindings::type_param_occurs(mir, *key, name)
                || type_param_in_dict_key(mir, *value, name)
        }
        Some(other) => other
            .children()
            .any(|child| type_param_in_dict_key(mir, child, name)),
        None => false,
    }
}

/// Render the generic parameter suffix used on inherent impl blocks.
///
/// The helper is intentionally separate from struct rendering because impl
/// blocks are the first place bounds may be introduced as class codegen grows.
/// A type parameter used as a map key in any field additionally gains the
/// `SmeltJsKeyEq` bound the map methods require (see
/// [`class_type_param_used_as_map_key`]).
pub(crate) fn class_impl_generics_text(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<String, EmitError> {
    let params = type_param_idents(mir, &class.type_params, "class")?
        .into_iter()
        .map(|(name, ident)| {
            let mut bound = format!("{ident}: {GENERATED_TYPE_PARAM_BOUNDS}");
            if class_type_param_used_as_map_key(mir, class, name) {
                bound.push_str(" + SmeltJsKeyEq");
            }
            bound
        })
        .collect();
    Ok(carried_generics_text(context, class.name, params))
}

/// How [`type_holds_function`] treats a class or interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NominalStorage {
    /// A class or interface is its own Rust type with its own `Clone`, `Debug`
    /// and `Default` impls, so naming one does not make a struct field "hold a
    /// function". This is the question a derive or clone decision asks: does
    /// THIS spelling store an `Rc<dyn Fn..>` that a derive cannot see through?
    Opaque,
    /// A class or interface is storage like any container: its struct owns its
    /// type arguments and its effective (inherited and own) fields, and every
    /// function-typed field is an owned `'static` handle that no borrowed
    /// `&dyn Fn` can be placed into. This is the question callback-escape
    /// analysis asks. A callable-object interface makes it load-bearing: its
    /// synthetic `__smelt_call` slot IS a function field, and stopping at the
    /// class left a captured callback parameter borrowed, which then failed
    /// borrowck ("`'1` must outlive `'static`", es-toolkit `curry`). Reading only
    /// a class's own fields missed an inherited callback field the same way.
    Followed,
}

/// Return whether a value of type `ty` holds a function value: a function
/// itself, or a list, set, dict, `Map`, optional, future, tuple or union that
/// holds one, and, under [`NominalStorage::Followed`], a class or interface
/// whose storage holds one.
///
/// The one function-storage walk in codegen. Three copies once existed; they
/// differed in whether they followed classes (correct per caller, hence the
/// explicit policy) but also in details that were not policy: the class hop
/// read a class's own fields instead of [`effective_class_fields`], so a
/// subclass inheriting a callback field did not retain it, and one copy walked
/// into generators while the other two did not. A generator is an opaque
/// handle under both policies: `SmeltGenerator` is `Clone` and `Debug` for any
/// element type.
pub(crate) fn type_holds_function(mir: &Mir, ty: TypeId, nominal: NominalStorage) -> bool {
    type_holds_function_seen(mir, ty, nominal, &mut HashSet::new())
}

/// [`type_holds_function`] with the cycle guard the class hop needs: a class
/// may name itself through a field (`interface Node { next: Node }`), and the
/// type graph is otherwise acyclic.
fn type_holds_function_seen(
    mir: &Mir,
    ty: TypeId,
    nominal: NominalStorage,
    seen: &mut HashSet<TypeId>,
) -> bool {
    if !seen.insert(ty) {
        return false;
    }
    let mut holds = |inner: TypeId| type_holds_function_seen(mir, inner, nominal, seen);
    match mir.types.get(ty) {
        Some(Type::Function(_)) => true,
        // `Type::Class` names either a class or an interface: both emit a Rust
        // struct that owns its fields.
        Some(Type::Class { name, args }) if nominal == NominalStorage::Followed => {
            if args.iter().any(|arg| holds(*arg)) {
                return true;
            }
            let fields = find_class(mir, *name)
                .map(|class| effective_class_fields(mir, class))
                .or_else(|| {
                    mir.interfaces
                        .iter()
                        .find(|interface| interface.name == *name)
                        .map(|interface| effective_interface_fields(mir, interface))
                })
                .unwrap_or_default();
            fields.iter().any(|field| holds(field.ty))
        }
        // A generator owns its state behind the runtime type, not as a field.
        Some(Type::Class { .. } | Type::Generator { .. } | Type::GeneratorResult { .. }) | None => {
            false
        }
        Some(other) => other.children().any(holds),
    }
}

/// Return the flattened field layout for a class.
///
/// Smelt stores subclass values with inherited fields first and own fields
/// after them. This helper follows the single-inheritance chain. When a
/// subclass redeclares a field from its base class, the subclass field
/// replaces the inherited slot so Rust struct storage stays valid and matches
/// the effective source member surface.
///
/// An inherited field's type is spelled in its declaring base's scope, so it
/// is rewritten into `class`'s scope through each level's `base_args`:
/// `class YearParser extends Parser<number>` stores `Parser`'s `parse:
/// (input) => ParseResult<T>` slot as `ParseResult<f64>`, not with a `T` the
/// subclass does not declare. The substituted types are interned by MIR's
/// `generic_records` pass; a layer missing from the table keeps its declared
/// type.
pub(crate) fn effective_class_fields(mir: &Mir, class: &MirClass) -> Vec<MirField> {
    // The chain root first, so a base's fields lead and an override replaces
    // the inherited slot in place. The slot lookup is a map: a linear `find`
    // per field made a long `extends` chain cubic overall (measured by
    // `tests/compile_scaling.rs`).
    let mut chain = vec![class];
    let mut seen = HashSet::from([class.name]);
    while let Some(base) = chain.last().and_then(|last| base_class(mir, last)) {
        if !seen.insert(base.name) {
            break;
        }
        chain.push(base);
    }
    let level_substitutions = chain_type_substitutions(mir, &chain);
    let mut fields: Vec<MirField> = Vec::new();
    let mut slots: HashMap<Symbol, usize> = HashMap::new();
    for (owner, substitutions) in chain.iter().zip(&level_substitutions).rev() {
        for declared in &owner.fields {
            let mut field = declared.clone();
            field.ty = generic_bindings::substitute_existing_type(mir, declared.ty, substitutions);
            if let Some(existing) = slots
                .get(&field.name)
                .and_then(|slot| fields.get_mut(*slot))
            {
                *existing = field;
            } else {
                slots.insert(field.name, fields.len());
                fields.push(field);
            }
        }
    }
    // TypeScript declaration merging: `interface Shape { kind: string }` beside
    // `class Shape { .. }` adds `kind` to every Shape instance. Both are one
    // symbol and one generated struct (see `names::type_claim_order`, where the
    // class owns it), so the interface's members the class does not declare
    // are fields of that struct, defaulted by the constructor like any field
    // the constructor leaves unassigned. A member the class implements as a
    // method stays a method.
    if let Some(merged) = mir.interface_named(class.name) {
        let methods = class_method_names(mir, class);
        for field in &merged.fields {
            if !slots.contains_key(&field.name) && !methods.contains(&field.name) {
                slots.insert(field.name, fields.len());
                fields.push(field.clone());
            }
        }
    }
    fields
}

/// The method names `class` answers, its own and inherited.
fn class_method_names(mir: &Mir, class: &MirClass) -> HashSet<Symbol> {
    effective_class_methods(mir, class)
        .into_iter()
        .filter_map(|method| mir.functions.get(usize::try_from(method.0).ok()?))
        .filter_map(|function| match function.origin {
            smelt_mir::HirOrigin::ClassMethod { method, .. } => Some(method),
            _ => None,
        })
        .collect()
}

/// Map each class in an inheritance `chain` (`chain[0]` the receiving class,
/// then its base, its base's base ..) from its type parameters to their
/// arguments in `chain[0]`'s scope; `chain[0]`'s own map is empty.
///
/// Level `k`'s `base_args` are spelled in level `k`'s scope, so they are
/// substituted through level `k`'s map before naming level `k + 1`'s
/// parameters: `C<U> extends B<U[]>`, `B<T> extends A<T>` maps `A`'s `T` to
/// `U[]`.
fn chain_type_substitutions(mir: &Mir, chain: &[&MirClass]) -> Vec<HashMap<Symbol, TypeId>> {
    let mut levels: Vec<HashMap<Symbol, TypeId>> = vec![HashMap::new()];
    for pair in chain.windows(2) {
        let [derived, base] = pair else { continue };
        let outer = levels.last().cloned().unwrap_or_default();
        let map = base
            .type_params
            .iter()
            .zip(&derived.base_args)
            .map(|(param, arg)| {
                (
                    param.name,
                    generic_bindings::substitute_existing_type(mir, *arg, &outer),
                )
            })
            .collect();
        levels.push(map);
    }
    levels
}

/// The type parameters of `declaring`, mapped to their arguments in `owner`'s
/// scope, when `owner` (transitively) extends `declaring`.
///
/// This is what an inherited method copied into `owner`'s `impl` renders its
/// declaring class's parameters as (see
/// `FunctionEmitter::inherited_copy`); [`effective_class_fields`] applies
/// the same map to the inherited fields, so the copy and the layout agree.
pub(crate) fn inherited_type_substitution(
    mir: &Mir,
    owner: Symbol,
    declaring: Symbol,
) -> HashMap<Symbol, TypeId> {
    let Some(owner_class) = find_class(mir, owner) else {
        return HashMap::new();
    };
    let mut chain = vec![owner_class];
    let mut seen = HashSet::from([owner]);
    while chain.last().is_some_and(|last| last.name != declaring)
        && let Some(base) = chain.last().and_then(|last| base_class(mir, last))
        && seen.insert(base.name)
    {
        chain.push(base);
    }
    if chain.last().is_none_or(|last| last.name != declaring) {
        return HashMap::new();
    }
    chain_type_substitutions(mir, &chain)
        .pop()
        .unwrap_or_default()
}

/// The declaring class of a method or constructor copied into `owner`'s
/// `impl`, when it is a generic ancestor whose parameters `owner` binds.
fn inherited_member_declaring_class(function: &MirFunction, owner: Symbol) -> Option<Symbol> {
    match function.origin {
        smelt_mir::HirOrigin::ClassMethod { class, .. }
        | smelt_mir::HirOrigin::ClassConstructor { class, .. }
            if class != owner =>
        {
            Some(class)
        }
        _ => None,
    }
}

/// The map an inherited member copy renders its declaring class's type
/// parameters through (empty when `function` is `owner`'s own member).
pub(crate) fn inherited_member_substitution(
    mir: &Mir,
    function: &MirFunction,
    owner: Symbol,
) -> HashMap<Symbol, TypeId> {
    inherited_member_declaring_class(function, owner)
        .map(|declaring| inherited_type_substitution(mir, owner, declaring))
        .unwrap_or_default()
}

/// A clone of an inherited `function` with its types rewritten into `owner`'s
/// scope, or `None` when nothing needs rewriting.
///
/// The copy is emitted inside `owner`'s `impl`, which does not declare the
/// declaring base's type parameters, so every local and cast target is
/// substituted (MIR's `inherited_types` pass interned the results). Emitting
/// from the substituted clone keeps every emitter rule that inspects a
/// `Type::TypeParam` seeing the concrete type the subclass binds.
pub(crate) fn substituted_inherited_member(
    mir: &Mir,
    function: &MirFunction,
    owner: Symbol,
) -> Option<MirFunction> {
    let substitutions = inherited_member_substitution(mir, function, owner);
    if substitutions.is_empty() {
        return None;
    }
    let mut copy = function.clone();
    // An absent literal spilled into a `T` temp (`v !== undefined`) must stay
    // a literal once `T` is concrete; see `inline_absent_constant_temps`.
    let spilled: Vec<LocalId> = copy
        .locals
        .iter()
        .enumerate()
        .filter(|(_, local)| {
            matches!(
                mir.types.get(local.ty),
                Some(Type::TypeParam { name }) if substitutions.contains_key(name)
            )
        })
        .filter_map(|(index, _)| u32::try_from(index).ok().map(LocalId))
        .collect();
    smelt_mir::opt::inline_absent_constant_temps(&mut copy, &spilled);
    smelt_mir::map_function_types(&mut copy, |ty| {
        generic_bindings::substitute_existing_type(mir, ty, &substitutions)
    });
    // A method copy's receiver is the RECEIVING class: its fields are read at
    // the flattened layout `owner` stores (whose slots are rendered at the
    // substituted types), not at the base instantiation the declaration
    // names. Only the receiver changes; every other base-typed local keeps
    // the base instantiation.
    // A method that answers its receiver (`route(..): Hono<..> { return this }`,
    // JavaScript's polymorphic `this`) answers the receiving class as well.
    if let smelt_mir::HirOrigin::ClassMethod {
        class: declaring, ..
    } = function.origin
        && let Some(owner_ty) = class_self_type(mir, owner)
    {
        if returns_the_receiver_at_its_own_class(mir, function) {
            copy.return_ty = owner_ty;
        }
        if let Some(receiver) = copy.locals.first_mut()
            && matches!(
                mir.types.get(receiver.ty),
                Some(Type::Class { name, .. }) if *name == declaring
            )
        {
            receiver.ty = owner_ty;
        }
    }
    Some(copy)
}

/// Whether a method's declared return type is its own declaring class and
/// every `return` in it answers the receiver (local 0).
///
/// That is JavaScript's polymorphic `this` written in TypeScript's nominal
/// spelling, and it is the only shape whose inherited copy may be re-typed to
/// the receiving class: a method that returns a FRESHLY BUILT base instance (a
/// `clone()`-style factory) genuinely answers the base type and keeps it.
pub(crate) fn returns_the_receiver_at_its_own_class(mir: &Mir, function: &MirFunction) -> bool {
    let smelt_mir::HirOrigin::ClassMethod { class, .. } = function.origin else {
        return false;
    };
    if !matches!(
        mir.types.get(function.return_ty),
        Some(Type::Class { name, .. }) if *name == class
    ) {
        return false;
    }
    let mut saw_return = false;
    for block in &function.blocks {
        if let Some(Terminator::Return(operand)) = &block.terminator {
            saw_return = true;
            if !matches!(
                operand,
                Operand::Copy(Place::Local(LocalId(0))) | Operand::Move(Place::Local(LocalId(0)))
            ) {
                return false;
            }
        }
    }
    saw_return
}

/// The already-interned type of a class instantiated at its own parameters
/// (`Middle<T>` inside `Middle`'s `impl`), if the program spells it.
pub(crate) fn class_self_type(mir: &Mir, class: Symbol) -> Option<TypeId> {
    let declared = find_class(mir, class)?;
    let args = declared
        .type_params
        .iter()
        .map(|param| mir.types.lookup(&Type::TypeParam { name: param.name }))
        .collect::<Option<Vec<_>>>()?;
    mir.types.lookup(&Type::Class { name: class, args })
}

/// The virtual method slots of `class`'s flattened layout: function-typed
/// fields named like a method (own, inherited or abstract) of the chain.
///
/// TypeScript forbids a property and a method of the same name, so such a
/// field can only be a slot the frontend synthesized for overridable dispatch
/// (`add_virtual_class_method_fields`).
pub(crate) fn own_virtual_slot_names(mir: &Mir, class: &MirClass) -> Vec<Symbol> {
    let mut method_names: HashSet<Symbol> = effective_class_methods(mir, class)
        .into_iter()
        .filter_map(|id| mir.functions.get(usize::try_from(id.0).ok()?))
        .filter_map(|function| match function.origin {
            smelt_mir::HirOrigin::ClassMethod { method, .. } => Some(method),
            _ => None,
        })
        .collect();
    let mut current = Some(class);
    let mut seen = HashSet::new();
    while let Some(level) = current
        && seen.insert(level.name)
    {
        method_names.extend(level.abstract_methods.iter().map(|method| method.name));
        current = base_class(mir, level);
    }
    effective_class_fields(mir, class)
        .into_iter()
        .filter(|field| {
            method_names.contains(&field.name)
                && matches!(mir.types.get(field.ty), Some(Type::Function(_)))
        })
        .map(|field| field.name)
        .collect()
}

/// Whether a `Type::Class { name }` renders as the erased `SmeltUnknown`
/// carrier: it names no class or interface of this crate and no stdlib class
/// with a concrete runtime type (`Date` is the common case).
///
/// A modeled class with a CONCRETE generated Rust type never erases: that is
/// the whole point of modeling it. Asked of the registry
/// (`StdlibClass::has_concrete_runtime_type`) rather than of a list of
/// spellings — the list is what silently omitted `ArrayBuffer` when the
/// typed-array family became concrete. Shared by the emitter and by the
/// prelude's "does this crate need the carrier" question, which must agree.
pub(crate) fn class_name_renders_erased(mir: &Mir, name: Symbol) -> bool {
    if !mir.has_class(name)
        && mir.symbols.get(name).is_some_and(|type_name| {
            smelt_stdlib::typescript_stdlib_class(type_name)
                .is_some_and(smelt_stdlib::StdlibClass::has_concrete_runtime_type)
        })
    {
        return false;
    }
    !mir.has_class(name) && !mir.has_interface(name)
}

/// Return the flattened method set for a class: its own methods plus every
/// inherited one it does not override.
///
/// Smelt flattens inheritance — a subclass struct stores its base's fields
/// inline (see [`effective_class_fields`]) and there is no base value to
/// delegate to at runtime. Rust has no method inheritance either, so a base
/// method is only callable on a subclass receiver if it is emitted into the
/// subclass's own `impl` block.
///
/// Without this, a class body that calls an inherited method lowered fine in
/// the frontend (method resolution walks the base chain) but emitted Rust that
/// does not compile: `self.fetch()` on a `B` whose `fetch` lives in `impl A`
/// fails with `no method named 'fetch' found for reference '&B'`.
///
/// Ordering matches [`effective_class_fields`]: base methods first, then the
/// class's own, with an override *replacing* the inherited slot in place so a
/// subclass never emits two methods of the same name. Re-emitting the base body
/// under the subclass is sound precisely because of flattening: every field the
/// base body touches exists on the subclass, and any sibling method it calls is
/// itself inherited by the same rule.
pub(crate) fn effective_class_methods(mir: &Mir, class: &MirClass) -> Vec<FuncId> {
    let mut methods = base_class(mir, class)
        .map(|base| effective_class_methods(mir, base))
        .unwrap_or_default();
    for method in &class.methods {
        match methods
            .iter_mut()
            .find(|candidate| method_names_match(mir, **candidate, *method))
        {
            Some(existing) => *existing = *method,
            None => methods.push(*method),
        }
    }
    methods
}

/// The class `class` directly extends, when it extends one this crate declares.
fn base_class<'mir>(mir: &'mir Mir, class: &MirClass) -> Option<&'mir MirClass> {
    let base = class.base?;
    find_class(mir, base)
}

/// Whether two method functions declare the same method name, i.e. one
/// overrides the other.
fn method_names_match(mir: &Mir, left: FuncId, right: FuncId) -> bool {
    let method_name = |id: FuncId| {
        let function = mir.functions.get(usize::try_from(id.0).ok()?)?;
        match function.origin {
            smelt_mir::HirOrigin::ClassMethod { method, .. } => Some(method),
            _ => None,
        }
    };
    match (method_name(left), method_name(right)) {
        (Some(left_name), Some(right_name)) => left_name == right_name,
        _ => false,
    }
}

/// Render a primitive class-level value captured by specialization.
///
/// Static members are emitted as associated getter functions because owned
/// values such as `String` cannot be Rust `const` items. `js_text` says
/// whether a string value is a JavaScript string (`JsString`).
pub(crate) fn materialized_static_value(
    literal: Option<&smelt_hir::Literal>,
    js_text: bool,
) -> RustExpr {
    match literal {
        Some(smelt_hir::Literal::Bool(value)) => RustExpr::chain(value.to_string()),
        Some(smelt_hir::Literal::Int(value)) => RustExpr::chain(value.to_string()),
        Some(smelt_hir::Literal::Float(value)) if value.is_nan() => RustExpr::atom("f64::NAN"),
        Some(smelt_hir::Literal::Float(value))
            if value.is_infinite() && value.is_sign_positive() =>
        {
            RustExpr::atom("f64::INFINITY")
        }
        Some(smelt_hir::Literal::Float(value)) if value.is_infinite() => {
            RustExpr::atom("f64::NEG_INFINITY")
        }
        Some(smelt_hir::Literal::Float(value)) => RustExpr::chain(format!("{value:?}")),
        Some(smelt_hir::Literal::String(value)) if js_text => RustExpr::js_string_literal(value),
        Some(smelt_hir::Literal::StringUnits(units)) => RustExpr::js_string_units_literal(units),
        Some(smelt_hir::Literal::String(value)) => RustExpr::string_literal(value),
        Some(smelt_hir::Literal::BigInt(digits)) => crate::bigint_prelude::literal_expr(digits),
        Some(
            smelt_hir::Literal::Symbol(_)
            | smelt_hir::Literal::Undefined
            | smelt_hir::Literal::None,
        )
        | None => RustExpr::atom("Default::default()"),
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
    // Keyed by the field's symbol in a map (the linear `find` made a long
    // `extends` chain cubic). Not by its rendering: two symbols that render
    // alike are two fields, which the name plan renames apart
    // (`names::RustNames::field`), where keying by rendering merged them.
    let _ = mir;
    let mut fields: Vec<MirField> = Vec::new();
    let mut slots: HashMap<Symbol, usize> = HashMap::new();
    for field in &interface.fields {
        if let Some(existing) = slots
            .get(&field.name)
            .and_then(|slot| fields.get_mut(*slot))
        {
            *existing = field.clone();
        } else {
            slots.insert(field.name, fields.len());
            fields.push(field.clone());
        }
    }
    fields
}

/// The name of the copy of the ancestor method `function` that a `super` call
/// reaches on a subclass, whose own method of that name is the override.
///
/// Composed from the planned class and method names after the emitter's
/// `smelt_super_` prefix, which no source member name can carry.
pub(crate) fn super_copy_name(context: &EmitContext, function: &MirFunction) -> Option<String> {
    let smelt_mir::HirOrigin::ClassMethod { class, method, .. } = function.origin else {
        return None;
    };
    Some(format!(
        "smelt_super_{}_{}",
        context.names().type_name(class).ok()?,
        context.names().method(method)?
    ))
}

/// Whether calling the method `function` on a receiver of class `receiver` is
/// a `super` call: `function` is declared on a strict ancestor, and the method
/// of that name `receiver` has is another one.
pub(crate) fn is_super_call(mir: &Mir, receiver: Symbol, function: FuncId) -> bool {
    let Some(declared) = usize::try_from(function.0)
        .ok()
        .and_then(|index| mir.functions.get(index))
    else {
        return false;
    };
    if !matches!(declared.origin, smelt_mir::HirOrigin::ClassMethod { class, .. } if class != receiver)
    {
        return false;
    }
    let Some(class) = find_class(mir, receiver) else {
        return false;
    };
    effective_class_methods(mir, class)
        .iter()
        .any(|method| *method != function && method_names_match(mir, *method, function))
}

/// Every (subclass, ancestor method) pair a `super` call in `mir` reaches, so
/// the subclass can carry a copy of that method under [`super_copy_name`].
pub(crate) fn super_calls(mir: &Mir) -> Vec<(Symbol, FuncId)> {
    let mut found = Vec::new();
    let bodies = mir
        .functions
        .iter()
        .map(|function| (&function.locals, &function.blocks))
        .chain(
            mir.closures
                .iter()
                .map(|closure| (&closure.locals, &closure.blocks)),
        );
    for (locals, blocks) in bodies {
        for block in blocks {
            let Some(Terminator::Call {
                callee: smelt_mir::Callee::Static(function),
                args,
                ..
            }) = &block.terminator
            else {
                continue;
            };
            let Some(receiver) = args.first().and_then(Operand::local) else {
                continue;
            };
            let Some(Type::Class { name, .. }) = usize::try_from(receiver.0)
                .ok()
                .and_then(|index| locals.get(index))
                .and_then(|local| mir.types.get(local.ty))
            else {
                continue;
            };
            if is_super_call(mir, *name, *function) && !found.contains(&(*name, *function)) {
                found.push((*name, *function));
            }
        }
    }
    found
}

/// Whether `ancestor` is `class` or one of the classes it extends.
pub(crate) fn is_self_or_ancestor(mir: &Mir, ancestor: Symbol, class: Symbol) -> bool {
    let mut current = Some(class);
    for _ in 0..64_u32 {
        let Some(name) = current else {
            return false;
        };
        if name == ancestor {
            return true;
        }
        current = find_class(mir, name).and_then(|found| found.base);
    }
    false
}
