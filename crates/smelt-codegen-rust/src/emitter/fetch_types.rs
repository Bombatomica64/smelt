//! Rust emission for the WHATWG fetch types.
//!
//! One emitter per generated fetch runtime type (`SmeltHeaders` today). Every
//! operation lands on a real method of the concrete struct emitted by
//! `crate::fetch_types_prelude`, so the generated call is what a hand-written
//! Rust program would say: `headers.get("content-type")` is
//! `headers.get("content-type")`, typed `Option<String>`, with no tagged value
//! and no runtime member lookup in between.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit `new Headers(init?)`.
    ///
    /// The initializer's static type selects the conversion, which is why the
    /// three source spellings can share one MIR rvalue: a record of
    /// name/value strings, a list of `[name, value]` pairs, or another
    /// `Headers` (whose pairs are copied, so the copy is a distinct JS object).
    /// An unmodeled initializer type is a named blocker rather than a silent
    /// empty header list.
    pub(super) fn headers_new_text(
        &self,
        initializer: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let Some(init) = initializer else {
            return Ok("SmeltHeaders::new()".to_owned());
        };
        let init_ty = self.operand_ty(init)?;
        let init_text = self.operand_text(init)?;
        match self.mir.types.get(init_ty) {
            // Another `Headers`: copy its pairs into a fresh list.
            Some(Type::Class { .. }) if self.is_headers_class_type(init_ty)? => {
                Ok(format!("SmeltHeaders::from_pairs({init_text}.entries_sorted())"))
            }
            // `Record<string, string>`: each own entry is one header.
            Some(&Type::Dict(key, value))
                if matches!(self.mir.types.get(key), Some(Type::String))
                    && matches!(self.mir.types.get(value), Some(Type::String)) =>
            {
                Ok(format!(
                    "SmeltHeaders::from_pairs({init_text}.iter().map(|(smelt_name, smelt_value)| (smelt_name.clone(), smelt_value.clone())).collect::<Vec<(String, String)>>())"
                ))
            }
            // `[string, string][]`: pairs in order.
            Some(&Type::List(item)) if self.is_string_pair_type(item) => Ok(format!(
                "SmeltHeaders::from_pairs({init_text}.to_vec().into_iter().map(|smelt_pair| (smelt_pair.0, smelt_pair.1)).collect::<Vec<(String, String)>>())"
            )),
            // `string[][]`: the same initializer written without a tuple
            // annotation, which is what TypeScript infers for a bare
            // `[["a", "b"]]` literal. A row shorter than two entries has no
            // name/value pair to contribute and is skipped, as the spec's
            // "sequence of sequences" conversion rejects it.
            Some(&Type::List(item))
                if matches!(self.mir.types.get(item), Some(&Type::List(inner))
                    if matches!(self.mir.types.get(inner), Some(Type::String))) =>
            {
                Ok(format!(
                    "SmeltHeaders::from_pairs({init_text}.to_vec().into_iter().filter_map(|smelt_pair| {{ let smelt_pair = smelt_pair.to_vec(); Some((smelt_pair.first()?.clone(), smelt_pair.get(1)?.clone())) }}).collect::<Vec<(String, String)>>())"
                ))
            }
            _ => Err(EmitError::new(format!(
                "`new Headers(init)` initializer type is not modeled: {}",
                self.type_text_with_impl_trait(init_ty, false)?
            ))),
        }
    }

    /// Emit one `Headers` operation as a method call on the concrete value.
    ///
    /// Argument arity is checked here because the source surface fixes it: a
    /// missing name argument is a lowering bug, not a runtime `undefined`.
    pub(super) fn headers_op_text(
        &self,
        op: smelt_hir::HeadersOp,
        headers: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(headers)?;
        let string_ty = self.type_id(Type::String)?;
        let name = |index: usize| -> Result<String, EmitError> {
            let arg = args.get(index).ok_or_else(|| {
                EmitError::new(format!("`Headers` operation {op:?} is missing an argument"))
            })?;
            self.value_at_type(arg, string_ty)
        };
        let text = match op {
            smelt_hir::HeadersOp::Get => format!("{receiver}.get(&{})", name(0)?),
            smelt_hir::HeadersOp::Has => format!("{receiver}.has(&{})", name(0)?),
            smelt_hir::HeadersOp::Set => {
                format!("{receiver}.set(&{}, &{})", name(0)?, name(1)?)
            }
            smelt_hir::HeadersOp::Append => {
                format!("{receiver}.append(&{}, &{})", name(0)?, name(1)?)
            }
            smelt_hir::HeadersOp::Delete => format!("{receiver}.delete(&{})", name(0)?),
            smelt_hir::HeadersOp::Keys => format!("{receiver}.keys()"),
            smelt_hir::HeadersOp::Values => format!("{receiver}.values()"),
            smelt_hir::HeadersOp::Entries => format!("{receiver}.entries_sorted()"),
            smelt_hir::HeadersOp::GetSetCookie => format!("{receiver}.get_set_cookie()"),
        };
        // The mutating operations are `void` in the source, so their statement
        // form is the call itself; the reads coerce to their destination.
        match op {
            smelt_hir::HeadersOp::Set
            | smelt_hir::HeadersOp::Append
            | smelt_hir::HeadersOp::Delete => Ok(text),
            smelt_hir::HeadersOp::Get => {
                let string_option = self.type_id(Type::Optional(string_ty))?;
                self.value_at_type_text(&text, string_option, dest_ty)
            }
            smelt_hir::HeadersOp::Has => {
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            smelt_hir::HeadersOp::Keys
            | smelt_hir::HeadersOp::Values
            | smelt_hir::HeadersOp::GetSetCookie => {
                let list_ty = self.type_id(Type::List(string_ty))?;
                self.value_at_type_text(&format!("SmeltList::new({text})"), list_ty, dest_ty)
            }
            smelt_hir::HeadersOp::Entries => {
                let pair_ty = self.type_id(Type::Tuple(Vec::from([string_ty, string_ty])))?;
                let list_ty = self.type_id(Type::List(pair_ty))?;
                self.value_at_type_text(&format!("SmeltList::new({text})"), list_ty, dest_ty)
            }
        }
    }

    /// Emit `new URLSearchParams(init?)`.
    ///
    /// The initializer's static type selects the conversion: a string is a
    /// query to parse, a record or pair list is appended in order, and another
    /// `URLSearchParams` is copied (so the copy is a distinct JS object).
    pub(super) fn url_search_params_new_text(
        &self,
        initializer: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let Some(init) = initializer else {
            return Ok("SmeltUrlSearchParams::new()".to_owned());
        };
        let init_ty = self.operand_ty(init)?;
        let init_text = self.operand_text(init)?;
        match self.mir.types.get(init_ty) {
            Some(Type::String) => Ok(format!("SmeltUrlSearchParams::from_query(&{init_text})")),
            Some(Type::Class { .. }) if self.is_url_search_params_class_type(init_ty)? => Ok(
                format!("SmeltUrlSearchParams::from_pairs({init_text}.entries_in_order())"),
            ),
            Some(&Type::Dict(key, value))
                if matches!(self.mir.types.get(key), Some(Type::String))
                    && matches!(self.mir.types.get(value), Some(Type::String)) =>
            {
                Ok(format!(
                    "SmeltUrlSearchParams::from_pairs({init_text}.iter().map(|(smelt_name, smelt_value)| (smelt_name.clone(), smelt_value.clone())).collect::<Vec<(String, String)>>())"
                ))
            }
            Some(&Type::List(item)) if self.is_string_pair_type(item) => Ok(format!(
                "SmeltUrlSearchParams::from_pairs({init_text}.to_vec().into_iter().map(|smelt_pair| (smelt_pair.0, smelt_pair.1)).collect::<Vec<(String, String)>>())"
            )),
            Some(&Type::List(item))
                if matches!(self.mir.types.get(item), Some(&Type::List(inner))
                    if matches!(self.mir.types.get(inner), Some(Type::String))) =>
            {
                Ok(format!(
                    "SmeltUrlSearchParams::from_pairs({init_text}.to_vec().into_iter().filter_map(|smelt_pair| {{ let smelt_pair = smelt_pair.to_vec(); Some((smelt_pair.first()?.clone(), smelt_pair.get(1)?.clone())) }}).collect::<Vec<(String, String)>>())"
                ))
            }
            _ => Err(EmitError::new(format!(
                "`new URLSearchParams(init)` initializer type is not modeled: {}",
                self.type_text_with_impl_trait(init_ty, false)?
            ))),
        }
    }

    /// Emit one `URLSearchParams` operation as a method call.
    pub(super) fn url_search_params_op_text(
        &self,
        op: smelt_hir::UrlSearchParamsOp,
        params: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(params)?;
        let string_ty = self.type_id(Type::String)?;
        let name = |index: usize| -> Result<String, EmitError> {
            let arg = args.get(index).ok_or_else(|| {
                EmitError::new(format!(
                    "`URLSearchParams` operation {op:?} is missing an argument"
                ))
            })?;
            self.value_at_type(arg, string_ty)
        };
        let text = match op {
            smelt_hir::UrlSearchParamsOp::Get => format!("{receiver}.get(&{})", name(0)?),
            smelt_hir::UrlSearchParamsOp::GetAll => format!("{receiver}.get_all(&{})", name(0)?),
            smelt_hir::UrlSearchParamsOp::Has => format!("{receiver}.has(&{})", name(0)?),
            smelt_hir::UrlSearchParamsOp::Set => {
                format!("{receiver}.set(&{}, &{})", name(0)?, name(1)?)
            }
            smelt_hir::UrlSearchParamsOp::Append => {
                format!("{receiver}.append(&{}, &{})", name(0)?, name(1)?)
            }
            smelt_hir::UrlSearchParamsOp::Delete => format!("{receiver}.delete(&{})", name(0)?),
            smelt_hir::UrlSearchParamsOp::Sort => format!("{receiver}.sort()"),
            smelt_hir::UrlSearchParamsOp::ToText => format!("{receiver}.to_text()"),
            smelt_hir::UrlSearchParamsOp::Keys => format!("{receiver}.keys()"),
            smelt_hir::UrlSearchParamsOp::Values => format!("{receiver}.values()"),
            smelt_hir::UrlSearchParamsOp::Entries => format!("{receiver}.entries_in_order()"),
        };
        match op {
            smelt_hir::UrlSearchParamsOp::Set
            | smelt_hir::UrlSearchParamsOp::Append
            | smelt_hir::UrlSearchParamsOp::Delete
            | smelt_hir::UrlSearchParamsOp::Sort => Ok(text),
            smelt_hir::UrlSearchParamsOp::Get => {
                let string_option = self.type_id(Type::Optional(string_ty))?;
                self.value_at_type_text(&text, string_option, dest_ty)
            }
            smelt_hir::UrlSearchParamsOp::Has => {
                let bool_ty = self.type_id(Type::Bool)?;
                self.value_at_type_text(&text, bool_ty, dest_ty)
            }
            smelt_hir::UrlSearchParamsOp::ToText => {
                self.value_at_type_text(&text, string_ty, dest_ty)
            }
            smelt_hir::UrlSearchParamsOp::GetAll
            | smelt_hir::UrlSearchParamsOp::Keys
            | smelt_hir::UrlSearchParamsOp::Values => {
                let list_ty = self.type_id(Type::List(string_ty))?;
                self.value_at_type_text(&format!("SmeltList::new({text})"), list_ty, dest_ty)
            }
            smelt_hir::UrlSearchParamsOp::Entries => {
                let pair_ty = self.type_id(Type::Tuple(Vec::from([string_ty, string_ty])))?;
                let list_ty = self.type_id(Type::List(pair_ty))?;
                self.value_at_type_text(&format!("SmeltList::new({text})"), list_ty, dest_ty)
            }
        }
    }

    /// Emit a data-property read on a concrete `URLSearchParams` value.
    ///
    /// `size` is the only such property in the spec; an unmodeled property is a
    /// named blocker rather than a fabricated value.
    pub(super) fn url_search_params_field_text(
        &self,
        receiver_text: &str,
        field: Symbol,
    ) -> Result<String, EmitError> {
        match self.symbol_name(field)? {
            "size" => Ok(format!("{receiver_text}.size()")),
            other => Err(EmitError::new(format!(
                "`URLSearchParams.{other}` is not a modeled property"
            ))),
        }
    }

    /// Return whether a type is one of the concrete fetch runtime types.
    ///
    /// These are the classes whose state is a shared cell rather than declared
    /// fields, so they cross the dynamic boundary through their own
    /// `IntoSmeltUnknown`/`SmeltFromUnknown` adapters instead of the generic
    /// struct record builder.
    pub(super) fn is_fetch_runtime_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(matches!(
            self.stdlib_class_of_symbol(*name)?,
            Some(smelt_stdlib::StdlibClass::Headers | smelt_stdlib::StdlibClass::UrlSearchParams)
        ))
    }

    /// Return whether a type names the generated `SmeltUrlSearchParams` type.
    pub(super) fn is_url_search_params_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)?
            == Some(smelt_stdlib::StdlibClass::UrlSearchParams))
    }

    /// Return whether a type names the generated `SmeltHeaders` runtime type.
    pub(super) fn is_headers_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Headers))
    }

    /// Return whether a type is a `[string, string]` tuple.
    fn is_string_pair_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::Tuple(items))
            if items.len() == 2
                && items
                    .iter()
                    .all(|item| matches!(self.mir.types.get(*item), Some(Type::String))))
    }

    /// Resolve a class symbol to its shared stdlib class identity, if any.
    pub(super) fn stdlib_class_of_symbol(
        &self,
        name: Symbol,
    ) -> Result<Option<smelt_stdlib::StdlibClass>, EmitError> {
        Ok(smelt_stdlib::typescript_stdlib_class(self.symbol_name(name)?))
    }
}
