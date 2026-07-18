//! Concrete Rust representation and conversions for HIR union types.
//!
//! A union whose members all retain concrete Rust storage lowers to one
//! canonical tagged enum. Truly dynamic members (`unknown`, unscoped type
//! parameters, or erased host shapes) continue to use `SmeltUnknown`.

use super::*;

/// Return the stable generated Rust name for an interned union type.
pub(crate) fn union_name(ty: TypeId) -> String {
    format!("SmeltUnion{}", ty.0)
}

impl FunctionEmitter<'_> {
    /// Return canonical members when a union has fully concrete Rust storage.
    pub(super) fn concrete_union_members(&self, ty: TypeId) -> Option<&[TypeId]> {
        let Type::Union(items) = self.mir.types.get(ty)? else {
            return None;
        };
        (items.len() >= 2
            && items
                .iter()
                .all(|item| self.union_member_is_concrete(*item)))
        .then_some(items)
    }

    /// Determine whether a union member can be stored without runtime erasure.
    fn union_member_is_concrete(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::Unknown | Type::Never | Type::TypeParam { .. } | Type::Union(_)) | None => {
                false
            }
            Some(Type::Class { .. }) => !self.is_erased_class_type(ty),
            Some(Type::List(item) | Type::Set(item) | Type::Future(item)) => {
                self.union_member_is_concrete(*item)
            }
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                self.union_member_is_concrete(*key) && self.union_member_is_concrete(*value)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.union_member_is_concrete(*item)),
            Some(Type::Function(function)) => {
                function
                    .params
                    .iter()
                    .all(|param| self.union_member_is_concrete(*param))
                    && self.union_member_is_concrete(function.return_ty)
            }
            Some(Type::Bool | Type::Int | Type::Float | Type::String) => true,
            // Nullish unions remain `Option`/erased-boundary work. Keeping them
            // out of tagged unions preserves missing-property/default semantics;
            // the concrete-union feature targets distinct value-bearing arms.
            Some(Type::None | Type::Optional(_)) => false,
        }
    }

    /// Erase `value_text` to `SmeltUnknown` when its type is a concrete union.
    ///
    /// A concrete union stores a tagged `SmeltUnion…` enum, but JavaScript value
    /// operations (string coercion, structural `match`/`matches!`, `typeof`,
    /// property-key stringification) are emitted against `SmeltUnknown`. The
    /// generated `into_smelt_unknown()` conversion projects the tagged enum back
    /// to the erased value those operations expect. `value_text` must be an
    /// owned expression (the conversion consumes `self`).
    pub(super) fn erase_concrete_union_text(&self, value_text: &str, ty: TypeId) -> String {
        if self.concrete_union_members(ty).is_some() {
            format!("{value_text}.into_smelt_unknown()")
        } else {
            value_text.to_owned()
        }
    }

    /// Wrap a concrete value in the matching variant of a target union.
    pub(super) fn inject_union_value_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let Some(members) = self.concrete_union_members(target) else {
            return Ok(None);
        };
        if let Some(index) = members.iter().position(|member| *member == source) {
            return Ok(Some(format!("{}::M{index}({value_text})", union_name(target))));
        }
        // Structural injection: the source is not an exact member but shares a
        // single member's collection shape (for example an empty
        // `SmeltList<SmeltUnknown>` default flowing into a `SmeltList<String>`
        // union arm). Coerce the value into that member's element types, then
        // wrap it in the variant. Requiring a unique shape-compatible member
        // keeps the choice unambiguous.
        let structural: Vec<usize> = members
            .iter()
            .enumerate()
            .filter(|(_, member)| self.union_member_shape_matches_source(source, **member))
            .map(|(index, _)| index)
            .collect();
        if let [index] = structural.as_slice() {
            let coerced = self.value_at_type_text(value_text, source, members[*index])?;
            return Ok(Some(format!("{}::M{index}({coerced})", union_name(target))));
        }
        // An erased source (object-field read, erased return, nullish default)
        // carries a `SmeltUnknown`; reconstruct the concrete union by routing the
        // runtime value to its matching variant. `value_text` must be an owned
        // `SmeltUnknown` expression (the reconstruction consumes it).
        if matches!(
            self.mir.types.get(source),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) {
            return Ok(Some(format!(
                "{}::from_smelt_unknown({value_text})",
                union_name(target)
            )));
        }
        Ok(None)
    }

    /// Whether `source` shares `member`'s collection shape while differing only
    /// in element types.
    ///
    /// Used to inject a structurally-compatible value (such as an empty list
    /// default whose element type erased to `SmeltUnknown`) into a concrete
    /// union arm by coercing its elements, rather than requiring an exact type
    /// match. Only collection shapes are considered so that unrelated scalar
    /// members are never selected.
    fn union_member_shape_matches_source(&self, source: TypeId, member: TypeId) -> bool {
        if source == member {
            return false;
        }
        matches!(
            (self.mir.types.get(source), self.mir.types.get(member)),
            (Some(Type::List(_)), Some(Type::List(_)))
                | (Some(Type::Set(_)), Some(Type::Set(_)))
                | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
                // An object literal (a string-keyed `Dict`/`SmeltRecord`, or a
                // structural record) injected into a union arm that is itself an
                // object-shaped type (a declared `Class`/interface such as
                // `RetryOptions`, or another record). The field-wise coercion is
                // emitted by `value_at_type_text`'s record adapters. Requiring a
                // single shape-compatible member keeps the arm choice
                // unambiguous, so a `number | RetryOptions` union routes the
                // options object to `RetryOptions` rather than the scalar arm.
                | (
                    Some(Type::Dict(_, _) | Type::Class { .. }),
                    Some(Type::Class { .. }),
                )
                | (Some(Type::Class { .. }), Some(Type::Dict(_, _)))
        )
    }

    /// Project a guarded union value into a concrete member or narrower union.
    pub(super) fn project_union_value_text(
        &self,
        value_text: &str,
        source: TypeId,
        target: TypeId,
    ) -> Result<Option<String>, EmitError> {
        if source == target {
            return Ok(None);
        }
        let Some(source_members) = self.concrete_union_members(source) else {
            return Ok(None);
        };
        let source_name = union_name(source);
        if let Some(target_members) = self.concrete_union_members(target) {
            let target_name = union_name(target);
            let arms = source_members
                .iter()
                .enumerate()
                .filter_map(|(source_index, member)| {
                    target_members
                        .iter()
                        .position(|target_member| target_member == member)
                        .map(|target_index| {
                            format!(
                                "{source_name}::M{source_index}(value) => {target_name}::M{target_index}(value)"
                            )
                        })
                })
                .collect::<Vec<_>>();
            if arms.is_empty() {
                return Ok(None);
            }
            return Ok(Some(format!(
                "match {value_text} {{ {}, _ => unreachable!(\"union guard selected an excluded member\") }}",
                arms.join(", ")
            )));
        }
        let Some(index) = source_members.iter().position(|member| *member == target) else {
            return Ok(None);
        };
        Ok(Some(format!(
            "match {value_text} {{ {source_name}::M{index}(value) => value, _ => unreachable!(\"union guard selected an excluded member\") }}"
        )))
    }

    /// Emit a discriminant check for concrete union members of one JS kind.
    pub(super) fn concrete_union_tag_check(
        &self,
        value_text: &str,
        union_ty: TypeId,
        kind: smelt_hir::UnknownKind,
    ) -> Option<String> {
        let members = self.concrete_union_members(union_ty)?;
        let union_enum_name = union_name(union_ty);
        let patterns = members
            .iter()
            .enumerate()
            .filter(|(_, member)| self.union_member_matches_kind(**member, kind))
            .map(|(index, _)| format!("{union_enum_name}::M{index}(_)"))
            .collect::<Vec<_>>();
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else {
            format!("matches!({value_text}, {})", patterns.join(" | "))
        })
    }

    /// Emit an `instanceof` discriminant test for concrete class union arms.
    pub(super) fn concrete_union_class_check(
        &self,
        value_text: &str,
        union_ty: TypeId,
        class: Symbol,
    ) -> Option<String> {
        let members = self.concrete_union_members(union_ty)?;
        let union_enum_name = union_name(union_ty);
        let patterns = members
            .iter()
            .enumerate()
            .filter(|(_, member)| {
                matches!(
                    self.mir.types.get(**member),
                    Some(Type::Class {
                        name: class_symbol,
                        ..
                    }) if *class_symbol == class
                )
            })
            .map(|(index, _)| format!("{union_enum_name}::M{index}(_)"))
            .collect::<Vec<_>>();
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else {
            format!("matches!({value_text}, {})", patterns.join(" | "))
        })
    }

    /// Return the `SmeltUnknown` variant pattern a concrete member reconstructs from.
    ///
    /// Reconstruction (`from_smelt_unknown`) routes an erased value to the union
    /// arm whose runtime JavaScript category matches, so each member maps to the
    /// `SmeltUnknown` discriminant its values carry at runtime.
    fn union_member_unknown_pattern(&self, member: TypeId) -> &'static str {
        match self.mir.types.get(member) {
            Some(Type::Bool) => "SmeltUnknown::Bool(_)",
            Some(Type::Int | Type::Float) => "SmeltUnknown::Number(_)",
            Some(Type::String) => "SmeltUnknown::String(_)",
            Some(Type::Function(_)) => "SmeltUnknown::Function(_)",
            Some(Type::List(_) | Type::Tuple(_)) => "SmeltUnknown::Array(_)",
            Some(Type::Future(_)) => "SmeltUnknown::Promise(_)",
            Some(Type::Set(_) | Type::Dict(_, _) | Type::Class { .. }) => "SmeltUnknown::Object(_)",
            _ => "SmeltUnknown::Null | SmeltUnknown::Undefined",
        }
    }

    /// Build the body of a union's `from_smelt_unknown` reconstruction.
    ///
    /// Erased values (object-field reads, erased returns, nullish-coalescing
    /// defaults) reach a concrete-union destination and must be projected back
    /// into the matching tagged variant. Each present value is routed by its
    /// runtime discriminant to the member sharing that JavaScript category, then
    /// extracted into the member's concrete Rust storage. The final member is the
    /// total fallback so the reconstruction is exhaustive (tsc has already proven
    /// the value inhabits the union).
    pub(super) fn union_from_smelt_unknown_body(
        &self,
        members: &[TypeId],
    ) -> Result<String, EmitError> {
        let mut body = String::new();
        for (index, member) in members.iter().enumerate() {
            let extracted = self.extract_value_text("value", *member)?;
            if Some(index) == members.len().checked_sub(1) {
                body.push_str(&format!("        Self::M{index}({extracted})\n"));
            } else {
                let pattern = self.union_member_unknown_pattern(*member);
                body.push_str(&format!(
                    "        if matches!(value, {pattern}) {{ return Self::M{index}({extracted}); }}\n"
                ));
            }
        }
        Ok(body)
    }

    /// Emit a structural `field in value` check over concrete union arms.
    ///
    /// A `"field" in value` guard on a concrete union does not need to erase the
    /// value and inspect a runtime object map: the set of arms that expose
    /// `field` is known statically, so the check compiles to a tagged-enum
    /// discriminant test. Returns `None` when the receiver is not a concrete
    /// union (the caller then falls back to the erased object lookup), keeping
    /// dynamic boundaries explicit. When no arm carries the field the check is a
    /// constant `false`; when every arm carries it, a constant `true`.
    pub(super) fn concrete_union_field_check(
        &self,
        value_text: &str,
        union_ty: TypeId,
        field: &str,
    ) -> Option<String> {
        let members = self.concrete_union_members(union_ty)?;
        let union_enum_name = union_name(union_ty);
        let patterns = members
            .iter()
            .enumerate()
            .filter(|(_, member)| self.union_member_has_field(**member, field))
            .map(|(index, _)| format!("{union_enum_name}::M{index}(_)"))
            .collect::<Vec<_>>();
        Some(if patterns.is_empty() {
            "false".to_owned()
        } else if patterns.len() == members.len() {
            "true".to_owned()
        } else {
            format!("matches!({value_text}, {})", patterns.join(" | "))
        })
    }

    /// Return whether a concrete union member type statically carries a field.
    ///
    /// Mirrors the frontend field-presence rule so the emitted discriminant
    /// check keeps exactly the arms whose declared shape exposes `field`.
    fn union_member_has_field(&self, ty: TypeId, field: &str) -> bool {
        match self.mir.types.get(ty) {
            Some(Type::String | Type::List(_) | Type::Tuple(_)) => field == "length",
            Some(Type::Dict(_, _)) => true,
            Some(Type::Class { name, .. }) => self.mir_class_has_field(*name, field),
            _ => false,
        }
    }

    /// Return whether a named MIR class declares a field.
    fn mir_class_has_field(&self, name: Symbol, field: &str) -> bool {
        self.mir.classes.iter().any(|class| {
            class.name == name
                && class
                    .fields
                    .iter()
                    .any(|candidate| self.symbol_name(candidate.name) == Ok(field))
        })
    }

    /// Map a concrete HIR type to the JavaScript runtime category used by guards.
    fn union_member_matches_kind(&self, ty: TypeId, kind: smelt_hir::UnknownKind) -> bool {
        matches!(
            (self.mir.types.get(ty), kind),
            (
                Some(Type::None),
                smelt_hir::UnknownKind::Null | smelt_hir::UnknownKind::Undefined,
            ) | (Some(Type::Bool), smelt_hir::UnknownKind::Bool)
                | (
                    Some(Type::Int | Type::Float),
                    smelt_hir::UnknownKind::Number
                )
                | (Some(Type::String), smelt_hir::UnknownKind::String)
                | (Some(Type::Function(_)), smelt_hir::UnknownKind::Function)
                | (
                    Some(Type::List(_) | Type::Tuple(_)),
                    smelt_hir::UnknownKind::Array
                )
                | (
                    Some(
                        Type::List(_)
                            | Type::Set(_)
                            | Type::Dict(_, _)
                            | Type::Tuple(_)
                            | Type::Class { .. }
                            | Type::Future(_),
                    ),
                    smelt_hir::UnknownKind::Object,
                )
                | (Some(Type::Future(_)), smelt_hir::UnknownKind::Promise)
                | (Some(Type::Optional(_)), smelt_hir::UnknownKind::Undefined)
        )
    }
}

