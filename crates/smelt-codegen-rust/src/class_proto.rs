//! Emission of a generated class's *prototype-carried* members.
//!
//! # Why this exists
//!
//! Erasing a class instance to [`SmeltUnknown`] used to keep only its fields
//! and the hidden `__smelt_class` provenance marker. Its methods were dropped
//! outright, because MIR has no method-as-value node: nothing in HIR or MIR
//! names "the `has` method of `CustomCache`, bound to this receiver". So an
//! erased instance answered `undefined` for every method read, and the erased
//! call site quietly substituted a fabricated default (`|_| false`,
//! `|_| SmeltUnknown::Null`). es-toolkit's `memoize` reads `cache.has` /
//! `cache.get` / `cache.set` off an erased custom cache; every read missed,
//! nothing was ever stored, and the failure was silent.
//!
//! In JavaScript those members live on the prototype, not on the instance. The
//! generated runtime already models exactly that: `smelt_get_object_field`
//! falls back from `field` to `__smelt_proto:{field}`, and enumeration,
//! structural equality, hashing and JSON all skip the `__smelt_proto:` prefix.
//! This module fills that slot for user classes by emitting, per class, one
//! inherent method
//!
//! ```ignore
//! fn __smelt_proto_entries(&self) -> Vec<(String, SmeltUnknown)>
//! ```
//!
//! that builds one erased, receiver-bound adapter per eligible method, keyed
//! under [`METHOD_KEY_PREFIX`]. Both
//! erasure paths (the inline `class_unknown_object_expr` adapter and the
//! generated `IntoSmeltUnknown` impl) append its result, so an instance erases
//! the same way whichever path the emitter took.
//!
//! # Why an adapter rather than a value node
//!
//! A hand-writing Rust team would not erase `CustomCache` at all — it would
//! stay a concrete struct. The erasure only happens because the *consumer*
//! (`memoize`'s `cache` parameter) is genuinely dynamic. The adapter is the
//! boundary shim that team would write at that seam: it coerces each argument
//! in from the tagged carrier, calls the real typed method, and erases the
//! typed result back out. Every eligible method keeps its precise Rust
//! signature; only the seam is tagged.
//!
//! # Eligibility
//!
//! [`method_is_proto_eligible`] is deliberately a *whitelist*, and a method it
//! rejects is simply absent rather than routed through a fabricated value.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use crate::rust::RustExpr;
use smelt_hir::{FunctionType, Type, TypeId};
use smelt_mir::{FuncId, HirOrigin, LocalId, Mir, MirClass, MirFunction};

use crate::emitter::{EmitContext, method_mutates_this};
use crate::{
    EmitError,
    classes::effective_class_methods,
    emitter::record_coercion::{record_field_unknown_expr, type_supports_from_unknown},
};

/// Name of the generated inherent method that lists a class's prototype members.
pub(crate) const PROTO_ENTRIES_METHOD: &str = "__smelt_proto_entries";

/// Key prefix under which a class's prototype methods are stored on an erased
/// instance.
///
/// Deliberately NOT the existing `__smelt_proto:` prefix, which
/// `smelt_object_from_prototype` uses for `Object.create(proto)` results.
/// Those inherited properties are *enumerable*, so `for...in` walks them;
/// a class's methods and accessors are defined non-enumerable by the language,
/// so `for...in` must not. Sharing one prefix would have to pick one of those
/// two behaviours and be wrong about the other — remeda's `isEmptyish`, which
/// probes emptiness with a bare `for (const _ in data) return false`, caught
/// exactly that. `smelt_get_object_field` resolves both prefixes; every
/// enumeration, structural-equality, hashing and JSON view skips both.
pub(crate) const METHOD_KEY_PREFIX: &str = "__smelt_method:";

/// Prefix the frontends give a synthesized getter: `get x()` lowers to an
/// ordinary method named `__smelt_get_x`. It is published under the property's
/// own name as an accessor record, which a member read runs.
const GETTER_PREFIX: &str = "__smelt_get_";

/// Prefix of a synthesized setter, which a member READ never reaches.
const SETTER_PREFIX: &str = "__smelt_set_";

