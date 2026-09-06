//! Members of the JavaScript builtins that Smelt models as first-class values.
//!
//! A builtin's members are ordinary properties in JavaScript, so
//! `Array.prototype.slice` and `Array.isArray` are *values* long before they are
//! calls: they get passed to helpers, stored in tables and probed with
//! `typeof`. Every frontend and the generated runtime read this one registry so
//! the value spelling and the call spelling of a member cannot disagree, and so
//! that "which members does Smelt model?" has a single answer instead of one per
//! call site.
//!
//! The registry is deliberately the set of members whose behavior the runtime
//! actually implements. A member that is absent here reads as `undefined`, which
//! is honest: Smelt does not pretend to hand back a callable it cannot run.

/// Where a modeled member lives on its builtin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BuiltinMemberKind {
    /// A method on the builtin's `prototype`, applied to a receiver
    /// (`Array.prototype.slice`).
    Prototype,
    /// A function on the builtin itself, with no receiver (`Array.isArray`).
    Static,
}

impl BuiltinMemberKind {
    /// The runtime discriminator string for this kind.
    ///
    /// The generated runtime looks members up by `(class, kind, member)` triples
    /// rendered as plain strings, so both sides share this spelling.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Prototype => "prototype",
            Self::Static => "static",
        }
    }
}

/// One modeled member of a JavaScript builtin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct BuiltinMember {
    /// The builtin's global name (`"Array"`, `"String"`).
    pub class: &'static str,
    /// Whether the member is a prototype method or a static function.
    pub kind: BuiltinMemberKind,
    /// The member's source name.
    pub member: &'static str,
    /// The function's JavaScript `length` (its declared parameter count).
    pub arity: usize,
}

/// Shorthand for a prototype-method entry.
const fn proto(class: &'static str, member: &'static str, arity: usize) -> BuiltinMember {
    BuiltinMember {
        class,
        kind: BuiltinMemberKind::Prototype,
        member,
        arity,
    }
}

/// Shorthand for a static-function entry.
const fn statics(class: &'static str, member: &'static str, arity: usize) -> BuiltinMember {
    BuiltinMember {
        class,
        kind: BuiltinMemberKind::Static,
        member,
        arity,
    }
}

/// Every builtin member Smelt hands out as a callable value.
///
/// `arity` is the member's JavaScript `length`, which is observable
/// (`Array.prototype.slice.length === 2`) and is what the runtime registers on
/// the callable. `Object.prototype`'s members are absent on purpose: Smelt
/// represents that prototype as a sentinel with its own member table
/// (`smelt_object_prototype_member`), and two registries for one prototype would
/// be two answers.
///
/// A builtin only reaches this registry once reading its bare name yields the
/// builtin-namespace value (`builtin_namespace_value_expression`). `String`,
/// `Number` and `Boolean` do not: their bare identifier is claimed by the
/// primitive-cast path, so `String.prototype.trim` never becomes a namespace
/// member read and entries for them would be unreachable. Adding a builtin to
/// that value path is what makes its rows here live.
pub const BUILTIN_MEMBER_FUNCTIONS: &[BuiltinMember] = &[
    proto("Array", "slice", 2),
    proto("Array", "concat", 1),
    proto("Array", "indexOf", 1),
    proto("Array", "lastIndexOf", 1),
    proto("Array", "includes", 1),
    proto("Array", "join", 1),
    statics("Array", "isArray", 1),
];

/// Return the modeled member for a `(class, kind, member)` triple.
#[must_use]
pub fn builtin_member(
    class: &str,
    kind: BuiltinMemberKind,
    member: &str,
) -> Option<&'static BuiltinMember> {
    BUILTIN_MEMBER_FUNCTIONS
        .iter()
        .find(|entry| entry.class == class && entry.kind == kind && entry.member == member)
}

/// The runtime dispatch key for a modeled member (`"Array.prototype.slice"`).
///
/// Used both as the `match` label in the generated dispatcher and as the
/// member's stable function identity, so two reads of the same member are the
/// same function value.
#[must_use]
pub fn builtin_member_key(entry: &BuiltinMember) -> String {
    match entry.kind {
        BuiltinMemberKind::Prototype => {
            format!("{}.prototype.{}", entry.class, entry.member)
        }
        BuiltinMemberKind::Static => format!("{}.{}", entry.class, entry.member),
    }
}

#[cfg(test)]
mod tests {
    use super::{BuiltinMemberKind, builtin_member, builtin_member_key};

    /// A modeled prototype method resolves, with its JavaScript `length`.
    #[test]
    fn prototype_member_resolves_with_arity() {
        let entry = builtin_member("Array", BuiltinMemberKind::Prototype, "slice")
            .expect("Array.prototype.slice is modeled");
        assert_eq!(entry.arity, 2);
        assert_eq!(builtin_member_key(entry), "Array.prototype.slice");
    }

    /// A static and a prototype member of the same name stay distinct.
    #[test]
    fn kinds_do_not_collide() {
        assert!(builtin_member("Array", BuiltinMemberKind::Static, "isArray").is_some());
        assert!(builtin_member("Array", BuiltinMemberKind::Prototype, "isArray").is_none());
    }

    /// An unmodeled member has no entry, so the runtime answers `undefined`.
    #[test]
    fn unmodeled_member_is_absent() {
        assert!(builtin_member("Array", BuiltinMemberKind::Prototype, "flatMap").is_none());
    }
}
