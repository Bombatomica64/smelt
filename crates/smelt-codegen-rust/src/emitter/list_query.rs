//! List query and fold emission helpers.

use super::*;
use smelt_hir::FunctionType;

/// Shared Rust fragments for closure-backed list callback iteration.
struct ListCallbackIterationParts {
    /// Statements that must run before iterating the callback receiver.
    prefix: String,
    /// Rust expression used as the iterable callback receiver.
    iter_text: String,
    /// Rendered callback arguments in JavaScript callback ABI order.
    call_args: Vec<String>,
}

impl FunctionEmitter<'_> {
    /// Converts a list search operation to Rust text.
    ///
    /// Handles `Array.prototype.indexOf`/`lastIndexOf`, including the optional
    /// JavaScript `fromIndex` argument. The emitted code searches every element
    /// but keeps the absolute element index so that a restricting window applied
    /// through `from_index` still reports the original position. JavaScript
    /// truncates `fromIndex` toward zero (`as i64`), lets negative values count
    /// back from the end, and treats it as the *first* searched index for
    /// `indexOf` and the *highest* searched index for `lastIndexOf`.
    pub(super) fn list_search_text(
        &self,
        op: smelt_hir::ListSearchOp,
        list: &Operand,
        item: &Operand,
        from_index: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list search receiver must be a list"));
        };
        let element_ty = *item_ty;
        let item_text = if self.operand_ty(item)? == element_ty {
            self.operand_text(item)?
        } else {
            self.value_at_type_text(&self.operand_text(item)?, self.operand_ty(item)?, element_ty)?
        };
        let owned_list_text = self.operand_text(list)?;
        let borrowed_list_text = match list {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place)?,
            Operand::Const(_) => owned_list_text,
        };
        let method_name = match op {
            smelt_hir::ListSearchOp::Find => "position",
            smelt_hir::ListSearchOp::RFind => "rposition",
        };
        // Without a `fromIndex` this reduces to a direct `position`/`rposition`
        // scan over the borrowed list. Element comparison mirrors JavaScript
        // `indexOf`'s SameValueZero semantics (so `NaN` matches `NaN` and
        // `+0`/`-0` are equal) for value-typed elements, and falls back to
        // structural equality for reference-typed elements.
        let Some(from_index_operand) = from_index else {
            if self.list_item_uses_same_value_zero(element_ty) {
                if self.mir.types.get(element_ty) == Some(&Type::Float) {
                    return Ok(format!(
                        "{{ let smelt_needle = {item_text}; {borrowed_list_text}.iter().{method_name}(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).map_or(-1.0, |idx| idx as f64) }}"
                    ));
                }
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; {borrowed_list_text}.iter().{method_name}(|item| item.same_js_key(&smelt_needle)).map_or(-1.0, |idx| idx as f64) }}"
                ));
            }
            return Ok(format!(
                "{borrowed_list_text}.iter().{method_name}(|item| item == &{item_text}).map_or(-1.0, |idx| idx as f64)"
            ));
        };
        // The `fromIndex` form keeps absolute element indexes while restricting
        // the searched window, so it enumerates and applies the same match
        // predicate bound to a single `item: &T` reference. The enumerate
        // iterator yields `(usize, &T)`, so `find` binds `item: &&T` and the
        // predicate is applied to `*item` (`&T`).
        let predicate = if self.list_item_uses_same_value_zero(element_ty) {
            if self.mir.types.get(element_ty) == Some(&Type::Float) {
                "|item: &f64| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())"
            } else {
                "|item: &_| item.same_js_key(&smelt_needle)"
            }
        } else {
            "|item: &_| *item == smelt_needle"
        };
        let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
        // Normalize `fromIndex`: truncate toward zero, then translate a negative
        // value into an offset from the end. `indexOf` clamps the start into
        // `[0, len]`; `lastIndexOf` clamps the inclusive end into `[-1, len - 1]`,
        // where a fully-out-of-range negative end means "search nothing". The
        // enumerate iterator yields `(usize, &T)`, so `find` binds `item: &&T`
        // and the predicate is applied to `*item` (`&T`).
        match op {
            smelt_hir::ListSearchOp::Find => Ok(format!(
                "{{ let smelt_needle = {item_text}; let smelt_predicate = {predicate}; let smelt_list = &{borrowed_list_text}; let smelt_len = smelt_list.len() as i64; let smelt_raw = {index_text} as i64; let smelt_start = if smelt_raw < 0 {{ (smelt_len + smelt_raw).max(0) }} else {{ smelt_raw }} as usize; smelt_list.iter().enumerate().skip(smelt_start).find(|(_, item)| smelt_predicate(*item)).map_or(-1.0, |(idx, _)| idx as f64) }}"
            )),
            smelt_hir::ListSearchOp::RFind => Ok(format!(
                "{{ let smelt_needle = {item_text}; let smelt_predicate = {predicate}; let smelt_list = &{borrowed_list_text}; let smelt_len = smelt_list.len() as i64; let smelt_raw = {index_text} as i64; let smelt_end = if smelt_raw < 0 {{ smelt_len + smelt_raw }} else {{ smelt_raw.min(smelt_len - 1) }}; if smelt_end < 0 {{ -1.0 }} else {{ let smelt_take = (smelt_end as usize).saturating_add(1).min(smelt_list.len()); smelt_list.iter().enumerate().take(smelt_take).rev().find(|(_, item)| smelt_predicate(*item)).map_or(-1.0, |(idx, _)| idx as f64) }} }}"
            )),
        }
    }

    /// Converts a closure-backed callback list operation to Rust iterator text.
    pub(super) fn list_callback_text(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Ok("Default::default()".to_owned());
        };
        let element_ty = *list_element_ty;
        self.list_callback_closure_text(op, list, list_ty, element_ty, callback, dest_ty)
    }

    /// Emits a list callback operation through a normal MIR closure body.
    fn list_callback_closure_text(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match op {
            smelt_hir::ListCallbackOp::Map => {
                return self.list_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
            }
            smelt_hir::ListCallbackOp::ForEach => {
                return self
                    .list_for_each_closure_text(list, list_ty, element_ty, callback, dest_ty);
            }
            smelt_hir::ListCallbackOp::FlatMap => {
                return self
                    .list_flat_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
            }
            _ => {}
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        // JavaScript array predicates (`filter`/`find`/`findIndex`/`some`/`every`)
        // coerce the callback result with the usual truthiness rules rather than
        // requiring a `boolean`. When the declared return type is not already
        // `bool` (e.g. an erased `unknown` predicate, `x => x.length`, or
        // `x => x && cond`), route the single call through `value_truthy_text` so
        // the predicate observes the same truthiness the source does; a `bool`
        // return passes through unchanged.
        let raw_call_text = format!("(smelt_callback)({})", call_args.join(", "));
        let call_text = if self.mir.types.get(function_ty.return_ty) == Some(&Type::Bool) {
            raw_call_text
        } else {
            self.value_truthy_text(&raw_call_text, function_ty.return_ty)?
        };
        let prefix = format!(
            "let mut smelt_callback = {closure_text}; {prefix}",
            prefix = list_iteration.prefix
        );
        let iter_text = list_iteration.iter_text;
        match op {
            smelt_hir::ListCallbackOp::Filter => {
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array filter destination must match the receiver list type",
                    ));
                }
                Ok(format!(
                    "{{ {prefix}{iter_text}.iter().enumerate().filter_map(|(index, item)| if {call_text} {{ Some(item.clone()) }} else {{ None }}).collect::<Vec<_>>() }}"
                ))
            }
            smelt_hir::ListCallbackOp::Find | smelt_hir::ListCallbackOp::FindLast => {
                if self.mir.types.get(dest_ty) != Some(&Type::Optional(element_ty)) {
                    return Err(EmitError::new(
                        "array find destination must be optional element type",
                    ));
                }
                let direction = if matches!(op, smelt_hir::ListCallbackOp::FindLast) {
                    ".rev()"
                } else {
                    ""
                };
                Ok(format!(
                    "{{ {prefix}{iter_text}.iter().enumerate(){direction}.find_map(|(index, item)| if {call_text} {{ Some(item.clone()) }} else {{ None }}) }}"
                ))
            }
            smelt_hir::ListCallbackOp::FindIndex | smelt_hir::ListCallbackOp::FindLastIndex => {
                if self.mir.types.get(dest_ty) != Some(&Type::Float) {
                    return Err(EmitError::new(
                        "array findIndex destination must be a number",
                    ));
                }
                let direction = if matches!(op, smelt_hir::ListCallbackOp::FindLastIndex) {
                    ".rev()"
                } else {
                    ""
                };
                Ok(format!(
                    "{{ {prefix}{iter_text}.iter().enumerate(){direction}.find_map(|(index, item)| if {call_text} {{ Some(index as f64) }} else {{ None }}).unwrap_or(-1.0) }}"
                ))
            }
            smelt_hir::ListCallbackOp::Some | smelt_hir::ListCallbackOp::Every => {
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new(
                        "array predicate destination must be boolean",
                    ));
                }
                let method = if matches!(op, smelt_hir::ListCallbackOp::Some) {
                    "any"
                } else {
                    "all"
                };
                Ok(format!(
                    "{{ {prefix}{iter_text}.iter().enumerate().{method}(|(index, item)| {call_text}) }}"
                ))
            }
            smelt_hir::ListCallbackOp::Map
            | smelt_hir::ListCallbackOp::ForEach
            | smelt_hir::ListCallbackOp::FlatMap => Err(EmitError::new(
                "list callback operation should have been handled before predicate emission",
            )),
        }
    }

    /// Emits `Array.map` for full MIR closures that cannot use callback trees.
    fn list_map_closure_text(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array map destination must be a list"));
        };
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        let callback_call_text = format!("(smelt_callback)({})", call_args.join(", "));
        let call_text = if function_ty.may_throw {
            format!(
                "({callback_call_text}).unwrap_or_else(|error: Box<dyn std::error::Error>| panic!(\"{{}}\", error))"
            )
        } else {
            callback_call_text
        };
        let value_text =
            self.value_at_type_text(&call_text, function_ty.return_ty, *dest_item_ty)?;
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; {}{}.iter().enumerate().map(|(index, item)| {{ {value_text} }}).collect::<Vec<_>>() }}",
            list_iteration.prefix, list_iteration.iter_text
        ))
    }

    /// Emits `Array.forEach` for function callbacks without inline callback trees.
    fn list_for_each_closure_text(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if dest_ty != self.none_ty {
            return Err(EmitError::new("array forEach destination must be none"));
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; {}{}.iter().enumerate().for_each(|(index, item)| {{ let _ = {call_text}; }}); () }}",
            list_iteration.prefix, list_iteration.iter_text
        ))
    }

    /// Emits `Array.flatMap` for function callbacks without inline callback trees.
    fn list_flat_map_closure_text(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array flatMap destination must be a list"));
        };
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        let flattened_text = match self.mir.types.get(function_ty.return_ty) {
            Some(Type::List(callback_item_ty)) => {
                let value_text =
                    self.value_at_type_text("value", *callback_item_ty, *dest_item_ty)?;
                format!("smelt_result.into_iter().map(|value| {value_text}).collect::<Vec<_>>()")
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                let unknown_ty = self.type_id(Type::Unknown)?;
                let value_text = self.value_at_type_text("value", unknown_ty, *dest_item_ty)?;
                format!(
                    "match smelt_result {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| {value_text}).collect::<Vec<_>>(), value => vec![{value_text}] }}"
                )
            }
            _ => {
                let value_text =
                    self.value_at_type_text("smelt_result", function_ty.return_ty, *dest_item_ty)?;
                format!("vec![{value_text}]")
            }
        };
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; {}{}.iter().enumerate().flat_map(|(index, item)| {{ let smelt_result = {call_text}; {flattened_text} }}).collect::<Vec<_>>() }}",
            list_iteration.prefix, list_iteration.iter_text
        ))
    }

    /// Build shared list-iteration text for closure-backed array callbacks.
    ///
    /// Most callbacks only consume `(item)` or `(item, index)`, so the emitter
    /// can borrow the original receiver and clone each item as it is passed.
    /// JavaScript's optional third callback argument observes the whole source
    /// array; only that ABI needs a stable cloned snapshot named
    /// `smelt_array`.
    fn list_callback_iteration_parts(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        _callback: &Operand,
        function_ty: &FunctionType,
    ) -> Result<ListCallbackIterationParts, EmitError> {
        let owned_list_text = self.operand_text(list)?;
        let borrowed_list_text = match list {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place)?,
            Operand::Const(_) => owned_list_text.clone(),
        };
        // A zero-parameter callback (`values.map(stubTrue)`) ignores every
        // supplied argument, so it is called with no arguments at all.
        let mut call_args = Vec::new();
        if let Some(item_param_ty) = function_ty.params.first().copied() {
            call_args.push(self.value_at_type_text("item.clone()", element_ty, item_param_ty)?);
        }
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            let index_source_ty = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                self.type_id(Type::Int)?
            } else {
                self.type_id(Type::Float)?
            };
            let index_value = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                "index as i64"
            } else {
                "index as f64"
            };
            call_args.push(self.value_at_type_text(
                index_value,
                index_source_ty,
                index_param_ty,
            )?);
        }
        let needs_array_snapshot = function_ty.params.get(2).is_some();
        let (prefix, iter_text) = if needs_array_snapshot {
            let array_param_ty = function_ty.params.get(2).copied().ok_or_else(|| {
                EmitError::new("array callback snapshot requires an array parameter")
            })?;
            call_args.push(self.value_at_type_text(
                "smelt_array.clone()",
                list_ty,
                array_param_ty,
            )?);
            (
                format!("let smelt_array = {owned_list_text}; "),
                "smelt_array".to_owned(),
            )
        } else {
            (String::new(), borrowed_list_text)
        };
        Ok(ListCallbackIterationParts {
            prefix,
            iter_text,
            call_args,
        })
    }

    /// Converts `Array.from({ length }, mapper)` into an indexed Rust loop.
    pub(super) fn list_from_length_map_text(
        &self,
        length: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_text(length)?;
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        if self.mir.types.get(dest_ty) != Some(&Type::List(function_ty.return_ty)) {
            return Err(EmitError::new(
                "Array.from mapper destination must be a list of callback results",
            ));
        }
        let closure_text = self.closure_operand_text_for_declared_type(callback)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let float_ty = self.type_id(Type::Float)?;
        let mut call_args = Vec::new();
        if let Some(item_param_ty) = function_ty.params.first().copied() {
            call_args.push(self.value_at_type_text(
                "SmeltUnknown::Null",
                unknown_ty,
                item_param_ty,
            )?);
        }
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            call_args.push(self.value_at_type_text("index as f64", float_ty, index_param_ty)?);
        }
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..array_from_length).map(|index| {call_text}).collect::<Vec<_>>() }}"
        ))
    }

    /// Converts `Array.from({ length })` into a sparse-like unknown vector.
    pub(super) fn list_from_length_text(
        &self,
        length: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if self.mir.types.get(dest_ty) != Some(&Type::List(self.type_id(Type::Unknown)?)) {
            return Err(EmitError::new(
                "Array.from length destination must be list[unknown]",
            ));
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_text(length)?;
        Ok(format!(
            "{{ let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; vec![SmeltUnknown::Null; array_from_length] }}"
        ))
    }

    /// Converts a sized repeated list expression into Rust vector construction.
    pub(super) fn list_repeat_text(
        &self,
        value: &Operand,
        count: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::List(item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("list repeat destination must be a list"));
        };
        if self.operand_ty(value)? != *item_ty {
            return Err(EmitError::new(
                "list repeat value must match the list item type",
            ));
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(count)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("list repeat count must be numeric"));
        }
        let value_text = self.operand_text(value)?;
        let count_text = self.operand_text(count)?;
        Ok(format!(
            "{{ let smelt_repeat_count = ({count_text} as f64).max(0.0).floor() as usize; vec![{value_text}; smelt_repeat_count] }}"
        ))
    }

    /// Converts an array reduce callback into Rust `fold` text.
    ///
    /// The emitted `fold` threads the accumulator through as `acc: dest_ty` and
    /// invokes the callback with only as many of `(acc, item, index, array)` as
    /// the callback declares — a JS `reduce` callback commonly takes fewer than
    /// the four arguments the runtime supplies (e.g. a named `step(acc, x)` or an
    /// arrow `(acc, value) => …`). Each supplied argument is coerced to the
    /// callback parameter's type, and the callback result is coerced back to the
    /// accumulator type `dest_ty` through the shared coercion seam, so a callback
    /// whose declared return type merely reconciles with (rather than equals) the
    /// accumulator — a concrete-union member, an optional/`unknown` widening —
    /// still folds into a value the next step accepts (issue #113). The frontend
    /// (`lower_list_reduce`) picks `dest_ty` so this reconciliation is statically
    /// valid; here we only render the arity- and type-matched call.
    pub(super) fn list_reduce_text(
        &self,
        list: &Operand,
        initial: Option<&Operand>,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array reduce receiver must be a list"));
        };
        let element_ty = *list_element_ty;
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Err(EmitError::new("array reduce callback must be a function"));
        };
        let callback_return_ty = function_ty.return_ty;
        // The four values the JS runtime would pass, paired with the source type
        // of the Rust expression that produces each. Only the leading prefix the
        // callback actually declares is forwarded (and coerced) below.
        let float_ty = self.type_id(Type::Float)?;
        // `index` here names the fold body's `let index = index as f64;` binding
        // (see the `fold` closures below), so it is already `f64`. Passing the
        // bare local — rather than a pre-cast `index as f64` string — lets the
        // shared coercion seam emit a single, minimal cast to the callback's
        // declared parameter type instead of stacking a redundant `as f64`
        // (e.g. `(index.trunc() as i64)` for an `i64` param, `index` for `f64`).
        let candidate_args: [(&str, TypeId); 4] = [
            ("acc", dest_ty),
            ("item", element_ty),
            ("index", float_ty),
            ("array", list_ty),
        ];
        let mut call_args = Vec::with_capacity(function_ty.params.len());
        for (index, param_ty) in function_ty.params.iter().copied().enumerate() {
            let Some((value_text, source_ty)) = candidate_args.get(index).copied() else {
                return Err(EmitError::new(
                    "array reduce callback declares more parameters than reduce supplies",
                ));
            };
            call_args.push(self.value_at_type_text(value_text, source_ty, param_ty)?);
        }
        if let Some(initial_operand) = initial
            && self.operand_ty(initial_operand)? != dest_ty
        {
            return Err(EmitError::new(
                "array reduce initial value and callback result must match the destination type",
            ));
        }
        let owned_list_text = self.operand_text(list)?;
        let borrowed_list_text = match list {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place)?,
            Operand::Const(_) => owned_list_text.clone(),
        };
        let callback_closure = match self.closure_operand_text_for_declared_type(callback) {
            Ok(callback_closure) => callback_closure,
            // A reduce callback passed as a borrowed function parameter (rather
            // than a local closure) renders as the parameter name itself, which
            // calls through the `&dyn Fn`. Fall back to it instead of dropping
            // the reduce body to a default value.
            Err(_) => self.operand_text(callback)?,
        };
        // Coerce the callback result to the accumulator type so a reconcilable
        // (but not identical) return type still produces the next `acc` value.
        let call_expr = format!("(smelt_callback)({})", call_args.join(", "));
        let callback_result_text =
            self.value_at_type_text(&call_expr, callback_return_ty, dest_ty)?;
        let callback_text =
            format!("{{ let smelt_callback = {callback_closure}; {callback_result_text} }}");
        if let Some(initial_operand) = initial {
            let initial_text = self.operand_text(initial_operand)?;
            Ok(format!(
                "{borrowed_list_text}.iter().enumerate().fold({initial_text}, |acc, (index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {owned_list_text}; {callback_text} }})"
            ))
        } else if dest_ty == element_ty {
            Ok(format!(
                "{{ let mut reduce_items = {borrowed_list_text}.iter().enumerate(); let (_, first) = reduce_items.next().expect(\"reduce of empty array with no initial value\"); reduce_items.fold(first.clone(), |acc, (index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {owned_list_text}; {callback_text} }}) }}"
            ))
        } else {
            Err(EmitError::new(
                "array reduce without an initial value must produce the element type",
            ))
        }
    }

    /// Converts a non-escaping MIR closure into a Rust closure literal.
    pub(super) fn closure_text(&self, id: smelt_mir::ClosureId) -> Result<String, EmitError> {
        self.closure_text_with_extra_params(id, &[], None)
    }

    /// Converts a MIR closure into a Rust closure literal shaped for `dest_ty`.
    pub(super) fn closure_text_for_type(
        &self,
        id: smelt_mir::ClosureId,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        // A JS callback often accepts fewer arguments than the contextual
        // function type provides (e.g. `.map(value => ...)` is called with
        // `(value, index, array)`). The emitted closure appends one ignored
        // parameter per surplus contextual argument; capture the *types* of
        // those surplus parameters so the emitted `_arg{i}` can be annotated
        // (an unannotated ignored param is uninferable — E0282 — when the
        // closure is stored behind an `Rc`/`dyn Fn` that erases its signature).
        let extra_param_tys: Vec<TypeId> = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => {
                let closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                function
                    .params
                    .iter()
                    .skip(closure.params.len())
                    .copied()
                    .collect()
            }
            _ => Vec::new(),
        };
        let has_captures = !self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
            .captures
            .is_empty();
        let captures_borrowed_callback = self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
            .captures
            .iter()
            .any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            });
        if captures_borrowed_callback
            && self
                .mir
                .closures
                .get(id_index(id.0, "closure id does not fit usize")?)
                .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
                .escapes
            && matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
        {
            return self.default_value(dest_ty);
        }
        let target_return_ty = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => Some(function.return_ty),
            _ => None,
        };
        let closure =
            self.closure_text_with_extra_params(id, &extra_param_tys, target_return_ty)?;
        if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
            let adjusted_closure = if !has_captures
                || closure.starts_with("move ")
                || closure.starts_with('{')
            {
                closure
            } else {
                let source_closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                let mut cloned_captures = HashSet::new();
                let mut shared_replacements = Vec::new();
                let capture_prelude = source_closure
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let local = self.local_decl(capture.source_local).ok()?;
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                            && matches!(local.kind, LocalKind::Param { .. })
                        {
                            return None;
                        }
                        if !matches!(local.kind, LocalKind::Param { .. })
                            && !self
                                .declared_locals
                                .borrow()
                                .contains(&capture.source_local)
                        {
                            return None;
                        }
                        let name = self.local_name(capture.source_local).ok()?.to_owned();
                        if name.starts_with("(*smelt_capture_") {
                            return None;
                        }
                        cloned_captures.insert(name.clone()).then(|| {
                            if self.closure_capture_needs_shared_access(source_closure, capture)
                                || self.local_uses_shared_capture_storage(capture.source_local)
                            {
                                shared_replacements.push((
                                    name.clone(),
                                    format!("(*smelt_capture_{name}.borrow_mut())"),
                                ));
                                return format!(
                                    "let smelt_capture_{name} = smelt_capture_{name}.clone();"
                                );
                            }
                            let mutability = if self.mutable_locals.contains(&capture.source_local)
                                || source_closure
                                    .captures
                                    .iter()
                                    .find(|candidate| {
                                        candidate.source_local == capture.source_local
                                    })
                                    .is_some_and(|candidate| {
                                        self.closure_capture_body_writes(source_closure, candidate)
                                    })
                                || matches!(
                                    self.mir.types.get(local.ty),
                                    Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _))
                                ) {
                                "mut "
                            } else {
                                ""
                            };
                            format!("let {mutability}{name} = {name}.clone();")
                        })
                    })
                    .collect::<Vec<_>>();
                let adjusted_closure = replace_shared_capture_uses(closure, &shared_replacements);
                if capture_prelude.is_empty() {
                    format!("move {adjusted_closure}")
                } else {
                    format!(
                        "{{\n    {}\n    move {adjusted_closure}\n}}",
                        capture_prelude.join("\n    ")
                    )
                }
            };
            let callback_text = format!("::std::rc::Rc::new({adjusted_closure})");
            if let Some(Type::Function(function)) = self.mir.types.get(dest_ty)
                && self.is_erased_unknown_rest_function(function)
                && !function.may_throw
            {
                let source_closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                let params = source_closure
                    .params
                    .iter()
                    .map(|param| {
                        let local_index =
                            id_index(param.0, "closure param index does not fit usize")?;
                        source_closure
                            .locals
                            .get(local_index)
                            .map(|local| local.ty)
                            .ok_or_else(|| EmitError::new("closure param has no local declaration"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?;
                let source_function = FunctionType {
                    params,
                    rest: source_closure.rest,
                    required_params: source_closure.required_params,
                    mutable_params: Vec::new(),
                    return_ty: source_closure.return_ty,
                    is_async: false,
                    may_throw: source_closure.can_throw,
                };
                let args = self.function_args_from_smelt_args_text(&source_function)?;
                let call = if source_closure.can_throw {
                    format!(
                        "(smelt_callback)({args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                    )
                } else {
                    format!("(smelt_callback)({args})")
                };
                let unknown_ty = self.type_id(Type::Unknown)?;
                let return_text =
                    if self.mir.types.get(source_closure.return_ty) == Some(&Type::None) {
                        format!("{{ {call}; SmeltUnknown::Null }}")
                    } else {
                        self.value_at_type_text(&call, source_closure.return_ty, unknown_ty)?
                    };
                let length = source_closure
                    .required_params
                    .unwrap_or_else(|| source_closure.rest.unwrap_or(source_closure.params.len()));
                let erased_function = format!(
                    "SmeltErasedFunction {{ callback: {{ let smelt_callback = ::std::rc::Rc::new({adjusted_closure}); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}) }}, length: {length}.0, object: None }}"
                );
                // A bare function-item-as-value wrapper carries a stable item
                // key. Building this `SmeltErasedFunction` inline would mint a
                // fresh callback `Rc` on every call, so two calls of the same
                // named function constant (e.g. `doNothing()`) would never be
                // `Rc::ptr_eq`. JavaScript returns one shared singleton instead.
                // Route the build through a per-item accessor that caches ONE
                // `SmeltErasedFunction`; every call returns clones sharing one
                // inner callback `Rc`. User arrows have no key and keep the
                // inline fresh build (each arrow expression is a distinct
                // function in JavaScript).
                if let Some(key) = source_closure.function_item_key {
                    self.context
                        .function_item_erased_fn_accessors
                        .borrow_mut()
                        .entry(key)
                        .or_insert(erased_function);
                    return Ok(format!("__smelt_fn_erased_{key}()"));
                }
                return Ok(erased_function);
            }
            return Ok(callback_text);
        }
        Ok(closure)
    }

    /// Emits ignored trailing parameters when a JS callback accepts fewer
    /// arguments than the contextual function type provides.
    ///
    /// `extra_param_tys` carries the contextual types of those surplus
    /// parameters (e.g. the `index: number` and `array: T[]` a `.map` callback
    /// ignores). Each is emitted as an annotated `_arg{i}: <ty>` binding so the
    /// closure signature is fully typed even when it is stored behind an erased
    /// `Rc`/`dyn Fn` where Rust cannot otherwise infer the ignored parameters.
    fn closure_text_with_extra_params(
        &self,
        id: smelt_mir::ClosureId,
        extra_param_tys: &[TypeId],
        return_override: Option<TypeId>,
    ) -> Result<String, EmitError> {
        let closure = self
            .mir
            .closures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?;
        if closure.escapes
            && closure.captures.iter().any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            })
        {
            let return_ty = return_override.unwrap_or(closure.return_ty);
            let mut param_decls = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let local_index = id_index(param.0, "closure param index does not fit usize")?;
                    let local = closure
                        .locals
                        .get(local_index)
                        .ok_or_else(|| EmitError::new("closure param has no local declaration"))?;
                    Ok(format!(
                        "arg{index}: {}",
                        self.type_text_with_impl_trait(local.ty, false)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            for (index, extra_ty) in extra_param_tys.iter().enumerate() {
                param_decls.push(format!(
                    "_arg{index}: {}",
                    self.type_text_with_impl_trait(*extra_ty, false)?
                ));
            }
            return Ok(format!(
                "|{}| -> {} {{ {} }}",
                param_decls.join(", "),
                self.type_text_with_impl_trait(return_ty, false)?,
                self.default_value(return_ty)?
            ));
        }
        let closure_param_names = closure
            .params
            .iter()
            .enumerate()
            .map(|(index, _)| format!("closure_arg_{index}"))
            .collect::<HashSet<_>>();
        let mut capture_aliases = HashMap::new();
        let body = {
            let mut closure_locals = closure.locals.clone();
            let fallback_span = closure_locals.first().map_or(
                Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
                |local| local.span,
            );
            for capture in &closure.captures {
                for captured_local in [Some(capture.source_local), capture.target_local]
                    .into_iter()
                    .flatten()
                {
                    let index = id_index(captured_local.0, "local index does not fit usize")?;
                    if closure_locals.len() <= index {
                        closure_locals.resize(
                            index.saturating_add(1),
                            LocalDecl {
                                ty: capture.ty,
                                kind: LocalKind::Temp,
                                span: fallback_span,
                            },
                        );
                        if let Some(local) = closure_locals.get_mut(index) {
                            local.ty = capture.ty;
                        }
                    }
                }
            }
            let function = MirFunction {
                id: FuncId(u32::MAX),
                name: Symbol(u32::MAX),
                type_params: Vec::new(),
                origin: HirOrigin::Body(smelt_hir::BodyId(u32::MAX)),
                is_async: false,
                is_test: false,
                can_throw: closure.can_throw,
                params: closure.params.clone(),
                rest: closure.rest,
                return_ty: return_override.unwrap_or(closure.return_ty),
                locals: closure_locals,
                blocks: closure.blocks.clone(),
                entry: closure.entry,
            };
            let mut emitter = FunctionEmitter::new(self.mir, self.context, &function)?;
            for (index, param) in closure.params.iter().enumerate() {
                emitter.names.insert(*param, format!("closure_arg_{index}"));
            }
            for capture in &closure.captures {
                if let Some(target) = capture.target_local {
                    let source = self.local_decl(capture.source_local)?;
                    let source_name = self.local_name(capture.source_local)?.to_owned();
                    let alias_name = if closure_param_names.contains(&source_name) {
                        capture_aliases
                            .entry(capture.source_local)
                            .or_insert_with(|| format!("smelt_captured_{source_name}"))
                            .clone()
                    } else {
                        source_name.clone()
                    };
                    if matches!(self.mir.types.get(source.ty), Some(Type::Function(_)))
                        && matches!(source.kind, LocalKind::Param { .. })
                        && !self.function_parameter_requires_owned(capture.source_local)?
                    {
                        emitter.borrowed_callback_names.insert(alias_name.clone());
                    }
                    let capture_name = if self.closure_capture_needs_shared_access(closure, capture)
                        || self.local_uses_shared_capture_storage(capture.source_local)
                    {
                        format!("(*smelt_capture_{alias_name}.borrow_mut())")
                    } else {
                        alias_name
                    };
                    emitter.names.insert(target, capture_name);
                    emitter.mark_local_declared(target);
                }
            }
            let mut params = closure
                .params
                .iter()
                .map(|param| {
                    let local = emitter.local_decl(*param)?;
                    let mutability = if emitter.local_binding_needs_mut(*param) {
                        "mut "
                    } else {
                        ""
                    };
                    Ok(format!(
                        "{mutability}{}: {}",
                        emitter.local_name(*param)?,
                        emitter.type_text_with_impl_trait(local.ty, false)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            for (index, extra_ty) in extra_param_tys.iter().enumerate() {
                params.push(format!(
                    "_arg{index}: {}",
                    emitter.type_text_with_impl_trait(*extra_ty, false)?
                ));
            }
            let params_text = params.join(", ");
            let mut body_text = String::new();
            emitter.emit_mutable_local_preludes(&mut body_text)?;
            emitter.emit_closure_block(emitter.entry_block()?, &mut body_text)?;
            let returns_future = matches!(
                emitter.mir.types.get(function.return_ty),
                Some(Type::Future(_))
            );
            // Whether this closure needs the async wrapper is decided from MIR,
            // not from scanning the emitted text: a closure is async when its
            // declared return type is a future, or when its own body performs an
            // `await` (nested Promise-continuation closures are separate MIR
            // functions and are excluded by `closure_body_awaits`).
            let awaits_inside_body = emitter.closure_body_awaits();
            if returns_future || awaits_inside_body {
                let output_ty = match emitter.mir.types.get(function.return_ty) {
                    Some(Type::Future(item)) => *item,
                    _ => function.return_ty,
                };
                let output_text = emitter.type_text_with_impl_trait(output_ty, false)?;
                let return_ty = format!("Result<{output_text}, Box<dyn std::error::Error>>");
                // Whether `smelt_async_value` is itself a future to await is read
                // from the MIR return operand types, not from the emitted text.
                let async_value_needs_await = emitter.closure_yields_future_value()?;
                let return_value = if closure.can_throw && async_value_needs_await {
                    if matches!(
                        emitter.mir.types.get(output_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || emitter.is_erased_class_type(output_ty)
                    {
                        "let smelt_async_output = smelt_async_value?.await; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_output.into_smelt_unknown())".to_owned()
                    } else {
                        format!(
                            "let smelt_async_output = smelt_async_value?.await; Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_output)"
                        )
                    }
                } else if closure.can_throw {
                    if matches!(
                        emitter.mir.types.get(output_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || emitter.is_erased_class_type(output_ty)
                    {
                        "Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_value?.into_smelt_unknown())".to_owned()
                    } else {
                        format!(
                            "Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_value?)"
                        )
                    }
                } else if matches!(
                    emitter.mir.types.get(output_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || emitter.is_erased_class_type(output_ty)
                {
                    if async_value_needs_await {
                        "let smelt_async_output = smelt_async_value.await?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_output.into_smelt_unknown())".to_owned()
                    } else {
                        "Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_value.into_smelt_unknown())".to_owned()
                    }
                } else if async_value_needs_await {
                    format!(
                        "let smelt_async_output = smelt_async_value.await?; Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_output)"
                    )
                } else {
                    format!("Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_value)")
                };
                let mut cloned_async_captures = HashSet::new();
                let async_capture_lines = closure
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let local = self.local_decl(capture.source_local).ok()?;
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                            && matches!(local.kind, LocalKind::Param { .. })
                            && !self
                                .function_parameter_requires_owned(capture.source_local)
                                .ok()?
                        {
                            return None;
                        }
                        let source_name = self.local_name(capture.source_local).ok()?.to_owned();
                        let name = capture_aliases
                            .get(&capture.source_local)
                            .cloned()
                            .unwrap_or_else(|| source_name.clone());
                        cloned_async_captures
                            .insert(name.clone())
                            .then(|| format!("let {name} = {source_name}.clone();"))
                    })
                    .collect::<Vec<_>>();
                let async_capture_prelude = if async_capture_lines.is_empty() {
                    String::new()
                } else {
                    format!("{} ", async_capture_lines.join(" "))
                };
                format!(
                    "|{params_text}| {{ {async_capture_prelude}Box::pin(async move {{\n        let smelt_async_value = {{\n{body_text}        }};\n        {return_value}\n    }}) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = {return_ty}>>> }}"
                )
            } else {
                // Shared captures are emitted as safe `Rc<RefCell<T>>` and accessed via
                // `borrow_mut()` (see core.rs), so the closure body contains no `unsafe`
                // operations. Emit the body directly; wrapping it in an `unsafe` block only
                // produced `unnecessary unsafe block` warnings in generated crates.
                format!("|{params_text}| {{\n{body_text}    }}")
            }
        };
        let capture_prefix = if closure.escapes
            || matches!(self.mir.types.get(closure.return_ty), Some(Type::Future(_)))
            || !closure.captures.is_empty()
            || closure.captures.iter().any(|capture| {
                capture.mode == smelt_hir::CaptureMode::ByValue
                    && !self
                        .capture_is_borrowed_callback_param(capture.source_local)
                        .unwrap_or(false)
                    && !self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            }) {
            "move "
        } else {
            ""
        };
        if capture_prefix.is_empty() {
            return Ok(body);
        }
        let mut cloned_captures = HashSet::new();
        let mut shared_replacements = Vec::new();
        let capture_prelude = closure
            .captures
            .iter()
            .filter_map(|capture| {
                let local = self.local_decl(capture.source_local).ok()?;
                if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                    && matches!(local.kind, LocalKind::Param { .. })
                    && !self
                        .function_parameter_requires_owned(capture.source_local)
                        .ok()?
                {
                    return None;
                }
                let source_name = self.local_name(capture.source_local).ok()?.to_owned();
                let name = capture_aliases
                    .get(&capture.source_local)
                    .cloned()
                    .unwrap_or_else(|| source_name.clone());
                if source_name.starts_with("(*smelt_capture_") {
                    return None;
                }
                cloned_captures.insert(name.clone()).then(|| {
                    if self.closure_capture_needs_shared_access(closure, capture)
                        || self.local_uses_shared_capture_storage(capture.source_local)
                    {
                        shared_replacements.push((
                            name.clone(),
                            format!("(*smelt_capture_{name}.borrow_mut())"),
                        ));
                        return format!("let smelt_capture_{name} = smelt_capture_{name}.clone();");
                    }
                    let mutability = if self.mutable_locals.contains(&capture.source_local)
                        || self.closure_capture_body_writes(closure, capture)
                        || matches!(
                            self.mir.types.get(local.ty),
                            Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _))
                        ) {
                        "mut "
                    } else {
                        ""
                    };
                    format!("let {mutability}{name} = {source_name}.clone();")
                })
            })
            .collect::<Vec<_>>();
        let adjusted_body = replace_shared_capture_uses(body, &shared_replacements);
        if capture_prelude.is_empty() {
            Ok(format!("{capture_prefix}{adjusted_body}"))
        } else {
            Ok(format!(
                "{{\n    {}\n    {capture_prefix}{adjusted_body}\n}}",
                capture_prelude.join("\n    ")
            ))
        }
    }

    /// Return whether this closure's own MIR body performs an `await`.
    ///
    /// The decision is read from MIR types rather than emitted text: a closure
    /// awaits when any of its own basic blocks contains an `Await` terminator or
    /// an `Rvalue::Await` statement. Nested closures (Promise continuations,
    /// `async move` blocks) are separate `MirFunction`s referenced through
    /// `Rvalue::Closure` and are never inlined into `self.function.blocks`, so
    /// their awaits are naturally excluded — this is the structural, formatting-
    /// independent replacement for the old brace-counting text scan.
    pub(super) fn closure_body_awaits(&self) -> bool {
        self.function.blocks.iter().any(|block| {
            let terminator_awaits = matches!(&block.terminator, Some(Terminator::Await { .. }));
            let statement_awaits = block.statements.iter().any(|statement| {
                matches!(
                    statement,
                    Statement::Assign {
                        value: Rvalue::Await(_),
                        ..
                    }
                )
            });
            terminator_awaits || statement_awaits
        })
    }

    /// Return whether the value this closure yields is itself a future.
    ///
    /// An async wrapper stores the closure body's result in `smelt_async_value`.
    /// When that value is already a future (the closure returns a `new Promise`
    /// or another future without awaiting it first) the wrapper must `.await`
    /// it before producing the resolved output. This is decided from the MIR
    /// return operand types: the closure yields a future iff every `Return`
    /// terminator carries an operand whose type is `Type::Future(_)`.
    ///
    /// Returns `false` when the closure has no `Return` terminator so the
    /// wrapper falls back to treating the body value as already-resolved, which
    /// matches the previous text-based behaviour for those degenerate shapes.
    pub(super) fn closure_yields_future_value(&self) -> Result<bool, EmitError> {
        let mut saw_return = false;
        for block in &self.function.blocks {
            if let Some(Terminator::Return(operand)) = &block.terminator {
                saw_return = true;
                if !matches!(
                    self.mir.types.get(self.operand_ty(operand)?),
                    Some(Type::Future(_))
                ) {
                    return Ok(false);
                }
            }
        }
        Ok(saw_return)
    }

    /// Emits a closure block with return terminators scoped to the closure body.
    pub(super) fn emit_closure_block(
        &self,
        block: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        self.emit_closure_block_inner(block, out, &mut Vec::new(), None)
    }

    /// Emits a closure block while guarding against unstructured CFG cycles.
    fn emit_closure_block_inner(
        &self,
        block: &BasicBlock,
        out: &mut String,
        active: &mut Vec<*const BasicBlock>,
        stop: Option<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        if Some(block.id) == stop {
            return Ok(());
        }
        let block_ptr = std::ptr::from_ref(block);
        if active.contains(&block_ptr) {
            out.push_str("    panic!(\"recursive closure control flow is not structured yet\")\n");
            return Ok(());
        }
        active.push(block_ptr);
        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("closure basic block has no terminator"));
        };
        if let Terminator::Switch {
            cond,
            then_block,
            else_block,
        } = terminator
            && self.block_can_reach(*then_block, block.id, &mut HashSet::new())
        {
            out.push_str("    loop {\n");
            for statement in &block.statements {
                self.emit_statement(statement, out)?;
            }
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_closure_block_inner(self.block(*then_block)?, out, active, Some(block.id))?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_closure_block_inner(self.block(*else_block)?, out, active, Some(block.id))?;
            out.push_str("    ;\n");
            out.push_str("    break;\n");
            out.push_str("    }\n");
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            active.pop();
            return Ok(());
        }
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        let result = match terminator {
            Terminator::Return(operand) => {
                if self.function.return_ty == self.none_ty {
                    if !matches!(operand, Operand::Const(Constant::None))
                        && self.operand_ty(operand)? != self.none_ty
                    {
                        out.push_str(&format!("    {};\n", self.operand_text(operand)?));
                    }
                    if self.function.can_throw {
                        out.push_str("    Ok::<(), Box<dyn std::error::Error>>(())\n");
                    } else {
                        out.push_str("    ()\n");
                    }
                } else {
                    // For an async closure whose declared return is `Future<inner>`
                    // the surrounding async wrapper stores this body value in
                    // `smelt_async_value` and awaits it when it is itself a future
                    // (see `closure_yields_future_value`). Deciding the tail type
                    // from MIR keeps the future flowing through: when the returned
                    // operand is already a future we yield it at the `Future` type,
                    // so a `new Promise(...)` body returns the future rather than
                    // collapsing to the inner type's default (the old null / `0.0`
                    // fallthrough that the textual promotion pass patched up after
                    // the fact). Only when the operand is a resolved value do we
                    // strip to the inner type as before.
                    let operand_is_future = matches!(
                        self.mir.types.get(self.operand_ty(operand)?),
                        Some(Type::Future(_))
                    );
                    let return_ty = match self.mir.types.get(self.function.return_ty) {
                        Some(Type::Future(item)) if !operand_is_future => *item,
                        _ => self.function.return_ty,
                    };
                    let value = self.value_at_type(operand, return_ty)?;
                    if self.function.can_throw {
                        let return_ty_text = self.type_text_with_impl_trait(return_ty, false)?;
                        out.push_str(&format!(
                            "    Ok::<{return_ty_text}, Box<dyn std::error::Error>>({value})\n"
                        ));
                    } else {
                        out.push_str(&format!("    {value}\n"));
                    }
                }
                Ok(())
            }
            Terminator::Goto(target) => {
                if Some(*target) == stop {
                    Ok(())
                } else {
                    self.emit_closure_block_inner(self.block(*target)?, out, active, stop)
                }
            }
            Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind: _,
            } => {
                let local = self.local_decl(*dest)?;
                let call_text = self.closure_call_text_for_dest(callee, args, local.ty)?;
                let name = self.local_name(*dest)?;
                out.push_str(&format!(
                    "    let {name}: {} = {call_text};\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(*dest);
                self.emit_closure_block_inner(self.block(*target)?, out, active, stop)
            }
            Terminator::Await {
                future,
                dest,
                target,
                unwind: _,
            } => {
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                let value = format!("{}.await", self.await_operand_text(future)?);
                out.push_str(&format!(
                    "    let {name}: {} = {value};\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(*dest);
                self.emit_closure_block_inner(self.block(*target)?, out, active, stop)
            }
            Terminator::Switch {
                cond,
                then_block,
                else_block,
            } => {
                let branch_declared = self.declared_locals_snapshot();
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
                self.emit_closure_block_inner(self.block(*then_block)?, out, active, stop)?;
                out.push_str("    } else {\n");
                self.restore_declared_locals(branch_declared.clone());
                self.emit_closure_block_inner(self.block(*else_block)?, out, active, stop)?;
                out.push_str("    }\n");
                self.restore_declared_locals(branch_declared);
                Ok(())
            }
            Terminator::Match {
                scrutinee,
                arms,
                default,
            } => self.emit_closure_match(scrutinee, arms, *default, out, active, stop),
            Terminator::Throw(operand) => {
                out.push_str(&format!(
                    "    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into());\n",
                    self.operand_text(operand)?
                ));
                Ok(())
            }
            Terminator::Unreachable => {
                out.push_str("    unreachable!()\n");
                Ok(())
            }
        };
        active.pop();
        result
    }

    /// Emits a MIR match inside a Rust closure body.
    ///
    /// Closure blocks can end as Rust tail expressions, unlike ordinary function
    /// blocks that usually emit explicit `return` statements. This helper keeps
    /// shared match join blocks outside the generated `match` expression and
    /// lets non-joining matches remain the closure tail expression.
    fn emit_closure_match(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        out: &mut String,
        active: &mut Vec<*const BasicBlock>,
        stop: Option<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        let join = self.match_join(arms, default)?;
        let scrutinee_text = self.match_scrutinee_text(scrutinee)?;
        out.push_str(&format!("    match {scrutinee_text} {{\n"));
        let match_declared = self.declared_locals_snapshot();
        for arm in arms {
            out.push_str(&format!(
                "        {} => {{\n",
                self.match_label_text(&arm.label)
            ));
            self.emit_closure_block_inner(self.block(arm.target)?, out, active, join)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared.clone());
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            self.emit_closure_block_inner(self.block(default_block)?, out, active, join)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared);
        } else {
            out.push_str("        _ => {}\n");
        }
        if join.is_some() {
            out.push_str("    };\n");
        } else {
            out.push_str("    }\n");
        }
        if let Some(join_block) = join {
            self.emit_closure_block_inner(self.block(join_block)?, out, active, stop)?;
        }
        Ok(())
    }

    /// Resolve a callback operand to the Rust closure expression that constructed it.
    pub(super) fn closure_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => {
                return Err(EmitError::new(
                    "list callback must be a non-escaping closure local",
                ));
            }
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == local
                {
                    return self.closure_text(*id);
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }

    /// Resolve a local to the closure local it aliases, following simple copy assignments.
    fn closure_source_local(&self, local: LocalId) -> LocalId {
        let mut current = local;
        for _ in 0_u8..8 {
            let mut next = None;
            for block in &self.function.blocks {
                for statement in &block.statements {
                    if let Statement::Assign {
                        dest,
                        value:
                            Rvalue::Use(
                                Operand::Copy(Place::Local(source))
                                | Operand::Move(Place::Local(source)),
                            ),
                    } = statement
                        && *dest == current
                    {
                        next = Some(*source);
                    }
                }
            }
            let Some(source) = next else {
                return current;
            };
            if source == current {
                return current;
            }
            current = source;
        }
        current
    }

    /// Resolve a callback operand to a Rust closure shaped like its function type.
    ///
    /// JavaScript array callbacks may declare fewer formal parameters than the
    /// runtime supplies. The declared operand type records the contextual
    /// callback shape, so this path asks closure emission to add ignored
    /// trailing parameters before iterator code calls the callback with
    /// `(item, index, array)` or `(acc, item, index, array)`.
    pub(super) fn closure_operand_text_for_declared_type(
        &self,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => {
                return Err(EmitError::new(
                    "list callback must be a non-escaping closure local",
                ));
            }
        };
        let dest_ty = self.operand_ty(operand)?;
        let source_local = self.closure_source_local(local);
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == source_local
                {
                    return self.closure_text_for_type(*id, dest_ty);
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }

    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    pub(super) fn list_count_text(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list count receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list count item must match the list element type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
            return Err(EmitError::new("list count destination must be int"));
        }
        let list_text = self.operand_text(list)?;
        let item_text = self.operand_text(item)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().filter(|item| **item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).count() as i64 }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().filter(|item| item.same_js_key(&smelt_needle)).count() as i64 }}"
            ));
        }
        Ok(format!(
            "{list_text}.iter().filter(|item| *item == &{item_text}).count() as i64"
        ))
    }

    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    pub(super) fn list_sum_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list sum receiver must be a list"));
        };
        if dest_ty != *item_ty {
            return Err(EmitError::new(
                "list sum destination must match the list element type",
            ));
        }
        match self.mir.types.get(*item_ty) {
            Some(Type::Int) => Ok(format!(
                "{}.iter().copied().sum::<i64>()",
                self.operand_text(list)?
            )),
            Some(Type::Float) => Ok(format!(
                "{}.iter().copied().sum::<f64>()",
                self.operand_text(list)?
            )),
            _ => Err(EmitError::new("list sum supports int and float lists")),
        }
    }

    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    pub(super) fn list_bool_fold_text(
        &self,
        op: smelt_hir::BoolFoldOp,
        list: &Operand,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list boolean fold receiver must be a list"));
        };
        if !matches!(self.mir.types.get(*item_ty), Some(Type::Bool)) {
            return Err(EmitError::new(
                "list boolean fold supports boolean lists only",
            ));
        }
        let method_name = match op {
            smelt_hir::BoolFoldOp::All => "all",
            smelt_hir::BoolFoldOp::Any => "any",
        };
        Ok(format!(
            "{}.iter().copied().{method_name}(|value| value)",
            self.operand_text(list)?
        ))
    }

    // Sorted-list helpers continue in `list_ordering.rs`.
}

/// Rewrites emitted closure text so shared captures use their `RefCell` storage.
///
/// Some closure emission paths wrap an already-rendered closure with a capture
/// prelude. The prelude creates `smelt_capture_<name>`, but the rendered body
/// can still mention the source name. This textual pass is intentionally
/// limited to identifier-boundary replacements for those wrapper-only cases.
fn replace_shared_capture_uses(mut text: String, replacements: &[(String, String)]) -> String {
    for (source, target) in replacements {
        text = replace_identifier(&text, source, target);
    }
    text
}

/// Replaces complete Rust identifier occurrences in generated text.
fn replace_identifier(text: &str, source: &str, target: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(offset) = text[index..].find(source) {
        let start = index.saturating_add(offset);
        let end = start.saturating_add(source.len());
        out.push_str(&text[index..start]);
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if before.is_some_and(is_rust_ident_char) || after.is_some_and(is_rust_ident_char) {
            out.push_str(source);
        } else {
            out.push_str(target);
        }
        index = end;
    }
    out.push_str(&text[index..]);
    out
}

/// Returns true for characters that can be part of emitted Rust identifiers.
fn is_rust_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}