/// Whether `ty` can be produced from an erased `SmeltUnknown` result.
///
/// The outbound mirror of [`type_supports_from_unknown`]. `Never`, futures,
/// generators and bare function handles have no faithful erased rendering —
/// [`record_field_unknown_expr`] answers `SmeltUnknown::Null` for them, which
/// would fabricate a value rather than report one — so a method returning one
/// is not eligible. `None` (a `void` method) IS eligible: JavaScript answers
/// `undefined`, which the carrier spells exactly.
fn return_type_is_erasable(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(
            Type::Never
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. }
            | Type::Function(_),
        )
        | None => false,
        Some(_) => true,
    }
}

/// Whether a class method can be exposed as a bound erased prototype member.
///
/// All of the following must hold:
///
/// 1. It is a real instance method (`HirOrigin::ClassMethod`), not a
///    constructor or a static.
/// 2. It is neither `async` nor a generator: both return a suspended handle
///    that has no erased rendering.
/// 3. Every parameter can be rebuilt from the erased call's arguments
///    ([`param_crosses_erased_call`]): a data parameter through
///    `SmeltFromUnknown`, a callback whose own parameters and return cross the
///    carrier by wrapping the erased function, a trailing rest list from the
///    remaining arguments. A compiled-regex parameter has no inbound impl, so
///    no adapter is emitted rather than one that would not compile.
/// 4. Its return type has a faithful erased rendering
///    ([`return_type_is_erasable`]).
/// 5. Its receiver is `&self`. A by-value class whose method takes `&mut self`
///    would mutate the adapter's captured *clone*, silently losing the write;
///    a class actually mutated through an alias is lifted to a reference class
///    by `classify::reference_classes` and takes `&self` uniformly, so this
///    only excludes writes that were never observable through an alias anyway.
pub(crate) fn method_is_proto_eligible(
    mir: &Mir,
    context: &EmitContext,
    owner: smelt_hir::Symbol,
    function: &MirFunction,
) -> bool {
    let HirOrigin::ClassMethod { class, .. } = function.origin else {
        return false;
    };
    if function.is_async || function.is_generator {
        return false;
    }
    let HirOrigin::ClassMethod { method: name, .. } = function.origin else {
        return false;
    };
    let Some(spelling) = mir.symbols.get(name) else {
        return false;
    };
    if spelling.starts_with(SETTER_PREFIX) {
        return false;
    }
    if !return_type_is_erasable(mir, function.return_ty) {
        return false;
    }
    if method_mutates_this(function) && !context.is_reference_class(class) {
        return false;
    }
    // A callback or rest parameter is spelled by the method's own emitter,
    // which renders the DECLARING class's type parameters; an inherited copy
    // in a subclass impl substitutes them, so only an own method takes it.
    let own_method = class == owner;
    function
        .params
        .iter()
        .enumerate()
        .skip(1)
        .all(|(index, param)| {
            local_decl(function, *param).is_some_and(|local| {
                if own_method {
                    param_crosses_erased_call(mir, context, function, index, local.ty)
                } else {
                    function.rest.is_none() && type_supports_from_unknown(mir, context, local.ty)
                }
            })
        })
}

/// Whether the erased adapter can rebuild parameter `index` of `function`
/// from the erased call's arguments: a data parameter through
/// `SmeltFromUnknown`, a callback by wrapping the erased function, and a
/// trailing rest list from the remaining arguments (`proto_argument`).
fn param_crosses_erased_call(
    mir: &Mir,
    context: &EmitContext,
    function: &MirFunction,
    index: usize,
    ty: TypeId,
) -> bool {
    // A parameter the method mutates in place enters as `&mut`. An argument
    // rebuilt from the erased call is a temporary the caller cannot see, so the
    // write would be lost: such a method has no erased adapter.
    if let Some(param) = function.params.get(index)
        && crate::emitter::parameter_is_mutable_reference(mir, context, function, *param)
    {
        return false;
    }
    if function.rest == Some(index) {
        return matches!(mir.types.get(ty), Some(Type::List(item))
            if type_supports_from_unknown(mir, context, *item)
                || matches!(mir.types.get(*item), Some(Type::Function(_))));
    }
    match callback_parameter(mir, ty) {
        Some(callback) => {
            !callback.is_async
                && callback
                    .params
                    .iter()
                    .all(|param| type_supports_from_unknown(mir, context, *param))
                // A `void` callback answers nothing to convert, and a `never`
                // one (it always throws) answers nothing at all: its
                // rendering is the erased carrier itself.
                && (matches!(
                    mir.types.get(callback.return_ty),
                    Some(Type::None | Type::Never)
                ) || type_supports_from_unknown(mir, context, callback.return_ty))
        }
        None => type_supports_from_unknown(mir, context, ty),
    }
}