/// Emit every canonical concrete union enum used by the MIR type table.
pub(crate) fn emit_union_definitions(
    mir: &Mir,
    context: &EmitContext,
) -> Result<String, EmitError> {
    let Some(function) = mir.functions.first() else {
        return Ok(String::new());
    };
    let emitter = FunctionEmitter::new(mir, context, function)?;
    let mut output = String::new();
    for (index, _) in mir.types.all().iter().enumerate() {
        let type_id = TypeId(compact_index(index, "union type index does not fit u32")?);
        let Some(members) = emitter.concrete_union_members(type_id) else {
            continue;
        };
        let name = union_name(type_id);
        output.push_str("#[derive(Clone)]\n");
        output.push_str(&format!("pub enum {name} {{\n"));
        for (member_index, member) in members.iter().enumerate() {
            let member_text = emitter.type_text_with_impl_trait(*member, false)?;
            output.push_str(&format!("    M{member_index}({member_text}),\n"));
        }
        output.push_str("}\n");
        output.push_str(&format!("impl IntoSmeltUnknown for {name} {{\n"));
        output.push_str("    fn into_smelt_unknown(self) -> SmeltUnknown {\n");
        output.push_str("        match self {\n");
        for (member_index, member) in members.iter().enumerate() {
            let erased = emitter.erase_value_text("value", *member)?;
            output.push_str(&format!(
                "            Self::M{member_index}(value) => {erased},\n"
            ));
        }
        output.push_str("        }\n    }\n}\n");
        output.push_str(&format!("impl {name} {{\n"));
        output.push_str("    fn from_smelt_unknown(value: SmeltUnknown) -> Self {\n");
        output.push_str(&emitter.union_from_smelt_unknown_body(members)?);
        output.push_str("    }\n}\n");
        output.push_str(&format!("impl PartialEq for {name} {{\n"));
        output.push_str("    fn eq(&self, other: &Self) -> bool {\n");
        output.push_str(
            "        self.clone().into_smelt_unknown() == other.clone().into_smelt_unknown()\n",
        );
        output.push_str("    }\n}\n");
        // Concrete unions surface as fields of generated classes, which derive
        // `Debug` and `Default`. A data-carrying enum can derive neither, and a
        // `#[derive(Debug)]` would additionally demand that every member type be
        // `Debug` (excluding `SmeltJsMap`, function members, and other runtime
        // carriers). Emit both by hand: `Debug` reuses the erased `SmeltUnknown`
        // view (always present, since the union already emits `IntoSmeltUnknown`)
        // exactly like the `PartialEq` impl above, and `Default` selects the
        // first arm, matching how `default_value` builds a concrete-union default.
        output.push_str(&format!("impl ::std::fmt::Debug for {name} {{\n"));
        output.push_str(
            "    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {\n",
        );
        output.push_str(
            "        ::std::fmt::Debug::fmt(&self.clone().into_smelt_unknown(), formatter)\n",
        );
        output.push_str("    }\n}\n");
        let first_member = *members
            .first()
            .ok_or_else(|| EmitError::new("concrete union has no members"))?;
        let first_default = emitter.default_value(first_member)?;
        output.push_str(&format!("impl Default for {name} {{\n"));
        output.push_str("    fn default() -> Self {\n");
        output.push_str(&format!("        Self::M0({first_default})\n"));
        output.push_str("    }\n}\n\n");
    }
    Ok(output)
}
