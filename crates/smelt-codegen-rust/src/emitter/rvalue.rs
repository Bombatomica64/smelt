//! Rvalue dispatch: one arm per `Rvalue` variant, each handing its operands to
//! the family that renders it. The match stays exhaustive, so a new op fails to
//! compile here until it has an arm.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Converts an rvalue to its Rust text representation.
    pub(super) fn rvalue_expr(&self, value: &Rvalue) -> Result<RustExpr, EmitError> {
        self.rvalue_expr_for_dest(value, self.none_ty)
    }

    /// Converts an rvalue to Rust text using the destination type when it affects emission.
    pub(super) fn rvalue_expr_for_dest(
        &self,
        value: &Rvalue,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let mut text = self.rvalue_expr_for_dest_inner(value, dest_ty)?;
        if rvalue_answers_host_text(value) {
            text = self.host_text_at_type(text, dest_ty);
        }
        // List-producing operations (concat/slice/flat/map/copy/…) emit a bare
        // `Vec`, but `Type::List` now lowers to the identity-bearing `SmeltList`.
        // Coerce through `Into` at this single choke point: it is a no-op when the
        // rvalue already produced a `SmeltList` (the blanket `From<T> for T`) and
        // wraps a fresh-identity list otherwise (`From<Vec<T>>`).
        if matches!(self.mir.types.get(dest_ty), Some(Type::List(_))) {
            return Ok(RustExpr::atom(format!("Into::<SmeltList<_>>::into({text})")));
        }
        Ok(text)
    }

    /// A plain read of `operand` at the destination.
    ///
    /// An optional value read at the ERASED carrier keeps its absence:
    /// `SmeltUnknown` has an `Undefined` arm, so `let output: unknown =
    /// data` with `data: object | null | undefined` is `undefined` when
    /// `data` is, exactly as in JavaScript. Asserting presence instead
    /// (the second rule) panicked as soon as a caller passed `undefined`
    /// as `None` (remeda `prop(undefined, 'a')`). A type parameter the
    /// function carries erased renders as that same carrier, so
    /// `defaultValue as TDefault` with `defaultValue?: TDefault` is
    /// `undefined` too (radash `get(null, 'a')`), not a panic.
    fn use_expr(&self, operand: &Operand, dest_ty: TypeId) -> Result<RustExpr, EmitError> {
        if (matches!(self.mir.types.get(dest_ty), Some(Type::Unknown))
            || matches!(self.mir.types.get(dest_ty), Some(Type::TypeParam { .. }))
                && self.renders_erased(dest_ty).unwrap_or(false))
            && matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Optional(inner)) if *inner == dest_ty
            )
        {
            return Ok(RustExpr::postfix_on(&crate::OPTIONAL_UNKNOWN_OR_UNDEFINED_FN, format!(
                "{}({}.clone())",
                crate::OPTIONAL_UNKNOWN_OR_UNDEFINED_FN,
                self.operand_expr(operand)?
            )));
        }
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Optional(inner))
                if *inner == dest_ty
                    && !matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
        ) {
            // Clone ONCE: the operand is only read, and `operand_expr` of a
            // place already ends in `.clone()`, so the owned read cloned the
            // whole optional twice (a regex match's captures, per request,
            // in Hono's router).
            let receiver = self.operand_borrow_expr(operand)?;
            return Ok(RustExpr::postfix_on(&receiver, format!("{receiver}.clone().expect(\"optional value was absent after narrowing\")")));
        }
        self.value_at_type(operand, dest_ty)
    }

    /// Render an rvalue at a destination type, before list-identity coercion.
    ///
    /// This is the dispatch body behind [`Self::rvalue_expr_for_dest`]: it
    /// matches every `Rvalue` variant and produces the raw Rust expression for
    /// it. The public wrapper then applies the single `SmeltList` `Into`
    /// choke-point coercion for list-typed destinations, so variants here may
    /// freely emit bare `Vec` expressions.
    fn rvalue_expr_for_dest_inner(
        &self,
        value: &Rvalue,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match value {
            Rvalue::Use(operand) => self.use_expr(operand, dest_ty),
            // `this` is dynamically scoped: the receiver is whatever the
            // innermost active call installed, so the read is a runtime channel
            // read rather than a local. See `Rvalue::BindThis` for the install.
            Rvalue::ThisRead => Ok(RustExpr::chain("smelt_this()")),
            Rvalue::BindThis { callee, receiver } => self.bind_this_expr(callee, receiver, dest_ty),
            Rvalue::GeneratorYield {
                value,
                unwind,
                cleanup,
            } => self.generator_yield_expr(value, unwind.as_ref(), cleanup.as_ref()),
            Rvalue::GeneratorNext {
                generator,
                value,
                kind,
            } => self.generator_next_expr(generator, value.as_ref(), *kind),
            Rvalue::GeneratorDone { result } => self.generator_done_expr(result),
            Rvalue::GeneratorValue { result } => self.generator_value_expr(result, dest_ty),
            Rvalue::GeneratorDelegate { generator } => self.generator_delegate_expr(generator, dest_ty),
            Rvalue::List(items) => self.list_literal_expr(value, items, dest_ty),
            Rvalue::Set(items) => self.set_literal_expr(items, dest_ty),
            Rvalue::Dict(entries) => self.dict_literal_expr(entries, dest_ty),
            Rvalue::Tuple(items) => self.tuple_literal_expr(items, dest_ty),
            Rvalue::Binary { op, lhs, rhs } => self.binary_expr(*op, lhs, rhs, dest_ty),
            Rvalue::Unary { op, operand } => self.unary_expr(*op, operand, dest_ty),
            Rvalue::FunctionTableLookup { key, cases } => {
                self.function_table_lookup_expr(key, cases, dest_ty)
            }
            Rvalue::OptionalField { receiver, field } => {
                self.optional_field_expr_for_dest(receiver, *field, dest_ty)
            }
            Rvalue::OptionalIndex { receiver, index } => {
                self.optional_index_expr(receiver, index, dest_ty)
            }
            Rvalue::OptionalMethod {
                receiver,
                method,
                args,
            } => self.optional_method_expr(receiver, *method, args, dest_ty),
            Rvalue::UnionMethod {
                receiver,
                method,
                args,
            } => self.union_method_expr(receiver, *method, args, dest_ty),
            Rvalue::BoundMethod {
                receiver,
                bound,
                method,
                args,
            } => self.bound_method_expr(receiver, *bound, *method, args, dest_ty),
            Rvalue::BoundField {
                receiver,
                bound,
                field,
            } => self.bound_field_expr(receiver, *bound, *field, dest_ty),
            Rvalue::AbortSignalOp { op, args } => {
                self.abort_signal_op_expr(*op, args, dest_ty)
            }
            Rvalue::CryptoOp { op, args } => self.crypto_op_expr(*op, args, dest_ty),
            Rvalue::HostModuleCall { op, args } => self.host_module_rvalue_expr(*op, args),
            // The ONE global object: every reference is a handle on the same
            // shared record (see `emit_global_object_support`).
            Rvalue::GlobalObject => Ok(RustExpr::postfix_on(&crate::thrown::GLOBAL_OBJECT_FN, format!("{}()", crate::thrown::GLOBAL_OBJECT_FN))),
            Rvalue::FormDataNew => Ok(RustExpr::atom("SmeltFormData::new()")),
            Rvalue::FormDataOp { op, form, args } => {
                self.form_data_op_expr(*op, form, args, dest_ty)
            }
            Rvalue::UrlSearchParamsNew { init } => {
                self.url_search_params_new_expr(init.as_ref())
            }
            Rvalue::UrlSearchParamsOp { op, params, args } => {
                self.url_search_params_op_expr(*op, params, args, dest_ty)
            }
            Rvalue::UrlOp { op, url, args } => self.url_op_expr(*op, url, args, dest_ty),
            Rvalue::PythonDivision { op, lhs, rhs, .. } => self.python_division_expr(*op, lhs, rhs),
            Rvalue::PyExceptionNew { classes, args } => self.py_exception_new_expr(classes, args),
            Rvalue::PyStr { value: text_of } => self.py_text_expr(text_of, "py_str"),
            Rvalue::PyRepr { value: text_of } => self.py_text_expr(text_of, "py_repr"),
            Rvalue::PyExceptionArgs { exception } => {
                let exception_text = self.operand_expr(exception)?;
                Ok(RustExpr::postfix_on(&exception_text, format!("{exception_text}.args()")))
            }
            Rvalue::PyExceptionCaughtBy { exception, classes, catches_unclassified } => {
                self.py_exception_caught_by_expr(exception, classes, *catches_unclassified)
            }
            // Lowering gives each of these a call terminator (`fallible_op`).
            Rvalue::Base64Transcode { .. }
            | Rvalue::PythonDictRead { .. }
            | Rvalue::DictPopItem { .. }
            | Rvalue::PythonSequenceRead { .. }
            | Rvalue::PythonStepSlice { .. }
            | Rvalue::PythonListRead { .. }
            | Rvalue::RegExpCompile { .. }
            | Rvalue::DataViewAccess { .. }
            | Rvalue::BigIntCall { .. }
            | Rvalue::UrlNew { .. }
            | Rvalue::JsonParse { .. } => Err(EmitError::new(
                "internal: a fallible operation reached the assignment path",
            )),
            Rvalue::TextEncoderNew => Ok(RustExpr::atom("SmeltTextEncoder::new()")),
            Rvalue::TextDecoderNew { label } => self.text_decoder_new_expr(label.as_ref()),
            Rvalue::TextEncoderOp { op, encoder, args } => {
                self.text_encoder_op_expr(*op, encoder, args, dest_ty)
            }
            Rvalue::TextDecoderOp { op, decoder, args } => {
                self.text_decoder_op_expr(*op, decoder, args, dest_ty)
            }
            Rvalue::ByteArrayOp { op, bytes, args } => {
                self.byte_array_op_expr(*op, bytes, args, dest_ty)
            }
            Rvalue::TypedArrayNew { class_name, args } => {
                self.typed_array_new_expr(class_name, args)
            }
            // A real read: a server must see its environment (`PORT`). The
            // profile's one fixed variable, `TZ`, never reaches here.
            Rvalue::ProcessEnvRead { name } => Ok(RustExpr::loose(format!("::std::env::var({name:?}).ok()"))),
            Rvalue::ProcessArgv => Ok(self.process_argv_expr()),
            Rvalue::EventEmitterNew => Ok(RustExpr::atom("SmeltEventEmitter::new()")),
            Rvalue::SqliteOpen { path } => self.sqlite_open_expr(path),
            Rvalue::SqliteOp { op, receiver, args } => {
                self.sqlite_op_expr(*op, receiver, args, dest_ty)
            }
            Rvalue::EventEmitterOp { op, emitter, args } => {
                self.event_emitter_op_expr(*op, emitter, args)
            }
            Rvalue::HttpCreateFetchServer { fetch } => self.http_create_fetch_server_expr(fetch),
            Rvalue::HttpCreateServer { handler } => self.http_create_server_expr(handler),
            Rvalue::HttpServerOp { op, server, args } => {
                self.http_server_op_expr(*op, server, args)
            }
            Rvalue::IncomingMessageOp { op, message } => {
                self.incoming_message_op_expr(*op, message, dest_ty)
            }
            Rvalue::ServerResponseOp { op, response, args } => {
                self.server_response_op_expr(*op, response, args)
            }
            Rvalue::RequestNew {
                input,
                method,
                headers,
                body,
                signal,
                init_members,
            } => self.request_new_expr(
                input,
                method.as_ref(),
                headers.as_ref(),
                body.as_ref(),
                signal.as_ref(),
                init_members,
            ),
            Rvalue::RequestOp {
                op: smelt_hir::RequestOp::Json,
                request,
                args,
            } if let Some(target) = self.json_body_decode_target(dest_ty) => {
                self.json_body_decode_expr(request, args, target)
            }
            Rvalue::RequestOp { op, request, args } => {
                self.request_op_expr(*op, request, args)
            }
            Rvalue::ResponseNew {
                body,
                status,
                status_text,
                headers,
            } => self.response_new_expr(
                body.as_ref(),
                status.as_ref(),
                status_text.as_ref(),
                headers.as_ref(),
            ),
            Rvalue::ResponseOp {
                op: smelt_hir::ResponseOp::Json,
                response,
                args,
            } if let Some(target) = self.json_body_decode_target(dest_ty) => {
                self.json_body_decode_expr(response, args, target)
            }
            Rvalue::ResponseOp { op, response, args } => {
                self.response_op_expr(*op, response, args)
            }
            Rvalue::HeadersNew { init } => self.headers_new_expr(init.as_ref()),
            Rvalue::HeadersOp { op, headers, args } => {
                self.headers_op_expr(*op, headers, args, dest_ty)
            }
            Rvalue::OptionalCoalesce { optional, fallback } => {
                self.optional_coalesce_expr(optional, fallback, dest_ty)
            }
            Rvalue::InstanceOf {
                value: operand,
                class,
            } => self.instance_of_expr(operand, *class),
            Rvalue::ClassValue { class, constructor } => {
                self.class_value(*class, constructor.as_ref())
            }
            Rvalue::InstanceOfValue { value, target } => {
                self.instance_of_value(value, target)
            }
            Rvalue::Construct { callee, args } => self.construct_expr(callee, args, dest_ty),
            Rvalue::ConstructLike { value: constructed, args } => {
                self.construct_like_expr(constructed, args, dest_ty)
            }
            Rvalue::UnknownIs {
                value: unknown_value,
                kind,
            } => self.tag_check(unknown_value, *kind).map(RustExpr::from),
            Rvalue::TypeofValue {
                value: unknown_value,
            } => self.typeof_value(unknown_value),
            Rvalue::PrototypeSentinel {
                value: unknown_value,
                own_slot_shadows,
            } => self.prototype_sentinel_expr(unknown_value, *own_slot_shadows),
            Rvalue::BoxPrimitive { value } => self.box_primitive_expr(value),
            Rvalue::ObjectFromPrototype { prototype } => {
                self.object_from_prototype_expr(prototype)
            }
            Rvalue::DefineProperties {
                target,
                descriptors,
            } => self.define_properties_expr(target, descriptors),
            Rvalue::UnknownCast {
                value: unknown_value,
                target,
            } => self.unknown_cast_expr(unknown_value, *target, dest_ty),
            Rvalue::Struct { class, fields } => self.struct_literal_expr(*class, fields, dest_ty),
            Rvalue::ExternalClassInstance { class, args } => self.external_class_instance_at_dest_expr(*class, args, dest_ty),
            Rvalue::Len { operand } => self.len_expr(operand, dest_ty),
            Rvalue::NumericAbs { operand } => self.numeric_abs_expr(operand),
            Rvalue::NumericRound { op, operand } => self.numeric_round_expr(*op, operand, dest_ty),
            Rvalue::NumericExtrema { op, args, spread } => {
                self.numeric_extrema_expr(*op, args, spread.as_ref(), dest_ty)
            }
            Rvalue::NumericHypot { args } => self.numeric_hypot_expr(args),
            Rvalue::NumericPredicate { op, operand } => self.numeric_predicate_expr(*op, operand),
            Rvalue::NumericUnaryFunc { op, operand } => self.numeric_unary_func_expr(*op, operand),
            Rvalue::NumericPow { base, exponent } => self.numeric_pow_expr(base, exponent),
            Rvalue::NumericAtan2 { y, x } => self.numeric_atan2_expr(y, x),
            Rvalue::NumericRandom => Ok(RustExpr::atom("rand::random::<f64>()")),
            Rvalue::NumericRandomInt { start, end } => self.numeric_random_int_expr(start, end),
            Rvalue::NumericToStringRadix { operand, radix } => {
                self.numeric_to_string_radix_expr(operand, radix)
            }
            Rvalue::NumericToFixed { operand, digits } => {
                self.numeric_to_fixed_expr(operand, digits)
            }
            Rvalue::ParseIntRadix { operand, radix } => self.parse_int_radix_expr(operand, radix),
            Rvalue::PrimitiveCast { op, operand } => {
                self.primitive_cast_expr(*op, operand, dest_ty)
            }
            Rvalue::StringCase { op, operand } => self.string_case_expr(*op, operand, dest_ty),
            Rvalue::StringIsWellFormed { operand } => self.string_well_formed_expr(operand, false, dest_ty),
            Rvalue::StringToWellFormed { operand } => self.string_well_formed_expr(operand, true, dest_ty),
            Rvalue::StringNormalize { form, operand } => self.string_normalize_expr(*form, operand),
            Rvalue::UriTranscode { op, operand } => self.uri_transcode_expr(*op, operand),
            Rvalue::StringLocaleCompare { left, right } => {
                self.string_locale_compare_expr(left, right)
            }
            Rvalue::ObjectToStringTag { operand } => self.object_to_string_tag_expr(operand),
            // `SmeltSymbol::description` answers the `Option<JsString>` an
            // optional string is.
            Rvalue::SymbolUnique { base } => {
                let base_text = self.operand_expr(base)?;
                Ok(RustExpr::chain(format!("SmeltSymbol::unique({base_text})")))
            }
            Rvalue::SymbolDescription { operand } => {
                let symbol = self.operand_expr(operand)?;
                Ok(RustExpr::postfix_on(&symbol, format!("{symbol}.description()")))
            }
            Rvalue::StructuredClone { operand } => self.structured_clone_expr(operand),
            Rvalue::StringTrim { side, operand } => self.string_trim_expr(*side, operand),
            Rvalue::StringAffix {
                op,
                haystack,
                needle,
            } => self.string_affix_expr(*op, haystack, needle),
            Rvalue::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            } => self.string_search_expr(*op, haystack, needle, from_index.as_ref(), dest_ty),
            Rvalue::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            } => self.string_replace_expr(*op, haystack, pattern, replacement),
            Rvalue::StringRemoveAffix {
                op,
                haystack,
                affix,
            } => self.string_remove_affix_expr(*op, haystack, affix),
            Rvalue::StringRepeat { operand, count } => self.string_repeat_expr(operand, count),
            Rvalue::StringPad {
                op,
                operand,
                target_len,
                pad,
            } => self.string_pad_expr(*op, operand, target_len, pad),
            Rvalue::StringPredicate { op, operand } => self.string_predicate_expr(*op, operand),
            Rvalue::RegexIsMatch {
                op,
                pattern,
                haystack,
            } => self.regex_is_match_expr(*op, pattern, haystack),
            Rvalue::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            } => self.regex_replace_expr(*op, pattern, haystack, replacement),
            Rvalue::RegexReplaceCallback {
                op,
                pattern,
                haystack,
                callback,
                args,
            } => self.regex_replace_callback_expr(*op, pattern, haystack, callback, args),
            Rvalue::RegexSplit { pattern, haystack } => self.regex_split_expr(pattern, haystack),
            Rvalue::RegexFind { pattern, haystack } => {
                self.regex_find_expr(pattern, haystack, dest_ty)
            }
            Rvalue::RegexExec { regex, haystack } => {
                self.regex_exec_expr(regex, haystack, dest_ty)
            }
            Rvalue::RegexTest { regex, haystack } => self.regex_test_expr(regex, haystack),
            Rvalue::RegexMatchAll { regex, haystack } => {
                self.regex_match_all_expr(regex, haystack, dest_ty)
            }
            Rvalue::StringCharAt { operand, index } => self.string_char_at_expr(operand, index),
            Rvalue::StringCharCodeAt { operand, index } => {
                self.string_char_code_at_expr(operand, index)
            }
            Rvalue::StringCodePointAt { operand, index } => {
                self.string_code_point_at_expr(operand, index)
            }
            Rvalue::StringContains {
                haystack,
                needle,
                from_index,
            } => self.string_contains_expr(haystack, needle, from_index.as_ref()),
            Rvalue::StringSlice {
                operand,
                start,
                end,
                kind,
            } => self.string_slice_expr(operand, start.as_ref(), end.as_ref(), *kind, dest_ty),
            Rvalue::ListContains { list, item } => {
                let text = self.list_contains_expr(list, item)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.rendered_at_type(&text, bool_ty, dest_ty, &self.render_scope())
            }
            Rvalue::SetContains { set, item } => {
                let text = self.set_contains_expr(set, item)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.rendered_at_type(&text, bool_ty, dest_ty, &self.render_scope())
            }
            Rvalue::SetDisjoint { left, right } => {
                let text = self.set_disjoint_expr(left, right)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.rendered_at_type(&text, bool_ty, dest_ty, &self.render_scope())
            }
            Rvalue::SetRelation { op, left, right } => {
                let text = self.set_relation_expr(*op, left, right)?;
                let bool_ty = self.type_id(Type::Bool)?;
                self.rendered_at_type(&text, bool_ty, dest_ty, &self.render_scope())
            }
            Rvalue::SetAdd { set, item } => self.set_add_expr(set, item, dest_ty),
            Rvalue::SetRemove { op, set, item } => self.set_remove_expr(*op, set, item, dest_ty),
            Rvalue::SetClear { set } => self.collection_clear_expr(set, dest_ty, "set"),
            Rvalue::SetCopy { set } => self.set_copy_expr(set, dest_ty),
            Rvalue::ListToSet { list } => self.list_to_set_expr(list, dest_ty),
            Rvalue::ListPairsToDict { list } => self.list_pairs_to_dict_expr(list, dest_ty),
            Rvalue::SetBinary { op, left, right } => {
                self.set_binary_expr(*op, left, right, dest_ty)
            }
            Rvalue::SetProjection { op, set } => self.set_projection_expr(*op, set, dest_ty),
            Rvalue::ListConcat { left, right } => self.list_concat_at_dest_expr(left, right, dest_ty),
            Rvalue::ConcatSpread { value } => self.concat_spread_expr(value),
            Rvalue::ListSearch {
                op,
                list,
                item,
                from_index,
            } => self.list_search_at_dest_expr(*op, list, item, from_index.as_ref(), dest_ty),
            Rvalue::Closure { id, .. } => self.closure_value_expr(*id, dest_ty),
            Rvalue::ClosureCall { callee, args } => self.closure_call_rvalue_expr(callee, args, dest_ty),
            Rvalue::ClosureCallSpread { callee, args } => self.closure_call_spread_expr(callee, args, dest_ty),
            Rvalue::ClosureCallRest { callee, items } => self.closure_call_rest_expr(callee, items, dest_ty),
            Rvalue::ErasedFunctionMethodCall { receiver, method, args } => {
                self.erased_function_method_call_expr(receiver, *method, args, dest_ty)
            }
            Rvalue::ListCallback { op, list, callback } => {
                self.list_callback_expr(*op, list, callback, dest_ty)
            }
            Rvalue::ListFromLength { length } => self.list_from_length_expr(length, dest_ty),
            Rvalue::ListFromLengthMap { length, callback } => {
                self.list_from_length_map_expr(length, callback, dest_ty)
            }
            Rvalue::ListReduce {
                list,
                initial,
                callback,
                from_right,
            } => self.list_reduce_expr(list, initial.as_ref(), callback, *from_right, dest_ty),
            Rvalue::ListSlice { list, start, end } => {
                self.list_slice_expr(list, start.as_ref(), end.as_ref(), dest_ty)
            }
            Rvalue::ListSplice {
                list,
                start,
                delete_count,
                items,
                mutate,
            } => self.list_splice_expr(list, start, delete_count.as_ref(), items, *mutate, dest_ty),
            Rvalue::ListFill {
                list,
                value: fill_value,
                start,
                end,
            } => self.list_fill_expr(list, fill_value, start.as_ref(), end.as_ref(), dest_ty),
            Rvalue::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => self.list_copy_within_expr(list, target, start, end.as_ref(), dest_ty),
            Rvalue::ListWith {
                list,
                index,
                value: replacement,
            } => self.list_with_expr(list, index, replacement, dest_ty),
            Rvalue::ListFlat { list, depth } => self.list_flat_expr(list, depth.as_ref(), dest_ty),
            Rvalue::ListProjection { op, list } => self.list_projection_expr(*op, list, dest_ty),
            Rvalue::DictEntryPush { entry, item } => {
                self.dict_entry_push_expr(entry, item, dest_ty)
            }
            Rvalue::ListPush { list, item } => self.list_push_expr(list, item, dest_ty),
            Rvalue::ListExtend { list, other } => self.list_extend_expr(list, other, dest_ty),
            Rvalue::ListInsert { list, index, item } => {
                self.list_insert_expr(list, index, item, dest_ty)
            }
            Rvalue::ListUnshift { list, items } => self.list_unshift_expr(list, items, dest_ty),
            Rvalue::ListReverse { list } => self.list_reverse_expr(list, dest_ty),
            Rvalue::ListClear { list } => self.collection_clear_expr(list, dest_ty, "list"),
            Rvalue::ListCopy { list } => self.list_copy_expr(list, dest_ty),
            Rvalue::TupleToList { tuple } => self.tuple_to_list_expr(tuple, dest_ty),
            Rvalue::ListToTuple { list } => self.list_to_tuple_expr(list, dest_ty),
            Rvalue::TupleToSet { tuple } => self.tuple_to_set_expr(tuple, dest_ty),
            Rvalue::ListCount { list, item } => self.list_count_expr(list, item, dest_ty),
            Rvalue::ListSum { list } => self.list_sum_expr(list, dest_ty),
            Rvalue::ListBoolFold { op, list } => self.list_bool_fold_expr(*op, list),
            Rvalue::ListSorted { list, key, reverse } => {
                self.list_sorted_expr(list, key.as_ref(), *reverse, dest_ty)
            }
            Rvalue::ListReversed { list } => self.list_reversed_expr(list, dest_ty),
            Rvalue::ListEnumerate { list } => self.list_enumerate_expr(list, dest_ty),
            Rvalue::ListZip { left, right } => self.list_zip_expr(left, right, dest_ty),
            Rvalue::ListRange { start, end, step } => {
                self.list_range_expr(start, end, step, dest_ty)
            }
            Rvalue::ListRandomChoice { list } => self.list_random_choice_expr(list, dest_ty),
            Rvalue::ListIndex { list, item } => self.list_index_expr(list, item, dest_ty),
            Rvalue::ListRemove { list, item } => self.list_remove_expr(list, item, dest_ty),
            Rvalue::ListSort {
                list,
                comparator,
                key,
                reverse,
            } => self.list_sort_expr(list, comparator.as_ref(), key.as_ref(), *reverse, dest_ty),
            Rvalue::ListPop { list } => self.list_pop_expr(list, dest_ty),
            Rvalue::ListShift { list } => self.list_shift_expr(list, dest_ty),
            Rvalue::ListNext { list } => self.list_next_expr(list, dest_ty),
            Rvalue::TupleContains { tuple, item } => self.tuple_contains_expr(tuple, item),
            Rvalue::TupleIndex { tuple, index } => self.tuple_index_expr(tuple, *index, dest_ty),
            Rvalue::TupleSlice { tuple, start, end } => {
                self.tuple_slice_expr(tuple, *start, *end, dest_ty)
            }
            Rvalue::DictContainsKey { dict, key, lookup } => {
                self.dict_contains_key_expr(dict, key, *lookup)
            }
            Rvalue::DictSet {
                dict,
                key,
                value: dict_value,
            } => self.dict_set_expr(dict, key, dict_value, dest_ty),
            Rvalue::DictRemoveKey { dict, key } => self.dict_remove_key_expr(dict, key, dest_ty),
            Rvalue::DictGet { dict, key, default } => {
                self.dict_get_expr(dict, key, default.as_ref(), dest_ty)
            }
            Rvalue::DictSetDefault { dict, key, default } => {
                self.dict_setdefault_expr(dict, key, default, dest_ty)
            }
            Rvalue::DictClear { dict } => self.collection_clear_expr(dict, dest_ty, "dict"),
            Rvalue::DictPop { dict, key, default } => {
                self.dict_pop_expr(dict, key, default.as_ref(), dest_ty)
            }
            Rvalue::DictUpdate { dict, other } => self.dict_update_expr(dict, other, dest_ty),
            Rvalue::DictAssign { target, sources } => {
                self.dict_assign_expr(target, sources, dest_ty)
            }
            Rvalue::CallableObjectAssign {
                callable,
                props,
                spreads,
            } => self.callable_object_assign_expr(callable, props, spreads, dest_ty),
            Rvalue::DictCopy { dict } => self.dict_copy_expr(dict, dest_ty),
            Rvalue::DictProjection { op, dict } => self.dict_projection_expr(*op, dict),
            Rvalue::StringSplit {
                haystack,
                separator,
                limit,
            } => self.string_split_expr(haystack, separator, limit.as_ref(), dest_ty),
            Rvalue::StringChars { haystack } => self.string_chars_expr(haystack, dest_ty),
            Rvalue::StringJoin { items, separator } => self.string_join_expr(items, separator),
            Rvalue::JsonStringify { value: json_value } => {
                self.json_stringify_expr(json_value, dest_ty)
            }
            Rvalue::HttpGetText { url } => self.http_get_expr(url),
            Rvalue::GlobalGet { global } => self.global_get_expr(*global),
            Rvalue::GlobalSet { global, value: stored } => {
                self.global_set_expr(*global, stored, dest_ty)
            }
            Rvalue::DateNow => self.date_now_expr(dest_ty),
            Rvalue::DateSetNow { timestamp } => self.date_set_now_expr(timestamp, dest_ty),
            Rvalue::DateResetNow => self.date_reset_now_expr(dest_ty),
            Rvalue::DateTimezoneOffset => {
                Ok(RustExpr::chain("SMELT_DATE_TIMEZONE_OFFSET.with(::std::cell::Cell::get)"))
            }
            Rvalue::DateSetTimezoneOffset { offset } => self.date_set_timezone_offset_expr(offset, dest_ty),
            Rvalue::DateResetTimezoneOffset => self.date_reset_timezone_offset_expr(dest_ty),
            Rvalue::VitestMockFn { implementation } => self.vitest_mock_fn_expr(implementation.as_ref()),
            Rvalue::VitestMockCalledTimes { mock, count } => self.vitest_mock_called_times_expr(mock, count),
            Rvalue::VitestMockCalledWith { mock, args, last } => self.vitest_mock_called_with_expr(mock, args, *last),
            Rvalue::VitestRestoreAllMocks => {
                Ok(RustExpr::chain("smelt_vitest_restore_all_mocks()"))
            }
            Rvalue::VitestSpyOn { target, name } => self.vitest_spy_on_expr(target, name),
            Rvalue::VitestAsymmetricEqual { actual, expected } => self.vitest_asymmetric_equal_expr(actual, expected),
            Rvalue::VitestMockLastResolvedWith { mock, expected } => self.vitest_mock_last_resolved_with_expr(mock, expected),
            Rvalue::DateTimezoneContext { timezone } => Ok(RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_timezone_name = {}; let smelt_timezone: chrono_tz::Tz = smelt_timezone_name.parse().expect(\"invalid IANA time zone\"); ::std::rc::Rc::new(move |value: SmeltUnknown| -> SmeltUnknown {{ let timestamp_ms = match value {{ SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"{SMELT_DATE}\") {{ Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }}, SmeltUnknown::String(value) => {{ let value = value.to_string_lossy(); chrono::DateTime::parse_from_rfc3339(&value).map(|date| date.timestamp_millis() as f64).unwrap_or_else(|_| value.parse::<f64>().unwrap_or(f64::NAN)) }}, SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, {}SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }}; let local_timestamp_ms = if timestamp_ms.is_finite() {{ chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map_or(f64::NAN, |date| date.with_timezone(&smelt_timezone).naive_local().and_utc().timestamp_millis() as f64) }} else {{ f64::NAN }}; SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"{SMELT_DATE}\".to_owned(), SmeltUnknown::Number(local_timestamp_ms)), (\"{SMELT_TIMEZONE}\".to_owned(), SmeltUnknown::String(smelt_timezone_name.clone().into()))]))) }}) }}",
                self.host_utf8_text(self.operand_expr(timezone)?),
                self.bigint_unknown_arm("SmeltUnknown::BigInt(value) => smelt_bigint_to_number(&value),")
            )))),
            Rvalue::DateToIsoString { timestamp_ms } => self.date_to_iso_string_expr(timestamp_ms),
            Rvalue::DateToString { timestamp_ms } => self.date_to_string_expr(timestamp_ms),
            Rvalue::DateToUtcString { timestamp_ms } => self.date_to_utc_string_expr(timestamp_ms),
            Rvalue::JsonIndent { text, space } => self.json_indent_expr(text, space),
            Rvalue::DateFromParts { parts } => {
                let text = self.date_from_parts_expr(parts)?;
                self.date_timestamp_result_expr(&text, dest_ty)
            }
            Rvalue::DateFromValue { value: date_value } => {
                let text = self.date_timestamp_expr(date_value)?;
                self.date_timestamp_result_preserving_receiver_expr(&text, dest_ty, date_value)
            }
            Rvalue::DateGetPart { part, timestamp_ms } => {
                self.date_get_part_expr(*part, timestamp_ms)
            }
            Rvalue::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => {
                let text = self.date_set_part_expr(*part, timestamp_ms, values)?;
                self.date_setter_result_expr(&text, dest_ty, timestamp_ms)
            }
            Rvalue::UrlField { field, url } => self.url_field_expr(*field, url),
            Rvalue::FileReadText { path } => self.file_read_expr(path),
            Rvalue::FileWriteText { path, text } => self.file_write_expr(path, text),
            Rvalue::BlobFromParts {
                parts,
                blob_type,
                name,
                last_modified,
            } => self.blob_new_expr(parts, blob_type, name.as_ref(), last_modified.as_ref()),
            Rvalue::BlobOp { op, blob, args } => self.blob_op_expr(*op, blob, args, dest_ty),
            Rvalue::HostConstruct { class_name, args } => {
                self.host_construct_expr(class_name, args, dest_ty)
            }
            Rvalue::BuiltinNamespace { name } => self.builtin_namespace_expr(name, dest_ty),
            Rvalue::UnmodeledImport => {
                let unknown_ty = self.type_id(Type::Unknown)?;
                let stub = self.carrier_expr(&RustExpr::chain("smelt_unmodeled_import()"));
                self.rendered_at_type(&stub, unknown_ty, dest_ty, &self.render_scope())
            }
            Rvalue::ArgumentsObject { fixed, rest } => {
                self.arguments_object_expr(fixed, rest.as_ref(), dest_ty)
            }
            Rvalue::HostGlobalRead { class } => self.host_global_read_expr(*class, dest_ty),
            Rvalue::HostGlobalWrite {
                class,
                value: stored,
            } => self.host_global_write_expr(*class, stored, dest_ty),
            Rvalue::HostGlobalPresent { class } => self.host_global_present_expr(*class),
            Rvalue::Await(operand) => self.await_rvalue_expr(operand, dest_ty),
            Rvalue::AsyncOp { op, args } => self.async_rvalue_expr(*op, args, dest_ty),
        }
    }
}

