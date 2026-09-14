//! The concrete typed-array family: construction, members, and methods.
//!
//! Increment 3 of `blocker-logs/standards-typed-array-views-plan.md`. Before
//! this, a source `Uint8Array` annotation and a `new Uint8Array(..)` both
//! resolved to the ERASED byte-backed host record: the value existed, carried a
//! marker and real bytes inside a `SmeltUnknown`, and every read of it crossed
//! the dynamic boundary. Now the eleven view spellings and `ArrayBuffer` are
//! [`smelt_stdlib::StdlibClass::TypedArray`] / `ArrayBuffer` — two generated
//! Rust types — and the erased record is their BOUNDARY form, reached only
//! through `IntoSmeltUnknown` / `SmeltFromUnknown`.
//!
//! ## One class for eleven spellings
//!
//! `Type::Class { name }` keeps the source spelling, so `Int16Array` and
//! `Uint8Array` stay distinguishable in HIR; both resolve to the one stdlib
//! class, and the element KIND is selected at the construction site from that
//! name (see `typed_array_prelude::kind_expression`). The kind is a runtime
//! property of the value — `Object.prototype.toString.call(view)` reports it,
//! and reflective construction reads it back off an erased record — so making
//! it a Rust type parameter would have forced it to be erased at every
//! boundary. The registry, not a `matches!` here, decides which spellings are
//! the family.
//!
//! ## `SharedArrayBuffer` and `DataView`
//!
//! Both are now concrete too, and each in the shape its surface asks for.
//!
//! `SharedArrayBuffer` is the SAME class as `ArrayBuffer`: the two differ in
//! their `[object X]` tag, their `instanceof` answer and the growth members,
//! and in nothing about the bytes — so one Rust type carries a species flag
//! rather than a second copy of storage. The growable form
//! (`new SharedArrayBuffer(n, { maxByteLength })`, `grow`, `growable`) is not
//! modeled, and its members are absent rather than wrong.
//!
//! `DataView` is its own class, because its element kind is an argument of
//! every accessor instead of a property of the value: the same view answers
//! `getInt16(0)` and `getFloat64(0)`, and its byte ORDER is a parameter whose
//! default — big-endian — is the opposite of every typed array's. The widths
//! and signednesses are still the kind table's, reached by reversing the
//! window for a big-endian call, so there is one definition of each.
//!
//! One divergence is recorded rather than hidden: an OUT-OF-RANGE accessor is
//! a `RangeError` in JavaScript, and Smelt has no throwing rvalue yet (only
//! fallible CALLS carry an unwind edge), so it answers zero instead. See
//! `blocker-logs/hono-fetch-demand.md`.

use oxc::ast::ast::Expression;
use smelt_hir::{Body, ByteArrayOp, Expr, ExprKind, Type};
use smelt_stdlib::RuleId;

