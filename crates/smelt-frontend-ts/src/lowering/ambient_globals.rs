//! Ambient global-object registry and compile-time feature-probe erasure.
//!
//! This module classifies the JavaScript ambient global *aliases* (`globalThis`,
//! `global`, `self`) for the active target profile and provides the AST-shape
//! helpers the lowering passes use to fold or normalize global references before
//! codegen. It deliberately does not lower expressions itself (see plan §4): it
//! answers classification questions, and the `ModuleBuilder` consumes the answers
//! from `typeof`, binary `in`, identifier, and member-access lowering.
//!
//! ## Target profile
//!
//! The default generated Rust test profile is a deterministic non-DOM,
//! Node-compatible environment:
//!
//! - `globalThis`: present, the canonical alias.
//! - `global`: present, aliased to `globalThis` (Node).
//! - `self`: treated as a global alias name *syntactically* (worker-style), but
//!   absent as a runtime *member* — `"self" in globalThis` is `false`.
//! - `window`: absent (DOM-only); `typeof window` is already handled elsewhere.
//!
//! The set of members a probe like `"Map" in globalThis` answers `true` for is
//! derived from the recognition registries in `smelt-stdlib`
//! ([`smelt_stdlib::global_member_presence`]), not a list maintained here, so the
//! compile-time answer cannot drift from what codegen can actually lower.
//!
//! ## Erasure denylist (conservative)
//!
//! Folding a probe to a literal is only sound when the global object's identity
//! never becomes observable. Per plan §2 the lowering passes refuse to erase or
//! normalize when any of these hold for the global reference:
//!
//! - the global object is assigned to a local that escapes or is returned;
//! - a dynamic (computed) key is read or written;
//! - a property is written onto the global object;
//! - global object identity is compared;
//! - the value flows into something that could inspect or mutate it.
//!
//! The helpers here cover the cases the probe erasure needs (a probe reads a
//! *static* member or tests a *string-literal* key). Anything outside those
//! shapes falls through to ordinary lowering, which keeps the honest blocker.

use oxc::ast::ast::Expression;
use oxc::syntax::operator::UnaryOperator;

/// Identifier names that denote the ambient global object in the non-DOM profile.
///
/// `window` is intentionally excluded: it is DOM-only and absent in this profile,
/// and `typeof window` feature detection is folded by the existing
/// describe-condition handling rather than treated as a live global alias.
const GLOBAL_ALIAS_NAMES: &[&str] = &["globalThis", "global", "self"];

/// Return whether a bare identifier name is an ambient global-object alias.
///
/// This is the syntactic alias set (`globalThis`, `global`, `self`); it does not
/// imply the alias is *usable* as a value, only that a reference to it should be
/// considered for global-path normalization and probe erasure.
#[must_use]
pub(super) fn is_global_alias_name(name: &str) -> bool {
    GLOBAL_ALIAS_NAMES.contains(&name)
}

/// Return the typeof operand identifier name when `expression` is `typeof <ident>`.
///
/// Used to recognize `typeof globalThis` feature probes. Returns `None` for any
/// non-`typeof` expression or a `typeof` of a non-identifier operand.
#[must_use]
pub(super) fn typeof_identifier_name<'a>(expression: &'a Expression<'a>) -> Option<&'a str> {
    let Expression::UnaryExpression(unary) = expression else {
        return None;
    };
    if unary.operator != UnaryOperator::Typeof {
        return None;
    }
    match &unary.argument {
        Expression::Identifier(identifier) => Some(identifier.name.as_str()),
        _ => None,
    }
}

/// The constant value a `typeof <global-alias> <op> "<kind>"` probe folds to.
///
/// In the non-DOM profile every recognized global alias is a present object, so
/// `typeof globalThis === "undefined"` is `false`, `!== "undefined"` is `true`,
/// `=== "object"` is `true`, and `!== "object"` is `false`. Returns `None` when
/// the comparison is not a recognized global-existence probe so the caller leaves
/// the expression to ordinary lowering.
#[must_use]
pub(super) fn global_typeof_probe_value(
    operand_is_global_alias: bool,
    compared_kind: &str,
    is_equality: bool,
) -> Option<bool> {
    if !operand_is_global_alias {
        return None;
    }
    match compared_kind {
        // A present global object: `typeof` is "object", never "undefined".
        "object" => Some(is_equality),
        "undefined" => Some(!is_equality),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The syntactic alias set covers the non-DOM globals but not `window`.
    #[test]
    fn classifies_global_alias_names() {
        for name in ["globalThis", "global", "self"] {
            assert!(is_global_alias_name(name), "{name}");
        }
        for name in ["window", "document", "console", "g", "Foo"] {
            assert!(!is_global_alias_name(name), "{name}");
        }
    }

    /// `typeof <alias>` existence probes fold to the present-object answer.
    #[test]
    fn folds_global_typeof_probes() {
        // typeof globalThis !== "undefined" -> true
        assert_eq!(
            global_typeof_probe_value(true, "undefined", false),
            Some(true)
        );
        // typeof globalThis === "undefined" -> false
        assert_eq!(
            global_typeof_probe_value(true, "undefined", true),
            Some(false)
        );
        // typeof globalThis === "object" -> true
        assert_eq!(global_typeof_probe_value(true, "object", true), Some(true));
        // typeof globalThis !== "object" -> false
        assert_eq!(global_typeof_probe_value(true, "object", false), Some(false));
    }

    /// Non-global operands and unrelated compared kinds do not fold.
    #[test]
    fn leaves_unrelated_typeof_probes() {
        assert_eq!(global_typeof_probe_value(false, "object", true), None);
        assert_eq!(global_typeof_probe_value(true, "function", true), None);
    }
}
