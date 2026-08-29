//! Concrete Rust representation and conversions for HIR union types.
//!
//! A union whose members all retain concrete Rust storage lowers to one
//! canonical tagged enum. Truly dynamic members (`unknown`, unscoped type
//! parameters, or erased host shapes) continue to use `SmeltUnknown`.

use super::*;
use crate::rust::RustIdent;

/// Return the stable generated Rust name for an interned union type.
pub(crate) fn union_name(ty: TypeId) -> String {
    format!("SmeltUnion{}", ty.0)
}

impl FunctionEmitter<'_> {
    /// Render a concrete union storage type including its scoped generic arguments.
    pub(super) fn union_type_text(&self, ty: TypeId) -> Result<String, EmitError> {
        let params = self.union_type_params(ty)?;
        if params.is_empty() {
            return Ok(union_name(ty));
        }
        let args = params
            .iter()
            .map(|param| self.union_type_param_name(*param))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(format!("{}<{args}>", union_name(ty)))
    }

    /// Collect free type parameters retained by a concrete union's members.
    fn union_type_params(&self, ty: TypeId) -> Result<Vec<Symbol>, EmitError> {
        let Some(members) = self.concrete_union_members(ty) else {
            return Ok(Vec::new());
        };
        let mut params = Vec::new();
        for member in members {
            self.collect_union_type_params(*member, &mut params)?;
        }
        Ok(params)
    }

    /// Walk one union member type and retain each free parameter once.
    fn collect_union_type_params(
        &self,
        ty: TypeId,
        params: &mut Vec<Symbol>,
    ) -> Result<(), EmitError> {
        let Some(kind) = self.mir.types.get(ty) else {
            return Err(EmitError::new("union member references an unknown type"));
        };
        match kind {
            Type::TypeParam { name } => {
                if !params.contains(name) {
                    params.push(*name);
                }
            }
            Type::Optional(item) | Type::List(item) | Type::Set(item) | Type::Future(item) => {
                self.collect_union_type_params(*item, params)?;
            }
            Type::Dict(key, value) | Type::JsMap(key, value) => {
                self.collect_union_type_params(*key, params)?;
                self.collect_union_type_params(*value, params)?;
            }
            Type::Tuple(items) | Type::Union(items) => {
                for item in items {
                    self.collect_union_type_params(*item, params)?;
                }
            }
            Type::Class { args, .. } => {
                for arg in args {
                    self.collect_union_type_params(*arg, params)?;
                }
            }
            Type::Function(function) => {
                for param in &function.params {
                    self.collect_union_type_params(*param, params)?;
                }
                self.collect_union_type_params(function.return_ty, params)?;
            }
            Type::Generator { yield_ty, return_ty, next_ty, .. } => {
                self.collect_union_type_params(*yield_ty, params)?;
                self.collect_union_type_params(*return_ty, params)?;
                self.collect_union_type_params(*next_ty, params)?;
            }
            Type::GeneratorResult { yield_ty, return_ty } => {
                self.collect_union_type_params(*yield_ty, params)?;
                self.collect_union_type_params(*return_ty, params)?;
            }
            Type::Unknown | Type::Never | Type::None | Type::Bool | Type::Int | Type::Float | Type::String => {}
        }
        Ok(())
    }

    /// Render one union type parameter as a Rust identifier.
    fn union_type_param_name(&self, param: Symbol) -> Result<String, EmitError> {
        Ok(RustIdent::new(self.symbol_name(param)?).into_string())
    }

    /// Emit a statically typed method call shared by every concrete union arm.
    ///
    /// TypeScript permits a union method call when every member exposes that
    /// method. Concrete unions are represented as generated tagged enums, so
    /// dispatch must project the active payload before invoking the Rust
    /// method; erasing the receiver would lose useful generic return shape.
    pub(super) fn union_method_text(
        &self,
        receiver: &Operand,
        method: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let Some(members) = self.concrete_union_members(receiver_ty) else {
            return Err(EmitError::new(
                "union method call requires a concrete tagged union receiver",
            ));
        };
        let receiver_text = self.operand_text(receiver)?;
        let method_name = sanitize_ident(self.symbol_name(method)?);
        let args_text = args
            .iter()
            .map(|arg| self.operand_text(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let union = union_name(receiver_ty);
        let mut arms = Vec::with_capacity(members.len());
        for (index, member) in members.iter().copied().enumerate() {
            let return_ty = self.union_member_method_return_type(member, method)?;
            let call = format!("value.{method_name}({args_text})");
            let converted = self.value_at_type_text(&call, return_ty, dest_ty)?;
            arms.push(format!("{union}::M{index}(value) => {converted}"));
        }
        Ok(format!("match {receiver_text} {{ {} }}", arms.join(", ")))
    }

    /// Resolve and instantiate one union member's declared method return type.
    fn union_member_method_return_type(
        &self,
        member_ty: TypeId,
        method: Symbol,
    ) -> Result<TypeId, EmitError> {
        let Some(Type::Class { name, args }) = self.mir.types.get(member_ty) else {
            return Err(EmitError::new("union method member must be a class value"));
        };
        let class = self
            .mir
            .classes
            .iter()
            .find(|class| class.name == *name)
            .ok_or_else(|| EmitError::new("union method class is not materialized"))?;
        let function = class
            .methods
            .iter()
            .filter_map(|id| self.mir.functions.get(id.0 as usize))
            .find(|function| function.name == method)
            .ok_or_else(|| EmitError::new("union member does not implement method"))?;
        let substitutions = class
            .type_params
            .iter()
            .zip(args.iter().copied())
            .map(|(param, arg)| (param.name, arg))
            .collect::<HashMap<_, _>>();
        Ok(self.substitute_type_params_in_type(function.return_ty, &substitutions))
    }

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
            Some(Type::Generator {
                yield_ty,
                return_ty,
                ..
            }) => {
                self.union_member_is_concrete(*yield_ty)
                    && self.union_member_is_concrete(*return_ty)
                // The caller-provided `.next(value)` type is not stored in the
                // generated wrapper, so a dynamic next-input boundary does not
                // make the generator value itself an erased runtime object.
            }
            Some(Type::GeneratorResult {
                yield_ty,
                return_ty,
            }) => {
                self.union_member_is_concrete(*yield_ty)
                    && self.union_member_is_concrete(*return_ty)
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
        let params = emitter.union_type_params(type_id)?;
        let declaration_generics = if params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                params
                    .iter()
                    .map(|param| emitter.union_type_param_name(*param))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ")
            )
        };
        let impl_generics = if params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                params
                    .iter()
                    .map(|param| {
                        Ok(format!(
                            "{}: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static",
                            emitter.union_type_param_name(*param)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ")
            )
        };
        let default_generics = if params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                params
                    .iter()
                    .map(|param| Ok(format!("{}: Default", emitter.union_type_param_name(*param)?)))
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ")
            )
        };
        let target = emitter.union_type_text(type_id)?;
        output.push_str("#[derive(Clone)]\n");
        output.push_str(&format!("pub enum {name}{declaration_generics} {{\n"));
        for (member_index, member) in members.iter().enumerate() {
            let member_text = emitter.type_text_with_impl_trait(*member, false)?;
            output.push_str(&format!("    M{member_index}({member_text}),\n"));
        }
        output.push_str("}\n");
        if members
            .iter()
            .any(|member| matches!(mir.types.get(*member), Some(Type::Generator { .. })))
        {
            // Generator carriers deliberately cannot cross into
            // `SmeltUnknown`: doing so would erase suspension and the selected
            // sync/async resume protocol. Generator unions therefore remain a
            // typed tagged enum consumed by `yield*` arm dispatch and omit the
            // generic erased conversion/equality implementations below.
            output.push_str(&format!(
                "impl{default_generics} ::std::fmt::Debug for {target} {{\n"
            ));
            output.push_str(
                "    fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"SmeltGeneratorUnion\") }\n",
            );
            output.push_str("}\n\n");
            continue;
        }
        output.push_str(&format!("impl{impl_generics} IntoSmeltUnknown for {target} {{\n"));
        output.push_str("    fn into_smelt_unknown(self) -> SmeltUnknown {\n");
        output.push_str("        match self {\n");
        for (member_index, member) in members.iter().enumerate() {
            let erased = emitter.erase_value_text("value", *member)?;
            output.push_str(&format!(
                "            Self::M{member_index}(value) => {erased},\n"
            ));
        }
        output.push_str("        }\n    }\n}\n");
        output.push_str(&format!("impl{impl_generics} {target} {{\n"));
        output.push_str("    fn from_smelt_unknown(value: SmeltUnknown) -> Self {\n");
        output.push_str(&emitter.union_from_smelt_unknown_body(members)?);
        output.push_str("    }\n}\n");
        output.push_str(&format!("impl{impl_generics} PartialEq for {target} {{\n"));
        output.push_str("    fn eq(&self, other: &Self) -> bool {\n");
        output.push_str(
            "        self.clone().into_smelt_unknown() == other.clone().into_smelt_unknown()\n",
        );
        output.push_str("    }\n}\n");
        // `SmeltFromUnknown` is the trait form of the inherent
        // `from_smelt_unknown` emitted just above. Without it a union cannot be
        // used as a generic argument, because every lifted type parameter is
        // bounded by it — es-toolkit's specs instantiate generic helpers at
        // union types and failed with "the trait bound `SmeltUnionN:
        // SmeltFromUnknown` is not satisfied".
        output.push_str(&format!(
            "impl{impl_generics} SmeltFromUnknown for {target} {{\n"
        ));
        output.push_str(
            "    fn smelt_from_unknown(value: SmeltUnknown) -> Self { Self::from_smelt_unknown(value) }\n",
        );
        output.push_str("}\n");
        // A union used as a map key needs JavaScript key equality. Both halves
        // route through the erased `SmeltUnknown` view, exactly like the
        // `PartialEq` impl above, so the union agrees with `SmeltUnknown` on key
        // identity — which matters because the same map is often compared
        // against an erased one.
        output.push_str(&format!(
            "impl{impl_generics} SmeltJsKeyEq for {target} {{\n"
        ));
        output.push_str(
            "    fn same_js_key(&self, other: &Self) -> bool { self.clone().into_smelt_unknown().same_js_key(&other.clone().into_smelt_unknown()) }\n",
        );
        output.push_str(
            "    fn js_key_hash(&self) -> Option<u64> { self.clone().into_smelt_unknown().js_key_hash() }\n",
        );
        output.push_str("}\n");
        // Concrete unions surface as fields of generated classes, which derive
        // `Debug` and `Default`. A data-carrying enum can derive neither, and a
        // `#[derive(Debug)]` would additionally demand that every member type be
        // `Debug` (excluding `SmeltJsMap`, function members, and other runtime
        // carriers). Emit both by hand: `Debug` reuses the erased `SmeltUnknown`
        // view (always present, since the union already emits `IntoSmeltUnknown`)
        // exactly like the `PartialEq` impl above, and `Default` selects the
        // first arm, matching how `default_value` builds a concrete-union default.
        output.push_str(&format!("impl{impl_generics} ::std::fmt::Debug for {target} {{\n"));
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
        output.push_str(&format!("impl{default_generics} Default for {target} {{\n"));
        output.push_str("    fn default() -> Self {\n");
        output.push_str(&format!("        Self::M0({first_default})\n"));
        output.push_str("    }\n}\n\n");
    }
    Ok(output)
}
