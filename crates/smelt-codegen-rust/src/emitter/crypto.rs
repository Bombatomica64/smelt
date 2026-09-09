//! Rust emission for the `WebCrypto` members Smelt models.
//!
//! The three keyless members of the `crypto` global. Two of them are one
//! expression each; the third calls the algorithm-dispatching helper
//! `crate::crypto_prelude` emits. Nothing here holds state, because `crypto`
//! has none — see that module's `Why no SmeltCrypto type`.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit one `WebCrypto` call.
    ///
    /// `dest_ty` is threaded through the two members whose result a caller can
    /// coerce (`randomUUID` answers a `String`, `digest` a byte view); the
    /// in-place fill answers the view it was handed, whose type the source
    /// already fixed, so there is nothing to coerce it to.
    pub(super) fn crypto_op_text(
        &self,
        op: smelt_hir::CryptoOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match op {
            // `Uuid`'s `Display` is already the spec's lowercase hyphenated
            // form, so the formatting is not respelled here.
            smelt_hir::CryptoOp::RandomUuid => {
                let string_ty = self.type_id(Type::String)?;
                self.value_at_type_text("uuid::Uuid::new_v4().to_string()", string_ty, dest_ty)
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
                // `crypto_prelude::emit_random_values_erased` for why the erased
                // arm exists and when it goes away.
                let view_ty = self.operand_ty(view)?;
                let is_concrete_view = matches!(self.mir.types.get(view_ty), Some(Type::Class { name, .. })
                    if self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::TypedArray));
                if is_concrete_view {
                    return Ok(format!(
                        "smelt_crypto_random_values(&{})",
                        self.operand_text(view)?
                    ));
                }
                Ok(format!(
                    "smelt_crypto_random_values_erased({})",
                    self.erase(view)?
                ))
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
                // Either half of the family answers `to_bytes()` directly; an
                // erased value enters through the view's boundary adapter,
                // which reads the bytes of any byte-backed record.
                let data_ty = self.operand_ty(data)?;
                let data_text = if self.is_typed_array_view_class_type(data_ty)?
                    || self.operand_is_array_buffer(data)?
                {
                    self.operand_text(data)?
                } else {
                    format!("SmeltTypedArray::smelt_from_unknown({})", self.erase(data)?)
                };
                Ok(format!(
                    "{{ let smelt_algorithm = ({algorithm_text}).clone(); let smelt_data = ({data_text}).to_bytes(); SmeltFuture::from_future(Box::pin(async move {{ smelt_crypto_digest(&smelt_algorithm, &smelt_data) }})) }}"
                ))
            }
        }
    }
}