/// The callback a parameter of type `ty` takes: a function, or an optional one
/// (`handler = fallback`, `handler?: ..`), whose absent argument (`undefined`)
/// rebuilds as `None`. `None` for a parameter that takes data.
fn callback_parameter(mir: &Mir, ty: TypeId) -> Option<&FunctionType> {
    match mir.types.get(ty)? {
        Type::Function(callback) => Some(callback),
        Type::Optional(inner) => match mir.types.get(*inner)? {
            Type::Function(callback) => Some(callback),
            _ => None,
        },
        _ => None,
    }
}

/// The prelude expression yielding the canonical identity of one class method.
///
/// A method reference read off an instance has to capture its receiver to stay
/// callable, so every read allocates a fresh `Rc` and reference equality on the
/// allocation address answers `false`. JavaScript disagrees: the method lives
/// once on the prototype, so `a.m === b.m === C.prototype.m`. Linking each
/// freshly built adapter to ONE canonical id per (defining class, method) makes
/// the generated answer match — while `a.m.bind(a)` stays a distinct value,
/// because binding builds a wrapper that is never linked here.
///
/// Keyed by the *defining* class, taken from the method's [`HirOrigin`], so a
/// method a subclass inherits resolves to the same identity as the base's own
/// read — again exactly the prototype-chain answer.
pub(crate) fn method_identity_expr(
    mir: &Mir,
    function: &MirFunction,
) -> Result<Option<RustExpr>, EmitError> {
    let HirOrigin::ClassMethod { class, method, .. } = function.origin else {
        return Ok(None);
    };
    let class_name = mir
        .symbols
        .get(class)
        .ok_or_else(|| EmitError::new("class method references an unknown class symbol"))?;
    let method_name = mir
        .symbols
        .get(method)
        .ok_or_else(|| EmitError::new("class method references an unknown symbol"))?;
    let key = format!("{class_name}::{method_name}");
    Ok(Some(RustExpr::chain(format!(
        "smelt_method_identity({key:?})"
    ))))
}

/// Render the erased adapter expression for one eligible method.
///
/// The adapter captures a receiver handle (`self.clone()` — an `Rc` bump for a
/// reference class, a struct copy for a value class whose methods cannot
/// mutate), coerces each positional argument in through `SmeltFromUnknown`
/// (padding missing arguments with `undefined`, as JavaScript does), calls the
/// real typed method, and erases the typed result back out.
fn method_adapter(
    mir: &Mir,
    context: &EmitContext,
    function: &MirFunction,
    rust_method: &str,
) -> Result<RustExpr, EmitError> {
    erased_callable_adapter(
        mir,
        context,
        function,
        &MethodReceiver::Instance,
        rust_method,
    )
}

/// What an erased adapter calls its method on.
enum MethodReceiver {
    /// An instance method: the adapter captures `self` and skips the `this`
    /// parameter.
    Instance,
    /// A static method, called as an associated function of this Rust type.
    Static(String),
    /// A prototype method: the adapter recovers its receiver from the
    /// dynamically scoped `this` the call installs, so one function serves
    /// every instance (and an `Object.create(C.prototype)` object).
    This,
}

