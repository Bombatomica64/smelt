//! Rust emission for the WHATWG `FormData` value.
//!
//! Every member lands on a real method of the concrete `SmeltFormData` emitted
//! by `crate::form_data_prelude`, so `form.get("name")` is
//! `form.get("name")` typed `Option<..>` and `form.getAll(..)` a real list —
//! no tagged record and no runtime member lookup in between.
//!
//! ## The one seam: the entry value
//!
//! A form entry's value is `string | File` in the source, which the emitter
//! renders as a **generated union** (`SmeltUnionNNNN`), while the runtime type
//! stores the concrete two-arm `SmeltFormDataValue`. Both are concrete, so the
//! seam is a `match` in either direction rather than an erasure, and the arm
//! wrapping goes through the ordinary union injection
//! (`inject_union_value_text`) so the generated union's arm numbering is never
//! spelled here.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit one `FormData` member as a method call on a concrete receiver.
    pub(super) fn form_data_op_text(
        &self,
        op: smelt_hir::FormDataOp,
        form: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(form)?;
        let string_ty = self.type_id(Type::String)?;
        let name = |index: usize| -> Result<String, EmitError> {
            let Some(argument) = args.get(index) else {
                return Err(EmitError::new("a FormData name argument is missing"));
            };
            self.value_at_type(argument, string_ty)
        };
        match op {
            smelt_hir::FormDataOp::Has => {
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&format!("{receiver}.has(&{})", name(0)?), bool_ty, dest_ty)
            }
            smelt_hir::FormDataOp::Delete => Ok(format!("{receiver}.delete(&{})", name(0)?)),
            smelt_hir::FormDataOp::Set | smelt_hir::FormDataOp::Append => {
                let method = if matches!(op, smelt_hir::FormDataOp::Set) {
                    "set"
                } else {
                    "append"
                };
                let value = self.form_data_entry_value_text(args.get(1), args.get(2))?;
                Ok(format!("{receiver}.{method}(&{}, {value})", name(0)?))
            }
            smelt_hir::FormDataOp::Keys => {
                let list_ty = self.type_id(Type::List(string_ty))?;
                self.value_at_type_text(
                    &format!("SmeltList::new({receiver}.keys())"),
                    list_ty,
                    dest_ty,
                )
            }
            smelt_hir::FormDataOp::Get => {
                let value_ty = self.form_data_value_type_id()?;
                let source_ty = self.type_id(Type::Optional(value_ty))?;
                let arm = self.form_data_value_to_union_text("smelt_entry", value_ty)?;
                self.value_at_type_text(
                    &format!("{receiver}.get(&{}).map(|smelt_entry| {arm})", name(0)?),
                    source_ty,
                    dest_ty,
                )
            }
            smelt_hir::FormDataOp::GetAll | smelt_hir::FormDataOp::Values => {
                let value_ty = self.form_data_value_type_id()?;
                let source_ty = self.type_id(Type::List(value_ty))?;
                let arm = self.form_data_value_to_union_text("smelt_entry", value_ty)?;
                let call = if matches!(op, smelt_hir::FormDataOp::GetAll) {
                    format!("{receiver}.get_all(&{})", name(0)?)
                } else {
                    format!("{receiver}.values()")
                };
                self.value_at_type_text(
                    &format!(
                        "SmeltList::new({call}.into_iter().map(|smelt_entry| {arm}).collect::<Vec<_>>())"
                    ),
                    source_ty,
                    dest_ty,
                )
            }
            smelt_hir::FormDataOp::Entries => {
                let value_ty = self.form_data_value_type_id()?;
                let pair_ty = self.type_id(Type::Tuple(Vec::from([string_ty, value_ty])))?;
                let source_ty = self.type_id(Type::List(pair_ty))?;
                let arm = self.form_data_value_to_union_text("smelt_entry", value_ty)?;
                self.value_at_type_text(
                    &format!(
                        "SmeltList::new({receiver}.entries_in_order().into_iter().map(|(smelt_name, smelt_entry)| (smelt_name, {arm})).collect::<Vec<_>>())"
                    ),
                    source_ty,
                    dest_ty,
                )
            }
            // A callback member, and the only one on this surface. It is left a
            // named blocker rather than approximated: the callback ABI (arity
            // adaptation, the throwing edge, a captured environment) is the
            // list-callback machinery, and reaching it from here needs the
            // desugaring to `entries()` plus a parameter reorder — the spec
            // calls back with `(value, name, form)`. `entries()` covers the
            // same ground today with an exact type.
            smelt_hir::FormDataOp::ForEach => Err(EmitError::new(
                "`FormData.forEach` is not implemented yet; iterate `form.entries()` instead",
            )),
        }
    }

    /// Emit a `set`/`append` value argument as a `SmeltFormDataValue`.
    ///
    /// The spec's `FormDataEntryValue` is `string | Blob`, and the optional
    /// third `filename` argument replaces a blob value's name. Each accepted
    /// static shape maps to one arm:
    ///
    /// * a `string` is a text entry, and `filename` is meaningless for it (the
    ///   spec forbids passing one);
    /// * a `Blob`/`File` becomes a file entry through
    ///   `smelt_form_data_file_value`, which applies the spec's naming rule;
    /// * a union of the two decides per arm, at runtime, with no erasure.
    ///
    /// Anything else is a named blocker: accepting it would mean stringifying a
    /// value the source did not ask to stringify.
    fn form_data_entry_value_text(
        &self,
        value: Option<&Operand>,
        filename: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let Some(value) = value else {
            return Err(EmitError::new("a FormData value argument is missing"));
        };
        let string_ty = self.type_id(Type::String)?;
        let filename_text = match filename {
            None => "None".to_owned(),
            Some(operand) => format!("Some({})", self.value_at_type(operand, string_ty)?),
        };
        let value_ty = self.operand_ty(value)?;
        let value_text = self.operand_text(value)?;
        if matches!(self.mir.types.get(value_ty), Some(Type::String)) {
            return Ok(format!("SmeltFormDataValue::Text(({value_text}).clone())"));
        }
        if self.is_blob_class_type(value_ty)? {
            return Ok(format!(
                "smelt_form_data_file_value(({value_text}).clone(), {filename_text})"
            ));
        }
        if let Some(members) = self.concrete_union_members(value_ty) {
            let members = members.to_vec();
            let mut arms = Vec::new();
            for (index, member) in members.iter().enumerate() {
                let arm = if matches!(self.mir.types.get(*member), Some(Type::String)) {
                    "SmeltFormDataValue::Text(smelt_entry)".to_owned()
                } else if self.is_blob_class_type(*member)? {
                    format!("smelt_form_data_file_value(smelt_entry, {filename_text})")
                } else {
                    return Err(EmitError::new(
                        "a FormData value must be a string or a Blob/File",
                    ));
                };
                arms.push(format!(
                    "{}::M{index}(smelt_entry) => {arm}",
                    super::union::union_name(value_ty)
                ));
            }
            return Ok(format!("match ({value_text}).clone() {{ {} }}", arms.join(", ")));
        }
        Err(EmitError::new(
            "a FormData value must be a string or a Blob/File",
        ))
    }

    /// Emit the `SmeltFormDataValue` -> generated-union conversion.
    ///
    /// `value_text` names a `SmeltFormDataValue` binding. Each arm is injected
    /// into the union through the shared union injection, so the arm indices
    /// come from the union's own member order rather than from an assumption
    /// spelled here.
    fn form_data_value_to_union_text(
        &self,
        value_text: &str,
        union_ty: TypeId,
    ) -> Result<String, EmitError> {
        let string_ty = self.type_id(Type::String)?;
        let Some(members) = self.concrete_union_members(union_ty) else {
            return Err(EmitError::new(
                "the FormData entry value type is not a concrete union",
            ));
        };
        let file_ty = members
            .iter()
            .copied()
            .find(|member| self.is_blob_class_type(*member).unwrap_or(false))
            .ok_or_else(|| {
                EmitError::new("the FormData entry value union has no File member")
            })?;
        let text_arm = self
            .inject_union_value_text("smelt_text", string_ty, union_ty)?
            .ok_or_else(|| EmitError::new("the FormData entry value has no string member"))?;
        let file_arm = self
            .inject_union_value_text("smelt_file", file_ty, union_ty)?
            .ok_or_else(|| EmitError::new("the FormData entry value has no File member"))?;
        Ok(format!(
            "match {value_text} {{ SmeltFormDataValue::Text(smelt_text) => {text_arm}, SmeltFormDataValue::File(smelt_file) => {file_arm} }}"
        ))
    }

    /// Find the interned `string | File` entry value union.
    ///
    /// Structural rather than by symbol: the frontend interns this union in one
    /// place (`form_data_value_type`) whenever a form member is lowered, so a
    /// two-member union of a string and the blob runtime class identifies it
    /// without the emitter having to reconstruct the `File` symbol.
    fn form_data_value_type_id(&self) -> Result<TypeId, EmitError> {
        for (index, ty) in self.mir.types.all().iter().enumerate() {
            let Type::Union(members) = ty else { continue };
            if members.len() != 2 {
                continue;
            }
            let has_string = members
                .iter()
                .any(|member| matches!(self.mir.types.get(*member), Some(Type::String)));
            let mut has_file = false;
            for member in members {
                if self.is_blob_class_type(*member)? {
                    has_file = true;
                }
            }
            if has_string && has_file {
                return Ok(TypeId(u32::try_from(index).unwrap_or(u32::MAX)));
            }
        }
        Err(EmitError::new(
            "type table does not contain the FormData `string | File` entry value union",
        ))
    }
}
