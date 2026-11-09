//! The default value of a type: what a predeclared local, an absent slot or a
//! void result holds before anything is written to it.

use super::*;
use smelt_hir::FunctionType;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// The filler JavaScript leaves in an array hole when an index write grows it.
    ///
    /// `arr[2] = 3` on a one-element array creates a hole at index 1, and reading
    /// that hole yields `undefined` — never `null`. At an erased element type the
    /// two are distinguishable, and deep-equality code sees the difference:
    /// es-toolkit's `isEqualWith` compares `[1, <hole>, 3]` against an explicitly
    /// written `[1, undefined, 3]` and must call them equal. Typed element types
    /// cannot represent a hole at all, so they keep their ordinary default.
    pub(super) fn array_hole_value(&self, item_ty: TypeId) -> Result<RustExpr, EmitError> {
        if matches!(self.mir.types.get(item_ty), Some(Type::Unknown)) {
            return Ok(self.carrier_expr(self.carrier()?, &RustExpr::undefined()));
        }
        self.default_value(item_ty)
    }

    /// Gets the default value for a given type.
    pub(super) fn default_value(&self, ty: TypeId) -> Result<RustExpr, EmitError> {
        match self
            .mir
            .types
            .get(ty)
            .ok_or_else(|| EmitError::new("MIR references an unknown type"))?
        {
            Type::Bool => Ok(RustExpr::path("false")),
            Type::Int | Type::FixedInt(_) => Ok(RustExpr::path("0")),
            Type::BigInt => Ok(RustExpr::atom(format!("{}::default()", crate::bigint_prelude::BIGINT_TYPE)).delegating_default()),
            Type::Float => Ok(RustExpr::path("0.0")),
            Type::String => Ok(RustExpr::atom("JsString::new()")),
            Type::PyStr => Ok(RustExpr::atom("String::new()")),
            // Placeholders for a slot declared before its first write; no
            // source value is ever this default.
            Type::Symbol => Ok(RustExpr::atom("SmeltSymbol::default()").delegating_default()),
            Type::PropKey => Ok(RustExpr::atom("SmeltPropKey::default()").delegating_default()),
            Type::Unknown => Ok(self.null_value()?),
            Type::Never => Ok(self.null_value()?),
            Type::None => Ok(RustExpr::unit()),
            // Annotate the element type: a bare `Vec::new()` is uninferable when
            // the default is used as a `.into_iter()`/`.map()` receiver whose
            // element type is not otherwise constrained (E0282).
            Type::List(item) => Ok(RustExpr::atom(format!(
                "SmeltList::new(Vec::<{}>::new())",
                self.rendered_type(*item)?
            ))),
            Type::Set(item) => Ok({ let receiver = self.set_container_name(*item); RustExpr::atom_on(&receiver, format!("{receiver}::new()")) }),
            Type::Dict(key, _) if self.dict_uses_smelt_record(*key) => {
                Ok(RustExpr::atom("SmeltRecord::new()"))
            }
            Type::Dict(key, _) if self.is_prop_key_record_key(*key) => {
                Ok(self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltPropKeyRecord::new()")))
            }
            Type::Dict(key, _) if self.dict_uses_js_key_map(*key) => {
                Ok(self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltJsMap::new()")))
            }
            Type::Dict(_, _) => Ok(RustExpr::loose("::indexmap::IndexMap::new()")),
            Type::JsMap(_, _) => Ok(self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltJsMap::new()"))),
            Type::Optional(inner) => Ok(RustExpr::opaque(format!(
                "None::<{}>",
                self.rendered_type(self.flatten_optional_inner(*inner))?
            ))),
            Type::Tuple(items) => {
                let item_values = items
                    .iter()
                    .map(|item| self.default_value(*item))
                    .collect::<Result<Vec<_>, _>>()?;
                let items_text = item_values.join(", ");
                let tuple = if items.len() == 1 {
                    RustExpr::atom(format!("({items_text},)"))
                } else {
                    RustExpr::atom(format!("({items_text})"))
                };
                Ok(tuple.delegating_with(&item_values))
            }
            Type::TypeParam { name } if self.current_function_has_type_param(*name) => {
                Ok(RustExpr::atom("Default::default()").delegating_default())
            }
            Type::Union(items) if self.concrete_union_members(ty).is_some() => {
                let first = *items
                    .first()
                    .ok_or_else(|| EmitError::new("concrete union has no members"))?;
                let arm = self.default_value(first)?;
                Ok(RustExpr::atom(format!("{}::M0({arm})", union::union_name(ty), arm = arm.delimited())).delegating_with([&arm]))
            }
            Type::TypeParam { .. } | Type::Union(_) => Ok(self.null_value()?),
            Type::Class { .. } if self.is_py_exception_type(ty) => {
                Ok(RustExpr::atom("SmeltPyException::default()"))
            }
            Type::Class { name, .. } if self.is_regexp_class_symbol(*name)? => {
                Ok(RustExpr::atom("SmeltRegExp::new(String::new(), String::new())"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::Headers) =>
            {
                Ok(RustExpr::atom("SmeltHeaders::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::UrlSearchParams) =>
            {
                Ok(RustExpr::atom("SmeltUrlSearchParams::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::FormData) =>
            {
                // An empty form, which is exactly what `new FormData()` is.
                Ok(RustExpr::atom("SmeltFormData::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::Response) =>
            {
                Ok(RustExpr::atom("SmeltResponse::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::TextEncoder) =>
            {
                Ok(RustExpr::atom("SmeltTextEncoder::new()"))
            }
            // An empty, untyped blob: a blob with no bytes is a value the type
            // can hold.
            Type::Class { name, .. }
                if self
                    .stdlib_class_of_symbol(*name)?
                    .is_some_and(smelt_stdlib::StdlibClass::is_blob_runtime_type) =>
            {
                Ok(RustExpr::atom("SmeltBlob::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::TextDecoder) =>
            {
                Ok(RustExpr::atom("SmeltTextDecoder::new()"))
            }
            // A slot assigned before it is read; see `SmeltHash`'s `Default`.
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::NodeHash) =>
            {
                Ok(RustExpr::atom("SmeltHash::default()").delegating_default())
            }
            // An empty byte view, which is what `new Uint8Array(0)` is: a view
            // with no bytes is a value the type can hold, unlike a server
            // without a handler. Zero-length storage is the same answer for the
            // buffer half.
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::TypedArray) =>
            {
                Ok(RustExpr::atom("SmeltTypedArray::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::ArrayBuffer) =>
            {
                Ok(self.carrier_expr(self.carrier()?, &RustExpr::atom("SmeltArrayBuffer::new(0)")))
            }
            // An empty window over empty storage, for the same reason.
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::DataView) =>
            {
                Ok(RustExpr::atom("SmeltDataView::new()"))
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::CryptoKey) =>
            {
                Ok(RustExpr::atom("SmeltCryptoKey::default()").delegating_default())
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::EventEmitter) =>
            {
                Ok(RustExpr::atom("SmeltEventEmitter::new()"))
            }
            // A `ServerResponse` has a default — a fresh 200 with nothing set —
            // for the same reason a `Response` does. A `Server` and an
            // `IncomingMessage` deliberately do NOT: a server without a handler
            // and a request without a method are not values their types can
            // hold, so a default for either would be an invented object rather
            // than an empty one.
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::ServerResponse) =>
            {
                Ok(RustExpr::atom("SmeltServerResponse::new()"))
            }
            // A database's default is a handle that was never opened: every
            // operation on it answers Node's "database is not open". A
            // constructor fills a field with it only until the field's own
            // assignment runs, so no program observes it as a real database.
            Type::Class { name, .. }
                if matches!(
                    self.stdlib_class_of_symbol(*name)?,
                    Some(
                        smelt_stdlib::StdlibClass::SqliteDatabase
                            | smelt_stdlib::StdlibClass::SqliteStatement
                    )
                ) =>
            {
                Ok(RustExpr::atom("Default::default()").delegating_default())
            }
            Type::Class { name, .. }
                if self.stdlib_class_of_symbol(*name)?
                    == Some(smelt_stdlib::StdlibClass::Request) =>
            {
                // No `Request::new()`: the spec has no default request, because
                // a request without a URL is not a value the type can hold.
                // `about:blank` is the one URL the platform treats as "no
                // document", so it is what an unreachable default uses.
                Ok(RustExpr::atom("SmeltRequest::from_parts(\"about:blank\", \"GET\".to_owned(), SmeltHeaders::new(), SmeltBody::empty())"))
            }
            Type::Class { name, .. } if self.is_match_class_symbol(*name)? => {
                Ok(RustExpr::atom("SmeltMatch::default()").delegating_default())
            }
            // A generic class's default has to NAME the class when its type
            // arguments are unresolved: a bare `Default::default()` leaves them
            // to inference, and a binding annotated `Class<_>` and initialized
            // from it pins nothing (E0283, the router slice's anonymous
            // `class<T> extends RegExpRouter<T>` bound to a const). Naming the
            // class puts the unresolved parameters on the DEFAULT's own path,
            // where they resolve to the dynamic carrier — which is what such a
            // value is used as: a class VALUE is the constructor, never an
            // instance, and every read of it goes through erasure.
            Type::Class { name, args } if args.is_empty() && self.class_type_param_count(*name) > 0 => {
                let placeholders = vec!["SmeltUnknown"; self.class_type_param_count(*name)].join(", ");
                Ok(RustExpr::atom(format!(
                    "{}::<{placeholders}>::default()",
                    self.type_rust_name(*name)?
                ))
                .delegating_default())
            }
            Type::Class { .. } => Ok(RustExpr::atom("Default::default()").delegating_default()),
            Type::Function(function) => self.function_default_value(function, ty),
            Type::Future(item) => {
                let item_value = self.default_value(*item)?;
                Ok(RustExpr::atom(format!("SmeltFuture::resolved({item_value})", item_value = item_value.delimited())).delegating_with([&item_value]))
            }
            Type::Generator { .. } => Err(EmitError::new(
                "a generator value has no eager default",
            )),
            Type::GeneratorResult { return_ty, .. } => Ok(RustExpr::atom(format!(
                "SmeltGeneratorResult::Complete({})",
                self.default_value(*return_ty)?.delimited()
            ))),
        }
    }

    /// Gets a default value while preserving explicitly scoped type parameters.
    pub(super) fn default_value_with_scoped_type_params(
        &self,
        ty: TypeId,
        substitution: &TypeSubstitution<'_>,
    ) -> Result<RustExpr, EmitError> {
        match self
            .mir
            .types
            .get(ty)
            .ok_or_else(|| EmitError::new("MIR references an unknown type"))?
        {
            Type::TypeParam { name } if substitution.spells(*name) => {
                Ok(RustExpr::atom("Default::default()").delegating_default())
            }
            Type::Optional(inner) => Ok(RustExpr::opaque(format!(
                "None::<{}>",
                self.rust_type(self.flatten_optional_inner(*inner), false, substitution)?
            ))),
            // A list default annotates its element type, and that annotation has
            // to be spelled through THIS substitution. The unscoped
            // `default_value` arm this would otherwise fall through to reaches
            // for `lexical_rust_type`, which rebuilds a substitution from
            // the CURRENT FUNCTION's type parameters — and there is no current
            // function here: `default_value_for_with_scoped_type_params` hosts
            // itself on `functions[0]` purely to have an emitter, so a class type
            // parameter is not in that scope and erased to `SmeltUnknown`.
            // Hono's `struct SmartRouterInner<T> { _routers: SmeltList<Router<T>> }`
            // got `SmeltList::new(Vec::<Router<SmeltUnknown>>::new())` in a
            // `Default for SmartRouterInner<T>` impl (E0308). Latent until H29
            // started emitting these impls for generic classes.
            Type::List(item) => Ok(RustExpr::atom(format!(
                "SmeltList::new(Vec::<{}>::new())",
                self.rust_type(*item, false, substitution)?
            ))),
            Type::Function(function) => {
                // An erased-unknown-rest function type renders as the concrete
                // `SmeltErasedFunction` struct (see `rust_type`),
                // not a bare `Rc<dyn Fn(..)>`. Its default must therefore be a
                // `SmeltErasedFunction` wrapping a no-op callback so a struct
                // field default agrees with the field's declared type (a
                // callable-interface `__smelt_call` slot). Without this guard the
                // default below emits an `Rc<dyn Fn(..)>` closure whose type
                // mismatches the `SmeltErasedFunction` field (E0308).
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok(RustExpr::from(ERASED_FUNCTION_DEFAULT.to_owned()));
                }
                let params = function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        Ok(format!(
                            "smelt_arg{index}: {}",
                            self.function_type_param(function, index, *param, substitution)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                let return_ty = if function.may_throw {
                    format!(
                        "Result<{}, {ERROR_BOX}>",
                        self.rust_type(function.return_ty, false, substitution)?
                    )
                } else {
                    self.rust_type(function.return_ty, false, substitution)?
                        .into_string()
                };
                let return_value =
                    self.default_value_with_scoped_type_params(function.return_ty, substitution)?;
                let body = if function.may_throw {
                    format!("Ok::<_, {ERROR_BOX}>({return_value})", return_value = return_value.delimited())
                } else {
                    return_value.into_string()
                };
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_default_callback: ::std::rc::Rc<dyn Fn({}) -> {}> = ::std::rc::Rc::new(move |{}| -> {} {{ {} }}); smelt_default_callback }}",
                    function
                        .params
                        .iter()
                        .enumerate()
                        .map(|(index, param)| {
                            self.function_type_param(function, index, *param, substitution)
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", "),
                    return_ty,
                    params,
                    return_ty,
                    body
                )))
            }
            _ => self.default_value(ty),
        }
    }
}

/// The default value of the erased-function carrier, `SmeltErasedFunction`.
///
/// A predeclared function-typed local (and a callable-interface `__smelt_call`
/// slot) needs a value before the real callable is built — in particular the
/// self-recursive closure knot, whose `Rc<RefCell<..>>` cell is created before
/// the closure that fills it exists. That placeholder is runtime machinery, not
/// program data, so it is factored into the prelude's `Default` impl (emitted
/// beside the struct in `lib.rs`) instead of being inlined at every site. Doing
/// so keeps the generated crate's `SmeltUnknown` occurrences to the places where
/// a program value is actually erased, which is what
/// `smelt smelt-unknown-report` measures.
const ERASED_FUNCTION_DEFAULT: &str = "SmeltErasedFunction::default()";

impl FunctionEmitter<'_> {
    /// The default of a function type: a callable answering its own default.
    fn function_default_value(
        &self,
        function: &FunctionType,
        ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if self.is_erased_unknown_rest_function(function) && !function.may_throw {
            return Ok(RustExpr::from(ERASED_FUNCTION_DEFAULT.to_owned()));
        }
        // The synthesized default callback is emitted INSIDE the body
        // that needs it, and it is bound to a handle whose type
        // (`function_type`, below) renders in that body's lexical scope.
        // Its parameter declarations must therefore render in the SAME
        // scope, or a generic function containing a defaulted callback
        // emits `Rc<dyn Fn(T, ..)> = Rc::new(move |arg0: SmeltUnknown|
        // ..)` and fails to type-check (E0631/E0308) inside its own
        // body.
        let scope = self.current_function_type_params();
        let substitution = TypeSubstitution::lexical(&scope);
        let params = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok(format!(
                    "smelt_arg{index}: {}",
                    self.function_type_param(function, index, *param, &substitution)?
                ))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        let return_ty = if let Some(Type::Future(item)) =
            self.mir.types.get(function.return_ty)
        {
            format!("SmeltFuture<{}>", self.rendered_type(*item)?)
        } else if function.may_throw {
            format!(
                "Result<{}, {ERROR_BOX}>",
                self.rendered_type(function.return_ty)?
            )
        } else {
            self.rendered_type(function.return_ty)?.into_string()
        };
        let return_value = self.default_value(function.return_ty)?;
        let return_text = if let Some(Type::Future(item)) =
            self.mir.types.get(function.return_ty)
        {
            let item_value = self.default_value(*item)?;
            format!("SmeltFuture::resolved({item_value})", item_value = item_value.delimited())
        } else if function.may_throw {
            format!("Ok::<_, {ERROR_BOX}>({return_value})", return_value = return_value.delimited())
        } else {
            return_value.into_string()
        };
        let function_type = self.rendered_type(ty)?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_default_callback: {function_type} = ::std::rc::Rc::new(move |{params}| -> {return_ty} {{ {return_text} }}); smelt_default_callback }}"
        )))
    }
}