use crate::error::SmeltError;
use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Return whether a source class name is one of the family's spellings.
    ///
    /// The eleven views plus `ArrayBuffer`/`SharedArrayBuffer` and `DataView`,
    /// asked of the shared registry so the construction side, the annotation
    /// side and codegen's Rust-type side cannot disagree. A user class of the
    /// same name shadows the global, as it does for every other modeled host
    /// class.
    pub(in crate::lowering) fn is_typed_array_family_name(&self, name: &str) -> bool {
        matches!(
            smelt_stdlib::typescript_stdlib_class(name),
            Some(
                smelt_stdlib::StdlibClass::TypedArray
                    | smelt_stdlib::StdlibClass::ArrayBuffer
                    | smelt_stdlib::StdlibClass::DataView
            )
        ) && !self.classes.contains(name)
    }

    /// Lower `new <view>(..)` / `new ArrayBuffer(..)` to the concrete family.
    ///
    /// The node carries the source NAME and the arguments as written; codegen
    /// picks the constructor from the name (which half, which element kind) and
    /// from the argument types (a length, elements, shared storage, another
    /// view, or the erased boundary). Deciding that here would mean re-deriving
    /// the argument types the emitter already has.
    pub(in crate::lowering) fn typed_array_constructor_expression(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        class_name: &str,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let args = new_expr
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.typed_array_class_type(class_name);
        Ok(body.push_expr(Expr {
            kind: ExprKind::TypedArrayNew {
                class_name: class_name.to_owned(),
                args,
            },
            ty,
            span,
        }))
    }

    /// Return the family class type for a source spelling.
    pub(in crate::lowering) fn typed_array_class_type(
        &mut self,
        class_name: &str,
    ) -> smelt_hir::TypeId {
        let name = self.intern_type_name(class_name);
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return the modeled `ArrayBuffer` class type, which `.buffer` answers.
    pub(in crate::lowering) fn array_buffer_type(&mut self) -> smelt_hir::TypeId {
        self.typed_array_class_type("ArrayBuffer")
    }

    /// Return whether a lowered type is an element VIEW of the family.
    pub(in crate::lowering) fn is_typed_array_view_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::TypedArray)
    }

    /// Return whether a lowered type is the family's byte STORAGE.
    pub(in crate::lowering) fn is_array_buffer_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::ArrayBuffer)
            && !self.user_class_shadows("ArrayBuffer")
    }

    /// Return whether a lowered type is a `DataView`.
    pub(in crate::lowering) fn is_data_view_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::DataView)
            && !self.user_class_shadows("DataView")
    }

    /// Dispatch a `DataView` element accessor: `getInt16`, `setFloat64`, ...
    ///
    /// Registered in the builtin call-handler chain beside the typed-array
    /// methods, and declining for every other receiver: `getX`/`setX` are
    /// ordinary user method names, so the receiver's lowered type is what
    /// decides. Which accessor this is — the width, the signedness and the
    /// direction — is the REGISTRY's answer about the name, not a match here,
    /// so a name JavaScript does not define (`getUint8Clamped`) declines and
    /// falls through to the ordinary member path.
    pub(in crate::lowering) fn dispatch_data_view_accessor(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        if smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::DataView,
            member_name,
        ) != Some(RuleId::TsDataViewAccess)
        {
            return Ok(None);
        }
        if let Some(hint) = self.receiver_class_hint(&member.object, body)
            && hint != smelt_stdlib::StdlibClass::DataView
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_data_view_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let Some((write, element)) = smelt_stdlib::data_view_accessor(member_name) else {
            return Ok(None);
        };
        // A read takes an offset and an optional flag; a write takes a value
        // between them. Arguments past the accessor's own arity are dropped
        // after being lowered for their effects, as they are for every other
        // modeled member.
        let arity = if write { 3 } else { 2 };
        let args = call
            .arguments
            .iter()
            .take(arity)
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        // A read answers a number; a write answers `undefined`.
        let ty = if write {
            self.ctx.krate.types.intern(Type::None)
        } else {
            self.ctx.krate.types.intern(Type::Float)
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DataViewAccess {
                write,
                element,
                view: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower a data-property read on the family: `length`, `byteLength`,
    /// `byteOffset`, `buffer`.
    ///
    /// Called from the static-member chain AFTER the receiver is lowered, and
    /// given that receiver, for the same reason the codec reads are: `length`
    /// is a member of nearly every value, so a probe that lowered the receiver
    /// itself would duplicate its side effects on every miss.
    ///
    /// `byteOffset` and `buffer` are VIEW members: storage has neither, so a
    /// read of one on an `ArrayBuffer` declines here and falls through to the
    /// ordinary (absent) field path rather than answering zero.
    pub(in crate::lowering) fn typed_array_member_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let is_view = self.is_typed_array_view_type(receiver_ty);
        let is_storage = self.is_array_buffer_type(receiver_ty);
        // A `DataView` has `byteLength`, `byteOffset` and `buffer` and no
        // `length`: it addresses BYTES, so an element count would be a
        // different number for every accessor width. The three it does have
        // are the same ops with the same names on its own runtime type.
        let is_data_view = self.is_data_view_type(receiver_ty);
        if !is_view && !is_storage && !is_data_view {
            return Ok(None);
        }
        let span = self.span(member.span.start, member.span.end);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let (op, ty) = match member.property.name.as_str() {
            // A view's `length` is its ELEMENT count and its `byteLength` the
            // byte count; they differ for every view wider than a byte. Storage
            // has only `byteLength`.
            "length" if is_view => (ByteArrayOp::Length, float_ty),
            "byteLength" => (ByteArrayOp::ByteLength, float_ty),
            "byteOffset" if is_view || is_data_view => (ByteArrayOp::ByteOffset, float_ty),
            "buffer" if is_view || is_data_view => {
                (ByteArrayOp::Buffer, self.array_buffer_type())
            }
            _ => return Ok(None),
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ByteArrayOp {
                op,
                bytes: receiver,
                args: Vec::new(),
            },
            ty,
            span,
        })))
    }

    /// Dispatch a modeled family method: `subarray`, `slice`, `set`, `fill`.
    ///
    /// Registered in the builtin call-handler chain BEFORE the collection and
    /// list dispatches, because `slice`, `set` and `fill` are all names those
    /// own for other receivers; the receiver's lowered type is what decides,
    /// and the handler declines through `receiver_class_hint` before lowering
    /// wherever the receiver's class is knowable from its binding.
    pub(in crate::lowering) fn dispatch_typed_array_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::TypedArray,
            member_name,
        ) else {
            return Ok(None);
        };
        if rule != RuleId::TsTypedArrayMethod {
            return Ok(None);
        }
        if let Some(hint) = self.receiver_class_hint(&member.object, body)
            && !matches!(
                hint,
                smelt_stdlib::StdlibClass::TypedArray | smelt_stdlib::StdlibClass::ArrayBuffer
            )
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        let is_view = self.is_typed_array_view_type(receiver_ty);
        if !is_view && !self.is_array_buffer_type(receiver_ty) {
            return Ok(None);
        }
        // `subarray`, `set` and `fill` are VIEW members: byte storage has no
        // elements to re-view, copy in, or overwrite. Declining rather than
        // erroring keeps the miss falling through to the ordinary member path.
        let op = match member_name {
            "slice" => ByteArrayOp::Slice,
            "subarray" if is_view => ByteArrayOp::Subarray,
            "set" if is_view => ByteArrayOp::Set,
            "fill" if is_view => ByteArrayOp::Fill,
            _ => return Ok(None),
        };
        let arity = Self::typed_array_op_max_arity(op).min(call.arguments.len());
        if matches!(op, ByteArrayOp::Set | ByteArrayOp::Fill) && call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`{member_name}` requires at least one argument"),
            ));
        }
        let args = call
            .arguments
            .iter()
            .take(arity)
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = match op {
            // `subarray`/`slice`/`fill` all answer a value of the receiver's own
            // class, which is what keeps `view.subarray(1).byteOffset` typed.
            ByteArrayOp::Subarray | ByteArrayOp::Slice | ByteArrayOp::Fill => receiver_ty,
            _ => self.ctx.krate.types.intern(Type::None),
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ByteArrayOp {
                op,
                bytes: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// The number of arguments a family method reads.
    ///
    /// Arguments beyond it are dropped after being lowered for their effects,
    /// as they are for every other modeled member.
    fn typed_array_op_max_arity(op: ByteArrayOp) -> usize {
        match op {
            ByteArrayOp::Set => 2,
            ByteArrayOp::Fill => 3,
            _ => 2,
        }
    }

    /// Lower `Object.keys`/`values`/`entries`/`Reflect.ownKeys` on a view.
    ///
    /// `None` when the receiver is not a concrete view, so the caller keeps its
    /// ordinary record path. `Reflect.ownKeys` answers the same list as
    /// `Object.keys` here: a view has no symbol-keyed own properties and no
    /// non-enumerable own ones.
    pub(in crate::lowering) fn typed_array_projection(
        &mut self,
        op: smelt_hir::DictProjectionOp,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        // Byte STORAGE has no own enumerable properties at all: an
        // `ArrayBuffer` addresses its bytes through accessors, so
        // `Object.keys(buffer)` is `[]` and `Object.values(buffer)` is `[]`,
        // where a view's are its element indices. That is the same distinction
        // `smelt_host_buffer_own_elements` draws on the erased side, and it is
        // answerable here without a record round trip because the storage's
        // emptiness is a property of its CLASS rather than of its bytes.
        // A `DataView` answers the same empty list and for the same reason: it
        // addresses BYTES through accessors and has no own enumerable
        // properties, which is why `JSON.stringify(dataView)` is `{}` where a
        // typed array's is its element indices.
        if self.is_array_buffer_type(receiver_ty) || self.is_data_view_type(receiver_ty) {
            return self.empty_projection_expression(op, span, body);
        }
        if !self.is_typed_array_view_type(receiver_ty) {
            return None;
        }
        let span = self.span(span.start, span.end);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let (byte_op, ty) = match op {
            smelt_hir::DictProjectionOp::Keys | smelt_hir::DictProjectionOp::OwnKeys => (
                ByteArrayOp::IndexKeys,
                self.ctx.krate.types.intern(Type::List(string_ty)),
            ),
            smelt_hir::DictProjectionOp::Values => (
                ByteArrayOp::Elements,
                self.ctx.krate.types.intern(Type::List(float_ty)),
            ),
            smelt_hir::DictProjectionOp::Entries => {
                let pair_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(Vec::from([string_ty, float_ty])));
                (
                    ByteArrayOp::IndexEntries,
                    self.ctx.krate.types.intern(Type::List(pair_ty)),
                )
            }
            // `for...in` keys and the remaining projections are not the view's
            // own-property question; they keep the ordinary path.
            _ => return None,
        };
        Some(body.push_expr(Expr {
            kind: ExprKind::ByteArrayOp {
                op: byte_op,
                bytes: receiver,
                args: Vec::new(),
            },
            ty,
            span,
        }))
    }

    /// The empty list a projection of byte STORAGE answers.
    ///
    /// Typed to match the projection, so `Object.keys(buffer).join(",")` still
    /// sees a `List<String>` and `Object.values(buffer)` a `List<Float>`: an
    /// empty list of the right element type rather than an erased one.
    fn empty_projection_expression(
        &mut self,
        op: smelt_hir::DictProjectionOp,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let span = self.span(span.start, span.end);
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let item_ty = match op {
            smelt_hir::DictProjectionOp::Keys | smelt_hir::DictProjectionOp::OwnKeys => string_ty,
            smelt_hir::DictProjectionOp::Values => float_ty,
            smelt_hir::DictProjectionOp::Entries => self
                .ctx
                .krate
                .types
                .intern(Type::Tuple(Vec::from([string_ty, float_ty]))),
            _ => return None,
        };
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Some(body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty,
            span,
        }))
    }

    /// Lower a SPREAD of a concrete view (`[...view]`, `Array.from(view)`).
    ///
    /// `None` when the value is not a view. Spreading a view iterates its
    /// elements, which the family decodes itself; the erased iterable path
    /// would read them back out of a marker record as `unknown`.
    pub(in crate::lowering) fn typed_array_spread_list(
        &mut self,
        value: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value_ty = Self::expr_ty(body, value);
        if !self.is_typed_array_view_type(value_ty) {
            return None;
        }
        Some(self.typed_array_elements_expression(value, span, body))
    }

    /// Lower the family's ELEMENTS, which is what iteration and spreading need.
    ///
    /// A `List<Float>` at the view's own width and signedness. Kept as one op
    /// so `for (const b of view)`, `[...view]` and `Array.from(view)` share one
    /// decode rather than each growing their own.
    pub(in crate::lowering) fn typed_array_elements_expression(
        &mut self,
        receiver: smelt_hir::ExprId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let ty = self.ctx.krate.types.intern(Type::List(float_ty));
        body.push_expr(Expr {
            kind: ExprKind::ByteArrayOp {
                op: ByteArrayOp::Elements,
                bytes: receiver,
                args: Vec::new(),
            },
            ty,
            span,
        })
    }
}
