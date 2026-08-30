//! Operator enums for lowered expression intrinsics.

use serde::{Deserialize, Serialize};

/// A directly lowered string case conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringCaseOp {
    /// Convert a string to lower case.
    Lower,
    /// Convert a string to upper case.
    Upper,
}

/// A directly lowered Unicode string normalization form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringNormalizeForm {
    /// Canonical decomposition followed by canonical composition.
    Nfc,
    /// Canonical decomposition.
    Nfd,
    /// Compatibility decomposition followed by canonical composition.
    Nfkc,
    /// Compatibility decomposition.
    Nfkd,
}

/// A directly lowered numeric rounding operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericRoundOp {
    /// Round toward negative infinity.
    Floor,
    /// Round toward positive infinity.
    Ceil,
    /// Round to the nearest integer value.
    Round,
    /// Round toward zero.
    Trunc,
}

/// A directly lowered numeric extrema operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericExtremaOp {
    /// Compute the smallest argument.
    Min,
    /// Compute the largest argument.
    Max,
}

/// A directly lowered boolean collection fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolFoldOp {
    /// Return true when every item is true.
    All,
    /// Return true when at least one item is true.
    Any,
}

/// A directly lowered unary numeric function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericUnaryFuncOp {
    /// Square root.
    Sqrt,
    /// Cube root.
    Cbrt,
    /// Numeric sign.
    Sign,
    /// Sine.
    Sin,
    /// Cosine.
    Cos,
    /// Tangent.
    Tan,
    /// Arcsine.
    Asin,
    /// Arccosine.
    Acos,
    /// Arctangent.
    Atan,
    /// Natural logarithm.
    Log,
    /// Base-10 logarithm.
    Log10,
    /// Base-2 logarithm.
    Log2,
    /// Exponential function.
    Exp,
}

/// A directly lowered primitive conversion operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveCastOp {
    /// Convert to boolean.
    ToBool,
    /// Convert to integer.
    ToInt,
    /// Convert to floating-point number.
    ToFloat,
    /// Parse a JavaScript-coerced string as a floating-point number.
    ParseFloat,
    /// Convert with JavaScript `Number(...)` and unary-plus semantics.
    ToJsNumber,
    /// Convert to string.
    ToString,
}

/// A directly lowered numeric predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericPredicateOp {
    /// Test whether a number is finite.
    IsFinite,
    /// Test whether a number is an integer.
    IsInteger,
    /// Test whether a number is an integer inside the IEEE-754 double
    /// exactly-representable range (`Number.isSafeInteger`).
    IsSafeInteger,
    /// Test whether a number is NaN.
    IsNaN,
}

/// Which side of a string trim operation is lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringTrimSide {
    /// Trim both leading and trailing whitespace.
    Both,
    /// Trim leading whitespace.
    Start,
    /// Trim trailing whitespace.
    End,
}

/// A directly lowered set removal operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetRemoveOp {
    /// Return whether an item was removed.
    Delete,
    /// Ignore missing items and return `None`.
    Discard,
    /// Panic when the item is missing and return `None`.
    Remove,
}

/// A directly lowered set algebra operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetBinaryOp {
    /// Return items in either set.
    Union,
    /// Return items in both sets.
    Intersection,
    /// Return items in the left set but not the right set.
    Difference,
    /// Return items present in exactly one set.
    SymmetricDifference,
}

/// A directly lowered set relation predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetRelationOp {
    /// Test whether the left set is a subset of the right set.
    IsSubset,
    /// Test whether the left set is a superset of the right set.
    IsSuperset,
}

/// A directly lowered set projection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetProjectionOp {
    /// Project set values.
    Values,
    /// Project item-item entries.
    Entries,
}

/// A directly lowered string affix test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringAffixOp {
    /// Test for a prefix.
    StartsWith,
    /// Test for a suffix.
    EndsWith,
}

/// A directly lowered string search operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringSearchOp {
    /// Find the first occurrence.
    Find,
    /// Find the last occurrence.
    RFind,
}

/// A directly lowered string replacement operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringReplaceOp {
    /// Replace only the first match.
    First,
    /// Replace every match.
    All,
}

/// A directly lowered string padding operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringPadOp {
    /// Pad the start of the string.
    Start,
    /// Pad the end of the string.
    End,
}

/// A directly lowered string character predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringPredicateOp {
    /// Test for digit characters.
    IsDigit,
    /// Test for alphabetic characters.
    IsAlpha,
    /// Test for alphanumeric characters.
    IsAlnum,
}

/// A directly lowered regex boolean match operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegexMatchOp {
    /// Search for a match anywhere in the string.
    Search,
    /// Require the match to start at the beginning of the string.
    Match,
    /// Require the match to cover the full string.
    FullMatch,
}

/// A directly lowered local-time `Date` component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatePart {
    /// Calendar year.
    FullYear,
    /// Zero-based month.
    Month,
    /// One-based day of month.
    Date,
    /// Zero-based day of week, starting on Sunday.
    Day,
    /// Hour of day.
    Hour,
    /// Minute of hour.
    Minute,
    /// Second of minute.
    Second,
    /// Millisecond of second.
    Millisecond,
}

/// A field available on a parsed URL value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UrlField {
    /// Full URL text.
    Href,
    /// URL scheme/protocol including the trailing colon.
    Protocol,
    /// Host, including port when present.
    Host,
    /// Origin, including protocol and host.
    Origin,
    /// Hostname without port.
    Hostname,
    /// Path component.
    Pathname,
    /// Query component including a leading `?` when present.
    Search,
}

/// A directly lowered dictionary projection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DictProjectionOp {
    /// Build a dictionary from key-value entry arrays.
    FromEntries,
    /// Project dictionary keys.
    Keys,
    /// Project keys for JavaScript `for...in` iteration.
    ForInKeys,
    /// Project dictionary symbol keys.
    Symbols,
    /// Project dictionary values.
    Values,
    /// Project dictionary key-value entries.
    Entries,
}

/// A directly lowered list search operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListSearchOp {
    /// Find the first matching item.
    Find,
    /// Find the last matching item.
    RFind,
}

/// A directly lowered callback-heavy list operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListCallbackOp {
    /// Transform each item into a new list.
    Map,
    /// Keep items where the callback returns true.
    Filter,
    /// Return the first item where the callback returns true.
    Find,
    /// Return the first index where the callback returns true, or -1.
    FindIndex,
    /// Return the last item where the callback returns true.
    FindLast,
    /// Return the last index where the callback returns true, or -1.
    FindLastIndex,
    /// Return true if any item satisfies the callback.
    Some,
    /// Return true if every item satisfies the callback.
    Every,
    /// Evaluate the callback for each item and return `None`.
    ForEach,
    /// Transform each item into a list and flatten one level.
    FlatMap,
}

/// A directly lowered list projection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListProjectionOp {
    /// Project numeric array indexes.
    Keys,
    /// Project array values as a shallow copy.
    Values,
    /// Project index-value entries.
    Entries,
}
