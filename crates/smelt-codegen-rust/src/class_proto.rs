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
//! erasure paths (the inline `class_unknown_object_text` adapter and the
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
//! rejects is simply absent — exactly the pre-existing behaviour — rather than
//! being routed through a fabricated value.

use smelt_hir::{Type, TypeId};
use smelt_mir::{FuncId, HirOrigin, Mir, MirClass, MirFunction};

use crate::emitter::{EmitContext, method_mutates_this};
use crate::{
    EmitError, classes::effective_class_methods, record_field_unknown_text, sanitize_ident,
    type_supports_from_unknown,
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

/// Prefixes the frontends give a synthesized accessor method.
///
/// A source `get x()` / `set x(v)` lowers to an ordinary method named
/// `__smelt_get_x` / `__smelt_set_x`; the accessor is reached in JavaScript by
/// *reading* `x`, never by calling `__smelt_get_x`. Publishing it as a callable
/// member would invent a method the source never declared, so accessors are
/// excluded here. Exposing them properly needs an accessor slot the erased
/// carrier does not yet have (a read that invokes), which is separate work.
const ACCESSOR_PREFIXES: [&str; 2] = ["__smelt_get_", "__smelt_set_"];

/// Whether `ty` can be produced from an erased `SmeltUnknown` result.
///
/// The outbound mirror of [`type_supports_from_unknown`]. `Never`, futures,
/// generators and bare function handles have no faithful erased rendering —
/// [`record_field_unknown_text`] answers `SmeltUnknown::Null` for them, which
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
/// 3. It declares no rest parameter, and every parameter type can be rebuilt
///    from the erased carrier ([`type_supports_from_unknown`]). A callback or
///    compiled-regex parameter has no inbound impl, so no adapter is emitted
///    rather than one that would not compile.
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
    function: &MirFunction,
) -> bool {
    let HirOrigin::ClassMethod { class, .. } = function.origin else {
        return false;
    };
    if function.is_async || function.is_generator || function.rest.is_some() {
        return false;
    }
    let HirOrigin::ClassMethod { method: name, .. } = function.origin else {
        return false;
    };
    let Some(spelling) = mir.symbols.get(name) else {
        return false;
    };
    if ACCESSOR_PREFIXES
        .iter()
        .any(|prefix| spelling.starts_with(prefix))
    {
        return false;
    }
    if !return_type_is_erasable(mir, function.return_ty) {
        return false;
    }
    if method_mutates_this(function) && !context.is_reference_class(class) {
        return false;
    }
    function.params.iter().skip(1).all(|param| {
        function
            .locals
            .get(param.0 as usize)
            .is_some_and(|local| type_supports_from_unknown(mir, local.ty))
    })
}

/// Render the erased adapter expression for one eligible method.
///
/// The adapter captures a receiver handle (`self.clone()` — an `Rc` bump for a
/// reference class, a struct copy for a value class whose methods cannot
/// mutate), coerces each positional argument in through `SmeltFromUnknown`
/// (padding missing arguments with `undefined`, as JavaScript does), calls the
/// real typed method, and erases the typed result back out.
fn method_adapter_text(
    mir: &Mir,
    function: &MirFunction,
    rust_method: &str,
) -> Result<String, EmitError> {
    let mut args = Vec::new();
    for (index, param) in function.params.iter().skip(1).enumerate() {
        let local = function
            .locals
            .get(param.0 as usize)
            .ok_or_else(|| EmitError::new("class method parameter has no local declaration"))?;
        let _ = local.ty;
        args.push(format!(
            "SmeltFromUnknown::smelt_from_unknown(smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined))"
        ));
    }
    let call = format!("smelt_receiver.{rust_method}({})", args.join(", "));
    // A throwing method returns `Result<_, Box<dyn Error>>`; the erased
    // callback's own return type is the same `Result`, so the `?` propagates
    // the thrown value across the seam rather than swallowing it.
    let call = if function.can_throw {
        format!("{call}?")
    } else {
        call
    };
    let result = if matches!(mir.types.get(function.return_ty), Some(Type::None)) {
        // A `void` method evaluates to `undefined` in JavaScript.
        "{ let () = smelt_result; SmeltUnknown::Undefined }".to_owned()
    } else {
        record_field_unknown_text(mir, "smelt_result", function.return_ty)?
    };
    Ok(format!(
        "SmeltUnknown::Function(::std::rc::Rc::new({{ let smelt_receiver = self.clone(); \
         move |smelt_args: Vec<SmeltUnknown>| {{ let _ = &smelt_args; \
         let smelt_result = {call}; Ok({result}) }} }}))"
    ))
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
        if !method_is_proto_eligible(mir, context, function) {
            continue;
        }
        let HirOrigin::ClassMethod { method: name, .. } = function.origin else {
            continue;
        };
        let source_name = mir
            .names
            .get(name)
            .or_else(|| mir.symbols.get(name))
            .ok_or_else(|| EmitError::new("class method references an unknown symbol"))?
            .to_owned();
        let rust_method = sanitize_ident(
            mir.symbols
                .get(name)
                .ok_or_else(|| EmitError::new("class method references an unknown symbol"))?,
        );
        let adapter = method_adapter_text(mir, function, &rust_method)?;
        let key = format!("{METHOD_KEY_PREFIX}{source_name}");
        pushes.push(format!(
            "        smelt_proto_entries.push(({key:?}.to_owned(), {adapter}));\n"
        ));
    }
    if pushes.is_empty() {
        return Ok(None);
    }
    let mut out = String::new();
    out.push_str("    /// Prototype-carried members of this class, as receiver-bound erased functions.\n");
    out.push_str("    ///\n");
    out.push_str("    /// Keyed under the runtime's `__smelt_proto:` prefix, which `smelt_get_object_field`\n");
    out.push_str("    /// resolves through and which key enumeration, structural equality and JSON skip.\n");
    out.push_str("    #[allow(dead_code)]\n");
    out.push_str(&format!(
        "    fn {PROTO_ENTRIES_METHOD}(&self) -> Vec<(String, SmeltUnknown)> {{\n"
    ));
    out.push_str("        let mut smelt_proto_entries: Vec<(String, SmeltUnknown)> = Vec::new();\n");
    for push in pushes {
        out.push_str(&push);
    }
    out.push_str("        smelt_proto_entries\n");
    out.push_str("    }\n");
    Ok(Some(out))
}

/// Whether a class emits a `__smelt_proto_entries` method at all.
///
/// The erasure sites consult this before appending the call, so a class with no
/// eligible method keeps its previous output exactly.
pub(crate) fn class_has_proto_entries(
    mir: &Mir,
    context: &EmitContext,
    class: &MirClass,
) -> bool {
    effective_class_methods(mir, class)
        .into_iter()
        .filter_map(|method| function_by_id(mir, method))
        .any(|function| method_is_proto_eligible(mir, context, function))
}

/// Look up a MIR function by id.
fn function_by_id(mir: &Mir, id: FuncId) -> Option<&MirFunction> {
    mir.functions.get(usize::try_from(id.0).ok()?)
}