/// The erased adapter for one method: arguments in through
/// `SmeltFromUnknown`, the typed call, the result erased back out.
fn erased_callable_adapter(
    mir: &Mir,
    context: &EmitContext,
    function: &MirFunction,
    receiver: &MethodReceiver,
    rust_method: &str,
) -> Result<RustExpr, EmitError> {
    let skipped = match receiver {
        MethodReceiver::Instance | MethodReceiver::This => 1,
        MethodReceiver::Static(_) => 0,
    };
    let mut args = Vec::new();
    // Callback and rest parameters are spelled by the method's own signature,
    // which only its emitter knows (`FunctionEmitter::erased_proto_argument`).
    let mut method_emitter = None;
    for (index, (position, param)) in function.params.iter().enumerate().skip(skipped).enumerate() {
        let local = local_decl(function, *param)
            .ok_or_else(|| EmitError::new("class method parameter has no local declaration"))?;
        let is_rest = function.rest == Some(position);
        if is_rest || callback_parameter(mir, local.ty).is_some() {
            let erased = if is_rest {
                // The remaining arguments, as the array JavaScript binds.
                format!(
                    "SmeltUnknown::Array(SmeltArray::new(smelt_args.iter().skip({index}).cloned().collect::<Vec<_>>()))"
                )
            } else {
                format!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)")
            };
            // The adapter is the erased boundary of a method that may declare
            // its own Rust generics (`map<U>`): it spells none of them, so
            // they instantiate at `SmeltUnknown`.
            if method_emitter.is_none() {
                let mut emitter = crate::emitter::FunctionEmitter::new(mir, context, function)?;
                emitter.erase_own_generics();
                method_emitter = Some(emitter);
            }
            let Some(emitter) = &method_emitter else {
                return Err(EmitError::new("class method adapter has no emitter"));
            };
            args.push(RustExpr::atom(
                emitter.erased_proto_argument(*param, &erased)?,
            ));
            continue;
        }
        // The parameter's TYPE decides how it is recovered: the blanket
        // `SmeltFromUnknown` does not compile for a parameter type that has no such
        // impl. See `record_field_from_unknown_expr`.
        args.push(
            crate::emitter::record_coercion::record_field_from_unknown_expr(
                mir,
                &RustExpr::chain(format!(
                    "smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                )),
                local.ty,
            )?,
        );
    }
    // A throwing method returns `Result<_, Box<dyn Error>>`; the erased
    // callback's own return type is the same `Result`, so the `?` propagates
    // the thrown value across the seam rather than swallowing it.
    let try_operator = if function.can_throw { "?" } else { "" };
    let (capture, callee) = match receiver {
        MethodReceiver::Instance => (
            "let smelt_receiver = self.clone(); ",
            "smelt_receiver.".to_owned(),
        ),
        MethodReceiver::Static(class) => ("", format!("{class}::")),
        MethodReceiver::This => ("", "smelt_receiver.".to_owned()),
    };
    let recover = if matches!(receiver, MethodReceiver::This) {
        "let smelt_receiver: Self = SmeltFromUnknown::smelt_from_unknown(smelt_this()); "
    } else {
        ""
    };
    let turbofish = crate::emitter::FunctionEmitter::new(mir, context, function)?
        .erased_own_generics_turbofish()?;
    let call = format!(
        "{callee}{rust_method}{turbofish}({}){try_operator}",
        args.join(", ")
    );
    let result = if matches!(mir.types.get(function.return_ty), Some(Type::None)) {
        // A `void` method evaluates to `undefined` in JavaScript.
        "{ let () = smelt_result; SmeltUnknown::Undefined }".to_owned()
    } else if matches!(
        mir.types.get(function.return_ty),
        Some(Type::Class { name: returned, args: returned_args }) if returned_args.is_empty()
            && crate::classes::find_class(mir, *returned).is_some_and(|returned_class| returned_class.type_params.is_empty())
    ) {
        // A (non-generic) class instance crosses as an instance: with its
        // class marker and its prototype-carried methods, as every other
        // erasure of it does (`FunctionEmitter::erase`). The record
        // projection alone dropped them, so `K.build(1).describe()` through
        // an erased class value found no method. A generic class keeps the
        // projection: its inline erasure needs the caller's render scope.
        let emitter = match &method_emitter {
            Some(emitter) => emitter,
            None => method_emitter.insert(crate::emitter::FunctionEmitter::new(
                mir, context, function,
            )?),
        };
        emitter
            .erased_value_text(&RustExpr::path("smelt_result"), function.return_ty)?
            .into_string()
    } else {
        record_field_unknown_expr(mir, &RustExpr::path("smelt_result"), function.return_ty)?
            .into_string()
    };
    // The adapter is a fresh allocation per erasure, but it denotes the ONE
    // prototype method; link it to that method's canonical identity so
    // `instance.m === OtherInstance.m` answers `true` as it does in JavaScript.
    let identity = method_identity_expr(mir, function)?;
    let body = format!(
        "{{ {capture}\
         move |smelt_args: Vec<SmeltUnknown>| {{ let _ = &smelt_args; {recover}\
         let smelt_result = {call}; Ok({result}) }} }}"
    );
    // Renamed rather than shadowed: the unwrapped value is what the identity
    // link takes, and `shadow_unrelated` reads a same-name rebind here as a
    // second, unrelated binding.
    let Some(identity_key) = identity else {
        return Ok(RustExpr::atom(format!(
            "SmeltUnknown::Function(::std::rc::Rc::new({body}))"
        )));
    };
    Ok(RustExpr::atom(format!(
        "{{ let smelt_method: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new({body}); \
         smelt_link_function_identity_key(&smelt_method, {identity_key}); SmeltUnknown::Function(smelt_method) }}",
        identity_key = identity_key.delimited()
    )))
}

/// Key of a class's erased constructor entry, which `smelt_construct_like`
/// builds through for `new value.constructor(args)` on an erased instance.
pub(crate) const CONSTRUCTOR_KEY: &str = "__smelt_method:constructor";

/// Whether `mir` can build an instance through its erased constructor:
/// `new value.constructor()` anywhere, or a reflective prototype read
/// (`Object.getPrototypeOf`, `__proto__`), whose `constructor` es-toolkit's
/// `clone` takes and calls with `new` (docs/design/class-prototypes.md).
fn program_constructs_like(mir: &Mir, context: &EmitContext) -> bool {
    *context.constructs_like.get_or_init(|| {
        let bodies = mir
            .functions
            .iter()
            .map(|function| &function.blocks)
            .chain(mir.closures.iter().map(|closure| &closure.blocks));
        let mut found = false;
        for blocks in bodies {
            for block in blocks {
                found |= block.statements.iter().any(|statement| {
                    matches!(
                        statement,
                        smelt_mir::Statement::Assign {
                            value: smelt_mir::Rvalue::ConstructLike { .. }
                                | smelt_mir::Rvalue::PrototypeSentinel { .. },
                            ..
                        }
                    )
                });
            }
        }
        found
    })
}

/// The constructor of `class` as an erased function building a new instance,
/// when the program needs one and every constructor parameter can be rebuilt
/// from the erased carrier. A generic or abstract class has none: no `new`
/// can be spelled without its type arguments.
fn constructor_adapter(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<Option<RustExpr>, EmitError> {
    if !program_constructs_like(mir, context)
        || !class.type_params.is_empty()
        || matches!(class.kind, smelt_hir::ClassKind::Abstract)
    {
        return Ok(None);
    }
    let Some(self_ty) = crate::classes::class_self_type(mir, class.name) else {
        return Ok(None);
    };
    let constructor = class.constructor.and_then(|id| function_by_id(mir, id));
    let mut args = Vec::new();
    if let Some(function) = constructor {
        if function.rest.is_some() || function.is_async {
            return Ok(None);
        }
        for (index, param) in function.params.iter().enumerate() {
            let Some(local) = local_decl(function, *param) else {
                return Ok(None);
            };
            if !type_supports_from_unknown(mir, context, local.ty) {
                return Ok(None);
            }
            args.push(
                crate::emitter::record_coercion::record_field_from_unknown_expr(
                    mir,
                    &RustExpr::chain(format!(
                        "smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                    )),
                    local.ty,
                )?,
            );
        }
    }
    let try_operator = if constructor.is_some_and(|function| function.can_throw) {
        "?"
    } else {
        ""
    };
    let built = record_field_unknown_expr(mir, &RustExpr::path("smelt_built"), self_ty)?;
    Ok(Some(RustExpr::atom(format!(
        "SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ let _ = &smelt_args; let smelt_built = Self::new({}){try_operator}; Ok({built}) }}))",
        args.join(", "),
        built = built.delimited()
    ))))
}

/// The methods a class's prototype object carries, as `(key, adapter)` pairs:
/// every eligible plain method (not an accessor) of the class, inherited ones
/// included, as a `this`-taking erased function (`MethodReceiver::This`).
///
/// Empty for a class whose receiver cannot be recovered from the erased
/// carrier: a generic class (no `Self` without type arguments), an abstract
/// one (no instance of its own type), or one with no `SmeltFromUnknown`. Such
/// a class keeps only its instances' receiver-bound adapters. See
/// docs/design/class-prototypes.md, section 7.
fn prototype_method_entries(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<Vec<(String, RustExpr)>, EmitError> {
    if !class.type_params.is_empty() || matches!(class.kind, smelt_hir::ClassKind::Abstract) {
        return Ok(Vec::new());
    }
    let Some(self_ty) = crate::classes::class_self_type(mir, class.name) else {
        return Ok(Vec::new());
    };
    // A reference class recovers its live instance from an erased view
    // (`reference_origin`), which is exactly the receiver a method mutates.
    if !context.is_reference_class(class.name) && !type_supports_from_unknown(mir, context, self_ty)
    {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for method in effective_class_methods(mir, class) {
        let Some(function) = function_by_id(mir, method) else {
            continue;
        };
        let HirOrigin::ClassMethod { method: name, .. } = function.origin else {
            continue;
        };
        let Some(source_name) = mir.names.get(name).or_else(|| mir.symbols.get(name)) else {
            continue;
        };
        if source_name.starts_with(SETTER_PREFIX)
            || source_name.starts_with(GETTER_PREFIX)
            || !method_is_proto_eligible(mir, context, class.name, function)
        {
            continue;
        }
        let Some(rust_method) = context.names().method(name) else {
            continue;
        };
        let adapter = erased_callable_adapter(
            mir,
            context,
            function,
            &MethodReceiver::This,
            rust_method.as_str(),
        )?;
        entries.push((format!("{METHOD_KEY_PREFIX}{source_name}"), adapter));
    }
    Ok(entries)
}

/// Whether `class` gets a `__smelt_prototype_methods` function, which the
/// runtime's `program_classes` table then names.
pub(crate) fn class_has_prototype_methods(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<bool, EmitError> {
    Ok(context
        .demand()
        .has(crate::runtime_demand::Need::ClassPrototypeMethods)
        && !prototype_method_entries(mir, context, class)?.is_empty())
}

/// Emit the `__smelt_prototype_methods` associated function of a class: the
/// methods its prototype object carries (see [`prototype_method_entries`]).
///
/// `None` when the class has none, or when the program never reaches a class
/// prototype reflectively (`Need::ClassPrototypeMethods`): a class used only
/// through its static type pays nothing for this.
///
/// # Dynamic boundary
///
/// Each entry is a `SmeltUnknown`-typed method, and that is a genuine dynamic
/// boundary, not avoidable erasure. The builder only exists for a program that
/// reaches a class prototype by reflection: `Object.getPrototypeOf(x)`,
/// `Object.create(proto)`, `Object.setPrototypeOf`, or `C.prototype` read off
/// an erased class value. Every such read hands the program a value whose
/// static type is `unknown` (the prototype), and the program then looks a
/// method up on it, or on an object linked to it, BY STRING NAME
/// (`proto.speak`, `Object.create(proto).bark()`). Which class's prototype that
/// is, and so which methods it carries, is a run-time fact of the value:
/// es-toolkit's `clone` reaches the prototype of whatever instance it was
/// handed. No concrete struct, generated union arm or scoped generic can name
/// that method set ahead of time, and the receiver is supplied by the call
/// through the `this` channel, itself a boundary. Each adapter is the explicit
/// boundary shim the policy asks for: `SmeltFromUnknown` recovers the typed
/// receiver, the real typed method runs, and the result is erased back out.
/// Where the class IS statically known, the call stays a typed method call and
/// none of this is emitted. Both sides are pinned by
/// `prototype_methods_exist_only_for_reflection` in
/// `src/tests/class_prototype_methods_tests.rs`.
pub(crate) fn class_prototype_methods_fn(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<Option<String>, EmitError> {
    if !context
        .demand()
        .has(crate::runtime_demand::Need::ClassPrototypeMethods)
    {
        return Ok(None);
    }
    let entries = prototype_method_entries(mir, context, class)?;
    if entries.is_empty() {
        return Ok(None);
    }
    let mut out = String::new();
    out.push_str("    /// The methods this class's prototype object carries, as `this`-taking erased functions.\n");
    out.push_str("    #[allow(dead_code)]\n");
    out.push_str(&format!(
        "    pub(crate) fn {}() -> Vec<(String, SmeltUnknown)> {{\n",
        smelt_stdlib::runtime_symbols::boundary::PROTOTYPE_METHODS
    ));
    out.push_str("        Vec::from([\n");
    for (key, adapter) in entries {
        out.push_str(&format!(
            "            ({key:?}.to_owned(), {adapter}),\n",
            adapter = adapter.delimited()
        ));
    }
    out.push_str("        ])\n    }\n");
    Ok(Some(out))
}

/// Emit the `__smelt_proto_entries` inherent method for a class.
///
/// Returns `None` when the class exposes no eligible method, so a class that
/// gains nothing keeps byte-identical output.
pub(crate) fn class_proto_entries_method(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<Option<String>, EmitError> {
    let mut pushes = Vec::new();
    for method in effective_class_methods(mir, class) {
        let Some(function) = function_by_id(mir, method) else {
            continue;
        };
        let HirOrigin::ClassMethod { method: name, .. } = function.origin else {
            continue;
        };
        let source_name = mir
            .names
            .get(name)
            .or_else(|| mir.symbols.get(name))
            .ok_or_else(|| EmitError::new("class method references an unknown symbol"))?
            .to_owned();
        if source_name.starts_with(SETTER_PREFIX) {
            continue;
        }
        let property = source_name.strip_prefix(GETTER_PREFIX);
        if !method_is_proto_eligible(mir, context, class.name, function) {
            let member = format!(
                "{}.{}",
                class_source_name(mir, class),
                property.unwrap_or(&source_name)
            );
            let key = format!("{METHOD_KEY_PREFIX}{}", property.unwrap_or(&source_name));
            pushes.push(format!(
                "        smelt_proto_entries.push(({key:?}.to_owned(), smelt_unbridged_member({member:?})));\n"
            ));
            continue;
        }
        let rust_method = context
            .names()
            .method(name)
            .ok_or_else(|| EmitError::new("class method has no planned Rust name"))?
            .as_str()
            .to_owned();
        let adapter = method_adapter(mir, context, function, &rust_method)?;
        let key = format!("{METHOD_KEY_PREFIX}{}", property.unwrap_or(&source_name));
        let member = if property.is_some() {
            format!(
                "SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_get\".to_owned(), {adapter})])))",
                adapter = adapter.delimited()
            )
        } else {
            adapter.into_string()
        };
        pushes.push(format!(
            "        smelt_proto_entries.push(({key:?}.to_owned(), {member}));\n"
        ));
    }
    pushes.extend(hidden_field_entries(mir, context, class)?);
    if let Some(adapter) = constructor_adapter(mir, context, class)? {
        pushes.push(format!(
            "        smelt_proto_entries.push(({CONSTRUCTOR_KEY:?}.to_owned(), {adapter}));\n",
            adapter = adapter.delimited()
        ));
    }
    if pushes.is_empty() {
        return Ok(None);
    }
    // Entries are keyed under the runtime's `__smelt_method:` prefix, which
    // `smelt_get_object_field` resolves after the own property misses and which
    // key enumeration, structural equality, hashing and JSON all skip: a class's
    // methods are non-enumerable.
    let mut out = String::new();
    out.push_str(
        "    /// Prototype-carried members of this class, as receiver-bound erased functions.\n",
    );
    out.push_str("    #[allow(dead_code)]\n");
    out.push_str(&format!(
        "    fn {PROTO_ENTRIES_METHOD}(&self) -> Vec<(String, SmeltUnknown)> {{\n"
    ));
    out.push_str(
        "        let mut smelt_proto_entries: Vec<(String, SmeltUnknown)> = Vec::new();\n",
    );
    for push in pushes {
        out.push_str(&push);
    }
    out.push_str("        smelt_proto_entries\n");
    out.push_str("    }\n");
    Ok(Some(out))
}

/// The source spelling of `class`'s name.
fn class_source_name<'mir>(mir: &'mir Mir, class: &MirClass) -> &'mir str {
    mir.names
        .get(class.name)
        .or_else(|| mir.symbols.get(class.name))
        .unwrap_or("class")
}

/// One entry per `#private` field, so an instance round-trips through the
/// erased carrier with its private state. Under the method prefix every
/// enumeration, equality and JSON view skips it; `value["#x"]` misses it,
/// since a member read of a `#` name does not consult the prefix.
fn hidden_field_entries(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> Result<Vec<String>, EmitError> {
    let base = if context.is_reference_class(class.name) {
        "self.0.borrow()"
    } else {
        "self"
    };
    class
        .fields
        .iter()
        .filter(|field| !field.visibility.is_own_property())
        .map(|field| {
            let source = mir
                .names
                .get(field.name)
                .or_else(|| mir.symbols.get(field.name))
                .unwrap_or("field");
            let key = format!("{METHOD_KEY_PREFIX}{source}");
            let rust_field = context.names().field(mir, field.name).into_string();
            let read = RustExpr::chain(format!("{base}.{rust_field}")).cloned_value();
            let value = record_field_unknown_expr(mir, &read, field.ty)?;
            Ok(format!(
                "        smelt_proto_entries.push(({key:?}.to_owned(), {value}));\n",
                value = value.delimited()
            ))
        })
        .collect()
}

/// Whether a class emits a `__smelt_proto_entries` method at all.
///
/// The erasure sites consult this before appending the call, so a class with no
/// eligible method keeps its previous output exactly.
pub(crate) fn class_has_proto_entries(mir: &Mir, context: &EmitContext, class: &MirClass) -> bool {
    effective_class_methods(mir, class)
        .into_iter()
        .filter_map(|method| function_by_id(mir, method))
        .any(|function| {
            matches!(function.origin, HirOrigin::ClassMethod { method, .. }
                if mir.symbols.get(method).is_some_and(|name| !name.starts_with(SETTER_PREFIX)))
        })
        || class
            .fields
            .iter()
            .any(|field| !field.visibility.is_own_property())
        || constructor_adapter(mir, context, class).is_ok_and(|adapter| adapter.is_some())
}

/// A class's static methods as `(property key, erased function)` entries of its
/// class value.
///
/// In JavaScript a static method is an own property of the class object, so
/// `Counter[key]()` and `pattern[P.matcher]()` on a class read through a
/// value find it there. A class value carried only its name and constructor,
/// so every such read missed and the call panicked. The key is the method's
/// source property name, which for a symbol-named method (`static [key]()`) is
/// the folded symbol key `smelt_property_key` computes from the runtime symbol.
///
/// The eligibility is [`method_is_proto_eligible`]'s, minus the receiver: a
/// generic class (no `Class::method` can be spelled without its arguments), a
/// generic, async or generator method, a rest parameter, or a parameter or
/// return the carrier cannot cross leaves the method out, as before.
pub(crate) fn class_static_entries(
    mir: &Mir,
    context: &EmitContext,
    class: smelt_hir::Symbol,
) -> Result<Vec<(String, RustExpr)>, EmitError> {
    let Some(found) = crate::classes::find_class(mir, class) else {
        return Ok(Vec::new());
    };
    if !found.type_params.is_empty() {
        return Ok(Vec::new());
    }
    let rust_class = context.names().type_name(class)?.as_str().to_owned();
    let mut entries = Vec::new();
    for function in &mir.functions {
        let HirOrigin::ClassStaticMethod {
            class: owner,
            method,
            ..
        } = function.origin
        else {
            continue;
        };
        if owner != class
            || !function.type_params.is_empty()
            || function.is_async
            || function.is_generator
            || !return_type_is_erasable(mir, function.return_ty)
            || !function.params.iter().enumerate().all(|(index, param)| {
                local_decl(function, *param).is_some_and(|local| {
                    param_crosses_erased_call(mir, context, function, index, local.ty)
                })
            })
        {
            continue;
        }
        let source_name = mir
            .names
            .get(method)
            .or_else(|| mir.symbols.get(method))
            .ok_or_else(|| EmitError::new("static method references an unknown symbol"))?
            .to_owned();
        let rust_method = crate::emitter::static_method_rust_name(context, function.id)?;
        let adapter = erased_callable_adapter(
            mir,
            context,
            function,
            &MethodReceiver::Static(rust_class.clone()),
            &rust_method,
        )?;
        entries.push((source_name, adapter));
    }
    Ok(entries)
}

/// Look up a MIR function by id.
fn function_by_id(mir: &Mir, id: FuncId) -> Option<&MirFunction> {
    mir.functions.get(usize::try_from(id.0).ok()?)
}

/// Look up a local declaration in a function by id.
fn local_decl(function: &MirFunction, local: LocalId) -> Option<&smelt_mir::LocalDecl> {
    function.locals.get(usize::try_from(local.0).ok()?)
}
