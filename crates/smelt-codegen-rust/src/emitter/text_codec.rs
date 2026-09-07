//! Rust emission for the WHATWG text codecs and the concrete byte view.
//!
//! Every operation lands on a real method of the concrete struct emitted by
//! `crate::text_codec_prelude`, so the generated call is what a hand-written
//! Rust program would say: `encoder.encode(text)` is `encoder.encode(&text)`
//! typed `SmeltUint8Array`, and `decoder.decode(bytes)` is
//! `decoder.decode(&bytes)` typed `String` — no tagged value and no runtime
//! member lookup in between.

use super::*;

impl FunctionEmitter<'_> {
    /// Emit `new TextDecoder(label?)`.
    ///
    /// The label has already been checked against the encoding standard's
    /// UTF-8 rows during lowering, so the only thing left here is to pass it to
    /// the constructor that normalizes it.
    pub(super) fn text_decoder_new_text(
        &self,
        label: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let Some(label_operand) = label else {
            return Ok("SmeltTextDecoder::new()".to_owned());
        };
        let string_ty = self.type_id(Type::String)?;
        let label_text = self.value_at_type(label_operand, string_ty)?;
        Ok(format!("SmeltTextDecoder::from_label(&{label_text})"))
    }

    /// Emit one `TextEncoder` member as a method call.
    pub(super) fn text_encoder_op_text(
        &self,
        op: smelt_hir::TextEncoderOp,
        encoder: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(encoder)?;
        match op {
            smelt_hir::TextEncoderOp::Encoding => {
                let string_ty = self.type_id(Type::String)?;
                self.value_at_type_text(&format!("{receiver}.encoding()"), string_ty, dest_ty)
            }
            smelt_hir::TextEncoderOp::Encode => {
                // `encode()` with no argument encodes the empty string, which
                // is the spec's own parameter default.
                let input = match args.first() {
                    None => "\"\"".to_owned(),
                    Some(arg) => {
                        let string_ty = self.type_id(Type::String)?;
                        format!("&{}", self.value_at_type(arg, string_ty)?)
                    }
                };
                Ok(format!("{receiver}.encode({input})"))
            }
        }
    }

    /// Emit one `TextDecoder` member as a method call.
    pub(super) fn text_decoder_op_text(
        &self,
        op: smelt_hir::TextDecoderOp,
        decoder: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(decoder)?;
        let string_ty = self.type_id(Type::String)?;
        match op {
            smelt_hir::TextDecoderOp::Encoding => {
                self.value_at_type_text(&format!("{receiver}.encoding()"), string_ty, dest_ty)
            }
            smelt_hir::TextDecoderOp::Decode => {
                let text = match args.first() {
                    // `decode()` with no argument answers the empty string.
                    None => format!("{receiver}.decode(&SmeltUint8Array::new())"),
                    Some(arg) => {
                        let bytes = self.byte_view_argument_text(arg)?;
                        format!("{receiver}.decode(&{bytes})")
                    }
                };
                self.value_at_type_text(&text, string_ty, dest_ty)
            }
        }
    }

    /// Emit the byte-view argument of `decode`.
    ///
    /// A concrete byte view is passed straight through. Any OTHER byte source —
    /// a `new Uint8Array(..)`, an `ArrayBuffer`, a `DataView`, or a generic `T`
    /// that is one of them at runtime — is still the byte-backed host record, so
    /// it enters through the view's `SmeltFromUnknown` boundary adapter rather
    /// than being refused: `decoder.decode(new Uint8Array([..]))` is the
    /// commonest spelling in the corpora, and the adapter reads exactly the
    /// bytes the record carries.
    fn byte_view_argument_text(&self, arg: &Operand) -> Result<String, EmitError> {
        let arg_ty = self.operand_ty(arg)?;
        let text = self.operand_text(arg)?;
        if let Some(Type::Class { name, .. }) = self.mir.types.get(arg_ty)
            && self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::ByteArray)
        {
            return Ok(text);
        }
        Ok(format!("SmeltUint8Array::smelt_from_unknown({text})"))
    }

    /// Emit a size read on a concrete byte view.
    pub(super) fn byte_array_op_text(
        &self,
        op: smelt_hir::ByteArrayOp,
        bytes: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let receiver = self.operand_text(bytes)?;
        let text = match op {
            smelt_hir::ByteArrayOp::Length => format!("{receiver}.length()"),
            smelt_hir::ByteArrayOp::ByteLength => format!("{receiver}.byte_length()"),
        };
        let float_ty = self.type_id(Type::Float)?;
        self.value_at_type_text(&text, float_ty, dest_ty)
    }
}
