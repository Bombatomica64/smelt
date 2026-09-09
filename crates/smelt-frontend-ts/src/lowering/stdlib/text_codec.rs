//! WHATWG text-codec lowering for the TypeScript frontend.
//!
//! `TextEncoder`, `TextDecoder`, and the concrete byte view the encoder answers
//! are modeled the same way the other standards types are: as *concrete* Rust
//! values with typed members, never as marker-bearing records. So
//! `new TextEncoder()` becomes [`ExprKind::TextEncoderNew`] typed
//! `Type::Class { name: TextEncoder }`, `encoder.encode(text)` becomes an
//! [`ExprKind::TextEncoderOp`] whose HIR type is the byte-view class, and
//! `decoder.decode(bytes)` is a `String` — the member's exact source type in
//! every case, so no caller re-narrows anything.
//!
//! # Interface
//!
//! * [`ModuleBuilder::text_encoder_constructor_expression`] and
//!   [`ModuleBuilder::text_decoder_constructor_expression`] are called from
//!   `new_expr.rs` when the constructor names the host class and no user class
//!   shadows it.
//! * [`ModuleBuilder::dispatch_text_encoder_method`] and
//!   [`ModuleBuilder::dispatch_text_decoder_method`] are registered in the
//!   builtin call-handler chain and recognize receiver/member pairs through the
//!   shared `smelt-stdlib` method metadata
//!   (`TypeScriptReceiverKind::TextEncoder` / `::TextDecoder`), never by
//!   matching a member name here: `encode` and `decode` are ordinary user
//!   method names too, so recognition has to be receiver-typed.
//! * [`ModuleBuilder::text_codec_member_read`] is called from the
//!   static-member read chain, after the receiver is lowered, and answers the
//!   data properties (`encoding` on either codec, `length`/`byteLength` on a
//!   byte view).
//!
//! # Why the byte view has a synthetic class name
//!
//! `TextEncoder.encode` returns a `Uint8Array` in the source types, but the
//! spelling `Uint8Array` still means the byte-backed HOST RECORD that the
//! eleven typed-array views share (`smelt_stdlib::host_object`): a view's
//! runtime identity carries an element type, a byte offset, and a shared
//! `ArrayBuffer` that reflective construction
//! (`new Object.getPrototypeOf(x).constructor(..)`) reads back. That is now
//! modeled: increment 3 of the typed-array plan made the eleven view spellings
//! the concrete `SmeltTypedArray` family, so `encode` answers a `Uint8Array`
//! under its own name and the reserved synthetic spelling that stood in for it
//! is gone.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Expression;
use smelt_hir::{Body, Expr, ExprKind, TextDecoderOp, TextEncoderOp, Type};
use smelt_stdlib::RuleId;

/// The encoding labels the WHATWG encoding standard maps to UTF-8.
///
/// The spec's label table is case-insensitive and whitespace-trimmed, and these
/// are its UTF-8 rows. Any other label names an encoding whose decoder Smelt
/// does not implement, and that is a named blocker rather than a decoder that
/// silently answers UTF-8.
const UTF8_LABELS: [&str; 6] = [
    "utf-8",
    "utf8",
    "unicode-1-1-utf-8",
    "unicode11utf8",
    "unicode20utf8",
    "x-unicode20utf8",
];

