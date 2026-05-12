//! Tuple emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a tuple containment operation to Rust text.
    pub(super) fn tuple_contains_text(
        &self,
        tuple: &Operand,
        item: &Operand,
    ) -> Result<String, EmitError> {
        let tuple_ty = self.operand_ty(tuple)?;
        let Some(Type::Tuple(items)) = self.mir.types.get(tuple_ty) else {
            return Err(EmitError::new("tuple contains receiver must be a tuple"));
        };
        let item_ty = self.operand_ty(item)?;
        if !items.contains(&item_ty) {
            return Err(EmitError::new(
                "tuple contains item must match at least one tuple element type",
            ));
        }
        if items.is_empty() {
            return Ok("false".to_owned());
        }
        let tuple_text = self.operand_text(tuple)?;
        let item_text = self.operand_text(item)?;
        Ok((0..items.len())
            .map(|idx| format!("{tuple_text}.{idx} == {item_text}"))
            .collect::<Vec<_>>()
            .join(" || "))
    }

    /// Converts a statically-known tuple index operation to Rust text.
    /// Converts a statically-known tuple index operation to Rust text.
    pub(super) fn tuple_index_text(
        &self,
        tuple: &Operand,
        index: usize,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let tuple_ty = self.operand_ty(tuple)?;
        let Some(Type::Tuple(items)) = self.mir.types.get(tuple_ty) else {
            return Err(EmitError::new("tuple index receiver must be a tuple"));
        };
        let Some(item_ty) = items.get(index) else {
            return Err(EmitError::new("tuple index is out of bounds"));
        };
        if *item_ty != dest_ty {
            return Err(EmitError::new(
                "tuple index destination must match the tuple field type",
            ));
        }
        Ok(format!("{}.{}.clone()", self.operand_text(tuple)?, index))
    }

    /// Converts a statically-known tuple slice operation to Rust text.
    /// Converts a statically-known tuple slice operation to Rust text.
    pub(super) fn tuple_slice_text(
        &self,
        tuple: &Operand,
        start: usize,
        end: usize,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let tuple_ty = self.operand_ty(tuple)?;
        let Some(Type::Tuple(items)) = self.mir.types.get(tuple_ty) else {
            return Err(EmitError::new("tuple slice receiver must be a tuple"));
        };
        if start > end || end > items.len() {
            return Err(EmitError::new("tuple slice bounds are out of range"));
        }
        let Some(Type::Tuple(dest_items)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("tuple slice destination must be a tuple"));
        };
        let Some(selected_items) = items.get(start..end) else {
            return Err(EmitError::new("tuple slice bounds are out of range"));
        };
        if dest_items.as_slice() != selected_items {
            return Err(EmitError::new(
                "tuple slice destination must match selected tuple field types",
            ));
        }
        let tuple_text = self.operand_text(tuple)?;
        let items_text = (start..end)
            .map(|idx| format!("{tuple_text}.{idx}.clone()"))
            .collect::<Vec<_>>()
            .join(", ");
        if end.checked_sub(start) == Some(1) {
            Ok(format!("({items_text},)"))
        } else {
            Ok(format!("({items_text})"))
        }
    }

    /// Converts a homogeneous tuple-to-list constructor conversion to Rust text.
    /// Converts a homogeneous tuple-to-list constructor conversion to Rust text.
    pub(super) fn tuple_to_list_text(
        &self,
        tuple: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (items, item_ty) = self.homogeneous_tuple_conversion_parts(tuple, dest_ty, "list")?;
        let tuple_text = self.operand_text(tuple)?;
        let items_text = (0..items.len())
            .map(|idx| format!("{tuple_text}.{idx}.clone()"))
            .collect::<Vec<_>>()
            .join(", ");
        if !matches!(self.mir.types.get(dest_ty), Some(Type::List(dest_item)) if *dest_item == item_ty)
        {
            return Err(EmitError::new(
                "tuple-to-list destination must be list of the tuple item type",
            ));
        }
        Ok(format!("vec![{items_text}]"))
    }

    /// Converts a list-to-tuple constructor conversion to Rust text.
    /// Converts a homogeneous tuple-to-set constructor conversion to Rust text.
    pub(super) fn tuple_to_set_text(
        &self,
        tuple: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let (items, item_ty) = self.homogeneous_tuple_conversion_parts(tuple, dest_ty, "set")?;
        let tuple_text = self.operand_text(tuple)?;
        let items_text = (0..items.len())
            .map(|idx| format!("{tuple_text}.{idx}.clone()"))
            .collect::<Vec<_>>()
            .join(", ");
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Set(dest_item)) if *dest_item == item_ty)
        {
            return Err(EmitError::new(
                "tuple-to-set destination must be set of the tuple item type",
            ));
        }
        Ok(format!("::std::collections::HashSet::from([{items_text}])"))
    }

    /// Returns tuple fields and their shared item type for homogeneous tuple conversions.
    /// Returns tuple fields and their shared item type for homogeneous tuple conversions.
    pub(super) fn homogeneous_tuple_conversion_parts(
        &self,
        tuple: &Operand,
        _dest_ty: TypeId,
        conversion_name: &str,
    ) -> Result<(&[TypeId], TypeId), EmitError> {
        let tuple_ty = self.operand_ty(tuple)?;
        let Some(Type::Tuple(items)) = self.mir.types.get(tuple_ty) else {
            return Err(EmitError::new(format!(
                "tuple-to-{conversion_name} source must be a tuple"
            )));
        };
        let Some(first_ty) = items.first().copied() else {
            return Err(EmitError::new(format!(
                "tuple-to-{conversion_name} requires a non-empty tuple"
            )));
        };
        if !items.iter().all(|item| *item == first_ty) {
            return Err(EmitError::new(format!(
                "tuple-to-{conversion_name} requires homogeneous tuple items"
            )));
        }
        Ok((items.as_slice(), first_ty))
    }

    // Converts a dictionary key containment operation to Rust text.
}
