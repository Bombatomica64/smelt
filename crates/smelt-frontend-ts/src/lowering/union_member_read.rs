//! A property read whose receiver is a tagged union, dispatched on the arms.
//!
//! A union is a generated Rust enum, so at a `u.f` read the ARM is a runtime
//! question and each arm's READ is a static one. Both halves were being thrown
//! away: the read erased the whole union to `SmeltUnknown` and looked the
//! property up at runtime —
//!
//! ```ignore
//! // arg: SmeltUnion2 (ResponseInit | Response)
//! smelt_get_unknown_field(&arg.clone().into_smelt_unknown(), "headers")
//! ```
//!
//! — typed `SmeltUnknown`, which then infected every consumer downstream. A
//! hand-writing Rust team would match the enum and read the field off the arm,
//! which is what a METHOD call on a union already does (`union_method_text` in
//! the emitter).
//!
//! # Why the desugaring lives here and not in the emitter
//!
//! Two things only the frontend can do:
//!
//! * **Pick each arm's read.** `res.headers` on a `Response` is not a struct
//!   field access at all — it is a `ResponseOp::Headers` node, and the same is
//!   true of the other modeled host classes. Which HIR node a member read
//!   becomes is decided here, so only here can one arm read a struct field
//!   while its sibling runs a runtime accessor.
//! * **Name the joined type.** The emitter can only FIND interned types, and
//!   the join of the arms' member types generally is not in the table yet.
//!   Interning is free here.
//!
//! # The shape
//!
//! One conditional per arm, chained, each guarded by a check the emitter
//! renders as a STATIC tag test over the enum (`concrete_union_tag_check` for
//! a `typeof`-style kind, `concrete_union_class_check` for a class), with the
//! one arm that has no available runtime check as the final `else`:
//!
//! ```ignore
//! if matches!(arg, SmeltUnion2::M0(_)) { /* number arm: undefined */ }
//! else if matches!(arg, SmeltUnion2::M2(_)) { /* Response arm: .headers() */ }
//! else { /* ResponseInit arm: the declared field */ }
//! ```
//!
//! An arm that does not carry the member reads `undefined`, which is what
//! JavaScript answers for `(5).headers` — so a union that still holds a
//! primitive arm needs no narrowing proof to be read from, and the joined type
//! simply becomes optional.
//!
//! The desugaring is all-or-nothing: if any arm's member read is not one of
//! the modeled shapes, the whole read falls back to the erased path, so this
//! can only remove erasure, never add a half-typed value.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{Body, Expr, ExprKind, Literal, ResponseOp, Symbol, Type, TypeId, UnknownKind};

/// How one union arm answers the member read.
enum ArmRead {
    /// The arm does not carry the member: JavaScript answers `undefined`.
    Absent,
    /// An ordinary declared member, read as a field off the projected arm.
    Field(TypeId),
    /// A modeled `Response` data property, read through its runtime accessor.
    Response(ResponseOp, TypeId),
}

impl ArmRead {
    /// The type this arm's read answers.
    fn ty(&self, none_ty: TypeId) -> TypeId {
        match self {
            Self::Absent => none_ty,
            Self::Field(ty) | Self::Response(_, ty) => *ty,
        }
    }
}

/// The runtime test that selects one arm of the union.
enum ArmGuard {
    /// A `typeof`-style tag test; the emitter renders it as a `matches!` over
    /// the arms of that JavaScript kind.
    Kind(UnknownKind),
    /// An `instanceof` test against a nominal class arm.
    Class(Symbol),
    /// No test is available for this arm — it can only be the final `else`.
    None,
}

