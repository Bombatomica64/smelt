//! List emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a list containment operation to Rust text.
    pub(super) fn list_contains_text(&self, list: &Operand, item: &Operand) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list contains receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list contains item must match the list element type",
            ));
        }
        Ok(format!(
            "{}.contains(&{})",
            self.operand_text(list)?,
            self.operand_text(item)?
        ))
    }

    /// Converts a set containment operation to Rust text.
    /// Converts a list-to-set constructor conversion to Rust text.
    /// Converts a set containment operation to Rust text.
    /// Converts a list-to-set constructor conversion to Rust text.
    pub(super) fn list_to_set_text(&self, list: &Operand, dest_ty: TypeId) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list-to-set source must be a list"));
        };
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Set(dest_item)) if dest_item == item_ty)
        {
            return Err(EmitError::new(
                "list-to-set destination must be set of the list item type",
            ));
        }
        Ok(format!(
            "{}.iter().cloned().collect::<::std::collections::HashSet<_>>()",
            self.operand_text(list)?
        ))
    }

    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    /// Converts a list of key/value pair tuples to a Rust `HashMap`.
    pub(super) fn list_pairs_to_dict_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("dict() pair source must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(*item_ty) else {
            return Err(EmitError::new(
                "dict() pair source must contain tuple items",
            ));
        };
        let [key_ty, value_ty] = items.as_slice() else {
            return Err(EmitError::new(
                "dict() pair source tuples must have length two",
            ));
        };
        if !matches!(
            self.mir.types.get(dest_ty),
            Some(Type::Dict(dest_key, dest_value)) if dest_key == key_ty && dest_value == value_ty
        ) {
            return Err(EmitError::new(
                "dict() destination must match pair key and value types",
            ));
        }
        Ok(format!(
            "{}.iter().cloned().collect::<::std::collections::HashMap<_, _>>()",
            self.operand_text(list)?
        ))
    }

    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    /// Converts a list concatenation operation to Rust text.
    pub(super) fn list_concat_text(&self, left: &Operand, right: &Operand) -> Result<String, EmitError> {
        let left_ty = self.operand_ty(left)?;
        if self.mir.types.get(left_ty) != self.mir.types.get(self.operand_ty(right)?) {
            return Err(EmitError::new(
                "list concat operands must have the same list type",
            ));
        }
        if !matches!(self.mir.types.get(left_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list concat operands must be lists"));
        }
        Ok(format!(
            "{}.iter().cloned().chain({}.iter().cloned()).collect::<Vec<_>>()",
            self.operand_text(left)?,
            self.operand_text(right)?
        ))
    }

    /// Converts a list search operation to Rust text.
    /// Converts a list search operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    pub(super) fn list_slice_text(
        &self,
        list: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list slice receiver must be a list"));
        }
        self.validate_optional_numeric_index(start, "list slice start index")?;
        self.validate_optional_numeric_index(end, "list slice end index")?;
        let list_text = self.operand_text(list)?;
        let len_source = format!("{list_text}.len()");
        let start_text = self.slice_start_text(start, &len_source)?;
        let len_text = self.slice_len_text(&list_text, start, end, SliceLenKind::Len)?;
        Ok(format!(
            "{list_text}.iter().skip({start_text}).take({len_text}).cloned().collect::<Vec<_>>()"
        ))
    }

    /// Converts a list push operation to Rust text.
    /// Converts a list copy operation to Rust text.
    /// Converts a list push operation to Rust text.
    /// Converts a list copy operation to Rust text.
    pub(super) fn list_copy_text(&self, list: &Operand, dest_ty: TypeId) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("list copy receiver must be a list"));
        }
        if dest_ty != list_ty {
            return Err(EmitError::new(
                "list copy destination must match the receiver list type",
            ));
        }
        Ok(format!("{}.clone()", self.operand_text(list)?))
    }

    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a tuple containment operation to Rust text.
    /// Converts a list-to-tuple constructor conversion to Rust text.
    pub(super) fn list_to_tuple_text(&self, list: &Operand, dest_ty: TypeId) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list-to-tuple source must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("list-to-tuple destination must be a tuple"));
        };
        if !items.iter().all(|tuple_item| tuple_item == item_ty) {
            return Err(EmitError::new(
                "list-to-tuple destination items must match the list item type",
            ));
        }
        let items_text = (0..items.len())
            .map(|idx| format!("tuple_items[{idx}].clone()"))
            .collect::<Vec<_>>()
            .join(", ");
        let tuple_text = if items.len() == 1 {
            format!("({items_text},)")
        } else {
            format!("({items_text})")
        };
        Ok(format!(
            "{{ let tuple_items = {}; if tuple_items.len() != {} {{ panic!(\"tuple() length mismatch\"); }} {tuple_text} }}",
            self.operand_text(list)?,
            items.len()
        ))
    }

    // Converts a homogeneous tuple-to-set constructor conversion to Rust text.

}