impl ModuleBuilder<'_> {
    /// Lower `new TextEncoder()` into a concrete encoder value.
    ///
    /// The spec gives the constructor no parameters, so an argument is a source
    /// error rather than something to ignore.
    pub(in crate::lowering) fn text_encoder_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        if !new_expr.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "`new TextEncoder()` takes no arguments",
            ));
        }
        let ty = self.text_encoder_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::TextEncoderNew,
            ty,
            span,
        }))
    }

    /// Lower `new TextDecoder(label?)` into a concrete decoder value.
    ///
    /// A literal label outside the UTF-8 rows of the encoding standard's label
    /// table is refused here, where the source span is still available, rather
    /// than decoded as UTF-8 at runtime. A non-literal label is refused for the
    /// same reason: the decoder it selects is not knowable at emit time, and
    /// answering UTF-8 anyway would be a silently wrong program.
    pub(in crate::lowering) fn text_decoder_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        // A second `TextDecoderOptions` argument selects `fatal` (throw instead
        // of substituting U+FFFD) and `ignoreBOM`. Neither is modeled, and both
        // change what the decoder answers, so an options object is refused
        // rather than dropped.
        if new_expr.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                span,
                "`new TextDecoder(label, options)`: the `fatal`/`ignoreBOM` options are not modeled",
            ));
        }
        let label = match new_expr.arguments.first() {
            None => None,
            Some(argument) => {
                let literal =
                    argument
                        .as_expression()
                        .and_then(|expression| match expression {
                            Expression::StringLiteral(literal) => Some(literal.value.as_str()),
                            _ => None,
                        });
                match literal {
                    Some(text)
                        if UTF8_LABELS.contains(&text.trim().to_ascii_lowercase().as_str()) =>
                    {
                        Some(self.argument(argument, body)?)
                    }
                    Some(text) => {
                        return Err(SmeltError::unsupported(
                            span,
                            format!(
                                "`new TextDecoder(\"{text}\")`: only the UTF-8 encoding labels are modeled"
                            ),
                        ));
                    }
                    None => {
                        return Err(SmeltError::unsupported(
                            span,
                            "`new TextDecoder(label)` needs a literal encoding label: only the UTF-8 labels are modeled",
                        ));
                    }
                }
            }
        };
        let ty = self.text_decoder_type();
        Ok(body.push_expr(Expr {
            kind: ExprKind::TextDecoderNew { label },
            ty,
            span,
        }))
    }

    /// Dispatch a modeled `TextEncoder` method on a concrete receiver.
    pub(in crate::lowering) fn dispatch_text_encoder_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::TextEncoder,
            member_name,
        ) != Some(RuleId::TsTextEncoderEncode)
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_text_encoder_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        // `encode()` with no argument encodes the empty string, which the spec
        // reaches by defaulting the parameter to `""`.
        let args = match call.arguments.first() {
            Some(argument) => Vec::from([self.argument(argument, body)?]),
            None => Vec::new(),
        };
        let ty = self.byte_array_type();
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::TextEncoderOp {
                op: TextEncoderOp::Encode,
                encoder: receiver,
                args,
            },
            ty,
            span,
        })))
    }

    /// Dispatch a modeled `TextDecoder` method on a concrete receiver.
    pub(in crate::lowering) fn dispatch_text_decoder_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::TextDecoder,
            member_name,
        ) != Some(RuleId::TsTextDecoderDecode)
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_text_decoder_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        // `decode()` with no argument answers the empty string.
        let args = match call.arguments.first() {
            Some(argument) => Vec::from([self.argument(argument, body)?]),
            None => Vec::new(),
        };
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::TextDecoderOp {
                op: TextDecoderOp::Decode,
                decoder: receiver,
                args,
            },
            ty,
            span,
        })))
    }

    /// Read a data property of a codec or of a concrete byte view.
    ///
    /// Called from the static-member chain AFTER the receiver has been lowered,
    /// and given that receiver: `length` is a member of nearly every value, so
    /// a probe that lowered the receiver itself would duplicate the receiver's
    /// side effects on every miss.
    pub(in crate::lowering) fn text_codec_member_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(member.span.start, member.span.end);
        match member.property.name.as_str() {
            "encoding" if self.is_text_encoder_type(receiver_ty) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::TextEncoderOp {
                        op: TextEncoderOp::Encoding,
                        encoder: receiver,
                        args: Vec::new(),
                    },
                    ty: string_ty,
                    span,
                })))
            }
            "encoding" if self.is_text_decoder_type(receiver_ty) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::TextDecoderOp {
                        op: TextDecoderOp::Encoding,
                        decoder: receiver,
                        args: Vec::new(),
                    },
                    ty: string_ty,
                    span,
                })))
            }
            _ => Ok(None),
        }
    }

    /// Return the modeled `TextEncoder` class type.
    pub(in crate::lowering) fn text_encoder_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("TextEncoder");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return the modeled `TextDecoder` class type.
    pub(in crate::lowering) fn text_decoder_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("TextDecoder");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return the modeled concrete byte-view class type: a `Uint8Array`.
    ///
    /// The SOURCE spelling, not a reserved synthetic name. It was synthetic
    /// while the source spelling still meant the erased byte-backed record and
    /// rebinding it would have changed the meaning of existing programs;
    /// increment 3 of the typed-array plan made `Uint8Array` the concrete
    /// family, so the two names denoted the same Rust type under two spellings
    /// and the synthetic one only hid what `encode` answers. A program can now
    /// write `const bytes: Uint8Array = new TextEncoder().encode(text)` and
    /// have it mean what it says.
    pub(in crate::lowering) fn byte_array_type(&mut self) -> smelt_hir::TypeId {
        self.typed_array_class_type("Uint8Array")
    }

    /// Return whether a lowered type is the modeled `TextEncoder` class.
    pub(in crate::lowering) fn is_text_encoder_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::TextEncoder)
            && !self.user_class_shadows("TextEncoder")
    }

    /// Return whether a lowered type is the modeled `TextDecoder` class.
    pub(in crate::lowering) fn is_text_decoder_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::TextDecoder)
            && !self.user_class_shadows("TextDecoder")
    }

}
