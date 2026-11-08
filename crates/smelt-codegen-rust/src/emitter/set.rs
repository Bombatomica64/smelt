//! Set emission helpers.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Converts a set containment operation to Rust text.
    pub(super) fn set_contains_expr(
        &self,
        set: &Operand,
        item: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let Some(Type::Set(set_item_ty)) = self.mir.types.get(set_ty) else {
            return Ok(RustExpr::path("false"));
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
            // Read by borrow: both branches use `&self` methods
            // (`contains`, `iter().any`).
            let set_text = self.operand_borrow_expr(set)?;
            let body = if inner == item_ty {
                // The narrowed needle has the element type, so the container's
                // own `contains` applies the correct membership (SameValueZero
                // through `SmeltJsSet`, value equality for the `HashSet` backing).
                format!("{set_text}.contains(&smelt_needle)", set_text = set_text.tight())
            } else if matches!(
                self.mir.types.get(inner),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) {
                // An erased needle compares against each element erased to the
                // runtime `SmeltUnknown` value (JS `Set.prototype.has`).
                let erased = self.erase_rendered(&RustExpr::path("smelt_value").cloned_value(), item_ty)?;
                self.carrier_format(self.carrier()?, format_args!("{set_text}.iter().any(|smelt_value| ({erased}).same_js_key(&smelt_needle))", set_text = set_text.tight(), erased = erased.delimited()))
            } else {
                return Ok(RustExpr::path("false"));
            };
            return Ok(RustExpr::atom(format!(
                "{{ match {} {{ Some(smelt_needle) => {body}, None => false }} }}",
                self.operand_expr(item)?.delimited()
            )));
        }
        // `shared_reference_argument_expr` parenthesizes a coerced needle, so the
        // reference wraps the whole cast (`&(x as f64)`) rather than casting a
        // reference (`&x as f64`, an invalid `&i64 as f64`), and hands over the
        // place itself when no coercion is needed.
        //
        // Membership goes through the container's own `contains`. For the
        // `HashSet` backing (`bool`/`i64`/`String`) that is value equality, which
        // matches JS SameValueZero for those primitives. For every other element
        // type the set is a `SmeltJsSet`, whose `contains` applies SameValueZero
        // via each element's erased runtime value: `NaN` equals `NaN`, `+0`
        // equals `-0`, and objects/functions match by reference identity — the
        // JS `Set.prototype.has` semantics — rather than `HashSet`'s structural
        // `Eq`.
        Ok({ let receiver = self.operand_expr(set)?; receiver.postfix(format!(".contains({})", self.shared_reference_argument_expr(item, item_ty)?.delimited())) })
    }

    /// Converts a set disjointness predicate to Rust text.
    pub(super) fn set_disjoint_expr(
        &self,
        left: &Operand,
        right: &Operand,
    ) -> Result<RustExpr, EmitError> {
        self.validate_set_pair_operands(left, right, "set isdisjoint")?;
        Ok({ let receiver = self.operand_expr(left)?; receiver.postfix(format!(".is_disjoint(&{})", self.operand_expr(right)?.tight())) })
    }

    /// Converts a set relation predicate to Rust text.
    pub(super) fn set_relation_expr(
        &self,
        op: smelt_hir::SetRelationOp,
        left: &Operand,
        right: &Operand,
    ) -> Result<RustExpr, EmitError> {
        self.validate_set_pair_operands(left, right, "set relation")?;
        let method = match op {
            smelt_hir::SetRelationOp::IsSubset => "is_subset",
            smelt_hir::SetRelationOp::IsSuperset => "is_superset",
        };
        Ok({ let receiver = self.operand_expr(left)?; receiver.postfix(format!(".{method}(&{})", self.operand_expr(right)?.tight())) })
    }

    /// Converts a set insertion operation to Rust text.
    pub(super) fn set_add_expr(
        &self,
        set: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = set else {
            return Err(EmitError::new(
                "set add receiver must be a mutable local for now",
            ));
        };
        let set_text = self.local_mut_value(*local)?;
        let Some(Type::Set(item_ty)) = self.mir.types.get(set_ty) else {
            return Err(EmitError::new("set add receiver must be a set"));
        };
        let item_text = self.value_at_type(item, *item_ty)?;
        // Both backings (`HashSet` for value-equality primitives, `SmeltJsSet`
        // for everything else) expose an `insert` that dedups, so a single
        // insertion shape works for every element type — including `f64`, whose
        // `SmeltJsSet` insert applies SameValueZero (`NaN` equals `NaN`).
        if matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
            Ok(RustExpr::atom(format!("{{ {set_text}.insert({item_text}); () }}", set_text = set_text.tight(), item_text = item_text.delimited())))
        } else if dest_ty == set_ty {
            Ok(RustExpr::atom(format!(
                "{{ {set_text}.insert({item_text}); {set_text}.clone() }}", set_text = set_text.tight(), item_text = item_text.delimited()
            )))
        } else {
            Err(EmitError::new(
                "set add destination must be None or the receiver set type",
            ))
        }
    }

    /// Converts a set removal operation to Rust text.
    pub(super) fn set_remove_expr(
        &self,
        op: smelt_hir::SetRemoveOp,
        set: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let set_ty = self.operand_ty(set)?;
        let Some(Type::Set(item_ty)) = self.mir.types.get(set_ty) else {
            return Err(EmitError::new("set remove receiver must be a set"));
        };
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = set else {
            return Err(EmitError::new(
                "set remove receiver must be a mutable local for now",
            ));
        };
        let set_text = self.local_mut_value(*local)?;
        // Coerce the removed item to the set's element type so removing a
        // concrete value (e.g. a `number`) from an erased `Set<unknown>` erases
        // the key the same way `set_add_expr` erases inserted items; the set
        // stores `SmeltUnknown`, so the lookup key must be erased to match.
        let item_text = self.shared_reference_argument_expr(item, *item_ty)?;
        match op {
            smelt_hir::SetRemoveOp::Delete => {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::Bool)) {
                    return Err(EmitError::new("set delete destination must be bool"));
                }
                Ok(set_text.postfix(format!(".remove({item_text})", item_text = item_text.delimited())))
            }
            smelt_hir::SetRemoveOp::Discard => {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
                    return Err(EmitError::new("set discard destination must be None"));
                }
                Ok(RustExpr::atom(format!("{{ {set_text}.remove({item_text}); () }}", set_text = set_text.tight(), item_text = item_text.delimited())))
            }
            smelt_hir::SetRemoveOp::Remove => {
                if !matches!(self.mir.types.get(dest_ty), Some(Type::None)) {
                    return Err(EmitError::new("set remove destination must be None"));
                }
                Ok(RustExpr::atom(format!(
                    "{{ if !{set_text}.remove({item_text}) {{ panic!(\"set remove missing item\"); }} () }}", set_text = set_text.tight(), item_text = item_text.delimited()
                )))
            }
        }
    }

    /// Converts a set copy operation to Rust text.
    pub(super) fn set_copy_expr(
        &self,
        set: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let set_ty = self.operand_ty(set)?;
        if !matches!(self.mir.types.get(set_ty), Some(Type::Set(_))) {
            return Err(EmitError::new("set copy receiver must be a set"));
        }
        if dest_ty != set_ty {
            return Err(EmitError::new(
                "set copy destination must match the receiver set type",
            ));
        }
        Ok(self.operand_expr(set)?.cloned_value())
    }

    /// Converts a set algebra operation to Rust text.
    pub(super) fn set_binary_expr(
        &self,
        op: smelt_hir::SetBinaryOp,
        left: &Operand,
        right: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        Ok({ let receiver = self.operand_expr(left)?; receiver.postfix(format!(".{}(&{}).cloned().collect()", method,
            self.operand_expr(right)?.tight())) })
    }

    /// Converts a set projection operation to Rust text.
    pub(super) fn set_projection_expr(
        &self,
        op: smelt_hir::SetProjectionOp,
        set: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        // Read by borrow: both projections are `.iter()` receivers.
        let set_text = self.operand_borrow_expr(set)?;
        match op {
            smelt_hir::SetProjectionOp::Values => {
                Ok(RustExpr::atom_on(&set_text, format!("{set_text}.iter().cloned().collect::<Vec<_>>()", set_text = set_text.tight())))
            }
            smelt_hir::SetProjectionOp::Entries => Ok(RustExpr::atom_on(&set_text, format!(
                "{set_text}.iter().map(|smelt_value| (smelt_value.clone(), smelt_value.clone())).collect::<Vec<_>>()", set_text = set_text.tight()
            ))),
        }
    }


    /// Validates two same-typed set operands.
    pub(super) fn validate_set_pair_operands(
        &self,
        left: &Operand,
        right: &Operand,
        context: &str,
    ) -> Result<TypeId, EmitError> {
        let left_ty = self.operand_ty(left)?;
        if self.operand_ty(right)? != left_ty {
            return Err(EmitError::new(format!(
                "{context} operands must have the same set type"
            )));
        }
        if !matches!(self.mir.types.get(left_ty), Some(Type::Set(_))) {
            return Err(EmitError::new(format!("{context} operands must be sets")));
        }
        Ok(left_ty)
    }
}
