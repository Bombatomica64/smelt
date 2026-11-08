//! `instanceof`: user classes, host classes and their markers, erased values and
//! host overrides.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Whether an `instanceof` operand's identity is a RUNTIME fact.
    ///
    /// True for the erased shapes — `unknown`, an unscoped type parameter, a
    /// union, an optional — whose storage does not settle which class the value
    /// belongs to, so the answer has to come from an identity marker on the
    /// value itself. False for concrete storage, where the type settles it.
    ///
    /// Shared by the per-class arms and by the marker probe they fall through
    /// to, so an arm cannot decline an operand the probe would have answered.
    fn instanceof_operand_is_dynamic(&self, value_ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        )
    }

    /// Emit a class-instance test.
    ///
    /// Smelt keeps this as an MIR rvalue instead of folding it in the
    /// TypeScript frontend, and codegen picks the check from the operand's
    /// storage. A concrete operand answers from its static type. An erased
    /// operand (see `instanceof_operand_is_dynamic`) answers at runtime from
    /// the identity markers on the value (`__smelt_error`, the
    /// `host_instance_markers` probe, ...). A host constructor the crate
    /// reassigns answers by reading its override slot. In every case the
    /// operand has already been evaluated by MIR lowering.
    pub(super) fn instance_of_expr(
        &self,
        value: &Operand,
        class: Symbol,
    ) -> Result<RustExpr, EmitError> {
        let value_ty = self.operand_ty(value)?;
        let class_name = self.symbol_name(class)?;
        if let Some(check) =
            self.concrete_union_class_check(&self.operand_expr(value)?, value_ty, class)
        {
            return Ok(RustExpr::from(check));
        }
        // The same union one optional deep. Asked here, before the marker probe
        // below, because that probe treats any `Optional` operand as erased and
        // would emit a `SmeltUnknown::Object` pattern against an
        // `Option<SmeltUnion…>`.
        if let Some(check) =
            self.optional_union_class_check(&self.operand_expr(value)?, value_ty, class)
        {
            return Ok(RustExpr::from(check));
        }
        // A host constructor this crate REASSIGNS (`globalThis.File = class File
        // extends Blob {}`) lives in an override slot, and `instanceof` reads the
        // binding — so the check has to read the slot too. The static marker
        // probe below answers for the native builtin only, which made
        // `new File(...) instanceof File` false for exactly the override the
        // crate had just installed. Every other spelling of "is the global
        // present / what does it construct" already goes through the slot.
        if let Some(check) = self.host_override_instance_of_expr(value, value_ty, class_name)? {
            return Ok(check);
        }
        // A class the program declares is its own class, whatever host class
        // its name spells: answered by the class hierarchy, or by the erased
        // instance's class marker.
        if self.mir.has_class(class) {
            return self.declared_class_instance_of_expr(value, value_ty, class);
        }
        // One list of builtin error classes, shared with the erasure that stamps
        // `__smelt_error` for a user class whose base chain reaches one
        // (`host_base_markers`), so the probe and the marker cannot disagree.
        if smelt_stdlib::is_error_class_name(class_name)
            && matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        ) {
            let value_text = self.operand_expr(value)?;
            // `Error` is the base class, so ANY error marker satisfies it. A
            // subclass must match the class name the marker records — every error
            // class used to share one boolean marker, so `new Error('x') instanceof
            // AggregateError` answered `true`. es-toolkit `clone` branches on
            // exactly that and rebuilt a plain Error as
            // `new Ctor(obj.errors, obj.message, ..)`, putting `errors` in the
            // message slot and dropping the message.
            //
            // This models the one level of the built-in hierarchy that exists:
            // every built-in error derives directly from `Error`, so a subclass
            // check is an equality test on the recorded name. A USER class
            // `extends Error` resolves through the class path while it is still
            // typed; once it has crossed an erasure seam into a `SmeltUnknown`
            // (which is exactly what a predicate like `isError(value: unknown)`
            // does) only the markers survive, so its erasure stamps
            // `__smelt_error` with the nearest builtin error base's name — see
            // `host_base_markers` — and answers these same probes.
            let marker_probe = if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::Error) {
                std::iter::once(SMELT_ERROR)
                    .chain(smelt_stdlib::host_markers_extending(class_name))
                    .map(|marker| format!("value.contains_key(\"{marker}\")"))
                    .collect::<Vec<_>>()
                    .join(" || ")
            } else {
                self.carrier_format(format_args!(
                    "matches!(value.get(\"{SMELT_ERROR}\"), Some(SmeltUnknown::String(smelt_error_class)) if &*smelt_error_class == {class_name:?})"
                ))
            };
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if {marker_probe})"
                ))));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if {marker_probe})"
            ))));
        }
        // A concrete source `Map`/`Set` is unconditionally an instance of its own
        // class, and any OTHER concrete operand carries no such identity. Those
        // are static answers about storage, which is why the two keep an arm; the
        // dynamic case falls through to the shared marker probe below, where
        // `__smelt_map` / `__smelt_set` answer it like any other identity marker.
        if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::Map) {
            if matches!(self.mir.types.get(value_ty), Some(Type::JsMap(_, _))) {
                return Ok(RustExpr::path("true"));
            }
            if !self.instanceof_operand_is_dynamic(value_ty) {
                return Ok(RustExpr::path("false"));
            }
        }
        if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::Set) {
            if matches!(self.mir.types.get(value_ty), Some(Type::Set(_))) {
                return Ok(RustExpr::path("true"));
            }
            if !self.instanceof_operand_is_dynamic(value_ty) {
                return Ok(RustExpr::path("false"));
            }
        }
        self.host_instance_of_expr(value, class, value_ty, class_name)
    }

    /// `instanceof` against host classes, then user classes.
    fn host_instance_of_expr(
        &self,
        value: &Operand,
        class: Symbol,
        value_ty: TypeId,
        class_name: &str,
    ) -> Result<RustExpr, EmitError> {
        // A CONCRETE family value answers from its own type, before the erased
        // marker probes below, which would be handed a `SmeltTypedArray` /
        // `SmeltArrayBuffer` where they expect a `SmeltUnknown`.
        if let Some(check) = self.typed_array_instance_of_expr(value, class_name)? {
            return Ok(check);
        }
        if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::ArrayBuffer)
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_expr(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"{SMELT_ARRAYBUFFER}\"))"
                ))));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"{SMELT_ARRAYBUFFER}\"))"
            ))));
        }
        // A CONCRETE blob answers both spellings statically: it always is a
        // `Blob`, and it is a `File` exactly when its optional name is present
        // (the two share one Rust type — see `blob_prelude`). Without these arms
        // the marker probe below would be handed a `SmeltBlob` where it expects
        // a `SmeltUnknown`.
        if smelt_stdlib::typescript_stdlib_class(class_name)
            .is_some_and(smelt_stdlib::StdlibClass::is_blob_runtime_type)
            && self.is_blob_class_type(value_ty)?
        {
            if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::File) {
                return Ok({ let receiver = self.operand_expr(value)?; RustExpr::postfix_on(&receiver, format!("{receiver}.is_file()")) });
            }
            return Ok(RustExpr::path("true"));
        }
        if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::Blob)
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_expr(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"{SMELT_BLOB}\"))"
                ))));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"{SMELT_BLOB}\"))"
            ))));
        }
        if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::Number)
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_expr(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"{SMELT_NUMBER}\"))"
                ))));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"{SMELT_NUMBER}\"))"
            ))));
        }
        if smelt_stdlib::instance_of_rule(class_name) == Some(smelt_stdlib::InstanceOfRule::Promise)
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_expr(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Promise(_)))"
                ))));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "matches!({value_text}.clone(), SmeltUnknown::Promise(_))"
            ))));
        }
        // `AbortController`/`AbortSignal` erase to marker-bearing records (see
        // `abort_controller_constructor_expression`). They are recognized by
        // their `__smelt_abortcontroller` / `__smelt_abortsignal` markers on the
        // erased object, whether the static value type is dynamic (`unknown`,
        // generics, unions) or the erased abort class itself.
        // Marker-only host builtins (WeakMap/WeakSet/DataView/SharedArrayBuffer/
        // File) erase to records carrying a dedicated identity marker (see
        // `marker_only_builtin_constructor_expression` in the frontend). They
        // share the abort-marker recognition path: a dynamic or erased-class
        // value resolves `instanceof X` through its marker key.
        // Boxed primitive wrappers (`Boolean`/`String`) share the same marker
        // recognition path: a primitive `true`/`"a"` erases to `SmeltUnknown::Bool`
        // / `SmeltUnknown::String` (never an `Object`), so the marker check is the
        // correct `false`, while a boxed wrapper object carrying the dedicated
        // marker resolves to `true`.
        // Every modeled host object and boxed primitive wrapper — including the
        // helper-backed AbortController/AbortSignal entries — resolves its marker
        // through the shared `smelt_stdlib::host_object` registry, so this `instanceof` path
        // cannot drift from the frontend construction path or the runtime for-in
        // filter. (`ArrayBuffer`/`Blob`/`Number` are already handled by dedicated
        // branches above; sourcing them here too is harmless since those
        // short-circuit first.)
        // The shared identity probe. `host_instance_markers` answers for every
        // class whose identity survives erasure — the host-object registry and
        // the subsystem-owned markers alike — so this one arm replaces what used
        // to be a separate `if class_name == ".."` block per class, and a class
        // that gains a marker is answered here without touching this function.
        if let Some(markers) = host_instance_markers(class_name) {
            if self.instanceof_operand_is_dynamic(value_ty) || self.is_erased_class_type(value_ty) {
                let value_text = self.operand_expr(value)?;
                let probe = markers
                    .iter()
                    .map(|marker| format!("value.contains_key(\"{marker}\")"))
                    .collect::<Vec<_>>()
                    .join(" || ");
                if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                    return Ok(RustExpr::atom(self.carrier_format(format_args!(
                        "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if {probe})"
                    ))));
                }
                return Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "matches!({value_text}.clone(), SmeltUnknown::Object(value) if {probe})"
                ))));
            }
        }
        // Nothing above recognized the pair. For a CONCRETE operand that is a
        // real static answer: the class hierarchy settles whether the operand's
        // class is or extends the target.
        if let Some(Type::Class { name, .. }) = self.mir.types.get(value_ty) {
            return Ok(RustExpr::from(self.class_extends_or_equals(*name, class).to_string()));
        }
        // For a DYNAMIC operand it is not, and round 10 made that a blocker
        // rather than a `false` fold, because folding silently deleted the
        // branch: `const x: unknown = new TextEncoder(); if (x instanceof
        // TextEncoder)` became `if false` with no diagnostic.
        //
        // Every modeled class now carries an identity marker and erases through
        // its own adapter, so the probe above answers all of them and nothing
        // modeled reaches here any more. The blocker stays as the honest answer
        // for a target that is modeled but has no marker — the state no class is
        // in today, and the state a NEW modeled class starts in before its
        // erasure adapter is written. Keeping it makes that omission a build
        // error instead of a deleted branch.
        if self.instanceof_operand_is_dynamic(value_ty)
            && smelt_stdlib::typescript_stdlib_class(class_name).is_some()
        {
            return Err(EmitError::new(self.carrier_format(format_args!(
                "`instanceof {class_name}` on a dynamically-typed value is not modeled: \
                 `{class_name}` values carry no identity marker on an erased value, so the \
                 check cannot be answered at runtime. A modeled class needs a host-object \
                 registry marker and an `IntoSmeltUnknown` that stamps it."
            ))));
        }
        if let Some(check) = self.erased_user_class_instance_of_expr(value, value_ty, class)? {
            return Ok(check);
        }
        Ok(RustExpr::path("false"))
    }

    /// `value instanceof C` for a class `C` the program declares.
    fn declared_class_instance_of_expr(
        &self,
        value: &Operand,
        value_ty: TypeId,
        class: Symbol,
    ) -> Result<RustExpr, EmitError> {
        if let Some(Type::Class { name, .. }) = self.mir.types.get(value_ty) {
            return Ok(RustExpr::from(self.class_extends_or_equals(*name, class).to_string()));
        }
        if let Some(check) = self.erased_user_class_instance_of_expr(value, value_ty, class)? {
            return Ok(check);
        }
        Ok(RustExpr::path("false"))
    }

    /// Answer `value instanceof C` for an ERASED operand and a source class `C`.
    ///
    /// A class instance that crosses an erasure seam keeps its provenance in
    /// the hidden `__smelt_class` marker (stamped with the class's source name
    /// by the struct erasure), so the check is a runtime probe of that marker
    /// against every generated class that is `C` or extends it. Folding it to
    /// `false` instead silently deleted the branch: a caught
    /// `class UnsupportedPathError extends Error {}` value — a `catch` binding is
    /// always erased — never answered `instanceof UnsupportedPathError`.
    ///
    /// Answers `None` for a target that is not a generated class, and for an
    /// operand whose Rust value is not a `SmeltUnknown` (or an optional one),
    /// so the caller keeps its existing answer.
    fn erased_user_class_instance_of_expr(
        &self,
        value: &Operand,
        value_ty: TypeId,
        class: Symbol,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !self.mir.has_class(class) {
            return Ok(None);
        }
        let optional = match self.mir.types.get(value_ty) {
            Some(Type::Unknown | Type::TypeParam { .. }) => false,
            Some(Type::Union(_)) if self.concrete_union_members(value_ty).is_none() => false,
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Unknown)) =>
            {
                true
            }
            _ => return Ok(None),
        };
        let mut names = Vec::new();
        for item in &self.mir.classes {
            if self.class_extends_or_equals(item.name, class) {
                let name = format!("{:?}", self.symbol_source_name(item.name)?);
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let probe = self.carrier_format(format_args!(
            "matches!(value.get(\"{SMELT_CLASS}\"), Some(SmeltUnknown::String(smelt_class_name)) if {})",
            names.iter().map(|name| format!("smelt_class_name.eq_str({name})")).collect::<Vec<_>>().join(" || ")
        ));
        let value_text = self.operand_expr(value)?;
        Ok(Some(if optional {
            RustExpr::atom(self.carrier_format(format_args!("matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if {probe})")))
        } else {
            RustExpr::atom(self.carrier_format(format_args!("matches!({value_text}.clone(), SmeltUnknown::Object(value) if {probe})")))
        }))
    }

    /// Render a method call through a stored callable field when the receiver's
    /// concrete Rust representation carries virtual method slots as fields.
    ///
    /// Abstract/base TypeScript methods such as `Setter.validate(...)` can be
    /// represented by generated struct fields so subclass instances can be
    /// converted back to the base shape without losing overrides. In that ABI,
    /// `receiver.method(args)` must call `receiver.method` as a function field
    /// instead of the base inherent method.
    pub(super) fn call_virtual_function_field_expr(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
    ) -> Result<Option<RustExpr>, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let Some(field) = self
            .structural_record_fields(receiver_ty)
            .into_iter()
            .flatten()
            .find(|field| {
                field.name == method
            })
        else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.mir.types.get(field.ty) else {
            return Ok(None);
        };

        // The function-valued FIELD of the method's symbol, spelled as that
        // struct declares it (a field, not the method's planned name).
        let method_name = self.context.names().field(self.mir, field.name);
        let receiver_text = self.operand_expr(receiver)?;
        let rendered_args = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let Some(param_ty) = function.params.get(index).copied() else {
                    return self.operand_expr(arg);
                };
                self.value_at_type(arg, param_ty)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        // A reference class keeps its declared fields inside the shared cell, so
        // the callable field is read through `.0.borrow()` exactly as
        // `place_expr` reads any other declared field of one. Without this the
        // call named a field the handle newtype does not have (E0609). The
        // `.clone()` on the read is what ends the borrow guard before the call
        // runs, which matters here more than anywhere: the closure being called
        // is precisely the one that borrows the same cell to mutate the
        // instance.
        let call = if self.is_reference_class_type(receiver_ty) {
            // The `Ref` guard from `.0.borrow()` lives to the end of the
            // enclosing statement, and the callable being read is typically the
            // class-field arrow that mutates that very cell ("already
            // borrowed"). Bind it first so the guard drops before the call.
            RustExpr::atom(format!(
                "{{ let smelt_callable = {receiver_text}.0.borrow().{method_name}.clone(); (smelt_callable)({rendered_args}) }}"
            ))
        } else if rendered_args.is_empty() {
            RustExpr::atom(format!("({receiver_text}.{method_name}.clone())()"))
        } else {
            RustExpr::atom(format!("({receiver_text}.{method_name}.clone())({rendered_args})"))
        };
        if self.function_value_call_is_fallible(function) {
            Ok(Some(RustExpr::atom(format!("{call}?"))))
        } else {
            Ok(Some(call))
        }
    }

    /// Slot-aware `instanceof` for a host name this crate reassigns.
    ///
    /// Returns `None` when the class has no override slot in this crate, no
    /// identity marker to recognize its native records by, or a statically
    /// concrete operand that the ordinary class path answers — leaving every
    /// other case exactly as it was.
    fn host_override_instance_of_expr(
        &self,
        value: &Operand,
        value_ty: TypeId,
        class_name: &str,
    ) -> Result<Option<RustExpr>, EmitError> {
        if !crate::stdlib::host_override_slot_names(self.mir)
            .iter()
            .any(|(_, name)| name == class_name)
        {
            return Ok(None);
        }
        let Some(markers) = host_instance_markers(class_name) else {
            return Ok(None);
        };
        let value_is_dynamic = matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        );
        if !(value_is_dynamic || self.is_erased_class_type(value_ty)) {
            return Ok(None);
        }
        let value_text = self.operand_expr(value)?;
        let probed = if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
            self.carrier_format(format_args!("{value_text}.clone().unwrap_or(SmeltUnknown::Undefined)"))
        } else {
            format!("{value_text}.clone()")
        };
        let marker_list = markers
            .iter()
            .map(|marker| format!("\"{marker}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let slot = RustExpr::opaque(format!(
            "{prefix}{suffix}",
            prefix = smelt_stdlib::runtime_symbols::host_override::SLOT_PREFIX,
            suffix = crate::stdlib::host_override_slot_suffix(class_name),
        ));
        Ok(Some(RustExpr::chain(format!(
            "{slot}.with(|smelt_slot| smelt_host_override_instance_of(smelt_slot, &{probed}, &[{marker_list}]))"
        ))))
    }

    /// Return whether `source` is `target` or derives from it.
    ///
    /// Walks the declared base chain, which is how a statically typed
    /// `instanceof` between two generated classes is answered.
    pub(super) fn class_extends_or_equals(&self, source: Symbol, target: Symbol) -> bool {
        let mut current = source;
        loop {
            if current == target {
                return true;
            }
            let Some(class) = crate::classes::find_class(self.mir, current) else {
                break;
            };
            let Some(base) = class.base else {
                return false;
            };
            current = base;
        }
        // The chain left the program at a builtin (`class X extends
        // DOMException`), whose own ancestors are the registry's to answer.
        match (self.symbol_name(current), self.symbol_name(target)) {
            (Ok(builtin), Ok(ancestor)) => smelt_stdlib::builtin_class_extends(builtin, ancestor),
            _ => false,
        }
    }
}

