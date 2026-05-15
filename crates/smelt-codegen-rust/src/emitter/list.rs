//! List emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a list containment operation to Rust text.
    pub(super) fn list_contains_text(
        &self,
        list: &Operand,
        item: &Operand,
    ) -> Result<String, EmitError> {
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
    pub(super) fn list_to_set_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
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
    pub(super) fn list_concat_text(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<String, EmitError> {
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

    /// Converts an array splice or toSpliced operation to Rust text.
    pub(super) fn list_splice_text(
        &self,
        list: &Operand,
        start: &Operand,
        delete_count: Option<&Operand>,
        items: &[MirListSpliceItem],
        mutate: bool,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array splice receiver must be a list"));
        };
        self.validate_optional_numeric_index(Some(start), "array splice start index")?;
        self.validate_optional_numeric_index(delete_count, "array splice delete count")?;
        for item in items {
            let expected_ty = if item.spread { list_ty } else { *item_ty };
            if self.operand_ty(&item.value)? != expected_ty {
                return Err(EmitError::new(
                    "array splice replacement items must match the array element type",
                ));
            }
        }
        if dest_ty != list_ty {
            return Err(EmitError::new("array splice destination must be a list"));
        }
        let list_text = self.operand_text(list)?;
        let start_text = self.operand_text(start)?;
        let delete_count_text = delete_count
            .map(|count| self.operand_text(count))
            .transpose()?
            .unwrap_or_else(|| "splice_len as f64".to_owned());
        let replacement_text = self.list_splice_replacement_text(items)?;
        let receiver_text = if mutate {
            let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list
            else {
                return Err(EmitError::new(
                    "array splice receiver must be a mutable local for now",
                ));
            };
            self.local_name(*local)?.to_owned()
        } else {
            format!("{list_text}.clone()")
        };
        if mutate {
            Ok(format!(
                "{{ let splice_len = {receiver_text}.len(); let splice_start = if {start_text} < 0.0 {{ splice_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(splice_len) }}; let splice_delete = (({delete_count_text}).max(0.0) as usize).min(splice_len.saturating_sub(splice_start)); {receiver_text}.splice(splice_start..splice_start + splice_delete, {replacement_text}).collect::<Vec<_>>() }}"
            ))
        } else {
            Ok(format!(
                "{{ let mut spliced = {receiver_text}; let splice_len = spliced.len(); let splice_start = if {start_text} < 0.0 {{ splice_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(splice_len) }}; let splice_delete = (({delete_count_text}).max(0.0) as usize).min(splice_len.saturating_sub(splice_start)); spliced.splice(splice_start..splice_start + splice_delete, {replacement_text}).for_each(drop); spliced }}"
            ))
        }
    }

    /// Converts scalar and spread splice replacements into a Rust vector expression.
    fn list_splice_replacement_text(
        &self,
        items: &[MirListSpliceItem],
    ) -> Result<String, EmitError> {
        if items.iter().all(|item| !item.spread) {
            let items_text = items
                .iter()
                .map(|item| self.operand_text(&item.value))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            return Ok(format!("vec![{items_text}]"));
        }
        let mut statements = Vec::new();
        for item in items {
            let value_text = self.operand_text(&item.value)?;
            if item.spread {
                statements.push(format!(
                    "splice_replacements.extend(({value_text}).iter().cloned());"
                ));
            } else {
                statements.push(format!("splice_replacements.push({value_text});"));
            }
        }
        Ok(format!(
            "{{ let mut splice_replacements = Vec::new(); {} splice_replacements }}",
            statements.join(" ")
        ))
    }

    /// Converts an array fill operation to Rust text.
    pub(super) fn list_fill_text(
        &self,
        list: &Operand,
        value: &Operand,
        start: Option<&Operand>,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array fill receiver must be a list"));
        };
        if self.operand_ty(value)? != *item_ty || dest_ty != list_ty {
            return Err(EmitError::new(
                "array fill value and destination must match the list type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "array fill receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_name(*local)?;
        let value_text = self.operand_text(value)?;
        let start_text = start
            .map(|operand| self.operand_text(operand))
            .transpose()?
            .unwrap_or_else(|| "0.0".to_owned());
        let end_text = end
            .map(|operand| self.operand_text(operand))
            .transpose()?
            .unwrap_or_else(|| "fill_len as f64".to_owned());
        Ok(format!(
            "{{ let fill_len = {list_text}.len(); let fill_start = if {start_text} < 0.0 {{ fill_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(fill_len) }}; let fill_end = if {end_text} < 0.0 {{ fill_len.saturating_sub((-{end_text}) as usize) }} else {{ ({end_text} as usize).min(fill_len) }}; for fill_index in fill_start..fill_end {{ {list_text}[fill_index] = {value_text}.clone(); }} {list_text}.clone() }}"
        ))
    }

    /// Converts an array copyWithin operation to Rust text.
    pub(super) fn list_copy_within_text(
        &self,
        list: &Operand,
        target: &Operand,
        start: &Operand,
        end: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) || dest_ty != list_ty {
            return Err(EmitError::new(
                "array copyWithin destination must match the list type",
            ));
        }
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = list else {
            return Err(EmitError::new(
                "array copyWithin receiver must be a mutable local for now",
            ));
        };
        let list_text = self.local_name(*local)?;
        let target_text = self.operand_text(target)?;
        let start_text = self.operand_text(start)?;
        let end_text = end
            .map(|operand| self.operand_text(operand))
            .transpose()?
            .unwrap_or_else(|| "copy_len as f64".to_owned());
        Ok(format!(
            "{{ let copy_len = {list_text}.len(); let copy_target = if {target_text} < 0.0 {{ copy_len.saturating_sub((-{target_text}) as usize) }} else {{ ({target_text} as usize).min(copy_len) }}; let copy_start = if {start_text} < 0.0 {{ copy_len.saturating_sub((-{start_text}) as usize) }} else {{ ({start_text} as usize).min(copy_len) }}; let copy_end = if {end_text} < 0.0 {{ copy_len.saturating_sub((-{end_text}) as usize) }} else {{ ({end_text} as usize).min(copy_len) }}; let copy_items = {list_text}.iter().skip(copy_start).take(copy_end.saturating_sub(copy_start)).cloned().collect::<Vec<_>>(); for (offset, item) in copy_items.into_iter().enumerate() {{ if copy_target + offset < copy_len {{ {list_text}[copy_target + offset] = item; }} }} {list_text}.clone() }}"
        ))
    }

    /// Converts a list push operation to Rust text.
    /// Converts a list copy operation to Rust text.
    /// Converts a list push operation to Rust text.
    /// Converts a list copy operation to Rust text.
    pub(super) fn list_copy_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
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

    /// Converts an array `with` operation to Rust text.
    pub(super) fn list_with_text(
        &self,
        list: &Operand,
        index: &Operand,
        value: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array with receiver must be a list"));
        };
        if dest_ty != list_ty || self.operand_ty(value)? != *item_ty {
            return Err(EmitError::new(
                "array with value and destination must match the list type",
            ));
        }
        let list_text = self.operand_text(list)?;
        let index_text = self.operand_text(index)?;
        let value_text = self.operand_text(value)?;
        Ok(format!(
            "{{ let mut with_items = {list_text}.clone(); let with_len = with_items.len(); let with_index = if {index_text} < 0.0 {{ with_len.saturating_sub((-{index_text}) as usize) }} else {{ {index_text} as usize }}; if with_index >= with_len {{ panic!(\"array with index out of bounds\"); }} with_items[with_index] = {value_text}; with_items }}"
        ))
    }

    /// Converts one-level array flattening to Rust text.
    pub(super) fn list_flat_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(nested_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array flat receiver must be a list"));
        };
        let Some(Type::List(item_ty)) = self.mir.types.get(*nested_ty) else {
            return Err(EmitError::new("array flat receiver must contain arrays"));
        };
        if self.mir.types.get(dest_ty) != Some(&Type::List(*item_ty)) {
            return Err(EmitError::new(
                "array flat destination must match nested item type",
            ));
        }
        Ok(format!(
            "{}.iter().flat_map(|items| items.iter().cloned()).collect::<Vec<_>>()",
            self.operand_text(list)?
        ))
    }

    /// Converts array keys, values, and entries projections to Rust text.
    pub(super) fn list_projection_text(
        &self,
        op: smelt_hir::ListProjectionOp,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array projection receiver must be a list"));
        };
        let list_text = self.operand_text(list)?;
        match op {
            smelt_hir::ListProjectionOp::Keys => {
                let int_ty = self.type_id(Type::Int)?;
                if self.mir.types.get(dest_ty) != Some(&Type::List(int_ty)) {
                    return Err(EmitError::new("array keys destination must be int list"));
                }
                Ok(format!(
                    "(0..{list_text}.len()).map(|idx| idx as i64).collect::<Vec<_>>()"
                ))
            }
            smelt_hir::ListProjectionOp::Values => {
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array values destination must match receiver",
                    ));
                }
                Ok(format!("{list_text}.clone()"))
            }
            smelt_hir::ListProjectionOp::Entries => {
                let int_ty = self.type_id(Type::Int)?;
                let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("array entries destination must be a list"));
                };
                let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
                    return Err(EmitError::new("array entries item must be a tuple"));
                };
                if items.as_slice() != [int_ty, *item_ty] {
                    return Err(EmitError::new(
                        "array entries destination must contain (int, item) tuples",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().cloned().enumerate().map(|(idx, item)| (idx as i64, item)).collect::<Vec<_>>()"
                ))
            }
        }
    }

    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a tuple containment operation to Rust text.
    /// Converts a list-to-tuple constructor conversion to Rust text.
    pub(super) fn list_to_tuple_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
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
