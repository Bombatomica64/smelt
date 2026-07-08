//! Set emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a set containment operation to Rust text.
    pub(super) fn set_contains_text(
        &self,
        set: &Operand,
        item: &Operand,
    ) -> Result<String, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let Some(Type::Set(set_item_ty)) = self.mir.types.get(set_ty) else {
            return Ok("false".to_owned());
        };
        let item_ty = *set_item_ty;
        let needle_ty = self.operand_ty(item)?;
        // A `T | undefined` needle (e.g. `set.has(sample(set))`) still checks
        // containment against a `Set<T>`: unwrap the optional and guard on
        // `Some`, since a `None`/`undefined` needle is never an element of a set
        // of non-optional `T`. The narrowed `smelt_needle` already has the
        // element type, so it feeds the same per-element comparison directly.
        if needle_ty != item_ty
            && let Some(Type::Optional(opt_inner)) = self.mir.types.get(needle_ty)
        {
            let inner = *opt_inner;
            let set_text = self.operand_text(set)?;
            let body = if inner == item_ty {
                if self.mir.types.get(item_ty) == Some(&Type::Float) {
                    format!("{set_text}.iter().any(|value| *value == smelt_needle)")
                } else if matches!(
                    self.mir.types.get(item_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) {
                    format!("{set_text}.iter().any(|value| value.same_js_key(&smelt_needle))")
                } else {
                    format!("{set_text}.contains(&smelt_needle)")
                }
            } else if matches!(
                self.mir.types.get(inner),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) {
                // An erased needle compares against each element erased to the
                // runtime `SmeltUnknown` value (JS `Set.prototype.has`).
                let erased = self.erase_value_text("value.clone()", item_ty)?;
                format!("{set_text}.iter().any(|value| ({erased}).same_js_key(&smelt_needle))")
            } else {
                return Ok("false".to_owned());
            };
            return Ok(format!(
                "{{ match {} {{ Some(smelt_needle) => {body}, None => false }} }}",
                self.operand_text(item)?
            ));
        }
        let item_text = self.value_at_type(item, item_ty)?;
        if self.mir.types.get(item_ty) == Some(&Type::Float) {
            return Ok(format!(
                "{}.iter().any(|value| *value == {item_text})",
                self.operand_text(set)?
            ));
        }
        // A `Set<unknown>` follows JS `Set.prototype.has` reference semantics
        // (SameValueZero): objects/arrays match by identity, not structural
        // contents — exactly like the data-first `Array.prototype.includes` path
        // (`same_js_key`). `HashSet::contains` would instead use SmeltUnknown's
        // structural `Eq`, so e.g. `new Set([obj]).has({...obj})` would wrongly
        // report `true`. Primitive elements still compare by value because
        // `same_js_key` falls through to `==` for them.
        if matches!(
            self.mir.types.get(item_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return Ok(format!(
                "{}.iter().any(|value| value.same_js_key(&{item_text}))",
                self.operand_text(set)?
            ));
        }
        Ok(format!(
            "{}.contains(&{})",
            self.operand_text(set)?,
            item_text
        ))
    }

    /// Converts a set disjointness predicate to Rust text.
    /// Converts a set disjointness predicate to Rust text.
    pub(super) fn set_disjoint_text(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<String, EmitError> {
        self.validate_set_pair_operands(left, right, "set isdisjoint")?;
        Ok(format!(
            "{}.is_disjoint(&{})",
            self.operand_text(left)?,
            self.operand_text(right)?
        ))
    }

    /// Converts a set relation predicate to Rust text.
    /// Converts a set relation predicate to Rust text.
    pub(super) fn set_relation_text(
        &self,
        op: smelt_hir::SetRelationOp,
        left: &Operand,
        right: &Operand,
    ) -> Result<String, EmitError> {
        self.validate_set_pair_operands(left, right, "set relation")?;
        let method = match op {
            smelt_hir::SetRelationOp::IsSubset => "is_subset",
            smelt_hir::SetRelationOp::IsSuperset => "is_superset",
        };
        Ok(format!(
            "{}.{method}(&{})",
            self.operand_text(left)?,
            self.operand_text(right)?
        ))
    }

    /// Validates two same-typed set operands.
    /// Converts a set insertion operation to Rust text.
    pub(super) fn set_add_text(
        &self,
        set: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = set else {
            return Err(EmitError::new(
                "set add receiver must be a mutable local for now",
            ));
        };
        let set_text = self.local_mut_value_text(*local)?;
        let Some(Type::Set(item_ty)) = self.mir.types.get(set_ty) else {
            return Err(EmitError::new("set add receiver must be a set"));
        };
        let item_text = self.value_at_type(item, *item_ty)?;
        if self.mir.types.get(*item_ty) == Some(&Type::Float) {
            return if matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
                Ok(format!(
                    "{{ if !{set_text}.iter().any(|value| *value == {item_text}) {{ {set_text}.push({item_text}); }} () }}"
                ))
            } else if dest_ty == set_ty {
                Ok(format!(
                    "{{ if !{set_text}.iter().any(|value| *value == {item_text}) {{ {set_text}.push({item_text}); }} {set_text}.clone() }}"
                ))
            } else {
                Err(EmitError::new(
                    "set add destination must be None or the receiver set type",
                ))
            };
        }
        if matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            Ok(format!("{{ {set_text}.insert({item_text}); () }}"))
        } else if dest_ty == set_ty {
            Ok(format!(
                "{{ {set_text}.insert({item_text}); {set_text}.clone() }}"
            ))
        } else {
            Err(EmitError::new(
                "set add destination must be None or the receiver set type",
            ))
        }
    }

    /// Converts a set removal operation to Rust text.
    /// Converts a set removal operation to Rust text.
    pub(super) fn set_remove_text(
        &self,
        op: smelt_hir::SetRemoveOp,
        set: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let Some(Type::Set(item_ty)) = self.mir.types.get(set_ty) else {
            return Err(EmitError::new("set remove receiver must be a set"));
        };
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = set else {
            return Err(EmitError::new(
                "set remove receiver must be a mutable local for now",
            ));
        };
        let set_text = self.local_mut_value_text(*local)?;
        // Coerce the removed item to the set's element type so removing a
        // concrete value (e.g. a `number`) from an erased `Set<unknown>` erases
        // the key the same way `set_add_text` erases inserted items; the set
        // stores `SmeltUnknown`, so the lookup key must be erased to match.
        let item_text = self.value_at_type(item, *item_ty)?;
        match op {
            smelt_hir::SetRemoveOp::Delete => {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("set delete destination must be bool"));
                }
                Ok(format!("{set_text}.remove(&{item_text})"))
            }
            smelt_hir::SetRemoveOp::Discard => {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
                    return Err(EmitError::new("set discard destination must be None"));
                }
                Ok(format!("{{ {set_text}.remove(&{item_text}); () }}"))
            }
            smelt_hir::SetRemoveOp::Remove => {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
                    return Err(EmitError::new("set remove destination must be None"));
                }
                Ok(format!(
                    "{{ if !{set_text}.remove(&{item_text}) {{ panic!(\"set remove missing item\"); }} () }}"
                ))
            }
        }
    }

    /// Converts a set copy operation to Rust text.
    /// Converts a set copy operation to Rust text.
    pub(super) fn set_copy_text(
        &self,
        set: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let set_ty = self.operand_ty(set)?;
        if !matches!(self.mir.types.get(set_ty), Some(Type::Set(_))) {
            return Err(EmitError::new("set copy receiver must be a set"));
        }
        if dest_ty != set_ty {
            return Err(EmitError::new(
                "set copy destination must match the receiver set type",
            ));
        }
        Ok(format!("{}.clone()", self.operand_text(set)?))
    }

    /// Converts a set algebra operation to Rust text.
    /// Converts a set algebra operation to Rust text.
    pub(super) fn set_binary_text(
        &self,
        op: smelt_hir::SetBinaryOp,
        left: &Operand,
        right: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let left_ty = self.operand_ty(left)?;
        if self.operand_ty(right)? != left_ty {
            return Err(EmitError::new(
                "set algebra operands must have the same set type",
            ));
        }
        if !matches!(self.mir.types.get(left_ty), Some(Type::Set(_))) {
            return Err(EmitError::new("set algebra operands must be sets"));
        }
        if dest_ty != left_ty {
            return Err(EmitError::new(
                "set algebra destination must match the operand set type",
            ));
        }
        let method = match op {
            smelt_hir::SetBinaryOp::Union => "union",
            smelt_hir::SetBinaryOp::Intersection => "intersection",
            smelt_hir::SetBinaryOp::Difference => "difference",
            smelt_hir::SetBinaryOp::SymmetricDifference => "symmetric_difference",
        };
        Ok(format!(
            "{}.{}(&{}).cloned().collect()",
            self.operand_text(left)?,
            method,
            self.operand_text(right)?
        ))
    }

    /// Converts a set projection operation to Rust text.
    /// Converts a set projection operation to Rust text.
    pub(super) fn set_projection_text(
        &self,
        op: smelt_hir::SetProjectionOp,
        set: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let Some(Type::Set(item_ty)) = self.mir.types.get(set_ty) else {
            return Err(EmitError::new("set projection receiver must be a set"));
        };
        match (op, self.mir.types.get(dest_ty)) {
            (smelt_hir::SetProjectionOp::Values, Some(Type::List(inner))) if inner == item_ty => {}
            (smelt_hir::SetProjectionOp::Entries, Some(Type::List(entry_ty)))
                if matches!(
                    self.mir.types.get(*entry_ty),
                    Some(Type::Tuple(items))
                        if matches!(items.as_slice(), [left, right] if left == item_ty && right == item_ty)
                ) => {}
            _ => {
                return Err(EmitError::new(
                    "set projection destination does not match projection type",
                ));
            }
        }
        let set_text = self.operand_text(set)?;
        match op {
            smelt_hir::SetProjectionOp::Values => {
                Ok(format!("{set_text}.iter().cloned().collect::<Vec<_>>()"))
            }
            smelt_hir::SetProjectionOp::Entries => Ok(format!(
                "{set_text}.iter().map(|value| (value.clone(), value.clone())).collect::<Vec<_>>()"
            )),
        }
    }

    // Converts a list-to-set constructor conversion to Rust text.
}
