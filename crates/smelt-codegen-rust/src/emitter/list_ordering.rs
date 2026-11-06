//! List ordering and slicing emission helpers.

use crate::rust::RustExpr;
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
    pub(super) fn list_sorted_expr(
        &self,
        list: &Operand,
        key: Option<&Operand>,
        reverse: bool,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        let list_text = self.operand_borrow_expr(list)?;
        if key.is_some() || reverse {
            let (prefix, closure) = self.list_sort_by_text(key, reverse, element_ty)?;
            return Ok(RustExpr::atom(format!(
                "{{ {prefix}let mut sorted = {list_text}.clone(); sorted.sort_by({closure}); sorted }}"
            )));
        }
        match self.mir.types.get(element_ty) {
            Some(Type::Bool | Type::Int | Type::String) => Ok(RustExpr::atom(format!(
                "{{ let mut sorted = {list_text}.clone(); sorted.sort(); sorted }}"
            ))),
            Some(Type::Float) => Ok(RustExpr::atom(format!(
                "{{ let mut sorted = {list_text}.clone(); sorted.sort_by(|left, right| left.partial_cmp(right).expect(\"sorted incomparable float\")); sorted }}"
            ))),
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
            let left_arg = self.rendered_at_type(&RustExpr::chain("left.clone()").ending_in_clone(), element_ty, param_ty, &self.render_scope())?;
            let right_arg = self.rendered_at_type(&RustExpr::chain("right.clone()").ending_in_clone(), element_ty, param_ty, &self.render_scope())?;
            let closure_expr = match self.closure_operand_expr_for_declared_type(key_operand) {
                Ok(closure_expr) => closure_expr,
                Err(_) => RustExpr::from(self.operand_expr(key_operand)?.into_string()),
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
            let prefix = format!("let mut smelt_sort_key = {closure_expr}; ");
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
    pub(super) fn list_reversed_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        if !matches!(self.mir.types.get(list_ty), Some(Type::List(_))) {
            return Err(EmitError::new("reversed() argument must be a list"));
        }
        if dest_ty != list_ty {
            return Err(EmitError::new(
                "reversed() destination must match the input list type",
            ));
        }
        Ok({ let receiver = list_read_expr(&self.operand_expr(list)?); RustExpr::atom_on(&receiver, format!("{receiver}.iter().rev().cloned().collect::<Vec<_>>()")) })
    }

    /// Converts list enumeration materialization to Rust text.
    ///
    /// TypeScript can narrow an erased generic through `Array.isArray` before
    /// calling `entries()`, while MIR retains its erased receiver type. The
    /// dynamic branch enumerates that array variant without library-specific
    /// lowering.
    pub(super) fn list_enumerate_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
            let item_text = self.extract_rendered(&RustExpr::path("item"), *value_ty, &self.render_scope())?;
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "match {}.clone() {{ SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(idx, item)| (idx as i64, {item_text})).collect::<Vec<_>>(), _ => Vec::new() }}",
                self.operand_expr(list)?
            ))));
        }
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("enumerate() argument must be a list"));
        };
        if *value_ty != *item_ty {
            return Err(EmitError::new(
                "enumerate() destination must contain (int, item) tuples",
            ));
        }
        Ok({ let receiver = list_read_expr(&self.operand_expr(list)?); RustExpr::atom_on(&receiver, format!("{receiver}.iter().cloned().enumerate().map(|(idx, item)| (idx as i64, item)).collect::<Vec<_>>()")) })
    }

    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    /// Converts Python `zip(left, right)` materialization to Rust text.
    pub(super) fn list_zip_expr(
        &self,
        left: &Operand,
        right: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        Ok({ let receiver = list_read_expr(&self.operand_expr(left)?); RustExpr::atom_on(&receiver, format!("{receiver}.iter().cloned().zip({}.iter().cloned()).collect::<Vec<_>>()", list_read_expr(&self.operand_expr(right)?))) })
    }

    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    /// Converts a Python `range(...)` materialization to Rust text.
    pub(super) fn list_range_expr(
        &self,
        start: &Operand,
        end: &Operand,
        step: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        Ok(RustExpr::atom(format!(
            "{{ let start = {}; let end = {}; let step = {}; if step == 0 {{ panic!(\"range() arg 3 must not be zero\"); }} let mut values = Vec::new(); let mut current = start; if step > 0 {{ while current < end {{ values.push(current); current += step; }} }} else {{ while current > end {{ values.push(current); current += step; }} }} values }}",
            self.operand_expr(start)?,
            self.operand_expr(end)?,
            self.operand_expr(step)?
        )))
    }

    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    /// Converts a Python `random.choice(list)` operation to Rust text.
    pub(super) fn list_random_choice_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        let list_text = self.operand_borrow_expr(list)?;
        Ok(RustExpr::atom(format!(
            "{{ let choice_items = &{list_text}; if choice_items.is_empty() {{ panic!(\"random.choice() from empty list\"); }} choice_items[rand::random_range(0..choice_items.len())].clone() }}"
        )))
    }

    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    /// Converts a list index operation to Rust text.
    pub(super) fn list_index_expr(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        // Read by borrow: every use below is an `.iter()` receiver, reading the
        // list's shared backing buffer.
        let list_text = list_read_expr(&self.operand_borrow_expr(list)?);
        let item_text = self.operand_expr(item)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(RustExpr::atom(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().position(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).expect(\"list index missing item\") as i64 }}"
                )));
            }
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().position(|item| item.same_js_key(&smelt_needle)).expect(\"list index missing item\") as i64 }}"
            ))));
        }
        Ok(RustExpr::opaque(format!(
            "{list_text}.iter().position(|item| item == &{item_text}).expect(\"list index missing item\") as i64"
        )))
    }

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
    pub(super) fn slice_start_expr(
        &self,
        maybe_start: Option<&Operand>,
        len_expr: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        maybe_start.map_or_else(
            || Ok(RustExpr::chain("0usize")),
            |bound| self.normalized_slice_bound_expr_with_default(bound, len_expr, &RustExpr::path("0.0")),
        )
    }

    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    /// Converts optional Python-style slice bounds to a Rust `take` length expression.
    pub(super) fn slice_len_expr(
        &self,
        receiver_text: &RustExpr,
        maybe_start: Option<&Operand>,
        maybe_end: Option<&Operand>,
        kind: SliceLenKind,
    ) -> Result<RustExpr, EmitError> {
        let len_source = match kind {
            SliceLenKind::Len => RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.len()")),
            SliceLenKind::Chars => RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.chars().count()")),
        };
        let start_text = self.slice_start_expr(maybe_start, &len_source)?;
        let end_text = match maybe_end {
            Some(bound) => self.normalized_slice_bound_expr_with_default(
                bound,
                &len_source,
                &RustExpr::opaque(format!("{len_source} as f64")),
            )?,
            None => len_source,
        };
        Ok(RustExpr::postfix_on(&end_text, format!("{end_text}.saturating_sub({start_text})")))
    }

    /// Converts a possibly optional slice bound to a clamped Rust `usize` expression.
    fn normalized_slice_bound_expr_with_default(
        &self,
        bound: &Operand,
        len_expr: &RustExpr,
        default_text: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        let bound_text = self.operand_expr(bound)?;
        let bound_ty = self.operand_ty(bound)?;
        let index_text = match self.mir.types.get(bound_ty) {
            Some(&Type::Optional(item)) => match self.mir.types.get(item) {
                Some(Type::Int | Type::Float) => format!("{bound_text}.unwrap_or({default_text})"),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                    // A concrete-union `Option` payload projects each present
                    // value back to `SmeltUnknown` before the erased number match.
                    let unwrapped = if self.concrete_union_members(item).is_some() {
                        self.carrier_format(format_args!("{bound_text}.map(|value| value.into_smelt_unknown()).unwrap_or(SmeltUnknown::Null)"))
                    } else {
                        self.carrier_format(format_args!("{bound_text}.unwrap_or(SmeltUnknown::Null)"))
                    };
                    self.carrier_format(format_args!(
                        "match {unwrapped} {{ SmeltUnknown::Number(value) => value, _ => {default_text} }}"
                    ))
                }
                _ => bound_text.into_string(),
            },
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                let scrutinee = self.erase_concrete_union_expr(&bound_text, bound_ty);
                self.carrier_format(format_args!(
                    "match {scrutinee} {{ SmeltUnknown::Number(value) => value, _ => {default_text} }}"
                ))
            }
            _ => bound_text.into_string(),
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_len = {len_expr} as i64; let smelt_index = {index_text} as i64; if smelt_index < 0 {{ (smelt_len + smelt_index).clamp(0, smelt_len) as usize }} else {{ smelt_index.clamp(0, smelt_len) as usize }} }}"
        )))
    }

    // Tuple helpers continue in `tuple.rs`.
}
