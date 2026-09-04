//! Node `Buffer` byte-buffer lowering.
//!
//! `Buffer` is Node's binary-data host object. es-toolkit constructs it
//! (`Buffer.from`, `Buffer.alloc`, `Buffer.concat`) and inspects it via
//! `Buffer.isBuffer(x)` / `value instanceof Buffer`, so — unlike a shapeless
//! dynamic value — it needs a concrete representation that carries a resolvable
//! *identity*.
//!
//! Smelt models a `Buffer` as a byte-backed host record — the same representation
//! `ArrayBuffer`, `DataView` and the eleven typed-array views use, built by the one
//! shared runtime constructor (`ExprKind::HostConstruct`) so a directly-built
//! buffer and a reflectively-built clone of it are the same shape. `Buffer`
//! subclasses `Uint8Array`, so the registry gives it that `Uint8` element type and
//! the `[object Uint8Array]` spec tag. The `__smelt_buffer`
//! marker is the single source of truth for `Buffer` identity: it lives in the
//! shared `smelt_stdlib::host_object` registry so `instanceof Buffer`
//! (`instance_of_text`) and `Buffer.isBuffer` resolve through the same key, and
//! the runtime for-in filter hides the marker. The retained `bytes` list plus
//! the mirrored `length`/`byteLength` fields give the observable byte-length
//! reads (`buf.length`, `buf.byteLength`) a concrete backing through the erased
//! object member path instead of a fabricated constant.

use oxc::ast::ast::{Argument, CallExpression, Expression};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, Literal, Type};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;

/// The JavaScript class name the shared byte-buffer constructor is keyed by.
///
/// `ExprKind::HostConstruct` names the host *class*; the shared
/// `smelt_stdlib::host_object` registry maps that name to the `__smelt_buffer`
/// identity marker and to `Buffer`'s `Uint8` element type, so the construction
/// side here, the `instanceof` lowering, and the runtime for-in filter cannot
/// drift.
const BUFFER_CLASS_NAME: &str = "Buffer";

