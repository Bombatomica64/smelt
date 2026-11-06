//! Rust emission for the WHATWG fetch types.
//!
//! One emitter per generated fetch runtime type (`SmeltHeaders` today). Every
//! operation lands on a real method of the concrete struct emitted by
//! `crate::fetch_types_prelude`, so the generated call is what a hand-written
//! Rust program would say: `headers.get("content-type")` is
//! `headers.get("content-type")`, typed `Option<String>`, with no tagged value
//! and no runtime member lookup in between.

use crate::rust::RustExpr;
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
    pub(super) fn headers_new_expr(
        &self,
        initializer: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let Some(init) = initializer else {
            return Ok(RustExpr::atom("SmeltHeaders::new()"));
        };
        let init_ty = self.operand_ty(init)?;
        let init_text = self.operand_expr(init)?;
        self.headers_conversion_expr(&init_text, init_ty)
    }

    /// Convert a value of type `ty`, named by `init_text`, into a `SmeltHeaders`.
    ///
    /// The `HeadersInit` conversion, selected by the value's static type. Split
    /// from [`Self::headers_new_expr`] so a `headers` init key read off a typed
    /// `ResponseInit` — where the value arrives already unwrapped from an
    /// `Option` and so has no operand of its own — uses the same conversion
    /// rather than a second copy of it.
    pub(super) fn headers_conversion_expr(
        &self,
        init_text: &RustExpr,
        init_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match self.mir.types.get(init_ty) {
            // Another `Headers`: copy its pairs into a fresh list.
            Some(Type::Class { .. }) if self.is_headers_class_type(init_ty)? => {
                Ok(RustExpr::atom(format!("SmeltHeaders::from_pairs({init_text}.entries_sorted())")))
            }
            // `Record<string, string>`: each own entry is one header.
            Some(&Type::Dict(key, value))
                if matches!(self.mir.types.get(key), Some(Type::String))
                    && matches!(self.mir.types.get(value), Some(Type::String)) =>
            {
                Ok(RustExpr::atom(format!(
                    "SmeltHeaders::from_pairs({init_text}.iter().map(|(smelt_name, smelt_value)| (smelt_name.clone(), smelt_value.clone())).collect::<Vec<(String, String)>>())"
                )))
            }
            // `Record<string, string | undefined>`: each own entry is one
            // header, and WebIDL's record-to-ByteString conversion runs
            // ToString on every value, so an `undefined` entry is the header
            // value `"undefined"` (Node agrees). This is the shape a
            // `{ 'Content-Type': contentType }` literal takes when its value's
            // narrowing is not carried into the record type (Hono's
            // `#cachedBody` rebuild).
            Some(&Type::Dict(key, value))
                if matches!(self.mir.types.get(key), Some(Type::String))
                    && matches!(self.mir.types.get(value), Some(&Type::Optional(inner))
                        if matches!(self.mir.types.get(inner), Some(Type::String))) =>
            {
                Ok(RustExpr::atom(format!(
                    "SmeltHeaders::from_pairs({init_text}.iter().map(|(smelt_name, smelt_value)| (smelt_name.clone(), smelt_value.clone().unwrap_or_else(|| \"undefined\".to_owned()))).collect::<Vec<(String, String)>>())"
                )))
            }
            // `[string, string][]`: pairs in order.
            Some(&Type::List(item)) if self.is_string_pair_type(item) => Ok(RustExpr::atom(format!(
                "SmeltHeaders::from_pairs({init_text}.to_vec().into_iter().map(|smelt_pair| (smelt_pair.0, smelt_pair.1)).collect::<Vec<(String, String)>>())"
            ))),
            // `string[][]`: the same initializer written without a tuple
            // annotation, which is what TypeScript infers for a bare
            // `[["a", "b"]]` literal. A row shorter than two entries has no
            // name/value pair to contribute and is skipped, as the spec's
            // "sequence of sequences" conversion rejects it.
            Some(&Type::List(item))
                if matches!(self.mir.types.get(item), Some(&Type::List(inner))
                    if matches!(self.mir.types.get(inner), Some(Type::String))) =>
            {
                Ok(RustExpr::atom(format!(
                    "SmeltHeaders::from_pairs({init_text}.to_vec().into_iter().filter_map(|smelt_pair| {{ let smelt_pair = smelt_pair.to_vec(); Some((smelt_pair.first()?.clone(), smelt_pair.get(1)?.clone())) }}).collect::<Vec<(String, String)>>())"
                )))
            }
            // A union initializer. `HeadersInit` IS a union in the spec
            // (`Headers | string[][] | Record<string, string>`), so source that
            // keeps it as one — Hono's `ResponseHeadersInit`, or the join of an
            // init arm's `headers` with a `Response` arm's — hands `new Headers`
            // a value whose ARM is decided at runtime while every arm's
            // conversion is decided statically. A generated union is a tagged
            // enum, so matching it recovers the arm and runs that arm's own
            // conversion, which is what a hand-writing Rust team would do.
            // Erasing the union to `SmeltUnknown` and inspecting the tag would
            // answer the same question with the static type thrown away, and
            // `SmeltHeaders` has no erased constructor to fall back to.
            Some(Type::Union(_)) if self.concrete_union_members(init_ty).is_some() => {
                let members: Vec<TypeId> = self
                    .concrete_union_members(init_ty)
                    .unwrap_or_default()
                    .to_vec();
                let union = union::union_name(init_ty);
                let arms = members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        let converted =
                            self.headers_conversion_expr(&RustExpr::path("smelt_headers_init"), *member)?;
                        Ok(format!("{union}::M{index}(smelt_headers_init) => {converted}"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?;
                Ok(RustExpr::atom(format!("match {init_text} {{ {} }}", arms.join(", "))))
            }
            // An initializer that may be absent. WHATWG's constructor takes
            // `HeadersInit?`, and `new Headers(undefined)` is the empty header
            // list — the same answer the no-argument spelling gives — so the
            // absent arm is a modeled case rather than a blocker. This is the
            // shape a `headers?:` init key arrives in when the source narrowed
            // it by truthiness without Smelt proving the narrowing.
            Some(&Type::Optional(inner)) => {
                let converted = self.headers_conversion_expr(&RustExpr::path("smelt_headers_init"), inner)?;
                Ok(RustExpr::atom(format!(
                    "match {init_text} {{ Some(smelt_headers_init) => {converted}, None => SmeltHeaders::new() }}"
                )))
            }
            // The function this stopped in is attached by the emission entry
            // points now (`EmitError::with_site`), so the message carries only
            // what it knows: the type it could not model and the initializer
            // text. It used to name the site itself, which is where that
            // mechanism came from.
            _ => Err(EmitError::new(format!(
                "`new Headers(init)` initializer type is not modeled: {} (initializer `{init_text}`)",
                self.rendered_type(init_ty)?,
            ))),
        }
    }

    /// Whether a value of this type is a `HeadersInit` the spec's conversion models.
    ///
    /// The predicate half of [`Self::headers_conversion_expr`]: it admits only the
    /// Dict and List shapes of `HeadersInit`, a subset of what the conversion
    /// accepts (the conversion also takes union and optional initializers), so a
    /// coercion seam can ask whether a conversion exists before committing to
    /// it. A value that already IS a `Headers` is excluded —
    /// there is nothing to convert, and the ordinary identity path answers it.
    pub(super) fn is_headers_init_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        if self.is_headers_class_type(ty)? {
            return Ok(false);
        }
        Ok(match self.mir.types.get(ty) {
            // `Record<string, string>`.
            Some(&Type::Dict(key, value)) => {
                matches!(self.mir.types.get(key), Some(Type::String))
                    && matches!(self.mir.types.get(value), Some(Type::String))
            }
            // `[string, string][]`, and the `string[][]` a bare literal infers.
            Some(&Type::List(item)) => {
                self.is_string_pair_type(item)
                    || matches!(self.mir.types.get(item), Some(&Type::List(inner))
                        if matches!(self.mir.types.get(inner), Some(Type::String)))
            }
            _ => false,
        })
    }

    /// Emit one `Headers` operation as a method call on the concrete value.
    ///
    /// Argument arity is checked here because the source surface fixes it: a
    /// missing name argument is a lowering bug, not a runtime `undefined`.
    pub(super) fn headers_op_expr(
        &self,
        op: smelt_hir::HeadersOp,
        headers: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(headers)?;
        let string_ty = self.type_id(Type::String)?;
        let name = |index: usize| -> Result<String, EmitError> {
            let arg = args.get(index).ok_or_else(|| {
                EmitError::new(format!("`Headers` operation {op:?} is missing an argument"))
            })?;
            self.value_at_type(arg, string_ty).map(RustExpr::into_string)
        };
        let text = match op {
            smelt_hir::HeadersOp::Get => RustExpr::postfix_on(&receiver, format!("{receiver}.get(&{})", name(0)?)),
            smelt_hir::HeadersOp::Has => RustExpr::postfix_on(&receiver, format!("{receiver}.has(&{})", name(0)?)),
            smelt_hir::HeadersOp::Set => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.set(&{}, &{})", name(0)?, name(1)?))
            }
            smelt_hir::HeadersOp::Append => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.append(&{}, &{})", name(0)?, name(1)?))
            }
            smelt_hir::HeadersOp::Delete => RustExpr::postfix_on(&receiver, format!("{receiver}.delete(&{})", name(0)?)),
            smelt_hir::HeadersOp::Keys => RustExpr::postfix_on(&receiver, format!("{receiver}.keys()")),
            smelt_hir::HeadersOp::Values => RustExpr::postfix_on(&receiver, format!("{receiver}.values()")),
            smelt_hir::HeadersOp::Entries => RustExpr::postfix_on(&receiver, format!("{receiver}.entries_sorted()")),
            smelt_hir::HeadersOp::GetSetCookie => RustExpr::postfix_on(&receiver, format!("{receiver}.get_set_cookie()")),
        };
        // The mutating operations are `void` in the source, so their statement
        // form is the call itself; the reads coerce to their destination.
        match op {
            smelt_hir::HeadersOp::Set
            | smelt_hir::HeadersOp::Append
            | smelt_hir::HeadersOp::Delete => Ok(text),
            smelt_hir::HeadersOp::Get => {
                let string_option = self.type_id(Type::Optional(string_ty))?;
                self.rendered_at_type(&text, string_option, dest_ty, &self.render_scope())
            }
            smelt_hir::HeadersOp::Has => {
                let bool_ty = self.type_id(Type::Bool)?;
                self.rendered_at_type(&text, bool_ty, dest_ty, &self.render_scope())
            }
            smelt_hir::HeadersOp::Keys
            | smelt_hir::HeadersOp::Values
            | smelt_hir::HeadersOp::GetSetCookie => {
                let list_ty = self.type_id(Type::List(string_ty))?;
                self.rendered_at_type(&RustExpr::atom(format!("SmeltList::new({text})")), list_ty, dest_ty, &self.render_scope())
            }
            smelt_hir::HeadersOp::Entries => {
                let pair_ty = self.type_id(Type::Tuple(Vec::from([string_ty, string_ty])))?;
                let list_ty = self.type_id(Type::List(pair_ty))?;
                self.rendered_at_type(&RustExpr::atom(format!("SmeltList::new({text})")), list_ty, dest_ty, &self.render_scope())
            }
        }
    }

    /// Emit `new URLSearchParams(init?)`.
    ///
    /// The initializer's static type selects the conversion: a string is a
    /// query to parse, a record or pair list is appended in order, and another
    /// `URLSearchParams` is copied (so the copy is a distinct JS object).
    pub(super) fn url_search_params_new_expr(
        &self,
        initializer: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let Some(init) = initializer else {
            return Ok(RustExpr::atom("SmeltUrlSearchParams::new()"));
        };
        let init_ty = self.operand_ty(init)?;
        let init_text = self.operand_expr(init)?;
        match self.mir.types.get(init_ty) {
            Some(Type::String) => Ok(RustExpr::atom(format!("SmeltUrlSearchParams::from_query(&{init_text})"))),
            Some(Type::Class { .. }) if self.is_url_search_params_class_type(init_ty)? => Ok(
                RustExpr::atom(format!("SmeltUrlSearchParams::from_pairs({init_text}.entries_in_order())")),
            ),
            Some(&Type::Dict(key, value))
                if matches!(self.mir.types.get(key), Some(Type::String))
                    && matches!(self.mir.types.get(value), Some(Type::String)) =>
            {
                Ok(RustExpr::atom(format!(
                    "SmeltUrlSearchParams::from_pairs({init_text}.iter().map(|(smelt_name, smelt_value)| (smelt_name.clone(), smelt_value.clone())).collect::<Vec<(String, String)>>())"
                )))
            }
            Some(&Type::List(item)) if self.is_string_pair_type(item) => Ok(RustExpr::atom(format!(
                "SmeltUrlSearchParams::from_pairs({init_text}.to_vec().into_iter().map(|smelt_pair| (smelt_pair.0, smelt_pair.1)).collect::<Vec<(String, String)>>())"
            ))),
            Some(&Type::List(item))
                if matches!(self.mir.types.get(item), Some(&Type::List(inner))
                    if matches!(self.mir.types.get(inner), Some(Type::String))) =>
            {
                Ok(RustExpr::atom(format!(
                    "SmeltUrlSearchParams::from_pairs({init_text}.to_vec().into_iter().filter_map(|smelt_pair| {{ let smelt_pair = smelt_pair.to_vec(); Some((smelt_pair.first()?.clone(), smelt_pair.get(1)?.clone())) }}).collect::<Vec<(String, String)>>())"
                )))
            }
            _ => Err(EmitError::new(format!(
                "`new URLSearchParams(init)` initializer type is not modeled: {}",
                self.rendered_type(init_ty)?
            ))),
        }
    }

    /// Emit one `URLSearchParams` operation as a method call.
    pub(super) fn url_search_params_op_expr(
        &self,
        op: smelt_hir::UrlSearchParamsOp,
        params: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(params)?;
        let string_ty = self.type_id(Type::String)?;
        let name = |index: usize| -> Result<String, EmitError> {
            let arg = args.get(index).ok_or_else(|| {
                EmitError::new(format!(
                    "`URLSearchParams` operation {op:?} is missing an argument"
                ))
            })?;
            self.value_at_type(arg, string_ty).map(RustExpr::into_string)
        };
        let text = match op {
            smelt_hir::UrlSearchParamsOp::Get => RustExpr::postfix_on(&receiver, format!("{receiver}.get(&{})", name(0)?)),
            smelt_hir::UrlSearchParamsOp::GetAll => RustExpr::postfix_on(&receiver, format!("{receiver}.get_all(&{})", name(0)?)),
            smelt_hir::UrlSearchParamsOp::Has => RustExpr::postfix_on(&receiver, format!("{receiver}.has(&{})", name(0)?)),
            smelt_hir::UrlSearchParamsOp::Set => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.set(&{}, &{})", name(0)?, name(1)?))
            }
            smelt_hir::UrlSearchParamsOp::Append => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.append(&{}, &{})", name(0)?, name(1)?))
            }
            smelt_hir::UrlSearchParamsOp::Delete => RustExpr::postfix_on(&receiver, format!("{receiver}.delete(&{})", name(0)?)),
            smelt_hir::UrlSearchParamsOp::Sort => RustExpr::postfix_on(&receiver, format!("{receiver}.sort()")),
            smelt_hir::UrlSearchParamsOp::ToText => RustExpr::postfix_on(&receiver, format!("{receiver}.to_text()")),
            smelt_hir::UrlSearchParamsOp::Keys => RustExpr::postfix_on(&receiver, format!("{receiver}.keys()")),
            smelt_hir::UrlSearchParamsOp::Values => RustExpr::postfix_on(&receiver, format!("{receiver}.values()")),
            smelt_hir::UrlSearchParamsOp::Entries => RustExpr::postfix_on(&receiver, format!("{receiver}.entries_in_order()")),
        };
        match op {
            smelt_hir::UrlSearchParamsOp::Set
            | smelt_hir::UrlSearchParamsOp::Append
            | smelt_hir::UrlSearchParamsOp::Delete
            | smelt_hir::UrlSearchParamsOp::Sort => Ok(text),
            smelt_hir::UrlSearchParamsOp::Get => {
                let string_option = self.type_id(Type::Optional(string_ty))?;
                self.rendered_at_type(&text, string_option, dest_ty, &self.render_scope())
            }
            smelt_hir::UrlSearchParamsOp::Has => {
                let bool_ty = self.type_id(Type::Bool)?;
                self.rendered_at_type(&text, bool_ty, dest_ty, &self.render_scope())
            }
            smelt_hir::UrlSearchParamsOp::ToText => {
                self.rendered_at_type(&text, string_ty, dest_ty, &self.render_scope())
            }
            smelt_hir::UrlSearchParamsOp::GetAll
            | smelt_hir::UrlSearchParamsOp::Keys
            | smelt_hir::UrlSearchParamsOp::Values => {
                let list_ty = self.type_id(Type::List(string_ty))?;
                self.rendered_at_type(&RustExpr::atom(format!("SmeltList::new({text})")), list_ty, dest_ty, &self.render_scope())
            }
            smelt_hir::UrlSearchParamsOp::Entries => {
                let pair_ty = self.type_id(Type::Tuple(Vec::from([string_ty, string_ty])))?;
                let list_ty = self.type_id(Type::List(pair_ty))?;
                self.rendered_at_type(&RustExpr::atom(format!("SmeltList::new({text})")), list_ty, dest_ty, &self.render_scope())
            }
        }
    }

    /// Emit a data-property read on a concrete `URLSearchParams` value.
    ///
    /// `size` is the only such property in the spec; an unmodeled property is a
    /// named blocker rather than a fabricated value.
    pub(super) fn url_search_params_field_expr(
        &self,
        receiver_text: &RustExpr,
        field: Symbol,
    ) -> Result<RustExpr, EmitError> {
        match self.symbol_name(field)? {
            "size" => Ok(RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.size()"))),
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
        Ok(self
            .stdlib_class_of_symbol(*name)?
            .is_some_and(smelt_stdlib::StdlibClass::erases_through_adapter))
    }

    /// Emit `new Request(input, init?)` as a concrete `SmeltRequest`.
    pub(super) fn request_new_expr(
        &self,
        input: &Operand,
        method: Option<&Operand>,
        headers: Option<&Operand>,
        body: Option<&Operand>,
        signal: Option<&Operand>,
        init_members: &[(smelt_hir::RequestInitMember, Operand)],
    ) -> Result<RustExpr, EmitError> {
        // The spec's "if init is not empty" test decides whether a `Request`
        // input's referrer survives a re-construction: by the WHATWG text an
        // absent or empty init (`new Request(src)`, `new Request(src, {})`)
        // copies it, and Node 22 (undici 6) agreed.
        //
        // The pinned oracle is Node 24, whose undici 7 converts EVERY init
        // through a `RequestInit` dictionary that defaults `priority` to
        // `'auto'` -- so the converted init is never empty, and every
        // re-construction from a `Request` resets `referrer` to
        // `about:client` and `referrerPolicy` to `''`. Node is the oracle the
        // corpus is checked against (`verify_end_to_end_example`), so the
        // generated program answers what Node 24 answers; the example
        // `92_request_init_members` records the version dependence. (For a
        // STRING input the reset is a no-op -- the defaults are already the
        // reset values -- so only the copy path observes this, and the string
        // paths keep the spelled-keys test their emitted text was built on.)
        let source_init_resets_referrer = true;
        let init_is_non_empty = method.is_some()
            || headers.is_some()
            || body.is_some()
            || signal.is_some()
            || !init_members.is_empty();
        let input_ty = self.operand_ty(input)?;
        // The spec's `RequestInfo` is `Request | string`. A REQUEST input is
        // the "copy the source" form: the new request starts from the source's
        // url, method, headers and body, and each init key that is present
        // overrides its slot. So the input decides the DEFAULTS rather than
        // needing a second constructor — which is also why the body default is
        // the source's `body()` handle: the spec SHARES it, so consuming the
        // copy's body is observable on the source, exactly as the
        // init-position `Request` already behaves.
        let request_text = if self.is_request_class_type(input_ty)? {
            let source = self.operand_expr(input)?;
            self.request_from_parts_expr(
                method,
                headers,
                body,
                &RustExpr::postfix_on(&source, format!("{source}.url()")),
                &format!("{source}.method()"),
                &format!("{source}.headers()"),
                &format!("SmeltBody::take_from_source(&{source}.body())"),
                &format!("{source}.init()"),
                init_members,
                source_init_resets_referrer,
            )?
        } else if matches!(self.mir.types.get(input_ty), Some(Type::String)) {
            self.request_from_parts_expr(
                method,
                headers,
                body,
                &self.operand_expr(input)?,
                "\"GET\".to_owned()",
                "SmeltHeaders::new()",
                "SmeltBody::empty()",
                "SmeltRequestInit::default()",
                init_members,
                init_is_non_empty,
            )?
        } else {
            // **Dynamic boundary.** `RequestInfo` is `Request | string`, and a
            // parameter declared as it (`input: string | Request | URL`, Hono's
            // `app.request`) has no concrete Rust spelling: which arm it holds
            // is a run-time fact. Both arms are distinguishable by tag and only
            // by tag, so the choice is made once, here, on the erased value —
            // a request RECORD recovers through the class's own boundary
            // adapter and takes the copy path; anything else is stringified,
            // which is what JavaScript does for a non-`Request` input and is
            // why `new Request(urlObject)` works at all. A concrete type cannot
            // stand in: the erasure is the parameter's own declared type.
            let source_request = self.request_from_parts_expr(
                method,
                headers,
                body,
                &RustExpr::chain("smelt_input_request.url()"),
                "smelt_input_request.method()",
                "smelt_input_request.headers()",
                "SmeltBody::take_from_source(&smelt_input_request.body())",
                "smelt_input_request.init()",
                init_members,
                source_init_resets_referrer,
            )?;
            let source_url = self.request_from_parts_expr(
                method,
                headers,
                body,
                &RustExpr::path("smelt_input_url"),
                "\"GET\".to_owned()",
                "SmeltHeaders::new()",
                "SmeltBody::empty()",
                "SmeltRequestInit::default()",
                init_members,
                init_is_non_empty,
            )?;
            let erased = self.erase(input)?;
            let coerced =
                self.js_string_coercion_match_expr(&RustExpr::path("smelt_input"), self.absent_spelling());
            RustExpr::atom(self.carrier_format(format_args!(
                "match {erased} {{ \
                 SmeltUnknown::Object(value) if value.contains_key(\"{SMELT_REQUEST}\") => {{ \
                 let smelt_input_request = <SmeltRequest as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(value)); \
                 {source_request} }}, \
                 smelt_input => {{ let smelt_input_url = {coerced}; {source_url} }} }}"
            )))
        };
        // `init.signal` does not become the request's signal: the spec makes
        // the request's a DEPENDENT signal that follows the given one, which is
        // why this registers a follow rather than storing the operand.
        let Some(signal) = signal else {
            return Ok(request_text);
        };
        Ok(RustExpr::atom(format!(
            "{{ let smelt_request = {request_text}; smelt_request_follow_signal(smelt_request.id(), {}); smelt_request }}",
            self.erase(signal)?
        )))
    }

    /// Assemble `SmeltRequest::from_parts` from the init keys and the defaults
    /// an ABSENT key means.
    ///
    /// Split out because the defaults differ by input form and by nothing else:
    /// a string input defaults to the spec's own values, a `Request` input
    /// defaults to the source's, and an erased input picks between the two at
    /// run time — so the assembly is one function taking four default
    /// expressions rather than three near-copies of it.
    fn request_from_parts_expr(
        &self,
        method: Option<&Operand>,
        headers: Option<&Operand>,
        body: Option<&Operand>,
        url_text: &RustExpr,
        method_default: &str,
        headers_default: &str,
        body_default: &str,
        init_default: &str,
        init_members: &[(smelt_hir::RequestInitMember, Operand)],
        init_is_non_empty: bool,
    ) -> Result<RustExpr, EmitError> {
        let method_expr = self.init_scalar_expr(method, &RustExpr::opaque(&method_default))?;
        let headers_expr = self.init_headers_expr(headers, &RustExpr::opaque(&headers_default))?;
        // A `Request` at the REQUEST INIT's `body` slot shares its handle;
        // see the flag's documentation for the Node measurement behind it.
        let body_expr = self.init_body_expr(body, &RustExpr::opaque(&body_default), true)?;
        let init_expr = self.request_init_expr(init_default, init_members, init_is_non_empty)?;
        Ok(RustExpr::atom(format!(
            "SmeltRequest::from_parts_with_init(&{url_text}, {method_expr}, {headers_expr}, {body_expr}, {init_expr})"
        )))
    }

    /// Assemble the `SmeltRequestInit` a constructed request stores.
    ///
    /// `init_default` is where an ABSENT key's value comes from: the spec's own
    /// defaults for a string input, and the source request's stored group for a
    /// `Request` input, which is the spec's "copy the source" form.
    ///
    /// Two adjustments sit on top of that copy, in the spec's order:
    ///
    /// * a NON-EMPTY init puts the referrer and the referrer policy back to
    ///   their defaults, because a request being re-initialized must not
    ///   inherit the source's referrer (`RequestInitMember::
    ///   resets_on_non_empty_init`);
    /// * each key the init actually supplied overrides its slot, referrer
    ///   included, and is serialized the way the spec stores it.
    fn request_init_expr(
        &self,
        init_default: &str,
        init_members: &[(smelt_hir::RequestInitMember, Operand)],
        init_is_non_empty: bool,
    ) -> Result<RustExpr, EmitError> {
        let resets: Vec<smelt_hir::RequestInitMember> = if init_is_non_empty {
            smelt_hir::RequestInitMember::ALL
                .into_iter()
                .filter(|member| member.resets_on_non_empty_init())
                .collect()
        } else {
            Vec::new()
        };
        if resets.is_empty() && init_members.is_empty() {
            return Ok(RustExpr::from(init_default.to_owned()));
        }
        let mut statements = String::new();
        for member in resets {
            statements.push_str(&format!(
                "smelt_request_init.{} = {}; ",
                member.field_name(),
                member.default_rust_literal()
            ));
        }
        for (member, operand) in init_members {
            let target = if member.is_boolean() {
                self.type_id(Type::Bool)?
            } else {
                self.type_id(Type::String)?
            };
            let operand_ty = self.operand_ty(operand)?;
            // Every key on an init INTERFACE is optional, so a key read off a
            // typed init arrives as `Optional<T>` where the same key written in
            // a literal arrives as `T`. An absent optional is the spec's "the
            // key was not supplied", which must leave the slot holding whatever
            // the input already put there — the source request's value, or the
            // default — rather than overwriting it with an empty string.
            if let Some(&Type::Optional(inner)) = self.mir.types.get(operand_ty) {
                let value = self.rendered_at_type(
                    &RustExpr::path("smelt_init_value"),
                    inner,
                    target,
                    &self.render_scope(),
                )?;
                statements.push_str(&format!(
                    "if let Some(smelt_init_value) = {} {{ smelt_request_init.{} = {}; }} ",
                    self.operand_expr(operand)?,
                    member.field_name(),
                    Self::stored_member_value(*member, &value),
                ));
                continue;
            }
            let value = self.value_at_type(operand, target)?;
            statements.push_str(&format!(
                "smelt_request_init.{} = {}; ",
                member.field_name(),
                Self::stored_member_value(*member, &value),
            ));
        }
        Ok(RustExpr::atom(format!(
            "{{ let mut smelt_request_init = {init_default}; {statements}smelt_request_init }}"
        )))
    }

    /// Wrap a stored member's value in the normalization the spec applies.
    ///
    /// Only `referrer` has one: it is parsed as a URL and stored as its
    /// serialization, so `"https://c.test"` reads back as `"https://c.test/"`.
    /// The other seven are stored exactly as spelled.
    fn stored_member_value(member: smelt_hir::RequestInitMember, value: &RustExpr) -> RustExpr {
        if matches!(member, smelt_hir::RequestInitMember::Referrer) {
            return RustExpr::atom(format!("SmeltRequestInit::serialize_referrer(&{value})"));
        }
        value.to_owned()
    }

    /// Emit a `Request` member operation on a concrete receiver.
    pub(super) fn request_op_expr(
        &self,
        op: smelt_hir::RequestOp,
        request: &Operand,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        if !args.is_empty() {
            return Err(EmitError::new("no modeled Request member takes arguments"));
        }
        let receiver = self.operand_expr(request)?;
        Ok(match op {
            smelt_hir::RequestOp::Url => RustExpr::postfix_on(&receiver, format!("{receiver}.url()")),
            smelt_hir::RequestOp::Method => RustExpr::postfix_on(&receiver, format!("{receiver}.method()")),
            smelt_hir::RequestOp::Headers => RustExpr::postfix_on(&receiver, format!("{receiver}.headers()")),
            smelt_hir::RequestOp::BodyUsed => RustExpr::postfix_on(&receiver, format!("{receiver}.body_used()")),
            // The body HANDLE, or `None` when there is no body. A handle
            // read SHARES the body — the same `Rc` payload and the same
            // `bodyUsed` cell — so handing it to a constructor is the spec's
            // "the body is shared" and not a copy. `Empty` is the spec's
            // `null`: `new Response(null).body` is `null` while
            // `new Response("").body` is a stream, and the payload keeps the
            // two apart.
            smelt_hir::RequestOp::Body => RustExpr::atom(format!(
                "{{ let smelt_body = {receiver}.body(); if smelt_body.is_empty() {{ None }} else {{ Some(smelt_body) }} }}"
            )),
            smelt_hir::RequestOp::Clone => RustExpr::postfix_on(&receiver, format!("{receiver}.tee()")),
            smelt_hir::RequestOp::Signal => RustExpr::chain(format!("smelt_request_signal({receiver}.id())")),
            // A stored `RequestInit` member: a read-only getter over the value
            // the constructor kept, named after the member itself.
            smelt_hir::RequestOp::Init(member) => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.{}()", member.field_name()))
            }
            // Same handle-clone-into-the-block shape as `Response::text`; see
            // the comment there for why the receiver is not moved.
            smelt_hir::RequestOp::Text => RustExpr::atom(format!(
                "{{ let smelt_request = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(smelt_request.take_text()?) }})) }}"
            )),
            // Same `text()` + fallible `JSON.parse` shape as `Response::json`.
            smelt_hir::RequestOp::Json => RustExpr::atom(format!(
                "{{ let smelt_request = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_text = smelt_request.take_text()?; {json}(&smelt_text) }})) }}",
                json = crate::thrown::JSON_PARSE_FN
            )),
            // The same single-use reader, handing its bytes plus the request's
            // OWN `Content-Type` to the parser: the encoding (and a multipart
            // boundary) is a header, not a property of the bytes.
            smelt_hir::RequestOp::FormData => RustExpr::atom(format!(
                "{{ let smelt_request = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_content_type = smelt_request.headers().get(\"content-type\"); Ok::<_, {ERROR_BOX}>(smelt_form_data_from_body(smelt_content_type, smelt_request.body().take_bytes()?)?) }})) }}"
            )),
            // The two BYTE readers; see the `Response` pair for why they are
            // two members rather than one.
            smelt_hir::RequestOp::ArrayBuffer => RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_request = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(SmeltArrayBuffer::from_bytes(smelt_request.body().take_bytes()?)) }})) }}"
            ))),
            smelt_hir::RequestOp::Bytes => RustExpr::atom(format!(
                "{{ let smelt_request = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(SmeltTypedArray::from_bytes(smelt_request.body().take_bytes()?)) }})) }}"
            )),
        })
    }

    /// Emit `new Response(body?, init?)` as a concrete `SmeltResponse`.
    ///
    /// Each init key is its own operand (see `Rvalue::ResponseNew`), so the
    /// value is assembled from typed parts. An absent key takes the spec's
    /// default: 200, an empty reason phrase, an empty header list.
    pub(super) fn response_new_expr(
        &self,
        body: Option<&Operand>,
        status: Option<&Operand>,
        status_text: Option<&Operand>,
        headers: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let status_text_expr = self.init_scalar_expr(status, &RustExpr::path("200.0"))?;
        let phrase_expr = self.init_scalar_expr(status_text, &RustExpr::atom("String::new()"))?;
        // A `Response` has no source to inherit from, so both absent keys mean
        // the spec's own empty value.
        let headers_expr = self.init_headers_expr(headers, &RustExpr::atom("SmeltHeaders::new()"))?;
        // A `Response`'s first argument is a `BodyInit`, not an init key:
        // a `Request` there is an ordinary body extraction, so it does NOT
        // take the sharing path.
        let body_expr = self.init_body_expr(body, &RustExpr::atom("SmeltBody::empty()"), false)?;
        Ok(RustExpr::atom(format!(
            "SmeltResponse::from_parts({status_text_expr}, {phrase_expr}, {headers_expr}, {body_expr})"
        )))
    }


    /// Convert a value of type `ty`, named by `body_text`, into a `SmeltBody`.
    ///
    pub(super) fn body_conversion_expr(
        &self,
        body_text: &RustExpr,
        body_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // A `Request` at the body position is the WHATWG "extract the source's
        // body" step: the new holder reads the SAME payload, and the source is
        // disturbed at once. Sharing the whole handle (`{body}.body()`) was
        // close but wrong at the edge that matters — it left the source
        // readable until the copy was consumed, where Node reports
        // `source.bodyUsed` true as soon as the copy exists.
        if self.is_request_class_type(body_ty)? {
            return Ok(RustExpr::atom(format!("SmeltBody::take_from_source(&{body_text}.body())")));
        }
        // A body HANDLE at the body position is passed straight through: it
        // already IS a `SmeltBody`, and sharing it is what the spec says
        // `new Response(other.body, init)` does. Checked before the payload
        // conversions below because a handle needs none of them.
        if self.is_readable_stream_class_type(body_ty)? {
            return Ok(body_text.to_owned());
        }
        // A `Blob` at the body position contributes its BYTES, and its `type`
        // becomes the body's content type — which is what makes
        // `new Response(blob).headers.get("content-type")` answer the blob's
        // MIME type, as the spec's "extract a body" step does.
        if self.is_blob_class_type(body_ty)? {
            return Ok(RustExpr::atom(format!(
                "SmeltBody::from_blob({body_text}.to_bytes(), {body_text}.blob_type())"
            )));
        }
        // A `BufferSource` at the body position contributes its BYTES. A VIEW
        // contributes its own window, not the whole buffer — `new
        // Response(buffer.subarray(2, 4))` is two bytes, which is what
        // `to_bytes()` answers for each half of the family. Neither carries a
        // content type: the spec's "extract a body" step leaves it unset for a
        // buffer source, unlike a blob or a form.
        if self.is_typed_array_view_class_type(body_ty)? || self.is_array_buffer_class_type(body_ty)? {
            return Ok(RustExpr::atom(format!("SmeltBody::from_bytes({body_text}.to_bytes())")));
        }
        // A `URLSearchParams` body is its QUERY STRING, with the content type
        // the spec fixes for it. Same shape as the blob arm: the value knows
        // both its bytes and its type.
        if self.is_url_search_params_class_type(body_ty)? {
            return Ok(RustExpr::atom(format!(
                "SmeltBody::from_blob({body_text}.to_text().into_bytes(), \"application/x-www-form-urlencoded;charset=UTF-8\".to_owned())"
            )));
        }
        // A `FormData` body is `multipart/form-data`, whose boundary is part of
        // the content type rather than of the bytes — so the encoder mints one
        // and the header carries it, which is exactly what makes
        // `await new Request(url, { body: form }).formData()` round-trip
        // through the parser that already existed.
        if self.is_form_data_class_type(body_ty)? {
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_boundary = smelt_multipart_boundary(); SmeltBody::from_blob({body_text}.to_multipart(&smelt_boundary), format!(\"multipart/form-data; boundary={{smelt_boundary}}\")) }}"
            )));
        }
        // A value whose static type is a GENERATED union is dispatched through
        // its OWN enum arms, never through the `SmeltUnknown` runtime-shape
        // `match` below. `BodyInit` is exactly such a union: every arm names a
        // concrete WHATWG type, so each arm's payload takes that type's own
        // extraction rule (a `string` arm is text, a `Blob` arm contributes its
        // bytes and MIME type, a `ReadableStream` arm passes its handle
        // through). Matching a `SmeltUnion…` scrutinee with `SmeltUnknown::…`
        // patterns does not type-check, and erasing the value first would throw
        // away the static arm the source already decided.
        if let Some(members) = self.concrete_union_members(body_ty) {
            let union = union::union_name(body_ty);
            let mut arms = Vec::with_capacity(members.len());
            for (index, member) in members.iter().copied().enumerate() {
                let arm = self.body_conversion_expr(&RustExpr::path("smelt_body_arm"), member)?;
                arms.push(format!("{union}::M{index}(smelt_body_arm) => {arm}"));
            }
            return Ok(RustExpr::atom(format!("match {body_text} {{ {} }}", arms.join(", "))));
        }
        match self.mir.types.get(body_ty) {
            Some(Type::String) => Ok(RustExpr::atom(format!("SmeltBody::from_text(&{body_text})"))),
            Some(Type::None) => Ok(RustExpr::atom("SmeltBody::empty()")),
            // An OPTIONAL body. WHATWG's constructor takes `BodyInit?` and an
            // absent body IS the empty body — the same answer the no-argument
            // spelling gives — so this is a modeled case rather than a blocker.
            // It is the shape `body?: BodyInit | null` arrives in.
            Some(&Type::Optional(inner)) => {
                let present = self.body_conversion_expr(&RustExpr::path("smelt_body_init"), inner)?;
                Ok(RustExpr::atom(format!(
                    "match {body_text} {{ Some(smelt_body_init) => {present}, None => SmeltBody::empty() }}"
                )))
            }
            // An ERASED body. `BodyInit` is a union whose unmodeled arms are
            // host classes, so a `BodyInit | null | undefined` parameter erases
            // and the whole call used to be refused at BUILD time — including
            // for the string every real caller passes.
            //
            // **Dynamic boundary.** The arms the spec allows are distinguishable
            // at run time by tag, and only there: the static type says nothing
            // beyond "some runtime value". A string is text and a nullish value
            // is the empty body, which covers every arm Smelt models; anything
            // else is an unmodeled arm and panics, naming itself, rather than
            // putting wrong bytes in the body. A concrete type cannot stand in
            // here — the erasure is the parameter's own declared type.
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                // A byte-backed record is a `BufferSource` arm that crossed the
                // boundary — a view or its storage — and a blob record is the
                // `Blob` arm. Both branches name a pay-for-use prelude type, so
                // each is emitted only where that type is, keyed on the same
                // gate that decides whether it exists at all.
                let byte_arm = if self.context.demand().has(Need::ByteArray) {
                    self.carrier_format(format_args!(
                        "SmeltUnknown::Object(value) if {elements}(&SmeltUnknown::Object(value.clone())).is_some() => SmeltBody::from_bytes(SmeltTypedArray::smelt_from_unknown(SmeltUnknown::Object(value)).to_bytes()), ",
                        elements = smelt_stdlib::runtime_symbols::byte_buffer::ELEMENTS,
                    ))
                } else {
                    String::new()
                };
                let blob_arm = if self.context.demand().has(Need::Blob) {
                    "SmeltUnknown::Object(value) if value.contains_key(\"__smelt_blob\") => { let smelt_blob = <SmeltBlob as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(value)); SmeltBody::from_blob(smelt_blob.to_bytes(), smelt_blob.blob_type()) }, "
                } else {
                    ""
                };
                // The `URLSearchParams` and `FormData` arms carry their host
                // marker across the boundary; each takes the same extraction
                // its typed arm above uses (query string / multipart), so an
                // erased form body keeps its bytes AND its content type.
                let search_params_arm = if self.context.demand().has(Need::UrlSearchParams) {
                    "SmeltUnknown::Object(value) if value.contains_key(\"__smelt_urlsearchparams\") => SmeltBody::from_blob(<SmeltUrlSearchParams as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(value)).to_text().into_bytes(), \"application/x-www-form-urlencoded;charset=UTF-8\".to_owned()), "
                } else {
                    ""
                };
                let form_data_arm = if self.context.demand().has(Need::FormData) {
                    "SmeltUnknown::Object(value) if value.contains_key(\"__smelt_formdata\") => { let smelt_boundary = smelt_multipart_boundary(); SmeltBody::from_blob(<SmeltFormData as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(value)).to_multipart(&smelt_boundary), format!(\"multipart/form-data; boundary={smelt_boundary}\")) }, "
                } else {
                    ""
                };
                Ok(RustExpr::atom(self.carrier_format(format_args!(
                    "match {body_text} {{ SmeltUnknown::String(value) => SmeltBody::from_text(&value.to_string()), \
                     SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltBody::empty(), \
                     {byte_arm}{blob_arm}{search_params_arm}{form_data_arm}\
                     value => panic!(\"body arm is not modeled yet: {{value:?}}\") }}"
                ))))
            }
            _ => Err(EmitError::new(format!(
                "body must be a string or null; this `BodyInit` arm is not modeled yet: {}",
                self.rendered_type(body_ty)?
            ))),
        }
    }

    /// Emit a `Response` member operation on a concrete receiver.
    pub(super) fn response_op_expr(
        &self,
        op: smelt_hir::ResponseOp,
        response: &Operand,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        if !args.is_empty() {
            return Err(EmitError::new(
                "no modeled Response member takes arguments",
            ));
        }
        let receiver = self.operand_expr(response)?;
        self.response_op_on_expr(op, &receiver)
    }

    /// Emit a `Response` member operation on an already-rendered receiver.
    ///
    /// The text-level half of [`Self::response_op_expr`]. A coercion seam holds
    /// a rendered expression and a type rather than a MIR operand — a structural
    /// conversion from a `Response` into a record type has to READ each member
    /// off the value it was handed — so the member rules live here and the
    /// operand entry point above is a thin wrapper that renders its receiver
    /// first. Splitting it keeps one statement of what each member means.
    pub(super) fn response_op_on_expr(
        &self,
        op: smelt_hir::ResponseOp,
        receiver: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        Ok(match op {
            smelt_hir::ResponseOp::Status => RustExpr::postfix_on(&receiver, format!("{receiver}.status()")),
            smelt_hir::ResponseOp::Ok => RustExpr::postfix_on(&receiver, format!("{receiver}.ok()")),
            smelt_hir::ResponseOp::StatusText => RustExpr::postfix_on(&receiver, format!("{receiver}.status_text()")),
            smelt_hir::ResponseOp::Headers => RustExpr::postfix_on(&receiver, format!("{receiver}.headers()")),
            smelt_hir::ResponseOp::BodyUsed => RustExpr::postfix_on(&receiver, format!("{receiver}.body_used()")),
            // The body HANDLE, or `None` when there is no body. A handle
            // read SHARES the body — the same `Rc` payload and the same
            // `bodyUsed` cell — so handing it to a constructor is the spec's
            // "the body is shared" and not a copy. `Empty` is the spec's
            // `null`: `new Response(null).body` is `null` while
            // `new Response("").body` is a stream, and the payload keeps the
            // two apart.
            smelt_hir::ResponseOp::Body => RustExpr::atom(format!(
                "{{ let smelt_body = {receiver}.body(); if smelt_body.is_empty() {{ None }} else {{ Some(smelt_body) }} }}"
            )),
            smelt_hir::ResponseOp::Clone => RustExpr::postfix_on(&receiver, format!("{receiver}.tee()")),
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
            smelt_hir::ResponseOp::Text => RustExpr::atom(format!(
                "{{ let smelt_response = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(smelt_response.take_text()?) }})) }}"
            )),
            // `json()` is `text()` followed by the fallible `JSON.parse`
            // adapter, so malformed text rejects with the same catchable
            // `SyntaxError` a source-level `JSON.parse` throws.
            smelt_hir::ResponseOp::Json => RustExpr::atom(format!(
                "{{ let smelt_response = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_text = smelt_response.take_text()?; {json}(&smelt_text) }})) }}",
                json = crate::thrown::JSON_PARSE_FN
            )),
            // The two BYTE readers, one `take_bytes` apart: storage owns the
            // bytes, a view is an element window over them. Both consume the
            // body, like every other reader.
            smelt_hir::ResponseOp::ArrayBuffer => RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_response = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(SmeltArrayBuffer::from_bytes(smelt_response.body().take_bytes()?)) }})) }}"
            ))),
            smelt_hir::ResponseOp::Bytes => RustExpr::atom(format!(
                "{{ let smelt_response = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(SmeltTypedArray::from_bytes(smelt_response.body().take_bytes()?)) }})) }}"
            )),
            // Same shape as `Request::formData`; see the comment there for why
            // the parser is handed the header rather than only the bytes.
            smelt_hir::ResponseOp::FormData => RustExpr::atom(format!(
                "{{ let smelt_response = {receiver}.clone(); SmeltFuture::from_future(Box::pin(async move {{ let smelt_content_type = smelt_response.headers().get(\"content-type\"); Ok::<_, {ERROR_BOX}>(smelt_form_data_from_body(smelt_content_type, smelt_response.body().take_bytes()?)?) }})) }}"
            )),
        })
    }

    /// Return whether a type is the modeled `ArrayBuffer`: byte STORAGE.
    ///
    /// The type-level twin of `operand_is_array_buffer`; the body conversion
    /// has a type without an operand, exactly as the headers conversion does.
    pub(super) fn is_array_buffer_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::ArrayBuffer))
    }

    /// Return whether a type names the generated `SmeltFormData` type.
    pub(super) fn is_form_data_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::FormData))
    }

    /// Return whether a type names the generated `SmeltUrlSearchParams` type.
    pub(super) fn is_url_search_params_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)?
            == Some(smelt_stdlib::StdlibClass::UrlSearchParams))
    }

    /// Emit an `EventEmitter` member operation on a concrete receiver.
    ///
    /// The first argument is always the event name; the rest are the
    /// operation's own. `emit`'s tail becomes the erased argument vector every
    /// listener is called with — the boundary the emitter's store is built on.
    pub(super) fn event_emitter_op_expr(
        &self,
        op: smelt_hir::EventEmitterOp,
        emitter: &Operand,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(emitter)?;
        let string_ty = self.type_id(Type::String)?;
        let Some(event) = args.first() else {
            return Err(EmitError::new(
                "every EventEmitter operation takes an event name",
            ));
        };
        let event_text = self.value_at_type(event, string_ty)?;
        let listener = |index: usize| -> Result<String, EmitError> {
            let arg = args.get(index).ok_or_else(|| {
                EmitError::new(format!("`EventEmitter` operation {op:?} needs a listener"))
            })?;
            self.erased_emitter_argument_expr(arg).map(RustExpr::into_string)
        };
        Ok(match op {
            smelt_hir::EventEmitterOp::On | smelt_hir::EventEmitterOp::Once => {
                let once = matches!(op, smelt_hir::EventEmitterOp::Once);
                RustExpr::atom(self.carrier_format(format_args!(
                    "{{ let smelt_listener = match {} {{ SmeltUnknown::Function(smelt_function) => smelt_function, _ => ::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Undefined)) }}; {receiver}.add(&{event_text}, smelt_listener, {once}) }}",
                    listener(1)?
                )))
            }
            smelt_hir::EventEmitterOp::Off => RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_listener = match {} {{ SmeltUnknown::Function(smelt_function) => smelt_function, _ => ::std::rc::Rc::new(move |_smelt_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Undefined)) }}; {receiver}.remove(&{event_text}, &smelt_listener) }}",
                listener(1)?
            ))),
            smelt_hir::EventEmitterOp::RemoveAll => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.remove_all(&{event_text})"))
            }
            smelt_hir::EventEmitterOp::ListenerCount => {
                RustExpr::postfix_on(&receiver, format!("{receiver}.listener_count(&{event_text})"))
            }
            smelt_hir::EventEmitterOp::Emit => {
                let mut erased = Vec::new();
                for arg in args.iter().skip(1) {
                    erased.push(self.erased_emitter_argument_expr(arg)?);
                }
                RustExpr::loose(format!(
                    "{receiver}.emit(&{event_text}, Vec::from([{}]))?",
                    erased.join(", ")
                ))
            }
        })
    }

    /// Erase one `EventEmitter` argument into the runtime's `SmeltUnknown` ABI.
    ///
    /// `operand_expr` deliberately leaves a function-typed `copy` un-cloned,
    /// because the usual consumer of a function operand is a callee position
    /// that only borrows it to call. The erasure wrapper MOVES its value into
    /// the adapter closure instead, so a listener that the source registers
    /// twice - `e.on("x", fn); e.on("x", fn)` - needs its own handle each time.
    fn erased_emitter_argument_expr(&self, arg: &Operand) -> Result<RustExpr, EmitError> {
        let ty = self.operand_ty(arg)?;
        let mut text = self.operand_expr(arg)?;
        if matches!(arg, Operand::Copy(_))
            && matches!(self.mir.types.get(ty), Some(Type::Function(_)))
        {
            text = RustExpr::from(RustExpr::postfix_on(&text, format!("{text}.clone()")).ending_in_clone());
        }
        self.erase_rendered(&text, ty)
    }

    /// Return whether a type names the generated `SmeltRequest` runtime type.
    pub(super) fn is_request_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::Request))
    }

    /// Return whether a type names the modeled `ReadableStream` body handle.
    pub(super) fn is_readable_stream_class_type(&self, ty: TypeId) -> Result<bool, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(false);
        };
        Ok(self.stdlib_class_of_symbol(*name)?
            == Some(smelt_stdlib::StdlibClass::ReadableStream))
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
    ///
    /// Delegates to `crate::stdlib::stdlib_class_of_class_symbol`, which is the
    /// one place that answers this for the pay-for-use gates and the emitters
    /// alike -- including the source-shadowing rule documented there.
    pub(super) fn stdlib_class_of_symbol(
        &self,
        name: Symbol,
    ) -> Result<Option<smelt_stdlib::StdlibClass>, EmitError> {
        Ok(crate::stdlib::stdlib_class_of_class_symbol(self.mir, name))
    }
    /// Render a scalar init key, falling back to the spec's default.
    ///
    /// An absent key and a key whose value is `undefined` are the same thing to
    /// the spec, so `Optional<T>` unwraps to the same default a missing operand
    /// uses. `default_text` is that default, written once per key.
    fn init_scalar_expr(
        &self,
        operand: Option<&Operand>,
        default_text: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        let Some(operand) = operand else {
            return Ok(default_text.to_owned());
        };
        let text = self.operand_expr(operand)?;
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Optional(_))
        ) {
            return Ok(RustExpr::postfix_on(&text, format!("{text}.unwrap_or_else(|| {default_text})")));
        }
        Ok(text)
    }

    /// Render the `headers` init key as a `SmeltHeaders`.
    ///
    /// The conversion for a present value is the `Headers` constructor's own
    /// (a `Headers`, a record, or an array of pairs — selected by the
    /// operand's type), so the init and `new Headers(init)` cannot disagree.
    /// `default_text` is what an ABSENT key means, which is not always the
    /// spec's empty list: for `new Request(source, init)` an absent `headers`
    /// means the SOURCE's headers, so the default travels from the call site
    /// rather than being hard-coded here. Every caller that has no source
    /// passes the spec's own default, so the two spellings share this one
    /// conversion instead of growing a second copy of it.
    fn init_headers_expr(
        &self,
        operand: Option<&Operand>,
        default_text: &RustExpr,
    ) -> Result<RustExpr, EmitError> {
        let Some(operand) = operand else {
            return Ok(default_text.to_owned());
        };
        let ty = self.operand_ty(operand)?;
        if let Some(&Type::Optional(inner)) = self.mir.types.get(ty) {
            let present = self.headers_conversion_expr(&RustExpr::path("smelt_init_headers"), inner)?;
            return Ok(RustExpr::atom(format!(
                "match {} {{ Some(smelt_init_headers) => {present}, None => {default_text} }}",
                self.operand_expr(operand)?
            )));
        }
        self.headers_new_expr(Some(operand))
    }

    /// Render the `body` init key as a `SmeltBody`.
    ///
    /// `default_text` carries the absent meaning for the same reason
    /// [`Self::init_headers_expr`]'s does. For the body it is load-bearing, and
    /// there are two separate cases:
    ///
    /// - An absent `body` in `new Request(source, init)` takes `default_text`,
    ///   which for a `Request` source is
    ///   `SmeltBody::take_from_source(&source.body())`: the copy shares the
    ///   source's payload under a fresh used flag, and the source is marked
    ///   used at once.
    /// - An init `body` that is itself a `Request` (with `shares_a_request_body`
    ///   set) shares that request's body HANDLE (`{body}.body()`), so consuming
    ///   the copy's body is observable on that request.
    fn init_body_expr(
        &self,
        operand: Option<&Operand>,
        default_text: &RustExpr,
        shares_a_request_body: bool,
    ) -> Result<RustExpr, EmitError> {
        let Some(operand) = operand else {
            return Ok(default_text.to_owned());
        };
        let convert = |body_text: &str, body_ty: TypeId| -> Result<String, EmitError> {
            if shares_a_request_body && self.is_request_class_type(body_ty)? {
                return Ok(format!("{body_text}.body()"));
            }
            self.body_conversion_expr(&RustExpr::opaque(&body_text), body_ty).map(RustExpr::into_string)
        };
        let ty = self.operand_ty(operand)?;
        if let Some(&Type::Optional(inner)) = self.mir.types.get(ty) {
            let present = convert("smelt_init_body", inner)?;
            return Ok(RustExpr::atom(format!(
                "match {} {{ Some(smelt_init_body) => {present}, None => {default_text} }}",
                self.operand_expr(operand)?
            )));
        }
        convert(&self.operand_expr(operand)?, ty).map(RustExpr::from)
    }


}
