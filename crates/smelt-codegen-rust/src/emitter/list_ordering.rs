//! List ordering and slicing emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a list sorted-copy operation to Rust text.
    ///
    /// Integer, boolean, and string lists use Rust's total `Ord` sort. Floating
    /// lists use `partial_cmp` and panic on unordered values such as `NaN`
    /// until Python-compatible edge semantics are modeled explicitly.
    /// Converts a list sorted-copy operation to Rust text.
    ///
    /// Integer, boolean, and string lists use Rust's total `Ord` sort. Floating
    /// lists use `partial_cmp` and panic on unordered values such as `NaN`
    /// until Python-compatible edge semantics are modeled explicitly.
    /// Converts a list sorted-copy operation to Rust text.
    ///
    /// Integer, boolean, and string lists use Rust's total `Ord` sort. Floating
    /// lists use `partial_cmp` and panic on unordered values such as `NaN`
    /// until Python-compatible edge semantics are modeled explicitly.
    /// Converts a list sorted-copy operation to Rust text.
    ///
    /// Integer, boolean, and string lists use Rust's total `Ord` sort. Floating
    /// lists use `partial_cmp` and panic on unordered values such as `NaN`
    /// until Python-compatible edge semantics are modeled explicitly.
    pub(super) fn list_sorted_text(
        &self,
        list: &Operand,
        key: Option<&Operand>,
        reverse: bool,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("sorted() argument must be a list"));
        };
        let element_ty = *item_ty;
        if dest_ty != list_ty {
            return Err(EmitError::new(
                "sorted() destination must match the input list type",
            ));
        }
        // Each branch below appends its own `.clone()` to get the mutable copy it
        // sorts, so reading by borrow leaves exactly one copy where cloning on the
        // read made it `x.clone().clone()`.
        let list_text = self.operand_borrow_text(list)?;
        if key.is_some() || reverse {
            let (prefix, closure) = self.list_sort_by_text(key, reverse, element_ty)?;
            return Ok(format!(
                "{{ {prefix}let mut sorted = {list_text}.clone(); sorted.sort_by({closure}); sorted }}"
            ));
        }
        match self.mir.types.get(element_ty) {
            Some(Type::Bool | Type::Int | Type::String) => Ok(format!(
                "{{ let mut sorted = {list_text}.clone(); sorted.sort(); sorted }}"
            )),
            Some(Type::Float) => Ok(format!(
                "{{ let mut sorted = {list_text}.clone(); sorted.sort_by(|left, right| left.partial_cmp(right).expect(\"sorted incomparable float\")); sorted }}"
            )),
            _ => Err(EmitError::new(
                "sorted() supports bool, int, float, and string lists",
            )),
        }
    }

    /// Build the prefix bindings and `sort_by` comparison closure for Python
    /// `key`/`reverse` sorting.
    ///
    /// When `key` is present it is bound once as `smelt_sort_key` and applied to
    /// both compared items, producing owned key values. Otherwise the list items
    /// compare directly through their references. `reverse` swaps the comparison
    /// operands rather than reversing the result, so equal items keep their
    /// original order to match Python's stable descending sort. Floating keys use
    /// `partial_cmp` and panic on unordered values such as `NaN`.
    pub(super) fn list_sort_by_text(
        &self,
        key: Option<&Operand>,
        reverse: bool,
        element_ty: TypeId,
    ) -> Result<(String, String), EmitError> {
        if let Some(key_operand) = key {
            let Some(Type::Function(function_ty)) =
                self.mir.types.get(self.operand_ty(key_operand)?)
            else {
                return Err(EmitError::new("sort key must be a closure"));
            };
            let return_ty = function_ty.return_ty;
            let param_ty = function_ty.params.first().copied().unwrap_or(element_ty);
            let left_arg = self.value_at_type_text("left.clone()", element_ty, param_ty)?;
            let right_arg = self.value_at_type_text("right.clone()", element_ty, param_ty)?;
            let closure_text = match self.closure_operand_text_for_declared_type(key_operand) {
                Ok(closure_text) => closure_text,
                Err(_) => self.operand_text(key_operand)?,
            };
            let (first, second) = if reverse {
                ("right_key", "left_key")
            } else {
                ("left_key", "right_key")
            };
            let comparison = match self.mir.types.get(return_ty) {
                Some(Type::Float) => format!(
                    "{first}.partial_cmp(&{second}).expect(\"sort key incomparable float\")"
                ),
                Some(Type::Bool | Type::Int | Type::String) => format!("{first}.cmp(&{second})"),
                _ => {
                    return Err(EmitError::new(
                        "sort key must return bool, int, float, or string",
                    ));
                }
            };
            let prefix = format!("let mut smelt_sort_key = {closure_text}; ");
            let closure = format!(
                "|left, right| {{ let left_key = (smelt_sort_key)({left_arg}); let right_key = (smelt_sort_key)({right_arg}); {comparison} }}"
            );
            return Ok((prefix, closure));
        }
        let (first, second) = if reverse {
            ("right", "left")
        } else {
            ("left", "right")
        };
        let comparison = match self.mir.types.get(element_ty) {
            Some(Type::Float) => {
                format!("{first}.partial_cmp({second}).expect(\"sort incomparable float\")")
            }
            Some(Type::Bool | Type::Int | Type::String) => format!("{first}.cmp({second})"),
            _ => {
                return Err(EmitError::new(
                    "sort supports bool, int, float, and string items",
                ));
            }
        };
        Ok((String::new(), format!("|left, right| {comparison}")))
    }

    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    /// Converts a reversed-copy list operation to Rust text.
    pub(super) fn list_reversed_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("reversed() argument must be a list"));
        }
        if dest_ty != list_ty {
            return Err(EmitError::new(
                "reversed() destination must match the input list type",
            ));
        }
        Ok(format!(
            "{}.iter().rev().cloned().collect::<Vec<_>>()",
            self.operand_text(list)?
        ))
    }

    /// Converts list enumeration materialization to Rust text.
    ///
    /// TypeScript can narrow an erased generic through `Array.isArray` before
    /// calling `entries()`, while MIR retains its erased receiver type. The
    /// dynamic branch enumerates that array variant without library-specific
    /// lowering.
    pub(super) fn list_enumerate_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("enumerate() destination must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
            return Err(EmitError::new("enumerate() list item must be a tuple"));
        };
        let [index_ty, value_ty] = items.as_slice() else {
            return Err(EmitError::new(
                "enumerate() destination must contain (int, item) tuples",
            ));
        };
        if self.mir.types.get(*index_ty) != Some(&Type::Int) {
            return Err(EmitError::new(
                "enumerate() destination must contain (int, item) tuples",
            ));
        }
        if matches!(
            self.mir.types.get(list_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let item_text = self.extract_value_text("item", *value_ty)?;
            return Ok(format!(
                "match {}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(idx, item)| (idx as i64, {item_text})).collect::<Vec<_>>(), _ => Vec::new() }}",
                self.operand_text(list)?
            ));
        }
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("enumerate() argument must be a list"));
        };
        if *value_ty != *item_ty {
            return Err(EmitError::new(
                "enumerate() destination must contain (int, item) tuples",
            ));
        }
        Ok(format!(
            "{}.iter().cloned().enumerate().map(|(idx, item)| (idx as i64, item)).collect::<Vec<_>>()",
            self.operand_text(list)?
        ))
    }

    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    pub(super) fn list_zip_text(
        &self,
        left: &Operand,
        right: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let left_ty = self.operand_ty(left)?;
        let right_ty = self.operand_ty(right)?;
        let Some(Type::List(left_item_ty)) = self.mir.types.get(left_ty) else {
            return Err(EmitError::new("zip() left argument must be a list"));
        };
        let Some(Type::List(right_item_ty)) = self.mir.types.get(right_ty) else {
            return Err(EmitError::new("zip() right argument must be a list"));
        };
        let Some(Type::List(tuple_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("zip() destination must be a list"));
        };
        let Some(Type::Tuple(items)) = self.mir.types.get(*tuple_ty) else {
            return Err(EmitError::new("zip() list item must be a tuple"));
        };
        let [dest_left_ty, dest_right_ty] = items.as_slice() else {
            return Err(EmitError::new("zip() destination must contain pair tuples"));
        };
        if *dest_left_ty != *left_item_ty || *dest_right_ty != *right_item_ty {
            return Err(EmitError::new(
                "zip() destination tuple items must match argument item types",
            ));
        }
        Ok(format!(
            "{}.iter().cloned().zip({}.iter().cloned()).collect::<Vec<_>>()",
            self.operand_text(left)?,
            self.operand_text(right)?
        ))
    }

    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    pub(super) fn list_range_text(
        &self,
        start: &Operand,
        end: &Operand,
        step: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let int_ty = self.type_id(Type::Int)?;
        if self.operand_ty(start)? != int_ty
            || self.operand_ty(end)? != int_ty
            || self.operand_ty(step)? != int_ty
        {
            return Err(EmitError::new("range() bounds must be integers"));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::List(item)) if *item == int_ty) {
            return Err(EmitError::new("range() destination must be list[int]"));
        }
        Ok(format!(
            "{{ let start = {}; let end = {}; let step = {}; if step == 0 {{ panic!(\"range() arg 3 must not be zero\"); }} let mut values = Vec::new(); let mut current = start; if step > 0 {{ while current < end {{ values.push(current); current += step; }} }} else {{ while current > end {{ values.push(current); current += step; }} }} values }}",
            self.operand_text(start)?,
            self.operand_text(end)?,
            self.operand_text(step)?
        ))
    }

    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    pub(super) fn list_random_choice_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("random.choice() argument must be a list"));
        };
        if *item_ty != dest_ty {
            return Err(EmitError::new(
                "random.choice() destination must match list item type",
            ));
        }
        // Read by borrow, and bind a reference: every use is `&self`
        // (`is_empty`/`len`/index), so binding the value itself would move the
        // caller's list out of its binding.
        let list_text = self.operand_borrow_text(list)?;
        Ok(format!(
            "{{ let choice_items = &{list_text}; if choice_items.is_empty() {{ panic!(\"random.choice() from empty list\"); }} choice_items[rand::random_range(0..choice_items.len())].clone() }}"
        ))
    }

    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    pub(super) fn list_index_text(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list index receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list index item must match the list element type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
            return Err(EmitError::new("list index destination must be int"));
        }
        // Read by borrow: every use below is an `.iter()` receiver.
        let list_text = self.operand_borrow_text(list)?;
        let item_text = self.operand_text(item)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().position(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).expect(\"list index missing item\") as i64 }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().position(|item| item.same_js_key(&smelt_needle)).expect(\"list index missing item\") as i64 }}"
            ));
        }
        Ok(format!(
            "{list_text}.iter().position(|item| item == &{item_text}).expect(\"list index missing item\") as i64"
        ))
    }

    /// Converts a list remove operation to Rust text.
    /// Validates that an optional slice index is numeric.
    /// Converts a list remove operation to Rust text.
    /// Validates that an optional slice index is numeric.
    /// Converts a list remove operation to Rust text.
    /// Validates that an optional slice index is numeric.
    /// Converts a list remove operation to Rust text.
    /// Validates that an optional slice index is numeric.
    pub(super) fn validate_optional_numeric_index(
        &self,
        maybe_operand: Option<&Operand>,
        context: &str,
    ) -> Result<(), EmitError> {
        if let Some(inner) = maybe_operand
            && !self.slice_bound_type_is_numeric(self.operand_ty(inner)?)
        {
            return Err(EmitError::new(format!("{context} must be numeric")));
        }
        Ok(())
    }

    /// Return whether a slice-bound type is numeric, allowing optional numeric bounds.
    fn slice_bound_type_is_numeric(&self, ty: TypeId) -> bool {
        match self.mir.types.get(ty) {
            Some(
                Type::Int | Type::Float | Type::Unknown | Type::Union(_) | Type::TypeParam { .. },
            ) => true,
            Some(Type::Optional(item)) => {
                matches!(
                    self.mir.types.get(*item),
                    Some(
                        Type::Int
                            | Type::Float
                            | Type::Unknown
                            | Type::Union(_)
                            | Type::TypeParam { .. }
                    )
                )
            }
            _ => false,
        }
    }

    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    /// Converts an optional Python-style slice start to a clamped Rust `usize` expression.
    pub(super) fn slice_start_text(
        &self,
        maybe_start: Option<&Operand>,
        len_expr: &str,
    ) -> Result<String, EmitError> {
        maybe_start.map_or_else(
            || Ok("0usize".to_owned()),
            |bound| self.normalized_slice_bound_text_with_default(bound, len_expr, "0.0"),
        )
    }

    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    pub(super) fn slice_len_text(
        &self,
        receiver_text: &str,
        maybe_start: Option<&Operand>,
        maybe_end: Option<&Operand>,
        kind: SliceLenKind,
    ) -> Result<String, EmitError> {
        let len_source = match kind {
            SliceLenKind::Len => format!("{receiver_text}.len()"),
            SliceLenKind::Chars => format!("{receiver_text}.chars().count()"),
        };
        let start_text = self.slice_start_text(maybe_start, &len_source)?;
        let end_text = match maybe_end {
            Some(bound) => self.normalized_slice_bound_text_with_default(
                bound,
                &len_source,
                &format!("{len_source} as f64"),
            )?,
            None => len_source,
        };
        Ok(format!("{end_text}.saturating_sub({start_text})"))
    }

    /// Converts a possibly optional slice bound to a clamped Rust `usize` expression.
    fn normalized_slice_bound_text_with_default(
        &self,
        bound: &Operand,
        len_expr: &str,
        default_text: &str,
    ) -> Result<String, EmitError> {
        let bound_text = self.operand_text(bound)?;
        let bound_ty = self.operand_ty(bound)?;
        let index_text = match self.mir.types.get(bound_ty) {
            Some(&Type::Optional(item)) => match self.mir.types.get(item) {
                Some(Type::Int | Type::Float) => format!("{bound_text}.unwrap_or({default_text})"),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                    // A concrete-union `Option` payload projects each present
                    // value back to `SmeltUnknown` before the erased number match.
                    let unwrapped = if self.concrete_union_members(item).is_some() {
                        format!("{bound_text}.map(|value| value.into_smelt_unknown()).unwrap_or(SmeltUnknown::Null)")
                    } else {
                        format!("{bound_text}.unwrap_or(SmeltUnknown::Null)")
                    };
                    format!(
                        "match {unwrapped} {{ SmeltUnknown::Number(value) => value, _ => {default_text} }}"
                    )
                }
                _ => bound_text,
            },
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let scrutinee = self.erase_concrete_union_text(&bound_text, bound_ty);
                format!(
                    "match {scrutinee} {{ SmeltUnknown::Number(value) => value, _ => {default_text} }}"
                )
            }
            _ => bound_text,
        };
        Ok(format!(
            "{{ let len = {len_expr} as i64; let index = {index_text} as i64; if index < 0 {{ (len + index).clamp(0, len) as usize }} else {{ index.clamp(0, len) as usize }} }}"
        ))
    }

    // Tuple helpers continue in `tuple.rs`.
}