impl ModuleBuilder<'_> {
    /// Route recognized static `Buffer.*` calls to their dedicated handlers.
    ///
    /// Dispatched from the shared recognition metadata (`RuleId::TsBufferStatic`,
    /// see `recognition.rs`), so only the exact `Buffer.from`/`Buffer.alloc`/
    /// `Buffer.concat`/`Buffer.isBuffer` spellings reach here.
    pub(in crate::lowering) fn exact_buffer_static_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.buffer_from_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.buffer_alloc_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.buffer_concat_call(call, body)? {
            return Ok(Some(expr));
        }
        self.buffer_is_buffer_call(call, body)
    }

    /// Return whether a `Buffer.<member>` static call is shadowed by a
    /// user-declared `Buffer` class, so the stdlib handler must decline.
    ///
    /// Mirrors the `!self.classes.contains(...)` guard the other host-object
    /// statics use: a source project that declares its own `class Buffer` owns
    /// the name and its statics dispatch through the ordinary class path.
    fn buffer_static_receiver<'a>(
        &self,
        call: &'a CallExpression<'a>,
        member_name: &str,
    ) -> Option<&'a oxc::ast::ast::StaticMemberExpression<'a>> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Buffer"
            || member.property.name != member_name
            || self.classes.contains("Buffer")
        {
            return None;
        }
        Some(member)
    }

    /// Build the modeled `Buffer` record from an already-lowered byte-list
    /// expression, erased to `SmeltUnknown`.
    ///
    /// This routes through `ExprKind::HostConstruct` — the *same* runtime
    /// byte-buffer constructor `new Uint8Array(...)`, `new ArrayBuffer(...)` and
    /// the reflected `new getPrototypeOf(x).constructor(...)` path all call — so a
    /// `Buffer.from([1, 2, 3])` and a cloned `Buffer` are byte-for-byte the same
    /// record shape. Building the record inline here instead left
    /// `Buffer.from(...)` without the `buffer`/`byteOffset` slots the shared
    /// constructor gives every view, and es-toolkit's `clone`/`cloneDeep` buffer
    /// specs compare a reflectively-built clone against the directly-built
    /// original with structural equality — so the two shapes have to agree.
    ///
    /// The record carries the `__smelt_buffer` identity marker plus the `bytes`
    /// list and its mirrored `length`/`byteLength`, so `instanceof`/`isBuffer`
    /// resolve through the marker while `buf.length`/`buf.byteLength` read the
    /// concrete byte count through the erased object member path.
    pub(in crate::lowering) fn buffer_record_from_bytes(
        &mut self,
        bytes: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        self.buffer_record_from_args(vec![bytes], span, body)
    }

    /// Construct a modeled `Buffer` record from raw `new Buffer(...)` arguments.
    ///
    /// The arguments reach the shared byte-buffer constructor unchanged, which is
    /// what lets a source the frontend cannot pre-digest (a string plus its
    /// encoding) be interpreted by the same registry-driven rules that back
    /// `new Uint8Array(...)`.
    pub(in crate::lowering) fn buffer_record_from_args(
        &mut self,
        args: Vec<smelt_hir::ExprId>,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        body.push_expr(Expr {
            kind: ExprKind::HostConstruct {
                class_name: BUFFER_CLASS_NAME.to_owned(),
                args,
            },
            ty: unknown_ty,
            span,
        })
    }

    /// Return whether a `Buffer.from` encoding argument names an implemented
    /// byte-preserving encoding.
    ///
    /// Only a string literal can be checked; a dynamic argument answers `false`
    /// so the caller raises a blocker instead of guessing UTF-8.
    fn is_supported_buffer_encoding_argument(argument: &Argument<'_>) -> bool {
        let Some(Expression::StringLiteral(literal)) = argument.as_expression() else {
            return false;
        };
        matches!(
            literal.value.to_ascii_lowercase().as_str(),
            "utf8" | "utf-8" | "latin1" | "binary" | "ascii" | "hex" | "base64" | "base64url"
        )
    }

    /// Lower Node `Buffer.from(value[, encoding])` to a modeled `Buffer` record.
    ///
    /// `Buffer.from([1, 2, 3])` reuses the numeric source list as the buffer's
    /// bytes. `Buffer.from("text"[, encoding])` hands the *string* (and its
    /// encoding, when given) to the byte-buffer constructor, which encodes the
    /// text into bytes — a Buffer built from a string holds that string's bytes,
    /// unlike a typed array, which ignores a text source. Any other opaque
    /// source has no cheaply-recoverable byte list, so it is evaluated for its
    /// effects and an empty byte list backs the record.
    ///
    /// An encoding the runtime does not implement is an honest blocker rather
    /// than silently mis-encoded bytes: `utf8`/`utf-8`, `latin1`/`binary`,
    /// `ascii`, `hex` and `base64`/`base64url` are lowered, and a dynamic
    /// encoding argument cannot be checked, so it is rejected too.
    pub(in crate::lowering) fn buffer_from_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.buffer_static_receiver(call, "from").is_none() {
            return Ok(None);
        }
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.from requires a value and optional encoding",
            ));
        }
        let Some(source_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.from requires a value argument",
            ));
        };
        let source = self.argument(source_argument, body)?;
        let encoding_argument = call.arguments.get(1);
        let encoding = match encoding_argument {
            Some(argument) => Some(self.argument(argument, body)?),
            None => None,
        };
        let span = self.span(call.span.start, call.span.end);
        let source_ty = Self::expr_ty(body, source);
        if matches!(self.ctx.krate.types.get(source_ty), Some(Type::List(_))) {
            return Ok(Some(self.buffer_record_from_bytes(source, span, body)));
        }
        if matches!(self.ctx.krate.types.get(source_ty), Some(Type::String)) {
            if let Some(argument) = encoding_argument
                && !Self::is_supported_buffer_encoding_argument(argument)
            {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "Buffer.from encoding must be a literal utf8/utf-8/latin1/binary/ascii/hex",
                ));
            }
            let mut args = vec![source];
            args.extend(encoding);
            return Ok(Some(self.buffer_record_from_args(args, span, body)));
        }
        let bytes = self.buffer_empty_bytes(span, body);
        Ok(Some(self.buffer_record_from_bytes(bytes, span, body)))
    }

    /// Lower Node `Buffer.alloc(length)` to a modeled `Buffer` record of `length`
    /// zero-filled bytes.
    pub(in crate::lowering) fn buffer_alloc_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.buffer_static_receiver(call, "alloc").is_none() {
            return Ok(None);
        }
        let [length_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.alloc requires exactly one length argument",
            ));
        };
        let length = self.argument(length_arg, body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, length)),
            Some(Type::Int | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(length_arg.span().start, length_arg.span().end),
                "Buffer.alloc length must be numeric",
            ));
        }
        let span = self.span(call.span.start, call.span.end);
        // `ListFromLength` emits a `list[unknown]` fill, so the destination type
        // must match `List<Unknown>`; the buffer's byte-count reads only observe
        // the list length, not the element type.
        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let bytes = body.push_expr(Expr {
            kind: ExprKind::ListFromLength { length },
            ty: list_ty,
            span,
        });
        Ok(Some(self.buffer_record_from_bytes(bytes, span, body)))
    }

    /// Lower Node `Buffer.concat(list[, totalLength])` to a modeled `Buffer`
    /// record.
    ///
    /// The concatenated source buffers are opaque erased records, so their bytes
    /// cannot be cheaply merged; the arguments are evaluated for their effects
    /// and the result carries a fresh empty byte list plus `Buffer` identity.
    /// es-toolkit only reads the identity and (through `.toString()`) a decoded
    /// string, neither of which depends on the recovered byte contents here.
    pub(in crate::lowering) fn buffer_concat_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.buffer_static_receiver(call, "concat").is_none() {
            return Ok(None);
        }
        if !(1..=2).contains(&call.arguments.len()) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.concat requires a buffer list and optional total length",
            ));
        }
        for argument in &call.arguments {
            let _ = self.argument(argument, body)?;
        }
        let span = self.span(call.span.start, call.span.end);
        let bytes = self.buffer_empty_bytes(span, body);
        Ok(Some(self.buffer_record_from_bytes(bytes, span, body)))
    }

    /// Lower Node `Buffer.isBuffer(x)` to the `Buffer` identity check.
    ///
    /// The check resolves through the shared `__smelt_buffer` marker exactly like
    /// `x instanceof Buffer` (reusing the marker-based `InstanceOf` path). A
    /// statically concrete, non-erased operand can never carry the marker, so it
    /// folds to `false`.
    pub(in crate::lowering) fn buffer_is_buffer_call(
        &mut self,
        call: &CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.buffer_static_receiver(call, "isBuffer").is_none() {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Buffer.isBuffer requires exactly one argument",
            ));
        };
        let value = self.argument(argument, body)?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let span = self.span(call.span.start, call.span.end);
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        ) {
            let class = self.intern_type_name("Buffer");
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::InstanceOf { value, class },
                ty: bool_ty,
                span,
            })));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        })))
    }

    /// Build an empty numeric byte list for buffers whose bytes are not
    /// cheaply recoverable from the source expression.
    pub(in crate::lowering) fn buffer_empty_bytes(
        &mut self,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let item_ty = self.ctx.krate.types.intern(Type::Float);
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty: list_ty,
            span,
        })
    }
}
