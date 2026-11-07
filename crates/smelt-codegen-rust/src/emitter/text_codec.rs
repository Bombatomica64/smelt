//! Rust emission for the WHATWG text codecs and the concrete byte view.
//!
//! Every operation lands on a real method of the concrete struct emitted by
//! `crate::text_codec_prelude`, so the generated call is what a hand-written
//! Rust program would say: `encoder.encode(text)` is `encoder.encode(&text)`
//! typed `SmeltUint8Array`, and `decoder.decode(bytes)` is
//! `decoder.decode(&bytes)` typed `String` — no tagged value and no runtime
//! member lookup in between.

use crate::rust::RustExpr;
use super::*;

impl FunctionEmitter<'_> {
    /// Emit `new TextDecoder(label?)`.
    ///
    /// The label has already been checked against the encoding standard's
    /// UTF-8 rows during lowering, so the only thing left here is to pass it to
    /// the constructor that normalizes it.
    pub(super) fn text_decoder_new_expr(
        &self,
        label: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let Some(label_operand) = label else {
            return Ok(RustExpr::atom("SmeltTextDecoder::new()"));
        };
        let string_ty = self.type_id(Type::String)?;
        let label_text = self.value_at_type(label_operand, string_ty)?;
        Ok(RustExpr::atom(format!("SmeltTextDecoder::from_label(&{label_text})")))
    }

    /// Emit one `TextEncoder` member as a method call.
    pub(super) fn text_encoder_op_expr(
        &self,
        op: smelt_hir::TextEncoderOp,
        encoder: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(encoder)?;
        match op {
            smelt_hir::TextEncoderOp::Encoding => {
                let string_ty = self.type_id(Type::String)?;
                self.rendered_at_type(&RustExpr::postfix_on(&receiver, format!("{receiver}.encoding()")), string_ty, dest_ty, &self.render_scope())
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
                Ok(RustExpr::postfix_on(&receiver, format!("{receiver}.encode({input})")))
            }
        }
    }

    /// Emit one `TextDecoder` member as a method call.
    pub(super) fn text_decoder_op_expr(
        &self,
        op: smelt_hir::TextDecoderOp,
        decoder: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(decoder)?;
        let string_ty = self.type_id(Type::String)?;
        match op {
            smelt_hir::TextDecoderOp::Encoding => {
                self.rendered_at_type(&RustExpr::postfix_on(&receiver, format!("{receiver}.encoding()")), string_ty, dest_ty, &self.render_scope())
            }
            smelt_hir::TextDecoderOp::Decode => {
                let text = match args.first() {
                    // `decode()` with no argument answers the empty string.
                    None => RustExpr::postfix_on(&receiver, format!("{receiver}.decode(&SmeltUint8Array::new())")),
                    Some(arg) => {
                        let bytes = self.byte_view_argument_expr(arg)?;
                        RustExpr::postfix_on(&receiver, format!("{receiver}.decode(&{bytes})"))
                    }
                };
                self.rendered_at_type(&text, string_ty, dest_ty, &self.render_scope())
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
    fn byte_view_argument_expr(&self, arg: &Operand) -> Result<RustExpr, EmitError> {
        let arg_ty = self.operand_ty(arg)?;
        let text = self.operand_expr(arg)?;
        if let Some(Type::Class { name, .. }) = self.mir.types.get(arg_ty)
            && self.stdlib_class_of_symbol(*name)? == Some(smelt_stdlib::StdlibClass::TypedArray)
        {
            return Ok(text);
        }
        let ty = self.operand_ty(arg)?;
        let erased = self.erase_rendered(&text, ty)?;
        Ok(RustExpr::atom(self.carrier_format(format_args!("SmeltTypedArray::smelt_from_unknown({erased})"))))
    }

    /// Emit a member read or method call on the concrete typed-array family.
    ///
    /// Every arm renders the spec member of the SAME name on the generated
    /// runtime type, so Rust's own method resolution decides whether a `slice`
    /// is the view's (element indices, a copy in fresh storage) or the storage
    /// half's (byte indices). That is why one op enum covers both halves: the
    /// receiver already carries the answer.
    ///
    /// The result type is the op's own, coerced to the destination the caller
    /// asked for — which is where a view crossing into an erased destination
    /// picks up its boundary adapter.
    pub(super) fn byte_array_op_expr(
        &self,
        op: smelt_hir::ByteArrayOp,
        bytes: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver = self.operand_expr(bytes)?;
        let receiver_ty = self.operand_ty(bytes)?;
        let float_ty = self.type_id(Type::Float)?;
        let (text, source_ty) = match op {
            smelt_hir::ByteArrayOp::Length => (RustExpr::postfix_on(&receiver, format!("{receiver}.length()")), float_ty),
            smelt_hir::ByteArrayOp::ByteLength => (RustExpr::postfix_on(&receiver, format!("{receiver}.byte_length()")), float_ty),
            smelt_hir::ByteArrayOp::ByteOffset => (RustExpr::postfix_on(&receiver, format!("{receiver}.byte_offset()")), float_ty),
            // `.buffer` SHARES the storage rather than copying it, so a write
            // through a second view over the same buffer is visible here.
            smelt_hir::ByteArrayOp::Buffer => {
                (RustExpr::postfix_on(&receiver, format!("{receiver}.buffer()")), self.array_buffer_type_id()?)
            }
            smelt_hir::ByteArrayOp::Subarray => (
                RustExpr::postfix_on(&receiver, format!("{receiver}.subarray({})", self.byte_range_args_expr(args)?)),
                receiver_ty,
            ),
            smelt_hir::ByteArrayOp::Slice => (
                RustExpr::postfix_on(&receiver, format!("{receiver}.slice({})", self.byte_range_args_expr(args)?)),
                receiver_ty,
            ),
            // `set` and `fill` are the two mutating members. `set` takes a
            // SOURCE view, converting per element rather than per byte, so a
            // non-view source enters through the view's boundary adapter the
            // same way `decode`'s argument does.
            smelt_hir::ByteArrayOp::Set => {
                let source = args.first().ok_or_else(|| {
                    EmitError::new("internal: a typed-array `set` needs its source")
                })?;
                let source_text = self.byte_view_argument_expr(source)?;
                let offset = match args.get(1) {
                    Some(offset) => {
                        format!("(({}) as f64).max(0.0) as usize", self.numeric_operand_expr(offset)?)
                    }
                    None => "0".to_owned(),
                };
                let none_ty = self.type_id(Type::None)?;
                (
                    RustExpr::postfix_on(&receiver, format!("{receiver}.set_from(&{source_text}, {offset})")),
                    none_ty,
                )
            }
            smelt_hir::ByteArrayOp::Fill => {
                let value = args.first().ok_or_else(|| {
                    EmitError::new("internal: a typed-array `fill` needs its value")
                })?;
                let value_text = self.numeric_operand_expr(value)?;
                let range = self.byte_range_args_expr(&args[1..])?;
                (
                    RustExpr::postfix_on(&receiver, format!("{receiver}.fill({value_text}, {range})")),
                    receiver_ty,
                )
            }
            smelt_hir::ByteArrayOp::Elements => {
                let list_ty = self.type_id(Type::List(float_ty))?;
                (
                    RustExpr::atom(format!("SmeltList::new({receiver}.to_elements())")),
                    list_ty,
                )
            }
            smelt_hir::ByteArrayOp::IndexKeys => {
                let string_ty = self.type_id(Type::String)?;
                let list_ty = self.type_id(Type::List(string_ty))?;
                (
                    RustExpr::atom(format!(
                        "SmeltList::new((0..{receiver}.length() as usize).map(|index| JsString::from(index.to_string())).collect::<Vec<_>>())"
                    )),
                    list_ty,
                )
            }
            smelt_hir::ByteArrayOp::IndexEntries => {
                let string_ty = self.type_id(Type::String)?;
                let pair_ty = self.type_id(Type::Tuple(Vec::from([string_ty, float_ty])))?;
                let list_ty = self.type_id(Type::List(pair_ty))?;
                (
                    RustExpr::atom(format!(
                        "SmeltList::new({receiver}.to_elements().into_iter().enumerate().map(|(index, element)| (JsString::from(index.to_string()), element)).collect::<Vec<_>>())"
                    )),
                    list_ty,
                )
            }
        };
        self.rendered_at_type(&text, source_ty, dest_ty, &self.render_scope())
    }

    /// Render the `(start, end)` pair every range member of the family takes.
    ///
    /// The bounds are `i64` because a negative bound counts back from the end,
    /// and the end is an `Option` because "omitted" and "zero" are different
    /// answers. Clamping is the prelude's, in one place, rather than repeated
    /// per call site.
    fn byte_range_args_expr(&self, args: &[Operand]) -> Result<RustExpr, EmitError> {
        let start = match args.first() {
            Some(start) => format!("({}) as i64", self.numeric_operand_expr(start)?),
            None => "0".to_owned(),
        };
        let end = match args.get(1) {
            Some(end) => format!("Some(({}) as i64)", self.numeric_operand_expr(end)?),
            None => "None".to_owned(),
        };
        Ok(RustExpr::loose(format!("{start}, {end}")))
    }
}
