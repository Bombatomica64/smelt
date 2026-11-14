//! List query and fold emission helpers.

use super::*;
use crate::rust::RustExpr;
use smelt_hir::FunctionType;

/// Shared Rust fragments for closure-backed list callback iteration.
struct ListCallbackIterationParts {
    /// Statements that must run before iterating the callback receiver.
    prefix: String,
    /// Rust expression used as the iterable callback receiver.
    iter_text: String,
    /// Rendered callback arguments in JavaScript callback ABI order.
    call_args: Vec<String>,
    /// Whether `iter_text` borrows a snapshot `prefix` binds in the block.
    borrows_snapshot: bool,
}

/// A block of `prefix` then `tail`.
///
/// Before edition 2024 a tail expression's temporaries outlive the block's locals,
/// so a borrow of a snapshot bound in `prefix` needs the value bound first.
fn iteration_block(prefix: &str, tail: &str, borrows_snapshot: bool) -> String {
    if borrows_snapshot {
        format!("{{ {prefix}let smelt_value = {tail}; smelt_value }}")
    } else {
        format!("{{ {prefix}{tail} }}")
    }
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
    pub(super) fn list_search_expr(
        &self,
        op: smelt_hir::ListSearchOp,
        list: &Operand,
        item: &Operand,
        from_index: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list search receiver must be a list"));
        };
        let element_ty = *item_ty;
        let item_text = if self.operand_ty(item)? == element_ty {
            self.operand_expr(item)?
        } else {
            self.rendered_at_type(
                &self.operand_expr(item)?,
                self.operand_ty(item)?,
                element_ty,
                &self.render_scope(),
            )?
        };
        // `operand_borrow_expr` is this borrow, plus the shared-capture guard the
        // hand-rolled version lacked (see its doc comment); the list's own shared
        // buffer is then read through `list_read_expr`.
        let borrowed_list_text = list_read_expr(&self.operand_borrow_expr(list)?);
        let method_name = match op {
            smelt_hir::ListSearchOp::Find => "position",
            smelt_hir::ListSearchOp::RFind => "rposition",
        };
        // Without a `fromIndex` this reduces to a direct `position`/`rposition`
        // scan over the borrowed list. JavaScript `indexOf` compares with strict
        // equality: `NaN` never matches and `+0`/`-0` are equal, which is `f64`'s
        // own `==`. Other value-typed elements compare by JS value and
        // reference-typed elements structurally.
        let Some(from_index_operand) = from_index else {
            if let Some(compare) = self.function_identity_compare(element_ty) {
                return Ok(RustExpr::atom(format!(
                    "{{ let smelt_needle = {item_text}; {borrowed_list_text}.iter().{method_name}(|smelt_item| {compare}(smelt_item, &smelt_needle)).map_or(-1.0, |smelt_idx| smelt_idx as f64) }}",
                    item_text = item_text.delimited(),
                    borrowed_list_text = borrowed_list_text.tight()
                )));
            }
            if self.list_item_uses_same_value_zero(element_ty) {
                if self.mir.types.get(element_ty) == Some(&Type::Float) {
                    return Ok(RustExpr::atom(format!(
                        "{{ let smelt_needle = {item_text}; {borrowed_list_text}.iter().{method_name}(|smelt_item| *smelt_item == smelt_needle).map_or(-1.0, |smelt_idx| smelt_idx as f64) }}",
                        item_text = item_text.delimited(),
                        borrowed_list_text = borrowed_list_text.tight()
                    )));
                }
                return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                    "{{ let smelt_needle = {item_text}; {borrowed_list_text}.iter().{method_name}(|smelt_item| smelt_item.same_js_key(&smelt_needle)).map_or(-1.0, |smelt_idx| smelt_idx as f64) }}", item_text = item_text.delimited(), borrowed_list_text = borrowed_list_text.tight()
                ))));
            }
            return Ok(borrowed_list_text.postfix(format!(
                ".iter().{method_name}(|smelt_item| smelt_item == &{item_text}).map_or(-1.0, |smelt_idx| smelt_idx as f64)", item_text = item_text.tight()
            )));
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
        let element_type_text = self.signature_type(element_ty)?;
        let predicate = if self.list_item_uses_same_value_zero(element_ty) {
            if self.mir.types.get(element_ty) == Some(&Type::Float) {
                "|item: &f64| *item == smelt_needle".to_owned()
            } else {
                self.carrier_format(
                    self.carrier()?,
                    format_args!(
                        "|smelt_item: &{element_type_text}| smelt_item.same_js_key(&smelt_needle)"
                    ),
                )
            }
        } else {
            format!("|smelt_item: &{element_type_text}| *smelt_item == smelt_needle")
        };
        let index_text = self.value_at_type(from_index_operand, self.type_id(Type::Float)?)?;
        // Normalize `fromIndex`: truncate toward zero, then translate a negative
        // value into an offset from the end. `indexOf` clamps the start into
        // `[0, len]`; `lastIndexOf` clamps the inclusive end into `[-1, len - 1]`,
        // where a fully-out-of-range negative end means "search nothing". The
        // enumerate iterator yields `(usize, &T)`, so `find` binds `item: &&T`
        // and the predicate is applied to `*item` (`&T`).
        match op {
            smelt_hir::ListSearchOp::Find => Ok(RustExpr::atom(format!(
                "{{ let smelt_needle = {item_text}; let smelt_predicate = {predicate}; let smelt_list = &{borrowed_list_text}; let smelt_len = smelt_list.len() as i64; let smelt_raw = {index_text} as i64; let smelt_start = if smelt_raw < 0 {{ (smelt_len + smelt_raw).max(0) }} else {{ smelt_raw }} as usize; smelt_list.iter().enumerate().skip(smelt_start).find(|(_, smelt_item)| smelt_predicate(*smelt_item)).map_or(-1.0, |(smelt_idx, _)| smelt_idx as f64) }}",
                item_text = item_text.delimited(),
                borrowed_list_text = borrowed_list_text.tight(),
                index_text = index_text.as_cast_operand()
            ))),
            smelt_hir::ListSearchOp::RFind => Ok(RustExpr::atom(format!(
                "{{ let smelt_needle = {item_text}; let smelt_predicate = {predicate}; let smelt_list = &{borrowed_list_text}; let smelt_len = smelt_list.len() as i64; let smelt_raw = {index_text} as i64; let smelt_end = if smelt_raw < 0 {{ smelt_len + smelt_raw }} else {{ smelt_raw.min(smelt_len - 1) }}; if smelt_end < 0 {{ -1.0 }} else {{ let smelt_take = (smelt_end as usize).saturating_add(1).min(smelt_list.len()); smelt_list.iter().enumerate().take(smelt_take).rev().find(|(_, smelt_item)| smelt_predicate(*smelt_item)).map_or(-1.0, |(smelt_idx, _)| smelt_idx as f64) }} }}",
                item_text = item_text.delimited(),
                borrowed_list_text = borrowed_list_text.tight(),
                index_text = index_text.as_cast_operand()
            ))),
        }
    }

    /// Converts a closure-backed callback list operation to Rust iterator text.
    pub(super) fn list_callback_expr(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list callback receiver must be a list"));
        };
        let element_ty = *list_element_ty;
        self.list_callback_closure(op, list, list_ty, element_ty, callback, dest_ty)
    }

    /// Emits a list callback operation through a normal MIR closure body.
    fn list_callback_closure(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match op {
            smelt_hir::ListCallbackOp::Map => {
                return self.list_map_closure(list, list_ty, element_ty, callback, dest_ty);
            }
            smelt_hir::ListCallbackOp::ForEach => {
                return self.list_for_each_closure(list, list_ty, element_ty, callback, dest_ty);
            }
            smelt_hir::ListCallbackOp::FlatMap => {
                return self.list_flat_map_closure(list, list_ty, element_ty, callback, dest_ty);
            }
            _ => {}
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Err(EmitError::new("list callback receiver must be a list"));
        };
        let closure_expr = match self.closure_operand_expr_for_declared_type(callback) {
            Ok(closure_expr) => closure_expr,
            Err(_) => self.operand_expr(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        // JavaScript array predicates (`filter`/`find`/`findIndex`/`some`/`every`)
        // coerce the callback result with the usual truthiness rules rather than
        // requiring a `boolean`. When the declared return type is not already
        // `bool` (e.g. an erased `unknown` predicate, `x => x.length`, or
        // `x => x && cond`), route the single call through `value_truthy_expr` so
        // the predicate observes the same truthiness the source does; a `bool`
        // return passes through unchanged.
        let callback_call_text = self.callback_invocation_expr(function_ty, &call_args.join(", "));
        // A fallible (`may_throw`) callback is emitted as a closure returning
        // `Result<_, Box<dyn Error>>`, but list predicates consume the result in
        // boolean position (`if …`, `.any`, `.all`). Unwrap the `Result` the same
        // way `list_map_closure` does so the predicate observes the inner
        // value rather than a `Result`; a non-throwing callback is used directly.
        let raw_call_text = if function_ty.may_throw {
            RustExpr::atom(format!(
                "({callback_call_text}).unwrap_or_else(|smelt_error: {ERROR_BOX}| smelt_panic_throw(smelt_error))",
                callback_call_text = callback_call_text.delimited()
            ))
        } else {
            callback_call_text
        };
        let call_text = if self.mir.types.get(function_ty.return_ty) == Some(&Type::Bool) {
            raw_call_text
        } else {
            self.value_truthy_expr(&raw_call_text, function_ty.return_ty)?
        };
        let prefix = format!(
            "let smelt_callback = {closure_expr}; {prefix}",
            prefix = list_iteration.prefix,
            closure_expr = closure_expr.delimited()
        );
        let iter_text = list_iteration.iter_text;
        let borrows_snapshot = list_iteration.borrows_snapshot;
        match op {
            smelt_hir::ListCallbackOp::Filter => {
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array filter destination must match the receiver list type",
                    ));
                }
                Ok(RustExpr::from(iteration_block(
                    &prefix,
                    &format!(
                        "{iter_text}.iter().enumerate().filter_map(|(smelt_index, smelt_item)| if {call_text} {{ Some(smelt_item.clone()) }} else {{ None }}).collect::<Vec<_>>()",
                        call_text = call_text.delimited()
                    ),
                    borrows_snapshot,
                )))
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
                Ok(RustExpr::from(iteration_block(
                    &prefix,
                    &format!(
                        "{iter_text}.iter().enumerate(){direction}.find_map(|(smelt_index, smelt_item)| if {call_text} {{ Some(smelt_item.clone()) }} else {{ None }})",
                        call_text = call_text.delimited()
                    ),
                    borrows_snapshot,
                )))
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
                Ok(RustExpr::from(iteration_block(
                    &prefix,
                    &format!(
                        "{iter_text}.iter().enumerate(){direction}.find_map(|(smelt_index, smelt_item)| if {call_text} {{ Some(smelt_index as f64) }} else {{ None }}).unwrap_or(-1.0)",
                        call_text = call_text.delimited()
                    ),
                    borrows_snapshot,
                )))
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
                Ok(RustExpr::from(iteration_block(
                    &prefix,
                    &format!(
                        "{iter_text}.iter().enumerate().{method}(|(smelt_index, smelt_item)| {call_text})",
                        call_text = call_text.delimited()
                    ),
                    borrows_snapshot,
                )))
            }
            smelt_hir::ListCallbackOp::Map
            | smelt_hir::ListCallbackOp::ForEach
            | smelt_hir::ListCallbackOp::FlatMap => Err(EmitError::new(
                "list callback operation should have been handled before predicate emission",
            )),
        }
    }

    /// Emits `Array.map` for full MIR closures that cannot use callback trees.
    fn list_map_closure(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array map destination must be a list"));
        };
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Err(EmitError::new("list callback must be a function"));
        };
        let closure_expr = match self.closure_operand_expr_for_declared_type(callback) {
            Ok(closure_expr) => closure_expr,
            Err(_) => self.operand_expr(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        let callback_call_text = self.callback_invocation_expr(function_ty, &call_args.join(", "));
        let call_text = if function_ty.may_throw {
            RustExpr::atom(format!(
                "({callback_call_text}).unwrap_or_else(|smelt_error: {ERROR_BOX}| smelt_panic_throw(smelt_error))",
                callback_call_text = callback_call_text.delimited()
            ))
        } else {
            callback_call_text
        };
        let value_text = self.rendered_at_type(
            &call_text,
            function_ty.return_ty,
            *dest_item_ty,
            &self.render_scope(),
        )?;
        Ok(RustExpr::from(iteration_block(
            &format!(
                "let smelt_callback = {closure_expr}; {}",
                list_iteration.prefix,
                closure_expr = closure_expr.delimited()
            ),
            &format!(
                "{}.iter().enumerate().map(|(smelt_index, smelt_item)| {{ {value_text} }}).collect::<Vec<_>>()",
                list_iteration.iter_text,
                value_text = value_text.delimited()
            ),
            list_iteration.borrows_snapshot,
        )))
    }

    /// Emits `Array.forEach` for function callbacks without inline callback trees.
    fn list_for_each_closure(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if dest_ty != self.none_ty {
            return Err(EmitError::new("array forEach destination must be none"));
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Err(EmitError::new("list callback must be a function"));
        };
        let closure_expr = match self.closure_operand_expr_for_declared_type(callback) {
            Ok(closure_expr) => closure_expr,
            Err(_) => self.operand_expr(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        let callback_call = self.callback_invocation_expr(function_ty, &call_args.join(", "));
        // A callback's throw must leave the loop; dropping the `Result` would
        // carry on as if nothing had been thrown.
        let call_text = if function_ty.may_throw {
            RustExpr::atom(format!(
                "({callback_call}).unwrap_or_else(|smelt_error: {ERROR_BOX}| smelt_panic_throw(smelt_error))",
                callback_call = callback_call.delimited()
            ))
        } else {
            callback_call
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_callback = {closure_expr}; {}{}.iter().enumerate().for_each(|(smelt_index, smelt_item)| {{ let _ = {call_text}; }}); () }}",
            list_iteration.prefix,
            list_iteration.iter_text,
            closure_expr = closure_expr.delimited(),
            call_text = call_text.delimited()
        )))
    }

    /// Emits `Array.flatMap` for function callbacks without inline callback trees.
    fn list_flat_map_closure(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array flatMap destination must be a list"));
        };
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Err(EmitError::new("list callback must be a function"));
        };
        let closure_expr = match self.closure_operand_expr_for_declared_type(callback) {
            Ok(closure_expr) => closure_expr,
            Err(_) => self.operand_expr(callback)?,
        };
        let list_iteration =
            self.list_callback_iteration_parts(list, list_ty, element_ty, callback, function_ty)?;
        let call_args = list_iteration.call_args;
        let call_text = self.callback_invocation_expr(function_ty, &call_args.join(", "));
        let flattened_text = match self.mir.types.get(function_ty.return_ty) {
            Some(Type::List(callback_item_ty)) => {
                let value_text = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    *callback_item_ty,
                    *dest_item_ty,
                    &self.render_scope(),
                )?;
                format!(
                    "smelt_result.into_iter().map(|smelt_value| {value_text}).collect::<Vec<_>>()",
                    value_text = value_text.delimited()
                )
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                let unknown_ty = self.type_id(Type::Unknown)?;
                let value_text = self.rendered_at_type(
                    &RustExpr::path("smelt_value"),
                    unknown_ty,
                    *dest_item_ty,
                    &self.render_scope(),
                )?;
                self.carrier_format(self.carrier()?, format_args!(
                    "match smelt_result {{ SmeltUnknown::Array(smelt_values) => smelt_values.into_iter().map(|smelt_value| {value_text}).collect::<Vec<_>>(), smelt_value => vec![{value_text}] }}", value_text = value_text.delimited()
                ))
            }
            _ => {
                let value_text = self.rendered_at_type(
                    &RustExpr::path("smelt_result"),
                    function_ty.return_ty,
                    *dest_item_ty,
                    &self.render_scope(),
                )?;
                format!("vec![{value_text}]", value_text = value_text.delimited())
            }
        };
        Ok(RustExpr::from(iteration_block(
            &format!(
                "let smelt_callback = {closure_expr}; {}",
                list_iteration.prefix,
                closure_expr = closure_expr.delimited()
            ),
            &format!(
                "{}.iter().enumerate().flat_map(|(smelt_index, smelt_item)| {{ let smelt_result = {call_text}; {flattened_text} }}).collect::<Vec<_>>()",
                list_iteration.iter_text,
                call_text = call_text.delimited()
            ),
            list_iteration.borrows_snapshot,
        )))
    }

    /// Renders an invocation of the `smelt_callback` binding used by the array
    /// callback lowerings.
    ///
    /// A callback whose type is an erased JS rest callable is a
    /// `SmeltErasedFunction` value rather than a Rust `Fn`, so it must be invoked
    /// through the erased callable ABI (`.call(..)`). Every other callback is a
    /// concrete closure and uses direct call syntax.
    pub(super) fn callback_invocation_expr(
        &self,
        function_ty: &FunctionType,
        args: &str,
    ) -> RustExpr {
        if self.is_erased_unknown_rest_function(function_ty) {
            // `SmeltErasedFunction::call` answers `SmeltUnknown` whatever the
            // callback's DECLARED return type is, while every caller here
            // treats the text as that declared type. Read it back out through
            // the checked extraction so a `(...args: any[]) => string` mapper
            // yields `String`s.
            let call = RustExpr::chain(format!("smelt_callback.call({args})"));
            if self.renders_erased(function_ty.return_ty).unwrap_or(false) {
                return call;
            }
            self.extract_rendered(&call, function_ty.return_ty, &self.render_scope())
                .unwrap_or(call)
        } else {
            RustExpr::atom(format!("(smelt_callback)({args})"))
        }
    }

    /// Build shared list-iteration text for closure-backed array callbacks.
    ///
    /// Most callbacks only consume `(item)` or `(item, index)`, so the emitter
    /// can borrow the original receiver and clone each item as it is passed.
    /// JavaScript's optional third callback argument observes the whole source
    /// array; only that ABI needs a stable cloned snapshot named
    /// `smelt_array`.
    ///
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
        let owned_list_text = self.operand_expr(list)?;
        // `operand_borrow_expr` is this borrow, plus the shared-capture guard the
        // hand-rolled version lacked (see its doc comment). That guard matters here:
        // this emitter invokes a callback, so a `Ref` guard spanning the call would
        // panic "already borrowed".
        //
        // KNOWN LIMITATION of the list's shared backing buffer: the `Ref` this
        // takes on the list's own cell DOES span the callback, because iterating
        // without it would mean copying the whole array once per call — the cost
        // this iteration shape exists to avoid. A callback that only reads the
        // array it is iterating (every case in the remeda/es-toolkit corpora) is
        // fine, because concurrent shared borrows are allowed. A callback that
        // WRITES the array it is iterating — legal in JavaScript — panics "already
        // borrowed".
        let borrowed_list_text = self.operand_borrow_expr(list)?;
        // A variadic erased callback (`(...args: any[]) => any`, a
        // `SmeltErasedFunction`) has ONE Rust parameter, its rest list, and
        // JavaScript fills that list with every argument the array method
        // supplies: `(item, index, array)`. Binding the element to the rest
        // slot converted the element itself into the argument list (an object
        // element panicked "unknown is not iterable"). Pack the arguments
        // instead, erased at the boundary the callee already declares.
        if self.is_erased_unknown_rest_function(function_ty) {
            let item =
                self.erase_rendered(&RustExpr::path("smelt_item").cloned_value(), element_ty)?;
            let array =
                self.erase_rendered(&RustExpr::path("smelt_array").cloned_value(), list_ty)?;
            return Ok(ListCallbackIterationParts {
                prefix: format!("let smelt_array = {owned_list_text}; ", owned_list_text = owned_list_text.delimited()),
                iter_text: list_read_expr(&RustExpr::path("smelt_array")).into_string(),
                call_args: vec![self.carrier_format(self.carrier()?, format_args!(
                    "SmeltList::from(vec![{item}, SmeltUnknown::Number(smelt_index as f64), {array}])", item = item.delimited(), array = array.delimited()
                ))],
                borrows_snapshot: true,
            });
        }
        // A zero-parameter callback (`values.map(stubTrue)`) ignores every
        // supplied argument, so it is called with no arguments at all.
        let mut call_args = Vec::new();
        if let Some(item_param_ty) = function_ty.params.first().copied() {
            // The element parameter is by shared reference whenever the elements are
            // themselves lists (`unzipWith`, `zipWith`, ...). The iteration binds
            // `item` as a reference already, so pass it straight through instead of
            // cloning a whole inner list per element. When the element type and the
            // parameter type differ a conversion still has to run, and its result is a
            // temporary whose lifetime Rust extends to the end of the statement.
            if self.callback_param_is_shared_reference(function_ty, 0, item_param_ty) {
                if element_ty == item_param_ty {
                    call_args.push("smelt_item".to_owned());
                } else {
                    call_args.push(format!(
                        "&({})",
                        self.rendered_at_type(
                            &RustExpr::path("smelt_item").cloned_value(),
                            element_ty,
                            item_param_ty,
                            &self.render_scope()
                        )?
                        .delimited()
                    ));
                }
            } else {
                call_args.push(
                    self.rendered_at_type(
                        &RustExpr::path("smelt_item").cloned_value(),
                        element_ty,
                        item_param_ty,
                        &self.render_scope(),
                    )?
                    .into_string(),
                );
            }
        }
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            let index_source_ty = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                self.type_id(Type::Int)?
            } else {
                self.type_id(Type::Float)?
            };
            let index_value = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                RustExpr::cast("smelt_index as i64")
            } else {
                RustExpr::cast("smelt_index as f64")
            };
            call_args.push(
                self.rendered_at_type(
                    &index_value,
                    index_source_ty,
                    index_param_ty,
                    &self.render_scope(),
                )?
                .into_string(),
            );
        }
        let needs_array_snapshot = function_ty.params.get(2).is_some();
        let (prefix, iter_text) = if needs_array_snapshot {
            let array_param_ty = function_ty.params.get(2).copied().ok_or_else(|| {
                EmitError::new("array callback snapshot requires an array parameter")
            })?;
            // The array argument is the JS third callback parameter. When the
            // callback takes it by shared reference, pass the snapshot by reference:
            // the snapshot itself is one copy per CALL, but `smelt_array.clone()`
            // here was one copy per ELEMENT.
            if self.callback_param_is_shared_reference(function_ty, 2, array_param_ty) {
                if list_ty == array_param_ty {
                    call_args.push("&smelt_array".to_owned());
                } else {
                    // A conversion has to run; its result is a temporary whose
                    // lifetime Rust extends to the end of the statement.
                    call_args.push(format!(
                        "&({})",
                        self.rendered_at_type(
                            &RustExpr::opaque("smelt_array.clone()"),
                            list_ty,
                            array_param_ty,
                            &self.render_scope()
                        )?
                        .delimited()
                    ));
                }
            } else {
                call_args.push(
                    self.rendered_at_type(
                        &RustExpr::path("smelt_array").cloned_value(),
                        list_ty,
                        array_param_ty,
                        &self.render_scope(),
                    )?
                    .into_string(),
                );
            }
            (
                format!(
                    "let smelt_array = {owned_list_text}; ",
                    owned_list_text = owned_list_text.delimited()
                ),
                list_read_expr(&RustExpr::path("smelt_array")),
            )
        } else {
            (String::new(), list_read_expr(&borrowed_list_text))
        };
        Ok(ListCallbackIterationParts {
            prefix,
            iter_text: iter_text.into_string(),
            call_args,
            borrows_snapshot: needs_array_snapshot,
        })
    }

    /// Converts `Array.from({ length }, mapper)` into an indexed Rust loop.
    pub(super) fn list_from_length_map_expr(
        &self,
        length: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_expr(length)?;
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Err(EmitError::new("Array.from mapper must be a function"));
        };
        if self.mir.types.get(dest_ty) != Some(&Type::List(function_ty.return_ty)) {
            return Err(EmitError::new(
                "Array.from mapper destination must be a list of callback results",
            ));
        }
        let closure_expr = self.closure_operand_expr_for_declared_type(callback)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let float_ty = self.type_id(Type::Float)?;
        let mut call_args = Vec::new();
        if let Some(item_param_ty) = function_ty.params.first().copied() {
            call_args.push(
                self.callback_call_arg_expr(
                    function_ty,
                    0,
                    item_param_ty,
                    self.rendered_at_type(
                        &self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltUnknown::Null")),
                        unknown_ty,
                        item_param_ty,
                        &self.render_scope(),
                    )?
                    .into_string(),
                ),
            );
        }
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            call_args.push(
                self.callback_call_arg_expr(
                    function_ty,
                    1,
                    index_param_ty,
                    self.rendered_at_type(
                        &RustExpr::cast("smelt_index as f64"),
                        float_ty,
                        index_param_ty,
                        &self.render_scope(),
                    )?
                    .into_string(),
                ),
            );
        }
        let call_text = self.callback_invocation_expr(function_ty, &call_args.join(", "));
        Ok(RustExpr::atom(format!(
            "{{ let smelt_callback = {closure_expr}; let smelt_array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..smelt_array_from_length).map(|smelt_index| {call_text}).collect::<Vec<_>>() }}",
            closure_expr = closure_expr.delimited(),
            length_text = length_text.as_cast_operand(),
            call_text = call_text.delimited()
        )))
    }

    /// Converts a length-only array allocation (`Array(n)`, `new Array(n)`,
    /// `Array.from({ length: n })`) into a Rust vector of `n` holes.
    ///
    /// JavaScript allocates a sparse array of LENGTH `n` here, not an empty one:
    /// `Array(3).length` is `3` and `Array(3)[0]` is `undefined`. Emitting an
    /// empty `vec![]` silently broke every consumer that drives a loop off
    /// `.length` (`fill`, `zip`, `unzip`) because the loop body never ran.
    ///
    /// A hole is materialised as the element type's missing value — exactly the
    /// value an out-of-range READ of that same list answers
    /// (`element_missing_value`), so construction and reads cannot
    /// disagree. That keeps a concrete element list concrete: `Array(3)` for a
    /// `Vec<f64>` allocates three `f64::default()` slots rather than erasing the
    /// list to `SmeltUnknown`.
    pub(super) fn list_from_length_expr(
        &self,
        length: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(&Type::List(item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new(
                "array length allocation destination must be a list",
            ));
        };
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_expr(length)?;
        let hole_text = self.element_missing_value(item_ty)?;
        // Each slot evaluates the hole expression ONCE, rather than cloning one
        // hole `n` times with `vec![hole; n]`.
        //
        // `vec![x; n]` is `Clone`-based, and a Smelt reference value clones by
        // SHARING: since the shared-buffer change, `SmeltList`/`SmeltObject`/
        // `SmeltJsMap`/`SmeltJsSet` all bump an `Rc` and copy the stable `id`.
        // So `Array(n)` of a list-typed element produced n handles to ONE array,
        // carrying one JavaScript identity between them — `a[0].push(x)` would
        // have been visible through `a[1]`. Evaluating the hole per slot mints a
        // fresh buffer and a fresh `id` for each, which is what distinct array
        // elements are.
        //
        // Stated as one rule for every element type rather than switched on
        // whether this particular `Clone` shares: `(0..n).map(..).collect()` over
        // a `Range` is `TrustedLen`, so it pre-allocates and fills in a tight
        // loop, and a primitive hole costs nothing extra for the uniformity.
        Ok(RustExpr::atom(format!(
            "{{ let smelt_array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..smelt_array_from_length).map(|_| {hole_text}).collect::<Vec<_>>() }}",
            length_text = length_text.as_cast_operand(),
            hole_text = hole_text.delimited()
        )))
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
    /// still folds into a value the next step accepts. The frontend
    /// (`lower_list_reduce`) picks `dest_ty` so this reconciliation is statically
    /// valid; here we only render the arity- and type-matched call.
    ///
    /// `from_right` renders `Array.prototype.reduceRight`: the same fold over the
    /// `enumerate()`d elements reversed with `.rev()`, so elements are visited
    /// from the last index down while each callback still receives its original
    /// index, and a seedless fold starts from the LAST element.
    pub(super) fn list_reduce_expr(
        &self,
        list: &Operand,
        initial: Option<&Operand>,
        callback: &Operand,
        from_right: bool,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // Index of the JS fourth callback argument (the receiver array) inside
        // `candidate_args` below. Named so the by-reference binding and the argument
        // rendering agree on which position is already a reference.
        const ARRAY_ARG_INDEX: usize = 3;
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
        // `smelt_index` here names the fold body's `let smelt_index = smelt_index as f64;` binding
        // (see the `fold` closures below), so it is already `f64`. Passing the
        // bare local — rather than a pre-cast `index as f64` string — lets the
        // shared coercion seam emit a single, minimal cast to the callback's
        // declared parameter type instead of stacking a redundant `as f64`
        // (e.g. `(index.trunc() as i64)` for an `i64` param, `index` for `f64`).
        let candidate_args: [(&str, TypeId); 4] = [
            ("smelt_acc", dest_ty),
            ("smelt_item", element_ty),
            ("smelt_index", float_ty),
            ("smelt_array", list_ty),
        ];
        // Whether the callback declares the array parameter by shared reference.
        // When it does, the fold body binds the borrow itself rather than a per-element
        // deep copy of the whole list — the quadratic cost
        // `callback_param_is_shared_reference` exists to remove.
        let array_by_ref = function_ty
            .params
            .get(ARRAY_ARG_INDEX)
            .copied()
            .is_some_and(|param_ty| {
                self.callback_param_is_shared_reference(function_ty, ARRAY_ARG_INDEX, param_ty)
                    && param_ty == list_ty
            });
        let mut call_args = Vec::with_capacity(function_ty.params.len());
        for (index, param_ty) in function_ty.params.iter().copied().enumerate() {
            let Some((value_text, source_ty)) = candidate_args.get(index).copied() else {
                return Err(EmitError::new(
                    "array reduce callback declares more parameters than reduce supplies",
                ));
            };
            if self.callback_param_is_shared_reference(function_ty, index, param_ty) {
                if source_ty != param_ty {
                    // A conversion has to run; its result is a temporary whose
                    // lifetime Rust extends to the end of the statement.
                    call_args.push(format!(
                        "&({})",
                        self.rendered_at_type(
                            &RustExpr::path(value_text),
                            source_ty,
                            param_ty,
                            &self.render_scope()
                        )?
                        .delimited()
                    ));
                } else if index == ARRAY_ARG_INDEX && array_by_ref {
                    // The array position is already bound as a reference (see
                    // `callback_array_text`).
                    call_args.push(value_text.to_owned());
                } else {
                    // The other fold-body bindings are owned, so they are borrowed
                    // here at the call.
                    call_args.push(format!("&{value_text}"));
                }
                continue;
            }
            call_args.push(
                self.rendered_at_type(
                    &RustExpr::path(value_text),
                    source_ty,
                    param_ty,
                    &self.render_scope(),
                )?
                .into_string(),
            );
        }
        if let Some(initial_operand) = initial
            && self.operand_ty(initial_operand)? != dest_ty
        {
            return Err(EmitError::new(
                "array reduce initial value and callback result must match the destination type",
            ));
        }
        // `operand_borrow_expr` is this borrow, plus the shared-capture guard the
        // hand-rolled version lacked (see its doc comment).
        let borrowed_list_text = self.operand_borrow_expr(list)?;
        // `fold` borrows the receiver for the duration of iteration. JavaScript
        // also supplies that receiver as the callback's fourth array argument. A
        // callback that takes it by value owns its argument, so each invocation must
        // clone it even when MIR classified the original receiver operand as a final
        // `Move`; a callback that takes it by shared reference binds the same borrow
        // the fold is already iterating, and no copy happens at all.
        let callback_array_text = if array_by_ref {
            format!(
                "&{borrowed_list_text}",
                borrowed_list_text = borrowed_list_text.tight()
            )
        } else {
            format!(
                "{borrowed_list_text}.clone()",
                borrowed_list_text = borrowed_list_text.tight()
            )
        };
        let callback_closure = match self.closure_operand_expr_for_declared_type(callback) {
            Ok(callback_closure) => callback_closure,
            // A reduce callback passed as a borrowed function parameter (rather
            // than a local closure) renders as the parameter name itself, which
            // calls through the `&dyn Fn`. Fall back to it instead of dropping
            // the reduce body to a default value.
            Err(_) => self.operand_expr(callback)?,
        };
        // Coerce the callback result to the accumulator type so a reconcilable
        // (but not identical) return type still produces the next `acc` value.
        let call_expr = self.callback_invocation_expr(function_ty, &call_args.join(", "));
        // A fallible (`may_throw`) reducer returns `Result<_, Box<dyn Error>>`,
        // but the fold threads its inner value as the next accumulator: unwrap
        // it through the panic channel, as the other list callbacks do, so a
        // throw still reaches the enclosing `catch` (a locale whose
        // `formatDistance` throws, in date-fns' specs). Before, the `Result`
        // was handed back as the accumulator (E0308).
        let call_expr = if function_ty.may_throw {
            RustExpr::atom(format!(
                "({call_expr}).unwrap_or_else(|smelt_error: {ERROR_BOX}| smelt_panic_throw(smelt_error))",
                call_expr = call_expr.delimited()
            ))
        } else {
            call_expr
        };
        let callback_result_text = self.rendered_at_type(
            &call_expr,
            callback_return_ty,
            dest_ty,
            &self.render_scope(),
        )?;
        let callback_text = format!(
            "{{ let smelt_callback = {callback_closure}; {callback_result_text} }}",
            callback_closure = callback_closure.delimited(),
            callback_result_text = callback_result_text.delimited()
        );
        // Same read borrow, and the same known limitation, as the shared
        // callback iteration path above: a reducer that writes the array it is
        // reducing panics rather than failing to compile.
        let list_read = list_read_expr(&borrowed_list_text);
        let direction = if from_right { ".rev()" } else { "" };
        if let Some(initial_operand) = initial {
            let initial_text = self.operand_expr(initial_operand)?;
            Ok(RustExpr::opaque(format!(
                "{list_read}.iter().enumerate(){direction}.fold({initial_text}, |smelt_acc, (smelt_index, smelt_item)| {{ let smelt_item = (*smelt_item).clone(); let smelt_index = smelt_index as f64; let smelt_array = {callback_array_text}; {callback_text} }})",
                list_read = list_read.tight(),
                initial_text = initial_text.delimited()
            )))
        } else if dest_ty == element_ty {
            Ok(RustExpr::atom(format!(
                "{{ let smelt_reduce_items = {list_read}; let mut smelt_reduce_items = smelt_reduce_items.iter().enumerate(){direction}; let (_, smelt_first) = smelt_reduce_items.next().expect(\"Reduce of empty array with no initial value\"); smelt_reduce_items.fold(smelt_first.clone(), |smelt_acc, (smelt_index, smelt_item)| {{ let smelt_item = (*smelt_item).clone(); let smelt_index = smelt_index as f64; let smelt_array = {callback_array_text}; {callback_text} }}) }}",
                list_read = list_read.delimited()
            )))
        } else {
            Err(EmitError::new(
                "array reduce without an initial value must produce the element type",
            ))
        }
    }

    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    pub(super) fn list_count_expr(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
        // Read by borrow: every use below is an `.iter()` receiver, reading the
        // list's shared backing buffer.
        let list_text = list_read_expr(&self.operand_borrow_expr(list)?);
        let item_text = self.operand_expr(item)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(RustExpr::atom(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().filter(|smelt_item| **smelt_item == smelt_needle || (smelt_item.is_nan() && smelt_needle.is_nan())).count() as i64 }}",
                    item_text = item_text.delimited(),
                    list_text = list_text.tight()
                )));
            }
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().filter(|smelt_item| smelt_item.same_js_key(&smelt_needle)).count() as i64 }}", item_text = item_text.delimited(), list_text = list_text.tight()
            ))));
        }
        Ok(RustExpr::opaque(format!(
            "{list_text}.iter().filter(|smelt_item| *smelt_item == &{item_text}).count() as i64",
            list_text = list_text.tight(),
            item_text = item_text.tight()
        )))
    }

    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    pub(super) fn list_sum_expr(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
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
            Some(Type::Int) => Ok({
                let receiver = list_read_expr(&self.operand_expr(list)?);
                RustExpr::atom_on(
                    &receiver,
                    format!(
                        "{receiver}.iter().copied().sum::<i64>()",
                        receiver = receiver.tight()
                    ),
                )
            }),
            Some(Type::Float) => Ok({
                let receiver = list_read_expr(&self.operand_expr(list)?);
                RustExpr::atom_on(
                    &receiver,
                    format!(
                        "{receiver}.iter().copied().sum::<f64>()",
                        receiver = receiver.tight()
                    ),
                )
            }),
            _ => Err(EmitError::new("list sum supports int and float lists")),
        }
    }

    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    pub(super) fn list_bool_fold_expr(
        &self,
        op: smelt_hir::BoolFoldOp,
        list: &Operand,
    ) -> Result<RustExpr, EmitError> {
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
        Ok({
            let receiver = list_read_expr(&self.operand_expr(list)?);
            receiver.postfix(format!(
                ".iter().copied().{method_name}(|smelt_value| smelt_value)"
            ))
        })
    }

    // Sorted-list helpers continue in `list_ordering.rs`.

    /// `concat` of two lists, at the destination type.
    pub(super) fn list_concat_at_dest_expr(
        &self,
        left: &Operand,
        right: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let text = self.list_concat_expr(left, right)?;
        let source_ty = self.concat_result_list_ty(left, right)?;
        self.rendered_at_type(&text, source_ty, dest_ty, &self.render_scope())
    }

    /// One erased `concat` argument.
    ///
    /// One erased `concat` argument, normalized by JavaScript's
    /// `IsConcatSpreadable` rule. The argument's static type does not say
    /// whether it is an array, so the array-vs-scalar choice happens at
    /// runtime in the prelude helper rather than being guessed here.
    pub(super) fn concat_spread_expr(&self, value: &Operand) -> Result<RustExpr, EmitError> {
        let value_ty = self.operand_ty(value)?;
        let value_text = self.erase_rendered(&self.operand_expr(value)?, value_ty)?;
        Ok(RustExpr::chain(format!(
            "smelt_concat_spread({value_text})",
            value_text = value_text.delimited()
        )))
    }

    /// `indexOf`-style search, at the destination type.
    pub(super) fn list_search_at_dest_expr(
        &self,
        op: smelt_hir::ListSearchOp,
        list: &Operand,
        item: &Operand,
        from_index: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let text = self.list_search_expr(op, list, item, from_index)?;
        let float_ty = self.type_id(Type::Float)?;
        self.rendered_at_type(&text, float_ty, dest_ty, &self.render_scope())
    }
}
