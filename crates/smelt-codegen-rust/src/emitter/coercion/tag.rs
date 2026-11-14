//! The `tag_check` verb and the `typeof` family: runtime narrowing of an erased
//! value, and the prototype and object helpers that read the same tags.

use super::*;

impl FunctionEmitter<'_> {
    /// Identity markers a class carries because its base chain reaches a modeled
    /// host object or a builtin `Error`.
    ///
    /// Walks the single-inheritance base chain through `mir.classes`; the first
    /// base that names a registered host object (`smelt_stdlib::host_object_by_class`)
    /// contributes its marker. `File` additionally contributes `Blob`'s marker,
    /// matching the host subtype relationship the native `new File(...)` records
    /// stamp. Returns an empty vector for a class with no host base (the common
    /// case, which keeps existing erased output byte-identical).
    ///
    /// A builtin `Error` class is not a `HOST_OBJECTS` entry — errors are modeled
    /// by the `__smelt_error: "<ClassName>"` convention instead — so a class whose
    /// chain reaches one contributes that marker with the NEAREST BUILTIN base's
    /// name as its value. That is what makes an erased `class CustomError extends
    /// Error` instance answer `instanceof Error` (the marker-presence probe) while
    /// `class MyTypeError extends TypeError` also satisfies `instanceof TypeError`
    /// (the recorded-name equality arm) and not `instanceof RangeError`; the user
    /// class itself keeps resolving through `__smelt_class`. `instance_of_expr`
    /// answers those probes off the same `smelt_stdlib::is_error_class_name` list,
    /// so neither side carries a hand-maintained copy of the error hierarchy.
    ///
    /// Each entry is the marker key plus the Rust text of its VALUE: an identity
    /// marker is a bare `true`, while the error marker records which builtin
    /// error class the chain reached.
    pub(in crate::emitter) fn host_base_markers(
        &self,
        class_name: Symbol,
    ) -> Result<Vec<(&'static str, String)>, EmitError> {
        let mut markers: Vec<(&'static str, String)> = Vec::new();
        let mut current = Some(class_name);
        for _ in 0u32..64 {
            let Some(name_sym) = current else { break };
            let Some(class) = crate::classes::find_class(self.mir, name_sym) else {
                break;
            };
            let Some(base) = class.base else { break };
            let Some(base_name) = self.mir.symbols.get(base) else {
                break;
            };
            if smelt_stdlib::host_object_by_class(base_name).is_some() {
                let mut host = Some(base_name);
                while let Some(entry) = host.and_then(smelt_stdlib::host_object_by_class) {
                    markers.push((
                        entry.marker,
                        self.carrier_expr(
                            self.carrier()?,
                            &RustExpr::atom("SmeltUnknown::Bool(true)"),
                        )
                        .into_string(),
                    ));
                    host = entry.base;
                }
                break;
            }
            if smelt_stdlib::is_error_class_name(base_name) {
                markers.push((
                    SMELT_ERROR,
                    self.carrier_format(
                        self.carrier()?,
                        format_args!("SmeltUnknown::String({base_name:?}.into())"),
                    ),
                ));
                break;
            }
            current = Some(base);
        }
        Ok(markers)
    }

    /// The built-in error class `class_name` extends, if its base chain
    /// reaches one (`class AbortError extends Error` is `Some("Error")`).
    pub(in crate::emitter) fn error_base_name(&self, class_name: Symbol) -> Option<String> {
        let mut current = Some(class_name);
        for _ in 0u32..64 {
            let class = crate::classes::find_class(self.mir, current?)?;
            let base = class.base?;
            let base_name = self.mir.symbols.get(base)?;
            if smelt_stdlib::host_object_by_class(base_name).is_some() {
                return None;
            }
            if smelt_stdlib::is_error_class_name(base_name) {
                return Some(base_name.to_owned());
            }
            current = Some(base);
        }
        None
    }

    /// Emits a runtime tag check for `SmeltUnknown`.
    pub(in crate::emitter) fn tag_check(
        &self,
        value: &Operand,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        let text = self.operand_expr(value)?;
        let value_ty = self.operand_ty(value)?;
        // A statically-`None` operand is an absent/`undefined` value (for
        // example a reference to an ambient host global that Smelt's non-DOM
        // profile does not model, such as `window`/`process`). It renders as the
        // Rust unit `()`, which cannot be matched against a `SmeltUnknown` tag
        // pattern (E0308). Fold the tag check to a compile-time constant with JS
        // semantics: an `undefined` value is nullish (`== null`) and `typeof`
        // `"undefined"`, but is not a boolean/number/string/object/etc.
        if matches!(self.mir.types.get(value_ty), Some(Type::None)) {
            let is_nullish = matches!(
                kind,
                smelt_hir::UnknownKind::Null | smelt_hir::UnknownKind::Undefined
            );
            return Ok(if is_nullish { "true" } else { "false" }.to_owned());
        }
        if let Some(check) = self.concrete_union_tag_check(&text, value_ty, kind) {
            return Ok(check);
        }
        if let Some(&Type::Optional(inner)) = self.mir.types.get(value_ty) {
            if kind == smelt_hir::UnknownKind::Null {
                return Ok(format!("{text}.is_none()", text = text.tight()));
            }
            if kind == smelt_hir::UnknownKind::Undefined {
                return Ok(format!("{text}.is_none()", text = text.tight()));
            }
            // The payload's own type may already decide the test, in which case
            // only PRESENCE is left to check at run time: `Array.isArray(xs)`
            // for `xs: string[] | undefined` is exactly `xs.is_some()`, and for
            // `xs: string | undefined` it is `false` whether or not `xs` is
            // there. See `static_tag_check` for why a concrete payload cannot
            // take the erased path below.
            if let Some(decided) = self.static_tag_check(inner, kind) {
                return Ok(if decided {
                    format!("{text}.is_some()", text = text.tight())
                } else {
                    "false".to_owned()
                });
            }
            // A concrete-union `Option` payload is a tagged enum, so the present
            // value is narrowed against its `SmeltUnion…` variants rather than
            // erased `SmeltUnknown` tags.
            let check =
                match self.concrete_union_tag_check(&RustExpr::path("smelt_value"), inner, kind) {
                    Some(check) => check,
                    None => self.tag_check_raw(&RustExpr::path("smelt_value"), kind)?,
                };
            return Ok(format!(
                "{text}.as_ref().is_some_and(|smelt_value| {check})",
                text = text.tight()
            ));
        }
        // Same rule without the `Option` wrapper: a value whose static type
        // answers the test needs no runtime match, and cannot be given one.
        if let Some(decided) = self.static_tag_check(value_ty, kind) {
            return Ok(decided.to_string());
        }
        self.tag_check_raw(&text, kind)
    }

    /// Emits the JavaScript `typeof` string for a value.
    ///
    /// The answer is one of seven fixed spellings, so both paths select a
    /// `&'static str` and only then take the owned `String` the destination
    /// wants -- a hand-written port would not build a fresh `String` per
    /// evaluation just to name a type.
    pub(in crate::emitter) fn typeof_value(&self, value: &Operand) -> Result<RustExpr, EmitError> {
        let receiver = self.typeof_str_expr(value)?;
        if self.indexes_utf16() {
            return Ok(RustExpr::chain(format!(
                "JsString::from({receiver})",
                receiver = receiver.delimited()
            )));
        }
        Ok(receiver.postfix(".to_owned()"))
    }

    /// Emits the JavaScript `typeof` spelling for a value as a `&'static str`.
    ///
    /// The borrowed half of [`Self::typeof_value`], so a destination that
    /// can hold a `&'static str` skips the copy entirely.
    ///
    /// A concrete union stays inline: its arms are decided per member type at
    /// compile time, so there is no shared table to factor out. An erased value
    /// goes to the `smelt_typeof` prelude helper instead -- the arms are the same
    /// ten variants (seven spellings) at every site, so inlining the match once per source `typeof`
    /// is pure duplication.
    pub(in crate::emitter) fn typeof_str_expr(
        &self,
        value: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let value_ty = self.operand_ty(value)?;
        // `typeof` only reads the value's tag, so both paths take it by shared
        // reference. A callback parameter already spelled `&T` is passed through
        // rather than borrowed again -- see `shared_reference_argument_expr` for
        // why a `&&T` is not what a hand-written port would spell.
        let place = self.operand_borrow_expr(value)?;
        let reference = if self.operand_renders_as_shared_reference(value) {
            place
        } else {
            place.borrowed_value()
        };
        if let Some(members) = self.concrete_union_members(value_ty) {
            let name = union::union_name(value_ty);
            let arms = members
                .iter()
                .enumerate()
                .map(|(index, member)| {
                    let kind = self.typeof_static_type_text(*member);
                    format!("{name}::M{index}(_) => {kind:?}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(RustExpr::atom(format!(
                "match {reference} {{ {arms} }}",
                reference = reference.delimited()
            )));
        }
        if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
            let erased = self.erase(value)?;
            return Ok(RustExpr::chain(format!(
                "smelt_typeof(&{erased})",
                erased = erased.tight()
            )));
        }
        Ok(RustExpr::chain(format!(
            "smelt_typeof({reference})",
            reference = reference.delimited()
        )))
    }

    /// Return the JavaScript `typeof` spelling for one concrete union member.
    ///
    /// A `None` or optional arm is `"undefined"`; every other arm answers
    /// through the shared concrete-type map (`Type::runtime_kind`), so a `Map`
    /// or generator arm is `"object"` here as it is under `static_tag_check`.
    fn typeof_static_type_text(&self, ty: TypeId) -> &'static str {
        match self.mir.types.get(ty) {
            Some(Type::None | Type::Optional(_)) | None => "undefined",
            Some(member) => member
                .runtime_kind()
                .map_or("undefined", smelt_hir::UnknownKind::typeof_name),
        }
    }

    /// Emits the opaque `Object.getPrototypeOf` sentinel for an erased value.
    ///
    /// Defers all prototype discrimination to the `smelt_prototype_sentinel`
    /// runtime helper so the array/null/plain-object/class branches stay in one
    /// place. Class instances carry a hidden `__smelt_class` marker and map to a
    /// distinct sentinel, while non-class values keep their existing sentinels.
    /// The operand is rendered through the erased-`unknown` coercion seam so a
    /// source spelling that narrows the receiver first (e.g. an
    /// `Optional(unknown)` from `as typeof Object.prototype | null`) still
    /// hands the helper a plain `SmeltUnknown`.
    /// With `own_slot_shadows` set the emission goes through
    /// `smelt_proto_accessor` instead, which answers an own `__proto__` slot
    /// before falling back to the sentinel. That is the `v.__proto__` accessor
    /// rather than `Object.getPrototypeOf(v)`; see
    /// [`smelt_hir::ExprKind::PrototypeSentinel`] for why the two differ.
    pub(in crate::emitter) fn prototype_sentinel_expr(
        &self,
        value: &Operand,
        own_slot_shadows: bool,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        let helper = if own_slot_shadows {
            "smelt_proto_accessor"
        } else {
            "smelt_prototype_sentinel"
        };
        Ok(RustExpr::chain(format!(
            "{helper}(&({text}))",
            text = text.delimited()
        )))
    }

    /// Emits `Object(value)` as a boxed primitive.
    ///
    /// Defers to the `smelt_box_value` runtime helper, which wraps a primitive in
    /// the same marker shape `new Number(..)` / `new Boolean(..)` /
    /// `new String(..)` build and passes objects through unchanged. The branch is
    /// necessarily a runtime one — a `Type::Unknown` argument's tag is only known
    /// then — so the operand goes through the erased-`unknown` coercion seam.
    pub(in crate::emitter) fn box_primitive_expr(
        &self,
        value: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        Ok(RustExpr::chain(format!(
            "smelt_box_value({text})",
            text = text.delimited()
        )))
    }

    /// Emits `Object.create(proto)` as a fresh erased object.
    ///
    /// Defers to the `smelt_object_from_prototype` runtime helper so the
    /// null-prototype, opaque-`__smelt_proto:*`-sentinel and concrete-prototype
    /// branches stay in one place. The operand goes through the erased-`unknown`
    /// coercion seam because a prototype is typically the `SmeltUnknown` returned
    /// by `smelt_prototype_sentinel`.
    pub(in crate::emitter) fn object_from_prototype_expr(
        &self,
        prototype: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(prototype, unknown_ty)?;
        Ok(RustExpr::chain(format!(
            "smelt_object_from_prototype({text})",
            text = text.delimited()
        )))
    }

    /// Emits `Object.defineProperty` / `Object.defineProperties` as a property
    /// installation on an erased object.
    ///
    /// Both operands cross the erased-`unknown` seam because a descriptor table
    /// is an ordinary JavaScript object whose values have no common static
    /// shape (a data descriptor holds `value`, an accessor descriptor holds
    /// `get`). The runtime helper returns the target, which is what the two
    /// `Object` statics answer.
    pub(in crate::emitter) fn define_properties_expr(
        &self,
        target: &Operand,
        descriptors: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let target_text = self.value_at_type(target, unknown_ty)?;
        let descriptors_text = self.value_at_type(descriptors, unknown_ty)?;
        Ok(RustExpr::chain(format!(
            "smelt_define_properties({target_text}, {descriptors_text})",
            target_text = target_text.delimited(),
            descriptors_text = descriptors_text.delimited()
        )))
    }

    /// Emits the JavaScript `Object.prototype.toString.call(x)` tag probe.
    ///
    /// Defers the tag resolution to the `smelt_object_to_string_tag` runtime
    /// helper so the primitive-variant and host-identity-marker branches stay
    /// in one place. The operand is rendered through the erased-`unknown`
    /// coercion seam because the probe is only lowered for erased values.
    pub(in crate::emitter) fn object_to_string_tag_expr(
        &self,
        value: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        Ok(RustExpr::chain(format!(
            "smelt_object_to_string_tag(&({text}))",
            text = text.delimited()
        )))
    }

    /// Emits `structuredClone(x)` as a fresh-identity deep copy.
    ///
    /// Defers to the `smelt_structured_clone` runtime helper, which rebuilds the
    /// object graph with new identities while preserving host markers. Only
    /// lowered for erased (`unknown`) values, so the operand and result flow
    /// through the erased-`unknown` coercion seam.
    pub(in crate::emitter) fn structured_clone_expr(
        &self,
        value: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let text = self.value_at_type(value, unknown_ty)?;
        Ok(RustExpr::chain(format!(
            "smelt_structured_clone({text})",
            text = text.delimited()
        )))
    }

    /// Decide a JavaScript tag test at COMPILE time when the value's static
    /// type already answers it, or `None` when the value is erased and the test
    /// genuinely has to run.
    ///
    /// A `SmeltUnknown::…(_)` pattern can only be matched against a
    /// `SmeltUnknown`. Emitting one against a value whose Rust type is already
    /// concrete does not narrow anything — it does not compile.
    /// `Array.isArray(values)` where `values: string[] | undefined` produced
    ///
    /// ```text
    /// values.clone().as_ref().is_some_and(|smelt_value| matches!(smelt_value, SmeltUnknown::Array(_)))
    /// //                                                        ^ &SmeltList<SmeltUnknown>, not a SmeltUnknown
    /// ```
    ///
    /// (es-toolkit `pullAllWith.rs:76`, H36.) And a concrete type needs no
    /// runtime test in the first place: a `SmeltList` IS an array, on every
    /// path, which is exactly what a hand-writing Rust team would observe and
    /// then not write the check at all.
    ///
    /// Erased shapes — `unknown`, an unscoped type parameter, a non-concrete
    /// union, `never` — return `None` and keep the runtime match. So does any
    /// shape this cannot answer confidently, and an erased class, which really
    /// is represented as a `SmeltUnknown` at run time.
    pub(in crate::emitter) fn static_tag_check(
        &self,
        ty: TypeId,
        kind: smelt_hir::UnknownKind,
    ) -> Option<bool> {
        if self.is_erased_class_type(ty) {
            return None;
        }
        // The shared concrete-type map (`Type::runtime_kind`); a generator is
        // an object and a future a promise, which `typeof` also calls "object".
        self.mir
            .types
            .get(ty)?
            .runtime_kind()
            .map(|actual| actual.satisfies(kind))
    }

    /// Emits a runtime tag check for already-rendered `SmeltUnknown` text.
    fn tag_check_raw(
        &self,
        text: &RustExpr,
        kind: smelt_hir::UnknownKind,
    ) -> Result<String, EmitError> {
        self.context.note_carrier_use();
        let pattern = match kind {
            smelt_hir::UnknownKind::Null => "SmeltUnknown::Null",
            smelt_hir::UnknownKind::Undefined => "SmeltUnknown::Undefined",
            smelt_hir::UnknownKind::Bool => "SmeltUnknown::Bool(_)",
            smelt_hir::UnknownKind::Number => "SmeltUnknown::Number(_)",
            // The tag only exists in a crate that holds a bigint; anywhere
            // else no value can carry it.
            smelt_hir::UnknownKind::BigInt if !self.context.needs_bigint() => {
                return Ok("false".to_owned());
            }
            smelt_hir::UnknownKind::BigInt => "SmeltUnknown::BigInt(_)",
            smelt_hir::UnknownKind::String => "SmeltUnknown::String(_)",
            smelt_hir::UnknownKind::Symbol => "SmeltUnknown::Symbol(_)",
            smelt_hir::UnknownKind::Function => {
                return Ok(self.carrier_format(
                    self.carrier()?,
                    format_args!(
                        "matches!({text}, SmeltUnknown::Function(_))",
                        text = text.delimited()
                    ),
                ));
            }
            smelt_hir::UnknownKind::Array => "SmeltUnknown::Array(_)",
            smelt_hir::UnknownKind::Object => {
                // `typeof x === "object"` is true for plain objects, arrays,
                // `null`, and built-in object wrappers such as promises (whose
                // own representation is the dedicated `Promise` variant).
                return Ok(self.carrier_format(self.carrier()?, format_args!(
                    "matches!({text}, SmeltUnknown::Object(_) | SmeltUnknown::Array(_) | SmeltUnknown::Null | SmeltUnknown::Promise(_))", text = text.delimited()
                )));
            }
            smelt_hir::UnknownKind::Promise => {
                return Ok(self.carrier_format(
                    self.carrier()?,
                    format_args!(
                        "matches!({text}, SmeltUnknown::Promise(_))",
                        text = text.delimited()
                    ),
                ));
            }
        };
        Ok(format!(
            "matches!({text}, {pattern})",
            text = text.delimited()
        ))
    }
}
