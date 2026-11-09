//! Tuple emission helpers.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Converts a tuple containment operation to Rust text.
    pub(super) fn tuple_contains_expr(
        &self,
        tuple: &Operand,
        item: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let tuple_ty = self.operand_ty(tuple)?;
        let Some(Type::Tuple(items)) = self.mir.types.get(tuple_ty) else {
            return Err(EmitError::new("tuple contains receiver must be a tuple"));
        };
        let items_len = items.len();
        let item_ty = self.operand_ty(item)?;
        if !items.contains(&item_ty) {
            // A `T | undefined` needle (e.g. `tuple.includes(sample(tuple))`)
            // still checks containment against the tuple elements: unwrap the
            // optional and guard on `Some`, since a `None`/`undefined` needle is
            // never one of the (non-optional) tuple fields. The narrowed needle
            // compares directly when its inner type is a tuple element type, or,
            // when it is erased (`unknown | undefined`), against each field
            // erased to the runtime `SmeltUnknown` value (JS `includes`).
            if let Some(Type::Optional(opt_inner)) = self.mir.types.get(item_ty) {
                let inner = *opt_inner;
                if items_len == 0 {
                    return Ok(RustExpr::path("false"));
                }
                let element_types: Vec<TypeId> = items.clone();
                let tuple_text = self.operand_expr(tuple)?;
                let comparisons = if element_types.contains(&inner) {
                    (0..items_len)
                        .map(|idx| format!("{tuple_text}.{idx} == smelt_needle", tuple_text = tuple_text.tight()))
                        .collect::<Vec<_>>()
                        .join(" || ")
                } else if matches!(
                    self.mir.types.get(inner),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) {
                    element_types
                        .iter()
                        .enumerate()
                        .map(|(idx, field_ty)| {
                            let erased =
                                self.erase_rendered(&RustExpr::tuple_field(&tuple_text, idx), *field_ty)?;
                            Ok(self.carrier_format(self.carrier()?, format_args!("({erased}).same_js_key(&smelt_needle)", erased = erased.delimited())))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(" || ")
                } else {
                    return Err(EmitError::new(
                        "tuple contains item must match at least one tuple element type",
                    ));
                };
                let item_text = self.operand_expr(item)?;
                return Ok(RustExpr::atom(format!(
                    "{{ match {item_text} {{ Some(smelt_needle) => {comparisons}, None => false }} }}", item_text = item_text.delimited()
                )));
            }
            return Err(EmitError::new(
                "tuple contains item must match at least one tuple element type",
            ));
        }
        if items_len == 0 {
            return Ok(RustExpr::path("false"));
        }
        let tuple_text = self.operand_expr(tuple)?;
        let item_text = self.operand_expr(item)?;
        Ok(RustExpr::from((0..items_len)
            .map(|idx| format!("{tuple_text}.{idx} == {item_text}", tuple_text = tuple_text.tight(), item_text = item_text.as_binary_operand()))
            .collect::<Vec<_>>()
            .join(" || ")))
    }

    /// Converts a statically-known tuple index operation to Rust text.
    pub(super) fn tuple_index_expr(
        &self,
        tuple: &Operand,
        index: usize,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let tuple_ty = self.operand_ty(tuple)?;
        let Some(Type::Tuple(items)) = self.mir.types.get(tuple_ty) else {
            if self.has_dynamic_shape(tuple_ty)
            {
                let runtime_index = u32::try_from(index).map_err(|err| {
                    EmitError::new(format!("tuple index does not fit in runtime number: {err}"))
                })?;
                let index_operand = Operand::Const(Constant::Float(f64::from(runtime_index)));
                // Same boundary as the erased index read in `place.rs`: a
                // concrete generated union reaches the runtime-narrowing match
                // through `IntoSmeltUnknown`, never as itself.
                let tuple_text =
                    self.erase_concrete_union_expr(&self.operand_expr(tuple)?, tuple_ty)?;
                let unknown_value = self.unknown_index_expr(&tuple_text, &index_operand)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                return self.rendered_at_type(&unknown_value, unknown_ty, dest_ty, &self.render_scope());
            }
            // A narrowed optional union or `unknown` holds the tuple the
            // narrowing proved.
            if self.is_optional_of_dynamic_shape(tuple_ty) {
                let runtime_index = u32::try_from(index).map_err(|err| {
                    EmitError::new(format!("tuple index does not fit in runtime number: {err}"))
                })?;
                let index_operand = Operand::Const(Constant::Float(f64::from(runtime_index)));
                let erased = self.erase_rendered(&self.operand_expr(tuple)?, tuple_ty)?;
                let unknown_value = self.unknown_index_expr(&erased, &index_operand)?;
                let unknown_ty = self.type_id(Type::Unknown)?;
                return self.rendered_at_type(&unknown_value, unknown_ty, dest_ty, &self.render_scope());
            }
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
        Ok(self.operand_expr(tuple)?.postfix(format!(".{index}")).cloned_value())
    }

    /// Converts a statically-known tuple slice operation to Rust text.
    pub(super) fn tuple_slice_expr(
        &self,
        tuple: &Operand,
        start: usize,
        end: usize,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        let tuple_text = self.operand_expr(tuple)?;
        let items_text = (start..end)
            .map(|idx| format!("{tuple_text}.{idx}.clone()", tuple_text = tuple_text.tight()))
            .collect::<Vec<_>>()
            .join(", ");
        if end.checked_sub(start) == Some(1) {
            Ok(RustExpr::atom(format!("({items_text},)")))
        } else {
            Ok(RustExpr::atom(format!("({items_text})")))
        }
    }

    /// Converts a homogeneous tuple-to-list constructor conversion to Rust text.
    pub(super) fn tuple_to_list_expr(
        &self,
        tuple: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let (items, item_ty) = self.homogeneous_tuple_conversion_parts(tuple, dest_ty, "list")?;
        let tuple_text = self.operand_expr(tuple)?;
        let items_text = (0..items.len())
            .map(|idx| format!("{tuple_text}.{idx}.clone()", tuple_text = tuple_text.tight()))
            .collect::<Vec<_>>()
            .join(", ");
        if !matches!(self.mir.types.get(dest_ty), Some(Type::List(dest_item)) if *dest_item == item_ty)
        {
            return Err(EmitError::new(
                "tuple-to-list destination must be list of the tuple item type",
            ));
        }
        Ok(RustExpr::atom(format!("vec![{items_text}]")))
    }

    /// Converts a homogeneous tuple-to-set constructor conversion to Rust text.
    pub(super) fn tuple_to_set_expr(
        &self,
        tuple: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let (items, item_ty) = self.homogeneous_tuple_conversion_parts(tuple, dest_ty, "set")?;
        let tuple_text = self.operand_expr(tuple)?;
        let items_text = (0..items.len())
            .map(|idx| format!("{tuple_text}.{idx}.clone()", tuple_text = tuple_text.tight()))
            .collect::<Vec<_>>()
            .join(", ");
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Set(dest_item)) if *dest_item == item_ty)
        {
            return Err(EmitError::new(
                "tuple-to-set destination must be set of the tuple item type",
            ));
        }
        Ok({ let receiver = self.set_container_name(item_ty); RustExpr::atom_on(&receiver, format!("{receiver}::from([{items_text}])")) })
    }

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

}
