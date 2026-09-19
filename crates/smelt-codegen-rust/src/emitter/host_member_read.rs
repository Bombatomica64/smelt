//! Reading a member off a MODELED HOST class from rendered text.
//!
//! `emitter::place` answers `x.member` from a [`smelt_hir::Place`], whose base
//! is a MIR local. A coercion seam has no local: it holds a rendered expression
//! and the type that expression has. Structural assignability needs exactly
//! that — "a value whose type exposes every member of a target record type
//! converts by reading each target field off it" — so the member read has to be
//! spellable from text.
//!
//! It also has to be spellable at all for a host class. A `Response`'s `status`
//! is not a struct field: `SmeltResponse` stores its data privately and exposes
//! `status()`, and the frontend lowers the source read to a
//! [`smelt_hir::ResponseOp`] rather than to a field place. The table below is
//! that same member model, asked by NAME, so the conversion and the ordinary
//! `response.status` read cannot disagree about what a member means.

use smelt_hir::{ResponseOp, Type, TypeId};

use super::{EmitError, FunctionEmitter};

/// A member a modeled host class exposes for reading.
struct HostMember {
    /// The source-level member name (`status`, `statusText`, ...).
    name: &'static str,
    /// How the member is read.
    read: HostMemberRead,
}

/// How one host member's value is produced from a rendered receiver.
enum HostMemberRead {
    /// A `Response` member operation, emitted by `response_op_on_text`.
    Response(ResponseOp),
}

/// The readable members of `Response`.
///
/// Only the DATA members belong here: the ones whose value a structural
/// conversion can copy into a record field. `text()`/`json()`/`clone()` are
/// operations a caller invokes, not properties a record type can declare, so
/// they are deliberately absent — a target field named after one of them finds
/// no member and the conversion declines rather than fabricating a value.
const RESPONSE_MEMBERS: &[HostMember] = &[
    HostMember {
        name: "status",
        read: HostMemberRead::Response(ResponseOp::Status),
    },
    HostMember {
        name: "statusText",
        read: HostMemberRead::Response(ResponseOp::StatusText),
    },
    HostMember {
        name: "headers",
        read: HostMemberRead::Response(ResponseOp::Headers),
    },
    HostMember {
        name: "ok",
        read: HostMemberRead::Response(ResponseOp::Ok),
    },
    HostMember {
        name: "bodyUsed",
        read: HostMemberRead::Response(ResponseOp::BodyUsed),
    },
];

impl FunctionEmitter<'_> {
    /// Read `member` off a modeled host value, from its rendered text.
    ///
    /// Answers the read's Rust text and the member's own type, so the caller
    /// can convert from it with the ordinary coercion seam. `None` when
    /// `base_ty` is not a modeled host class with a member table, when it
    /// declares no such member, or when the member's type is not interned in
    /// this crate's type table — a crate that never mentions `Headers` has no
    /// `Type::Class { name: Headers }` to name, and declining is then the right
    /// answer because the conversion could not have been well typed anyway.
    pub(super) fn host_class_member_read_text(
        &self,
        base_text: &str,
        base_ty: TypeId,
        member: &str,
    ) -> Result<Option<(String, TypeId)>, EmitError> {
        let Some(members) = self.host_class_members(base_ty)? else {
            return Ok(None);
        };
        let Some(entry) = members.iter().find(|entry| entry.name == member) else {
            return Ok(None);
        };
        match entry.read {
            HostMemberRead::Response(op) => {
                let Some(member_ty) = self.response_op_result_ty(op) else {
                    return Ok(None);
                };
                Ok(Some((self.response_op_on_text(op, base_text)?, member_ty)))
            }
        }
    }

    /// Whether this type is a modeled host class with a member table.
    ///
    /// The gate every structural conversion FROM a host value asks first:
    /// without it a source with no table at all (a dictionary, a tuple) would
    /// report "no member exposed" for each target field and an all-optional
    /// target would be built entirely out of absent fields — a conversion that
    /// compiles and silently drops the value.
    pub(super) fn exposes_host_members(&self, ty: TypeId) -> Result<bool, EmitError> {
        Ok(self.host_class_members(ty)?.is_some())
    }

    /// The member table for a modeled host class, if it has one.
    ///
    /// The class identity comes from the shared stdlib registry, never from a
    /// name comparison spelled here: a crate that declares its own
    /// `class Response` shadows the host one, and the registry is what already
    /// knows that (it answers `None` for any name the crate declares).
    fn host_class_members(&self, ty: TypeId) -> Result<Option<&'static [HostMember]>, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(ty) else {
            return Ok(None);
        };
        Ok(match self.stdlib_class_of_symbol(*name)? {
            Some(smelt_stdlib::StdlibClass::Response) => Some(RESPONSE_MEMBERS),
            _ => None,
        })
    }

    /// The interned type a `Response` member read produces.
    ///
    /// Mirrors the frontend's `response_op_result_type`; `None` when this crate
    /// never interned that type, which makes the whole read unavailable rather
    /// than wrongly typed.
    fn response_op_result_ty(&self, op: ResponseOp) -> Option<TypeId> {
        match op {
            ResponseOp::Status => self.find_type_id(&Type::Float),
            ResponseOp::StatusText => self.find_type_id(&Type::String),
            ResponseOp::Ok | ResponseOp::BodyUsed => self.find_type_id(&Type::Bool),
            ResponseOp::Headers => self.stdlib_class_ty(smelt_stdlib::StdlibClass::Headers),
            _ => None,
        }
    }

    /// The interned `Type::Class` for a stdlib class, if this crate has one.
    ///
    /// Resolved through the registry rather than by name, for the same reason
    /// `host_class_members` is; `None` when the crate never mentions the class,
    /// which makes the member read that needs it unavailable instead of
    /// wrongly typed.
    fn stdlib_class_ty(&self, class: smelt_stdlib::StdlibClass) -> Option<TypeId> {
        self.mir
            .types
            .all()
            .iter()
            .position(|ty| match ty {
                Type::Class { name, args } => {
                    args.is_empty()
                        && self.stdlib_class_of_symbol(*name).ok().flatten() == Some(class)
                }
                _ => false,
            })
            .and_then(|index| u32::try_from(index).ok())
            .map(TypeId)
    }
}
