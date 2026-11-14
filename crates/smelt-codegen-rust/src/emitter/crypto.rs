//! Rust emission for the `WebCrypto` members Smelt models.
//!
//! The three keyless members of the `crypto` global. Two of them are one
//! expression each; the third calls the algorithm-dispatching helper
//! `smelt-runtime`'s `crypto.rs` emits. Nothing here holds state, because `crypto`
//! has none — see that module's `Why no SmeltCrypto type`.

use super::union::union_name;
use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Emit one `WebCrypto` call.
    ///
    /// `dest_ty` is threaded through `randomUUID`, the one member whose result a
    /// caller can coerce (it answers a `String`); `digest` answers a promise of an
    /// `ArrayBuffer` (a `SmeltFuture` resolving to a `SmeltArrayBuffer`); the
    /// in-place fill answers the view it was handed, whose type the source
    /// already fixed, so there is nothing to coerce it to.
    pub(super) fn crypto_op_expr(
        &self,
        op: smelt_hir::CryptoOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match op {
            // `Uuid`'s `Display` is already the spec's lowercase hyphenated
            // form, so the formatting is not respelled here.
            smelt_hir::CryptoOp::RandomUuid => {
                let string_ty = self.type_id(Type::String)?;
                self.rendered_at_type(
                    &RustExpr::atom("uuid::Uuid::new_v4().to_string()"),
                    string_ty,
                    dest_ty,
                    &self.render_scope(),
                )
            }
            // The argument is passed BY REFERENCE: the helper fills the view
            // through its shared byte storage and hands back a value sharing
            // that storage and its JS reference id, which is what keeps the
            // source's `getRandomValues(buffer) === buffer` true.
            smelt_hir::CryptoOp::GetRandomValues => {
                let Some(view) = args.first() else {
                    return Err(EmitError::new(
                        "crypto.getRandomValues requires the view to fill",
                    ));
                };
                // Two arms, chosen by the argument's own type rather than by its
                // spelling. A concrete `SmeltUint8Array` fills through its byte
                // storage directly; a view that is still the byte-backed host
                // record fills through the record's storage helpers. See
                // `smelt-runtime`'s `crypto.rs` for why the erased
                // arm exists and when it goes away.
                let view_ty = self.operand_ty(view)?;
                let is_concrete_view = matches!(self.mir.types.get(view_ty), Some(Type::Class { name, .. })
                    if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::TypedArray));
                if is_concrete_view {
                    return Ok(RustExpr::chain(format!(
                        "smelt_crypto_random_values(&{})",
                        self.operand_expr(view)?.tight()
                    )));
                }
                Ok(RustExpr::chain(format!(
                    "smelt_crypto_random_values_erased({})",
                    self.erase(view)?.delimited()
                )))
            }
            // `digest` is `async` in the spec, so the value has to be a real
            // future or the source's `await` has nothing to poll — the same
            // shape `Blob.arrayBuffer()` takes. The hash itself is synchronous,
            // so the future is already-resolved rather than deferred work.
            smelt_hir::CryptoOp::Digest => {
                let (Some(algorithm), Some(data)) = (args.first(), args.get(1)) else {
                    return Err(EmitError::new(
                        "crypto.subtle.digest requires an algorithm and the data to hash",
                    ));
                };
                let string_ty = self.type_id(Type::String)?;
                let algorithm_text = self.value_at_type(algorithm, string_ty)?;
                let data_text = self.crypto_bytes_expr(data)?;
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_algorithm = ({algorithm_text}).clone(); let smelt_data = {data_text}; SmeltFuture::from_future(Box::pin(async move {{ smelt_crypto_digest(&smelt_algorithm, &smelt_data) }})) }}",
                    algorithm_text = algorithm_text.delimited(),
                    data_text = data_text.delimited()
                )))
            }
            // `importKey`: the format, key bytes, algorithm name and hash are
            // normalized by the frontend; the key is built synchronously and
            // handed back as an already-resolved future, as `digest` is.
            smelt_hir::CryptoOp::ImportKey => {
                let [format, key_data, name, hash, extractable, usages] = args else {
                    return Err(EmitError::new(
                        "crypto.subtle.importKey requires six normalized arguments",
                    ));
                };
                let string_ty = self.type_id(Type::String)?;
                let bool_ty = self.type_id(Type::Bool)?;
                let format_text = self.value_at_type(format, string_ty)?;
                let name_text = self.value_at_type(name, string_ty)?;
                let hash_text = self.value_at_type(hash, string_ty)?;
                let extractable_text = self.value_at_type(extractable, bool_ty)?;
                let key_text = self.crypto_bytes_expr(key_data)?;
                let usages_text = self.crypto_usages_expr(usages)?;
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_format = ({format_text}).clone(); let smelt_key = {key_text}; let smelt_name = ({name_text}).clone(); let smelt_hash = ({hash_text}).clone(); let smelt_extractable = {extractable_text}; let smelt_usages = {usages_text}; SmeltFuture::from_future(Box::pin(async move {{ smelt_crypto_import_key(&smelt_format, smelt_key, &smelt_name, &smelt_hash, smelt_extractable, smelt_usages) }})) }}",
                    format_text = format_text.delimited(),
                    key_text = key_text.delimited(),
                    name_text = name_text.delimited(),
                    hash_text = hash_text.delimited(),
                    extractable_text = extractable_text.delimited(),
                    usages_text = usages_text.delimited()
                )))
            }
            smelt_hir::CryptoOp::Sign => {
                let [algorithm, key, data] = args else {
                    return Err(EmitError::new(
                        "crypto.subtle.sign requires an algorithm, a key and data",
                    ));
                };
                let string_ty = self.type_id(Type::String)?;
                let algorithm_text = self.value_at_type(algorithm, string_ty)?;
                let key_text = self.crypto_key_expr(key)?;
                let data_text = self.crypto_bytes_expr(data)?;
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_algorithm = ({algorithm_text}).clone(); let smelt_key = {key_text}; let smelt_data = {data_text}; SmeltFuture::from_future(Box::pin(async move {{ smelt_crypto_sign(&smelt_algorithm, &smelt_key, &smelt_data) }})) }}",
                    algorithm_text = algorithm_text.delimited(),
                    key_text = key_text.delimited(),
                    data_text = data_text.delimited()
                )))
            }
            smelt_hir::CryptoOp::Verify => {
                let [algorithm, key, signature, data] = args else {
                    return Err(EmitError::new(
                        "crypto.subtle.verify requires an algorithm, a key, a signature and data",
                    ));
                };
                let string_ty = self.type_id(Type::String)?;
                let algorithm_text = self.value_at_type(algorithm, string_ty)?;
                let key_text = self.crypto_key_expr(key)?;
                let signature_text = self.crypto_bytes_expr(signature)?;
                let data_text = self.crypto_bytes_expr(data)?;
                Ok(RustExpr::atom(format!(
                    "{{ let smelt_algorithm = ({algorithm_text}).clone(); let smelt_key = {key_text}; let smelt_signature = {signature_text}; let smelt_data = {data_text}; SmeltFuture::from_future(Box::pin(async move {{ smelt_crypto_verify(&smelt_algorithm, &smelt_key, &smelt_signature, &smelt_data) }})) }}",
                    algorithm_text = algorithm_text.delimited(),
                    key_text = key_text.delimited(),
                    signature_text = signature_text.delimited(),
                    data_text = data_text.delimited()
                )))
            }
        }
    }

    /// The bytes of a `BufferSource` operand, as a `Vec<u8>` expression.
    ///
    /// Either half of the family answers `to_bytes()` directly; a concrete
    /// union of byte-backed arms reads its live arm; an erased value enters
    /// through the view's boundary adapter, which reads the bytes of any
    /// byte-backed record.
    fn crypto_bytes_expr(&self, data: &Operand) -> Result<RustExpr, EmitError> {
        let data_ty = self.operand_ty(data)?;
        if self.is_typed_array_view_class_type(data_ty)? || self.operand_is_array_buffer(data)? {
            return Ok(RustExpr::atom(format!(
                "({}).to_bytes()",
                self.operand_expr(data)?.delimited()
            )));
        }
        if let Some(bytes_text) =
            self.byte_source_union_bytes_expr(&self.operand_expr(data)?, data_ty)?
        {
            return Ok(bytes_text);
        }
        Ok(RustExpr::atom(self.carrier_format(
            self.carrier()?,
            format_args!(
                "SmeltTypedArray::smelt_from_unknown({}).to_bytes()",
                self.erase(data)?.delimited()
            ),
        )))
    }

    /// A `CryptoKey` operand as a `SmeltCryptoKey` value: the concrete key, or
    /// one recovered from an erased value.
    fn crypto_key_expr(&self, key: &Operand) -> Result<RustExpr, EmitError> {
        let key_ty = self.operand_ty(key)?;
        if matches!(self.mir.types.get(key_ty), Some(Type::Class { name, .. })
            if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::CryptoKey))
        {
            return Ok(self.operand_expr(key)?.cloned_value());
        }
        Ok(RustExpr::atom(self.carrier_format(
            self.carrier()?,
            format_args!(
                "SmeltCryptoKey::smelt_from_unknown({})",
                self.erase(key)?.delimited()
            ),
        )))
    }

    /// `importKey`'s usages as a `Vec<String>`: a string list is copied, and
    /// anything else (an erased list) is read element by element.
    fn crypto_usages_expr(&self, usages: &Operand) -> Result<RustExpr, EmitError> {
        let usages_ty = self.operand_ty(usages)?;
        if matches!(self.mir.types.get(usages_ty), Some(Type::List(item))
            if matches!(self.mir.types.get(*item), Some(Type::String | Type::PyStr)))
        {
            return Ok(self
                .operand_expr(usages)?
                .postfix(".borrow()")
                .cloned_value());
        }
        Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
            "match {} {{ SmeltUnknown::Array(smelt_items) => smelt_items.into_vec().into_iter().filter_map(|smelt_item| match smelt_item {{ SmeltUnknown::String(smelt_text) => Some(smelt_text.to_string()), _ => None }}).collect::<Vec<String>>(), _ => Vec::new() }}",
            self.erase(usages)?.delimited()
        ))))
    }

    /// Render the bytes of a concrete union whose every arm is byte-backed.
    ///
    /// `ArrayBufferView | ArrayBuffer` (the spec's `BufferSource`) lowers to a
    /// generated union whose arms are typed-array views, `DataView`s and
    /// `ArrayBuffer`s; each of those answers `to_bytes()`. The result is a
    /// `match` over every arm yielding a `Vec<u8>`, so the live arm is read
    /// without erasing the value or projecting it onto one member. Returns
    /// `None` when `ty` is not such a union.
    pub(super) fn byte_source_union_bytes_expr(
        &self,
        value_text: &RustExpr,
        ty: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(members) = self.concrete_union_members(ty) else {
            return Ok(None);
        };
        for member in members {
            let Some(Type::Class { name, .. }) = self.mir.types.get(*member) else {
                return Ok(None);
            };
            if !matches!(
                self.stdlib_class_of_symbol(*name)?,
                Some(
                    smelt_stdlib::StdlibClass::TypedArray
                        | smelt_stdlib::StdlibClass::DataView
                        | smelt_stdlib::StdlibClass::ArrayBuffer
                )
            ) {
                return Ok(None);
            }
        }
        let union_enum_name = union_name(ty);
        let arms = (0..members.len())
            .map(|index| format!("{union_enum_name}::M{index}(value) => value.to_bytes()"))
            .collect::<Vec<_>>();
        Ok(Some(RustExpr::atom(format!(
            "match &({value_text}) {{ {} }}",
            arms.join(", "),
            value_text = value_text.delimited()
        ))))
    }
}