/// The identity markers a `value instanceof <class>` probe must accept.
///
/// Usually one — the class's own registry marker. It is more than one where the
/// platform has a real subclass relation, which the registry already records in
/// `to_string_tag`: Node's `Buffer` *is* a `Uint8Array`, reports
/// `[object Uint8Array]`, and must satisfy `buf instanceof Uint8Array`. Deriving
/// the set from the tag keeps the subclass relation in the registry rather than
/// hard-coded at this call site, and returns `None` for names that are not
/// modeled host objects so the caller falls through to its class dispatch.
fn host_instance_markers(class_name: &str) -> Option<Vec<&'static str>> {
    let markers = smelt_stdlib::HOST_OBJECTS
        .iter()
        .filter(|entry| entry.class_name == class_name || entry.to_string_tag == class_name)
        .map(|entry| entry.marker)
        .collect::<Vec<_>>();
    if !markers.is_empty() {
        return Some(markers);
    }
    // The markers owned by a SUBSYSTEM rather than by the host-object registry.
    //
    // `host_object_markers()` deliberately excludes these — the runtime tracks
    // dates, regexps, maps and sets through their own helpers, and the
    // `for...in` filter must not treat them as host records. But `instanceof`
    // asks a different question, and for that question they are no different
    // from a registry marker: the erasure stamps the key, so an erased value's
    // identity is recoverable from it.
    //
    // Naming them here rather than at four separate `if class_name == ".."`
    // arms is what lets ONE probe below serve every class. Each arm that
    // remains does so because it asks something OTHER than marker presence,
    // and says which.
    //
    // `AbortController`/`AbortSignal` used to be here too. They are registry
    // entries now (`HostObject::helper_backed_methods`), so the branch above
    // answers for them and their internal keys are hidden from `for...in` like
    // every other host record's.
    let subsystem = smelt_stdlib::subsystem_instance_marker(class_name)?;
    Some(vec![subsystem])
}
