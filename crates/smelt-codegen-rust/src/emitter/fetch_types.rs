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
        self.headers_conversion_text(&init_text, init_ty)
    }

    /// Convert a value of type `ty`, named by `init_text`, into a `SmeltHeaders`.
    ///
    /// The `HeadersInit` conversion, selected by the value's static type. Split
    /// from [`Self::headers_new_text`] so a `headers` init key read off a typed
    /// `ResponseInit` — where the value arrives already unwrapped from an
    /// `Option` and so has no operand of its own — uses the same conversion
    /// rather than a second copy of it.
    pub(super) fn headers_conversion_text(
        &self,
        init_text: &str,
        init_ty: TypeId,
    ) -> Result<String, EmitError> {
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
            Some(
                smelt_stdlib::StdlibClass::Headers
                    | smelt_stdlib::StdlibClass::UrlSearchParams
                    | smelt_stdlib::StdlibClass::Response
                    | smelt_stdlib::StdlibClass::Request
            )
        ))
    }

    /// Emit `new Request(input, init?)` as a concrete `SmeltRequest`.
    pub(super) fn request_new_text(
        &self,
        input: &Operand,
        method: Option<&Operand>,
        headers: Option<&Operand>,
        body: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let input_ty = self.operand_ty(input)?;
        if !matches!(self.mir.types.get(input_ty), Some(Type::String)) {
            return Err(EmitError::new(
                "Request input must be a URL string; a Request input is not modeled yet",
            ));
        }
        let input_text = self.operand_text(input)?;
        let method_expr = self.init_scalar_text(method, "\"GET\".to_owned()")?;
        let headers_expr = self.init_headers_text(headers)?;
        let body_expr = self.init_body_text(body)?;
        Ok(format!(
            "SmeltRequest::from_parts(&{input_text}, {method_expr}, {headers_expr}, {body_expr})"
        ))
    }

    /// Emit a `Request` member operation on a concrete receiver.
    pub(super) fn request_op_text(
        &self,
        op: smelt_hir::RequestOp,
        request: &Operand,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        if !args.is_empty() {
            return Err(EmitError::new("no modeled Request member takes arguments"));
        }
        let receiver = self.operand_text(request)?;
        Ok(match op {
            smelt_hir::RequestOp::Url => format!("{receiver}.url()"),
            smelt_hir::RequestOp::Method => format!("{receiver}.method()"),
            smelt_hir::RequestOp::Headers => format!("{receiver}.headers()"),
            smelt_hir::RequestOp::BodyUsed => format!("{receiver}.body_used()"),
            smelt_hir::RequestOp::Clone => format!("{receiver}.tee()"),
            // Same handle-clone-into-the-block shape as `Response::text`; see
            // the comment there for why the receiver is not moved.
            smelt_hir::RequestOp::Text => format!(
                "{{ let smelt_request = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>(smelt_request.take_text()?) }})) }}"
            ),
        })
    }

    /// Emit `new Response(body?, init?)` as a concrete `SmeltResponse`.
    ///
    /// Each init key is its own operand (see `Rvalue::ResponseNew`), so the
    /// value is assembled from typed parts. An absent key takes the spec's
    /// default: 200, an empty reason phrase, an empty header list.
    pub(super) fn response_new_text(
        &self,
        body: Option<&Operand>,
        status: Option<&Operand>,
        status_text: Option<&Operand>,
        headers: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let status_text_expr = self.init_scalar_text(status, "200.0")?;
        let phrase_expr = self.init_scalar_text(status_text, "String::new()")?;
        let headers_expr = self.init_headers_text(headers)?;
        let body_expr = self.init_body_text(body)?;
        Ok(format!(
            "SmeltResponse::from_parts({status_text_expr}, {phrase_expr}, {headers_expr}, {body_expr})"
        ))
    }

    /// Build a `SmeltBody` from a `Response`/`Request` body argument.
    ///
    /// The argument's static type selects the conversion, exactly as the
    /// `Headers` initializer does. Only a string body is modeled so far —
    /// `BodyInit`'s other arms (`Blob`, `FormData`, `URLSearchParams`,
    /// `ReadableStream`, `BufferSource`) are types Smelt does not model yet, and
    /// silently treating one as text would put wrong bytes in the body.
    pub(super) fn response_body_text(&self, body: &Operand) -> Result<String, EmitError> {
        let body_ty = self.operand_ty(body)?;
        let body_text = self.operand_text(body)?;
        self.body_conversion_text(&body_text, body_ty)
    }

    /// Convert a value of type `ty`, named by `body_text`, into a `SmeltBody`.
    ///
    /// Split from [`Self::response_body_text`] for the same reason the headers
    /// conversion is: a `body` init key read off a typed `RequestInit` arrives
    /// already unwrapped and has no operand of its own.
    pub(super) fn body_conversion_text(
        &self,
        body_text: &str,
        body_ty: TypeId,
    ) -> Result<String, EmitError> {
        // A `Request` at the body position is the WHATWG "copy the source's
        // body" step: take its HANDLE, so consuming the new request's body is
        // observable on the source, exactly as Node reports it.
        if self.is_request_class_type(body_ty)? {
            return Ok(format!("{body_text}.body()"));
        }
        match self.mir.types.get(body_ty) {
            Some(Type::String) => Ok(format!("SmeltBody::from_text(&{body_text})")),
            Some(Type::None) => Ok("SmeltBody::empty()".to_owned()),
            _ => Err(EmitError::new(format!(
                "body must be a string or null; this `BodyInit` arm is not modeled yet: {}",
                self.type_text_with_impl_trait(body_ty, false)?
            ))),
        }
    }

    /// Emit a `Response` member operation on a concrete receiver.
    pub(super) fn response_op_text(
        &self,
        op: smelt_hir::ResponseOp,
        response: &Operand,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        if !args.is_empty() {
            return Err(EmitError::new(
                "no modeled Response member takes arguments",
            ));
        }
        let receiver = self.operand_text(response)?;
        Ok(match op {
            smelt_hir::ResponseOp::Status => format!("{receiver}.status()"),
            smelt_hir::ResponseOp::Ok => format!("{receiver}.ok()"),
            smelt_hir::ResponseOp::StatusText => format!("{receiver}.status_text()"),
            smelt_hir::ResponseOp::Headers => format!("{receiver}.headers()"),
            smelt_hir::ResponseOp::BodyUsed => format!("{receiver}.body_used()"),
            smelt_hir::ResponseOp::Clone => format!("{receiver}.tee()"),
            // `text()` answers a promise, so it is a future here, and the
            // future is fallible because a second read is the spec's
            // `TypeError`. The `?` inside the async block puts that on the same
            // error channel every other throw uses.
            //
            // The receiver is moved into the block through a HANDLE clone taken
            // outside it. Moving the receiver itself would end its lifetime at
            // the call, so `response.bodyUsed` after `response.text()` would
            // not compile; and a handle clone is the semantically right copy
            // anyway — it shares the payload and the used flag, so consuming
            // the body through the future is observable on the original, which
            // is exactly what the spec says happens.
            smelt_hir::ResponseOp::Text => format!(
                "{{ let smelt_response = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>(smelt_response.take_text()?) }})) }}"
            ),
        })
    }

    /// Return whether a type names the generated `SmeltUrlSearchParams` type.
    pub(super) fn is_url_search_params_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)?
            == Some(smelt_stdlib::StdlibClass::UrlSearchParams))
    }

    /// Return whether a type names the generated `SmeltRequest` runtime type.
    pub(super) fn is_request_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Request))
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
    /// Render a scalar init key, falling back to the spec's default.
    ///
    /// An absent key and a key whose value is `undefined` are the same thing to
    /// the spec, so `Optional<T>` unwraps to the same default a missing operand
    /// uses. `default_text` is that default, written once per key.
    fn init_scalar_text(
        &self,
        operand: Option<&Operand>,
        default_text: &str,
    ) -> Result<String, EmitError> {
        let Some(operand) = operand else {
            return Ok(default_text.to_owned());
        };
        let text = self.operand_text(operand)?;
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Optional(_))
        ) {
            return Ok(format!("{text}.unwrap_or_else(|| {default_text})"));
        }
        Ok(text)
    }

    /// Render the `headers` init key as a `SmeltHeaders`.
    ///
    /// The conversion for a present value is the `Headers` constructor's own
    /// (a `Headers`, a record, or an array of pairs — selected by the
    /// operand's type), so the init and `new Headers(init)` cannot disagree.
    fn init_headers_text(&self, operand: Option<&Operand>) -> Result<String, EmitError> {
        let Some(operand) = operand else {
            return Ok("SmeltHeaders::new()".to_owned());
        };
        let ty = self.operand_ty(operand)?;
        if let Some(&Type::Optional(inner)) = self.mir.types.get(ty) {
            let present = self.headers_conversion_text("smelt_init_headers", inner)?;
            return Ok(format!(
                "match {} {{ Some(smelt_init_headers) => {present}, None => SmeltHeaders::new() }}",
                self.operand_text(operand)?
            ));
        }
        self.headers_new_text(Some(operand))
    }

    /// Render the `body` init key as a `SmeltBody`.
    fn init_body_text(&self, operand: Option<&Operand>) -> Result<String, EmitError> {
        let Some(operand) = operand else {
            return Ok("SmeltBody::empty()".to_owned());
        };
        let ty = self.operand_ty(operand)?;
        if let Some(&Type::Optional(inner)) = self.mir.types.get(ty) {
            let present = self.body_conversion_text("smelt_init_body", inner)?;
            return Ok(format!(
                "match {} {{ Some(smelt_init_body) => {present}, None => SmeltBody::empty() }}",
                self.operand_text(operand)?
            ));
        }
        self.response_body_text(operand)
    }

}
