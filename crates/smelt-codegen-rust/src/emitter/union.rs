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
            Some(Type::Dict(key, value)) => {
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
        let Some(index) = members.iter().position(|member| *member == source) else {
            return Ok(None);
        };
        Ok(Some(format!(
            "{}::M{index}({value_text})",
            union_name(target)
        )))
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
        output.push_str(&format!("impl PartialEq for {name} {{\n"));
        output.push_str("    fn eq(&self, other: &Self) -> bool {\n");
        output.push_str(
            "        self.clone().into_smelt_unknown() == other.clone().into_smelt_unknown()\n",
        );
        output.push_str("    }\n}\n\n");
    }
    Ok(output)
}