impl ModuleBuilder<'_> {
    /// Lower `receiver.field` where `receiver`'s type is a tagged union.
    ///
    /// `Ok(None)` means this read is not one the desugaring can build, and the
    /// caller keeps its existing (erased) lowering.
    pub(in crate::lowering) fn union_member_read(
        &mut self,
        receiver: smelt_hir::ExprId,
        receiver_ty: TypeId,
        field: Symbol,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(Type::Union(arms)) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        if arms.len() < 2 {
            return Ok(None);
        }
        // A receiver whose DECLARED type is still optional (`arg?: A | B` read
        // as `arg.headers` after a `typeof` guard) is asserted present first,
        // the same way the `length` path asserts one: `tsc` proved the guard,
        // and the arm projections that follow need the union itself. A source
        // `?.` is not routed here at all — that spelling asks for `undefined`
        // when the receiver is absent, which is the caller's optional-chain
        // lowering, not an arm dispatch.
        let receiver = if Self::expr_ty(body, receiver) == receiver_ty {
            receiver
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: receiver },
                ty: receiver_ty,
                span,
            })
        };
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let mut plans = Vec::with_capacity(arms.len());
        for arm in &arms {
            let Some(read) = self.union_arm_read(*arm, field) else {
                return Ok(None);
            };
            plans.push((*arm, self.union_arm_guard(*arm), read));
        }
        // Every arm answering `undefined` is a read this desugaring has no
        // business claiming: the member is on no arm, and the existing lowering
        // already answers `undefined` for it without a dispatch.
        if plans
            .iter()
            .all(|(_, _, read)| matches!(read, ArmRead::Absent))
        {
            return Ok(None);
        }
        // Exactly one arm may be unguarded, and it has to be the `else`. Two
        // would make the chain answer the wrong arm for one of them.
        let unguarded = plans
            .iter()
            .filter(|(_, guard, _)| matches!(guard, ArmGuard::None))
            .count();
        if unguarded > 1 {
            return Ok(None);
        }
        let Some(result_ty) = self.union_member_result_ty(&plans, none_ty) else {
            return Ok(None);
        };
        // The unguarded arm goes last; the guarded ones keep source order so
        // the emitted chain reads like the union's own declaration.
        plans.sort_by_key(|(_, guard, _)| u8::from(matches!(guard, ArmGuard::None)));
        let mut chain: Option<smelt_hir::ExprId> = None;
        for (arm_ty, guard, read) in plans.into_iter().rev() {
            let value = self.union_arm_read_expr(receiver, arm_ty, field, &read, span, body)?;
            let Some(else_expr) = chain else {
                // The last arm in the chain is the fallthrough: with every
                // other arm's guard false, this one holds by exhaustion.
                chain = Some(value);
                continue;
            };
            let cond = match guard {
                ArmGuard::Kind(kind) => {
                    let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                    body.push_expr(Expr {
                        kind: ExprKind::UnknownIs {
                            value: receiver,
                            kind,
                        },
                        ty: bool_ty,
                        span,
                    })
                }
                ArmGuard::Class(class) => {
                    let bool_ty = self.ctx.krate.types.intern(Type::Bool);
                    body.push_expr(Expr {
                        kind: ExprKind::InstanceOf {
                            value: receiver,
                            class,
                        },
                        ty: bool_ty,
                        span,
                    })
                }
                // An unguarded arm was sorted last, so it is the fallthrough
                // handled above and cannot reach here.
                ArmGuard::None => return Ok(None),
            };
            chain = Some(body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond,
                    then_expr: value,
                    else_expr,
                },
                ty: result_ty,
                span,
            }));
        }
        Ok(chain.map(|expr| {
            // The chain's own type is the join; a single-arm chain (every other
            // arm guarded away) still has to answer it.
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: expr },
                ty: result_ty,
                span,
            })
        }))
    }

    /// Decide how one arm answers the member read.
    fn union_arm_read(&mut self, arm_ty: TypeId, field: Symbol) -> Option<ArmRead> {
        // A modeled `Response` arm reads its data properties through the
        // runtime accessor, never as a struct field: `SmeltResponse` keeps them
        // behind methods.
        if self.is_response_type(arm_ty) {
            let name = self.ctx.krate.symbols.get(field)?.to_owned();
            let op = match name.as_str() {
                "status" => ResponseOp::Status,
                "ok" => ResponseOp::Ok,
                "statusText" | "status_text" => ResponseOp::StatusText,
                "headers" => ResponseOp::Headers,
                "bodyUsed" | "body_used" => ResponseOp::BodyUsed,
                _ => return Some(ArmRead::Absent),
            };
            let ty = self.response_op_result_type(op);
            return Some(ArmRead::Response(op, ty));
        }
        match self.ctx.krate.types.get(arm_ty).cloned()? {
            // A record's every string key is a member, so the read is the
            // store's value type.
            Type::Dict(key, _) if matches!(self.ctx.krate.types.get(key), Some(Type::String)) => {
                let ty = self.class_field_type(arm_ty, field).ok()?;
                Some(ArmRead::Field(ty))
            }
            Type::Class { .. } => {
                if !self.arm_declares_member(arm_ty, field) {
                    return Some(ArmRead::Absent);
                }
                let ty = self.class_field_type(arm_ty, field).ok()?;
                // An erased member type would defeat the point: the whole
                // reason to dispatch is to keep a concrete answer.
                if matches!(self.ctx.krate.types.get(ty), Some(Type::Unknown)) {
                    return None;
                }
                Some(ArmRead::Field(ty))
            }
            // A primitive arm carries no user members at all, so the read is
            // `undefined` — `(5).headers` in JavaScript, not an error.
            Type::Float | Type::Int | Type::Bool | Type::None => Some(ArmRead::Absent),
            _ => None,
        }
    }

    /// Return whether an arm's declared shape carries the member.
    ///
    /// Deliberately narrower than [`Self::class_field_type`], which answers an
    /// erased type for a member it cannot resolve: an unresolved member has to
    /// read `undefined` here, not `SmeltUnknown`.
    fn arm_declares_member(&self, arm_ty: TypeId, field: Symbol) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(arm_ty).cloned() else {
            return false;
        };
        if self
            .class_by_symbol(name)
            .is_some_and(|class| class.fields.iter().any(|item| item.name == field))
        {
            return true;
        }
        if self
            .find_interface(name)
            .is_some_and(|interface| interface.fields.iter().any(|item| item.name == field))
        {
            return true;
        }
        let Some(class_name) = self
            .ctx
            .krate
            .names
            .get(name)
            .or_else(|| self.ctx.krate.symbols.get(name))
            .map(str::to_owned)
        else {
            return false;
        };
        self.classes
            .fields(&class_name)
            .is_some_and(|fields| fields.iter().any(|item| item.name == field))
    }

    /// The runtime test that selects one arm.
    fn union_arm_guard(&self, arm_ty: TypeId) -> ArmGuard {
        match self.ctx.krate.types.get(arm_ty).cloned() {
            Some(Type::Float | Type::Int) => ArmGuard::Kind(UnknownKind::Number),
            Some(Type::String) => ArmGuard::Kind(UnknownKind::String),
            Some(Type::Bool) => ArmGuard::Kind(UnknownKind::Bool),
            Some(Type::List(_) | Type::Tuple(_)) => ArmGuard::Kind(UnknownKind::Array),
            Some(Type::Function(_)) => ArmGuard::Kind(UnknownKind::Function),
            // A nominal class arm is `instanceof`-checkable; an interface is
            // not a runtime value, so it can only be the fallthrough.
            Some(Type::Class { name, .. }) if self.arm_is_nominal_class(name) => {
                ArmGuard::Class(name)
            }
            _ => ArmGuard::None,
        }
    }

    /// Return whether a class arm has a runtime identity `instanceof` can test.
    fn arm_is_nominal_class(&self, name: Symbol) -> bool {
        self.class_by_symbol(name).is_some() || self.stdlib_class_of_symbol(name).is_some()
    }

    /// The joined type of every arm's read.
    ///
    /// The arms' member types are folded into one set — a union arm's own
    /// members flattened in, so a `Headers` read next to a
    /// `[string, string][] | Record<string, string> | Headers` read joins to
    /// the union that already holds it rather than nesting a second one. An
    /// absent arm makes the result optional, which is what `undefined` in the
    /// join means.
    fn union_member_result_ty(
        &mut self,
        plans: &[(TypeId, ArmGuard, ArmRead)],
        none_ty: TypeId,
    ) -> Option<TypeId> {
        let mut optional = false;
        let mut members: Vec<TypeId> = Vec::new();
        for (_, _, read) in plans {
            let mut ty = read.ty(none_ty);
            if matches!(self.ctx.krate.types.get(ty), Some(Type::None)) {
                optional = true;
                continue;
            }
            if let Some(Type::Optional(inner)) = self.ctx.krate.types.get(ty).cloned() {
                optional = true;
                ty = inner;
            }
            match self.ctx.krate.types.get(ty).cloned() {
                Some(Type::Union(items)) => {
                    for item in items {
                        if !members.contains(&item) {
                            members.push(item);
                        }
                    }
                }
                _ => {
                    if !members.contains(&ty) {
                        members.push(ty);
                    }
                }
            }
        }
        let inner = match members.as_slice() {
            [] => return None,
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(members)),
        };
        Some(if optional {
            self.ctx.krate.types.intern(Type::Optional(inner))
        } else {
            inner
        })
    }

    /// Build one arm's read expression.
    ///
    /// The receiver is projected to the arm type first — a `TypeAssert`, the
    /// same node round 10's `instanceof` narrowing uses, which the emitter
    /// renders as `match u { M{i}(value) => value, .. }` — so the read that
    /// follows sees a value of exactly that arm's type.
    fn union_arm_read_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        arm_ty: TypeId,
        field: Symbol,
        read: &ArmRead,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match read {
            ArmRead::Absent => {
                let none_ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Undefined),
                    ty: none_ty,
                    span,
                }))
            }
            ArmRead::Field(ty) => {
                let projected = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: receiver },
                    ty: arm_ty,
                    span,
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver: projected,
                        field,
                    },
                    ty: *ty,
                    span,
                }))
            }
            ArmRead::Response(op, ty) => {
                let projected = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: receiver },
                    ty: arm_ty,
                    span,
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ResponseOp {
                        op: *op,
                        response: projected,
                        args: Vec::new(),
                    },
                    ty: *ty,
                    span,
                }))
            }
        }
    }
}
