//! What a chai assertion word means, once read off a chain.
//!
//! vitest's `expect` is chai's, extended, so both the vitest matcher lowering
//! (which must leave a chai word on `expect(..)` to the chai recognizer) and
//! the chai recognizer itself read this vocabulary; neither owns it.

use crate::lowering::ModuleBuilder;

/// An assertion rule, once the library's spelling is gone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum LibraryCheck {
    /// The vitest matcher of this name has the same meaning.
    Matcher(&'static str),
    /// `==`.
    LooseEqual,
    /// `===`.
    StrictEqual,
    /// `=== true` / `=== false`.
    IsBool(bool),
    /// `Object.prototype.toString` names this type.
    TypeIs(&'static str),
    /// As [`Self::TypeIs`], with the name in the first argument.
    TypeArgument,
    /// Neither `null` nor `undefined`.
    Exists,
    /// Answered by the runtime over erased values (`smelt_test_check`).
    Runtime(&'static str),
    /// Fails, with the message in the first argument.
    Fail,
}

/// The rule chai's `expect` chain ends in, given the flags collected before it.
///
/// `None` is a word this model does not have.
pub(super) fn chai_terminal(name: &str, deep: bool, contains: bool) -> Option<LibraryCheck> {
    use LibraryCheck::{Exists, Fail, IsBool, Matcher, Runtime, StrictEqual, TypeArgument};
    Some(match name {
        "equal" | "equals" | "eq" if deep => Matcher("toStrictEqual"),
        "equal" | "equals" | "eq" => StrictEqual,
        "eql" | "eqls" => Matcher("toStrictEqual"),
        "true" => IsBool(true),
        "false" => IsBool(false),
        "null" => Matcher("toBeNull"),
        "undefined" => Matcher("toBeUndefined"),
        "NaN" => Matcher("toBeNaN"),
        "ok" => Matcher("toBeTruthy"),
        "exist" | "exists" => Exists,
        "empty" => Runtime("is_empty"),
        "a" | "an" => TypeArgument,
        "instanceof" | "instanceOf" => Matcher("toBeInstanceOf"),
        "include" | "includes" | "contain" | "contains" | "string" if deep => {
            Runtime("deep_include")
        }
        "include" | "includes" | "contain" | "contains" | "string" => Runtime("include"),
        "lengthOf" | "length" => Runtime("length_is"),
        "match" | "matches" => Matcher("toMatch"),
        "above" | "gt" | "greaterThan" => Matcher("toBeGreaterThan"),
        "below" | "lt" | "lessThan" => Matcher("toBeLessThan"),
        "least" | "gte" | "greaterThanOrEqual" => Matcher("toBeGreaterThanOrEqual"),
        "most" | "lte" | "lessThanOrEqual" => Matcher("toBeLessThanOrEqual"),
        "within" => Runtime("within"),
        "closeTo" | "approximately" => Runtime("close_to"),
        "property" | "ownProperty" | "haveOwnProperty" => Runtime("has_property"),
        "keys" | "key" => Runtime("keys_are"),
        "members" if contains => Runtime("includes_members"),
        "members" => Runtime("same_members"),
        "oneOf" => Runtime("one_of"),
        "satisfy" | "satisfies" => Runtime("satisfies"),
        "throw" | "throws" | "Throw" => Matcher("toThrow"),
        "fail" => Fail,
        _ => return None,
    })
}

impl ModuleBuilder<'_> {
    /// Whether `name` ends a chai chain.
    pub(in crate::lowering) fn is_chai_terminal(name: &str) -> bool {
        chai_terminal(name, false, false).is_some()
    }
}
