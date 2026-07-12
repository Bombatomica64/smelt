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
        // Bind the predicate parameter to the concrete element type. When the
        // closure is hoisted into its own `let smelt_predicate = ...` binding it
        // is no longer inline in the iterator chain, so Rust cannot infer the
        // element type from `&_`; spelling it out keeps the standalone binding
        // well-typed (E0282).
        let element_type_text = self.type_text(element_ty)?;
        let predicate = if self.list_item_uses_same_value_zero(element_ty) {
            if self.mir.types.get(element_ty) == Some(&Type::Float) {
                "|item: &f64| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())".to_owned()
            } else {
                format!("|item: &{element_type_text}| item.same_js_key(&smelt_needle)")
            }
        } else {
            format!("|item: &{element_type_text}| *item == smelt_needle")
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
        let callback_call_text =
            self.callback_invocation_text(function_ty, &call_args.join(", "));
        // A fallible (`may_throw`) callback is emitted as a closure returning
        // `Result<_, Box<dyn Error>>`, but list predicates consume the result in
        // boolean position (`if …`, `.any`, `.all`). Unwrap the `Result` the same
        // way `list_map_closure_text` does so the predicate observes the inner
        // value rather than a `Result`; a non-throwing callback is used directly.
        let raw_call_text = if function_ty.may_throw {
            format!(
                "({callback_call_text}).unwrap_or_else(|error: Box<dyn std::error::Error>| panic!(\"{{}}\", error))"
            )
        } else {
            callback_call_text
        };
        let call_text = if self.mir.types.get(function_ty.return_ty) == Some(&Type::Bool) {
            raw_call_text
        } else {
            self.value_truthy_text(&raw_call_text, function_ty.return_ty)?
        };
        let prefix = format!(
            "let smelt_callback = {closure_text}; {prefix}",
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
        let callback_call_text =
            self.callback_invocation_text(function_ty, &call_args.join(", "));
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
            "{{ let smelt_callback = {closure_text}; {}{}.iter().enumerate().map(|(index, item)| {{ {value_text} }}).collect::<Vec<_>>() }}",
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
        let call_text = self.callback_invocation_text(function_ty, &call_args.join(", "));
        Ok(format!(
            "{{ let smelt_callback = {closure_text}; {}{}.iter().enumerate().for_each(|(index, item)| {{ let _ = {call_text}; }}); () }}",
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
        let call_text = self.callback_invocation_text(function_ty, &call_args.join(", "));
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
            "{{ let smelt_callback = {closure_text}; {}{}.iter().enumerate().flat_map(|(index, item)| {{ let smelt_result = {call_text}; {flattened_text} }}).collect::<Vec<_>>() }}",
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
    /// Renders an invocation of the `smelt_callback` binding used by the array
    /// callback lowerings.
    ///
    /// A callback whose type is an erased JS rest callable is a
    /// `SmeltErasedFunction` value rather than a Rust `Fn`, so it must be invoked
    /// through the erased callable ABI (`.call(..)`). Every other callback is a
    /// concrete closure and uses direct call syntax.
    fn callback_invocation_text(&self, function_ty: &FunctionType, args: &str) -> String {
        if self.is_erased_unknown_rest_function(function_ty) {
            format!("smelt_callback.call({args})")
        } else {
            format!("(smelt_callback)({args})")
        }
    }

    /// Builds the shared iteration scaffolding (receiver prefix, iterator text,
    /// and per-element call arguments) for an array callback lowering.
    ///
    /// The callback's declared parameters decide how many of `(item, index,
    /// array)` are forwarded and how each is coerced; a third parameter forces a
    /// cloned `smelt_array` snapshot so the callback can observe the whole source.
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
        let call_text = self.callback_invocation_text(function_ty, &call_args.join(", "));
        Ok(format!(
            "{{ let smelt_callback = {closure_text}; let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..array_from_length).map(|index| {call_text}).collect::<Vec<_>>() }}"
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
        let call_expr = self.callback_invocation_text(function_ty, &call_args.join(", "));
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

