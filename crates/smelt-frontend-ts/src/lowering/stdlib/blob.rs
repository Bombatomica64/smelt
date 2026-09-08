//! WHATWG `Blob`/`File` lowering for the TypeScript frontend.
//!
//! `Blob` and `File` used to be marker-bearing `SmeltUnknown` records whose
//! whole observable surface was `.size`/`.type` read back off the record. They
//! are now the concrete generated `SmeltBlob` type, and every member is the
//! member's exact source type: `size` a `number`, `type`/`name` a `string`,
//! `lastModified` a `number`, `text()` a `Promise<string>`, `bytes()` a
//! `Promise<Uint8Array>`, and `slice()` a `Blob`.
//!
//! # One Rust type for two spellings
//!
//! The spec's `File` is a `Blob` plus exactly two data properties, and Rust has
//! no inheritance. So both class names resolve to one `SmeltBlob` whose file
//! metadata is optional — a file is a blob whose name is present. That is what
//! makes `file instanceof Blob` free rather than a subtype relation the type
//! system would have to carry, and it is what the erased record already did
//! (`__smelt_file` stamped on top of `__smelt_blob`, one shape). The two
//! `File`-only members are refused on a plain `Blob` receiver at lowering,
//! where the receiver's spelling is still known.
//!
//! # Interface
//!
//! * `new_expr.rs` calls the constructors (already present; this concern only
//!   changed what they are TYPED as).
//! * [`ModuleBuilder::dispatch_blob_method`] is registered in the builtin
//!   call-handler chain and recognizes receiver/member pairs through the shared
//!   `smelt-stdlib` method metadata (`TypeScriptReceiverKind::Blob`).
//! * [`ModuleBuilder::blob_member_read`] is called from the static-member read
//!   chain AFTER the receiver is lowered, and reuses that receiver: `size`,
//!   `type` and `name` are members of many values, so a pre-receiver probe
//!   would lower the receiver a second time on every miss.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Expression;
use smelt_hir::{BlobOp, Body, Expr, ExprKind, Type};
use smelt_stdlib::RuleId;

impl ModuleBuilder<'_> {
    /// Dispatch a modeled `Blob`/`File` method on a concrete receiver.
    pub(in crate::lowering) fn dispatch_blob_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::Blob,
            member_name,
        ) else {
            return Ok(None);
        };
        // `text()` is also a `Response`/`Request` member, so decline a receiver
        // that is knowably some OTHER modeled class before lowering it — see
        // `receiver_class_hint` for why lowering-then-declining is not free.
        if self
            .receiver_class_hint(&member.object, body)
            .is_some_and(|class| !class.is_blob_runtime_type())
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        if !self.is_blob_class_type(Self::expr_ty(body, receiver)) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let op = match (rule, member_name) {
            (RuleId::TsBlobBodyRead, "text") => BlobOp::Text,
            (RuleId::TsBlobBodyRead, "arrayBuffer") => BlobOp::ArrayBuffer,
            (RuleId::TsBlobBodyRead, "bytes") => BlobOp::Bytes,
            (RuleId::TsBlobSlice, _) => BlobOp::Slice,
            _ => return Ok(None),
        };
        // The body readers are nullary; `slice` takes up to three arguments
        // (`start`, `end`, `contentType`) and every one of them is optional.
        let arity = if op == BlobOp::Slice { 3 } else { 0 };
        let args = call
            .arguments
            .iter()
            .take(arity)
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = self.blob_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BlobOp {
                op,
                blob: receiver,
                args,
            },
            ty,
            span,
        })))
    }

    /// Read a `Blob`/`File` data property off an already-lowered receiver.
    pub(in crate::lowering) fn blob_member_read(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        receiver: smelt_hir::ExprId,
        receiver_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_blob_class_type(receiver_ty) {
            return Ok(None);
        }
        let span = self.span(member.span.start, member.span.end);
        let member_name = member.property.name.as_str();
        let op = match member_name {
            "size" => BlobOp::Size,
            "type" => BlobOp::Type,
            "name" | "lastModified" => {
                // A `File`-only member. Refused on a plain `Blob` here, where
                // the receiver's class spelling is still known, rather than
                // answered from the optional metadata the shared runtime type
                // happens to carry.
                if self.stdlib_class_of_type(receiver_ty) != Some(smelt_stdlib::StdlibClass::File) {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("`Blob.{member_name}` does not exist; it is a `File` member"),
                    ));
                }
                if member_name == "name" {
                    BlobOp::Name
                } else {
                    BlobOp::LastModified
                }
            }
            _ => return Ok(None),
        };
        let ty = self.blob_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::BlobOp {
                op,
                blob: receiver,
                args: Vec::new(),
            },
            ty,
            span,
        })))
    }

    /// The HIR type a `Blob`/`File` member answers.
    ///
    /// Each is the member's exact source type. The body readers are `async`, so
    /// they answer a `Future`; `slice` answers a `Blob` and never a `File`,
    /// which is the spec's own signature.
    fn blob_op_result_type(&mut self, op: BlobOp) -> smelt_hir::TypeId {
        match op {
            BlobOp::Size | BlobOp::LastModified => self.ctx.krate.types.intern(Type::Float),
            BlobOp::Type | BlobOp::Name => self.ctx.krate.types.intern(Type::String),
            BlobOp::Text => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Future(string_ty))
            }
            // `arrayBuffer()` answers an `ArrayBuffer` in the spec and
            // `bytes()` a `Uint8Array`. Smelt has one concrete byte value, so
            // both answer it: the difference between the two host types is a
            // view-vs-storage distinction that only the typed-array family
            // observes, and that family is still the erased byte-backed record.
            BlobOp::ArrayBuffer | BlobOp::Bytes => {
                let bytes_ty = self.byte_array_type();
                self.ctx.krate.types.intern(Type::Future(bytes_ty))
            }
            BlobOp::Slice => self.blob_class_type(),
        }
    }

    /// Return the modeled `Blob` class type.
    pub(in crate::lowering) fn blob_class_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("Blob");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return the modeled `File` class type.
    pub(in crate::lowering) fn file_class_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("File");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `Blob` or `File` class.
    ///
    /// Both answer `true`: they share one runtime type, so every site that asks
    /// "can this receiver take a blob member" wants the pair.
    pub(in crate::lowering) fn is_blob_class_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty)
            .is_some_and(smelt_stdlib::StdlibClass::is_blob_runtime_type)
            && !self.user_class_shadows("Blob")
            && !self.user_class_shadows("File")
    }
}