/// Whether `value` is a host API call (URLs, headers, bodies, form data,
/// blobs, crypto, text codecs, the environment, files) or a formatting
/// operation (number radix and fixed-point text, dates, `toString` tags)
/// whose string results are UTF-8 `String`s built by Rust formatting.
fn rvalue_answers_host_text(value: &Rvalue) -> bool {
    matches!(
        value,
        Rvalue::CryptoOp { .. }
            | Rvalue::HostModuleCall { .. }
            | Rvalue::FormDataOp { .. }
            | Rvalue::UrlSearchParamsOp { .. }
            | Rvalue::UrlOp { .. }
            | Rvalue::TextDecoderOp { .. }
            | Rvalue::TextEncoderOp { .. }
            | Rvalue::ProcessEnvRead { .. }
            | Rvalue::ServerResponseOp { .. }
            | Rvalue::RequestOp { .. }
            | Rvalue::ResponseOp { .. }
            | Rvalue::HeadersOp { .. }
            | Rvalue::HttpGetText { .. }
            | Rvalue::UrlField { .. }
            | Rvalue::FileReadText { .. }
            | Rvalue::BlobOp { .. }
            | Rvalue::NumericToStringRadix { .. }
            | Rvalue::NumericToFixed { .. }
            | Rvalue::ObjectToStringTag { .. }
            | Rvalue::DateToIsoString { .. }
            | Rvalue::DateToString { .. }
            | Rvalue::DateToUtcString { .. }
    )
}

impl FunctionEmitter<'_> {
    /// A host call's `text` at the JavaScript string destination `dest_ty`.
    ///
    /// Host internals answer UTF-8 `String`; a JavaScript string is a
    /// `JsString`, so the result is transcoded at this edge (`JsString::from`,
    /// mapped through an `Option`). A result that is already a `JsString`
    /// passes through `From`'s identity impl unchanged, so the adapter is
    /// sound whichever a host emitter produced. Other destinations are left
    /// alone.
    fn host_text_at_type(&self, text: RustExpr, dest_ty: TypeId) -> RustExpr {
        match self.mir.types.get(dest_ty) {
            Some(Type::String) => RustExpr::chain(format!("JsString::from({text})")),
            Some(Type::Optional(inner)) if self.mir.types.get(*inner) == Some(&Type::String) => {
                RustExpr::atom(format!("({text}).map(JsString::from)"))
            }
            _ => text,
        }
    }
}
