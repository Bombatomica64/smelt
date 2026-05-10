//! Expression and literal representation for HIR.

use serde::{Deserialize, Serialize};

use crate::ids::{BlockId, BodyId, ExprId, ItemId, LocalId, Span, Symbol, TypeId};

/// An expression in the HIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expr {
    /// The kind of expression.
    pub kind: ExprKind,
    /// The type of this expression.
    pub ty: TypeId,
    /// Source location of the expression.
    pub span: Span,
}

/// The kind of an expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprKind {
    /// A literal value.
    Literal(Literal),
    /// A reference to a local variable.
    Local(LocalId),
    /// A reference to an item (function, const, etc.).
    Item(ItemId),
    /// A function call.
    Call {
        /// The function being called.
        callee: ExprId,
        /// The arguments to the call.
        args: Vec<ExprId>,
    },
    /// A method call.
    Method {
        /// The receiver object.
        receiver: ExprId,
        /// The method name.
        method: Symbol,
        /// The arguments to the method.
        args: Vec<ExprId>,
    },
    /// A field access.
    Field {
        /// The object whose field is accessed.
        receiver: ExprId,
        /// The field name.
        field: Symbol,
    },
    /// An index expression.
    Index {
        /// The object being indexed.
        receiver: ExprId,
        /// The index expression.
        index: ExprId,
    },
    /// The length of a string or collection.
    Len {
        /// The value whose length is read.
        operand: ExprId,
    },
    /// Absolute value of an integer or floating-point number.
    NumericAbs {
        /// The numeric value whose absolute value is computed.
        operand: ExprId,
    },
    /// Round a floating-point number with a standard numeric operation.
    NumericRound {
        /// Operation to apply to the number.
        op: NumericRoundOp,
        /// The numeric value being rounded.
        operand: ExprId,
    },
    /// Compute the minimum or maximum of numeric values.
    NumericExtrema {
        /// Operation to apply to the arguments.
        op: NumericExtremaOp,
        /// Numeric arguments to compare.
        args: Vec<ExprId>,
    },
    /// Compute the Euclidean norm of numeric values.
    NumericHypot {
        /// Numeric arguments to combine.
        args: Vec<ExprId>,
    },
    /// Test a numeric value with a numeric predicate.
    NumericPredicate {
        /// Predicate to apply to the number.
        op: NumericPredicateOp,
        /// Numeric operand to test.
        operand: ExprId,
    },
    /// Apply a direct unary numeric function.
    NumericUnaryFunc {
        /// Operation to apply to the number.
        op: NumericUnaryFuncOp,
        /// Numeric operand to transform.
        operand: ExprId,
    },
    /// Raise a floating-point base to a floating-point exponent.
    NumericPow {
        /// Base operand.
        base: ExprId,
        /// Exponent operand.
        exponent: ExprId,
    },
    /// Compute the arctangent of two numeric coordinates.
    NumericAtan2 {
        /// Y coordinate.
        y: ExprId,
        /// X coordinate.
        x: ExprId,
    },
    /// Generate a pseudo-random floating-point value in the half-open range `[0, 1)`.
    NumericRandom,
    /// Generate a pseudo-random integer in an inclusive range.
    NumericRandomInt {
        /// Inclusive lower bound.
        start: ExprId,
        /// Inclusive upper bound.
        end: ExprId,
    },
    /// Convert a primitive value with a direct source-language conversion.
    PrimitiveCast {
        /// Primitive conversion to apply.
        op: PrimitiveCastOp,
        /// Value to convert.
        operand: ExprId,
    },
    /// Change the case of a string value.
    StringCase {
        /// Operation to apply to the string.
        op: StringCaseOp,
        /// The string value being transformed.
        operand: ExprId,
    },
    /// Trim leading and trailing whitespace from a string value.
    StringTrim {
        /// Which side of the string to trim.
        side: StringTrimSide,
        /// The string value being trimmed.
        operand: ExprId,
    },
    /// Test whether a string has a prefix or suffix.
    StringAffix {
        /// Affix operation to apply.
        op: StringAffixOp,
        /// The string value being searched.
        haystack: ExprId,
        /// The affix to test for.
        needle: ExprId,
    },
    /// Find a substring index in a string, returning -1 when absent.
    StringSearch {
        /// Search direction.
        op: StringSearchOp,
        /// The string value being searched.
        haystack: ExprId,
        /// The substring to search for.
        needle: ExprId,
    },
    /// Replace literal string matches with a literal replacement string.
    StringReplace {
        /// Replacement operation to apply.
        op: StringReplaceOp,
        /// The string value being transformed.
        haystack: ExprId,
        /// The literal pattern to replace.
        pattern: ExprId,
        /// The literal replacement value.
        replacement: ExprId,
    },
    /// Remove a literal string prefix or suffix when it is present.
    StringRemoveAffix {
        /// Affix side to remove.
        op: StringAffixOp,
        /// The string value being transformed.
        haystack: ExprId,
        /// The affix to remove.
        affix: ExprId,
    },
    /// Repeat a string a numeric number of times.
    StringRepeat {
        /// The string value being repeated.
        operand: ExprId,
        /// Number of repetitions.
        count: ExprId,
    },
    /// Pad a string to a target length.
    StringPad {
        /// Padding side.
        op: StringPadOp,
        /// The string value being padded.
        operand: ExprId,
        /// Target string length.
        target_len: ExprId,
        /// Padding string.
        pad: ExprId,
    },
    /// Test whether every character in a string satisfies a predicate.
    StringPredicate {
        /// Predicate to apply.
        op: StringPredicateOp,
        /// The string value being tested.
        operand: ExprId,
    },
    /// Test whether a regex pattern matches a string.
    RegexIsMatch {
        /// Regex match shape to apply.
        op: RegexMatchOp,
        /// Regex pattern text.
        pattern: ExprId,
        /// String value being matched.
        haystack: ExprId,
    },
    /// Read a single character from a string as a string value.
    StringCharAt {
        /// The string value being indexed.
        operand: ExprId,
        /// Numeric character index.
        index: ExprId,
    },
    /// Read a single character code from a string as a numeric value.
    StringCharCodeAt {
        /// The string value being indexed.
        operand: ExprId,
        /// Numeric character index.
        index: ExprId,
    },
    /// Test whether one string contains another string.
    StringContains {
        /// The string value being searched.
        haystack: ExprId,
        /// The substring to search for.
        needle: ExprId,
    },
    /// Take a substring slice from a string.
    StringSlice {
        /// String value to slice.
        operand: ExprId,
        /// Inclusive start index, or omitted for zero.
        start: Option<ExprId>,
        /// Exclusive end index, or omitted for string length.
        end: Option<ExprId>,
    },
    /// Test whether a list contains an item.
    ListContains {
        /// The list value being searched.
        list: ExprId,
        /// The item to search for.
        item: ExprId,
    },
    /// Test whether a set contains an item.
    SetContains {
        /// The set value being searched.
        set: ExprId,
        /// The item to search for.
        item: ExprId,
    },
    /// Test whether two sets have no shared items.
    SetDisjoint {
        /// Left set operand.
        left: ExprId,
        /// Right set operand.
        right: ExprId,
    },
    /// Test a relation between two sets.
    SetRelation {
        /// Set relation to evaluate.
        op: SetRelationOp,
        /// Left set operand.
        left: ExprId,
        /// Right set operand.
        right: ExprId,
    },
    /// Insert an item into a set.
    SetAdd {
        /// Set value to mutate.
        set: ExprId,
        /// Item to insert.
        item: ExprId,
    },
    /// Remove an item from a set.
    SetRemove {
        /// Missing-item behavior.
        op: SetRemoveOp,
        /// Set value to mutate.
        set: ExprId,
        /// Item to remove.
        item: ExprId,
    },
    /// Clear all items from a set.
    SetClear {
        /// Set value to mutate.
        set: ExprId,
    },
    /// Return a shallow copy of a set.
    SetCopy {
        /// Set value to copy.
        set: ExprId,
    },
    /// Combine two sets into a new set.
    SetBinary {
        /// Set operation to apply.
        op: SetBinaryOp,
        /// Left set operand.
        left: ExprId,
        /// Right set operand.
        right: ExprId,
    },
    /// Project values or entries from a set.
    SetProjection {
        /// Projection to apply.
        op: SetProjectionOp,
        /// Set value to project.
        set: ExprId,
    },
    /// Concatenate two lists into a new list.
    ListConcat {
        /// Left list value.
        left: ExprId,
        /// Right list value.
        right: ExprId,
    },
    /// Find an item index in a list, returning -1 when absent.
    ListSearch {
        /// Search direction.
        op: ListSearchOp,
        /// List value to search.
        list: ExprId,
        /// Item to search for.
        item: ExprId,
    },
    /// Apply a capture-free callback to an array-like list operation.
    ListCallback {
        /// List callback operation to apply.
        op: ListCallbackOp,
        /// List value to process.
        list: ExprId,
        /// Capture-free callback expression.
        callback: CallbackExpr,
    },
    /// Reduce a list with a capture-free two-argument callback and initial value.
    ListReduce {
        /// List value to reduce.
        list: ExprId,
        /// Initial accumulator value.
        initial: ExprId,
        /// Capture-free reducer callback expression.
        callback: CallbackExpr,
    },
    /// Take a shallow slice from a list.
    ListSlice {
        /// List value to slice.
        list: ExprId,
        /// Inclusive start index, or omitted for zero.
        start: Option<ExprId>,
        /// Exclusive end index, or omitted for collection length.
        end: Option<ExprId>,
    },
    /// Push one item into a list and return the new length.
    ListPush {
        /// List value to mutate.
        list: ExprId,
        /// Item to append.
        item: ExprId,
    },
    /// Extend a list with items from another list and return `None`.
    ListExtend {
        /// List value to mutate.
        list: ExprId,
        /// List supplying items to copy.
        other: ExprId,
    },
    /// Insert one item into a list at an integer index and return `None`.
    ListInsert {
        /// List value to mutate.
        list: ExprId,
        /// Integer index where the item is inserted.
        index: ExprId,
        /// Item to insert.
        item: ExprId,
    },
    /// Insert zero or more items at the front of a list and return the new length.
    ListUnshift {
        /// List value to mutate.
        list: ExprId,
        /// Items to insert at the front.
        items: Vec<ExprId>,
    },
    /// Reverse a list in place and return the language-specific result.
    ListReverse {
        /// List value to mutate.
        list: ExprId,
    },
    /// Clear all items from a list and return `None`.
    ListClear {
        /// List value to mutate.
        list: ExprId,
    },
    /// Return a shallow copy of a list.
    ListCopy {
        /// List value to copy.
        list: ExprId,
    },
    /// Count list items equal to a target item.
    ListCount {
        /// List value to scan.
        list: ExprId,
        /// Item to count.
        item: ExprId,
    },
    /// Sum numeric items in a list.
    ListSum {
        /// List value to reduce.
        list: ExprId,
    },
    /// Fold boolean items in a list with `all` or `any` semantics.
    ListBoolFold {
        /// Fold operation to apply.
        op: BoolFoldOp,
        /// Boolean list value to reduce.
        list: ExprId,
    },
    /// Return a sorted copy of a list.
    ListSorted {
        /// List value to copy and sort.
        list: ExprId,
    },
    /// Return a reversed copy of a list.
    ListReversed {
        /// List value to copy in reverse order.
        list: ExprId,
    },
    /// Pair list items with zero-based integer indexes.
    ListEnumerate {
        /// List value to enumerate.
        list: ExprId,
    },
    /// Pair two lists elementwise, truncating to the shorter list.
    ListZip {
        /// Left list value to zip.
        left: ExprId,
        /// Right list value to zip.
        right: ExprId,
    },
    /// Materialize an integer range as a list.
    ListRange {
        /// Inclusive start value.
        start: ExprId,
        /// Exclusive end value.
        end: ExprId,
        /// Step value.
        step: ExprId,
    },
    /// Pick one item from a list with a pseudo-random index.
    ListRandomChoice {
        /// List value to choose from.
        list: ExprId,
    },
    /// Return the first index of an equal list item.
    ListIndex {
        /// List value to scan.
        list: ExprId,
        /// Item to locate.
        item: ExprId,
    },
    /// Remove the first list item equal to a target item and return `None`.
    ListRemove {
        /// List value to mutate.
        list: ExprId,
        /// Item to remove.
        item: ExprId,
    },
    /// Sort a list in place and return `None`.
    ListSort {
        /// List value to mutate.
        list: ExprId,
    },
    /// Pop the last item from a list.
    ListPop {
        /// List value to mutate.
        list: ExprId,
    },
    /// Remove and return the first item from a list.
    ListShift {
        /// List value to mutate.
        list: ExprId,
    },
    /// Test whether a tuple contains an item.
    TupleContains {
        /// The tuple value being searched.
        tuple: ExprId,
        /// The item to search for.
        item: ExprId,
    },
    /// Test whether a dictionary contains a key.
    DictContainsKey {
        /// The dictionary value being searched.
        dict: ExprId,
        /// The key to search for.
        key: ExprId,
    },
    /// Insert or replace a dictionary key-value pair.
    DictSet {
        /// Dictionary value to mutate.
        dict: ExprId,
        /// Key to write.
        key: ExprId,
        /// Value to write.
        value: ExprId,
    },
    /// Remove a dictionary key and return whether it existed.
    DictRemoveKey {
        /// Dictionary value to mutate.
        dict: ExprId,
        /// Key to remove.
        key: ExprId,
    },
    /// Look up a dictionary key and return an optional value or default.
    DictGet {
        /// Dictionary value to read.
        dict: ExprId,
        /// Key to look up.
        key: ExprId,
        /// Optional default value for missing keys.
        default: Option<ExprId>,
    },
    /// Return an existing dictionary value or insert and return a default.
    DictSetDefault {
        /// Dictionary value to mutate.
        dict: ExprId,
        /// Key to look up or insert.
        key: ExprId,
        /// Default value to insert for a missing key.
        default: ExprId,
    },
    /// Clear all entries from a dictionary and return `None`.
    DictClear {
        /// Dictionary value to mutate.
        dict: ExprId,
    },
    /// Remove a dictionary key and return its value.
    DictPop {
        /// Dictionary value to mutate.
        dict: ExprId,
        /// Key to remove.
        key: ExprId,
        /// Optional default value for missing keys.
        default: Option<ExprId>,
    },
    /// Extend a dictionary with entries from another dictionary and return `None`.
    DictUpdate {
        /// Dictionary value to mutate.
        dict: ExprId,
        /// Dictionary supplying entries to copy.
        other: ExprId,
    },
    /// Return a shallow copy of a dictionary.
    DictCopy {
        /// Dictionary value to copy.
        dict: ExprId,
    },
    /// Project keys, values, or entries from a dictionary.
    DictProjection {
        /// Projection to apply.
        op: DictProjectionOp,
        /// Dictionary value to project.
        dict: ExprId,
    },
    /// Split a string into a list of strings.
    StringSplit {
        /// The string value being split.
        haystack: ExprId,
        /// The separator string.
        separator: ExprId,
    },
    /// Join a list of strings with a separator string.
    StringJoin {
        /// String items to join.
        items: ExprId,
        /// Separator string.
        separator: ExprId,
    },
    /// Serialize a JSON-compatible value to a JSON string.
    JsonStringify {
        /// Value to serialize.
        value: ExprId,
    },
    /// Parse a JSON string into a statically known JSON-compatible type.
    JsonParse {
        /// JSON text to parse.
        text: ExprId,
    },
    /// Perform a blocking HTTP GET request and return the response body as text.
    HttpGetText {
        /// The URL to request.
        url: ExprId,
    },
    /// A binary operation.
    BinOp {
        /// The operator.
        op: BinOp,
        /// The left-hand side.
        lhs: ExprId,
        /// The right-hand side.
        rhs: ExprId,
    },
    /// A unary operation.
    UnaryOp {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: ExprId,
    },
    /// A block expression.
    Block(BlockId),
    /// A lambda function.
    Lambda {
        /// The body of the lambda.
        body: BodyId,
        /// The return type.
        return_ty: TypeId,
    },
    /// A list literal.
    ListLit(Vec<ExprId>),
    /// A set literal.
    SetLit(Vec<ExprId>),
    /// Convert a list into a set by collecting unique items.
    ListToSet {
        /// List value to collect into a set.
        list: ExprId,
    },
    /// Convert a list of key/value tuples into a dictionary.
    ListPairsToDict {
        /// List of pair tuples to collect into a dictionary.
        list: ExprId,
    },
    /// A dictionary literal.
    DictLit(Vec<(ExprId, ExprId)>),
    /// A tuple literal.
    TupleLit(Vec<ExprId>),
    /// Convert a homogeneous tuple into a list.
    TupleToList {
        /// Tuple value to collect into a list.
        tuple: ExprId,
    },
    /// Convert a homogeneous list into a statically-sized tuple.
    ListToTuple {
        /// List value to collect into a tuple.
        list: ExprId,
    },
    /// Convert a homogeneous tuple into a set by collecting unique items.
    TupleToSet {
        /// Tuple value to collect into a set.
        tuple: ExprId,
    },
    /// Read one statically-known tuple field by index.
    TupleIndex {
        /// Tuple value to read from.
        tuple: ExprId,
        /// Zero-based tuple item index after source-language normalization.
        index: usize,
    },
    /// Build a tuple from a statically-known slice of another tuple.
    TupleSlice {
        /// Tuple value to slice.
        tuple: ExprId,
        /// Inclusive normalized start index.
        start: usize,
        /// Exclusive normalized end index.
        end: usize,
    },
    /// A constructor call.
    New {
        /// The class being instantiated.
        class: Symbol,
        /// The constructor arguments.
        args: Vec<ExprId>,
    },
    /// An await expression that suspends on a future.
    Await(ExprId),
    /// A runtime-backed async operation.
    AsyncOp {
        /// Operation to perform.
        op: AsyncOp,
        /// Input future/value expressions.
        args: Vec<ExprId>,
    },
}

/// A directly lowered string case conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringCaseOp {
    /// Convert a string to lower case.
    Lower,
    /// Convert a string to upper case.
    Upper,
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
    /// Convert to string.
    ToString,
}

/// A directly lowered numeric predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericPredicateOp {
    /// Test whether a number is finite.
    IsFinite,
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

/// A directly lowered dictionary projection operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DictProjectionOp {
    /// Project dictionary keys.
    Keys,
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
    /// Return true if any item satisfies the callback.
    Some,
    /// Return true if every item satisfies the callback.
    Every,
    /// Evaluate the callback for each item and return `None`.
    ForEach,
}

/// A capture-free callback expression tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallbackExpr {
    /// Callback expression kind.
    pub kind: CallbackExprKind,
    /// Callback expression type.
    pub ty: TypeId,
}

/// The kind of a capture-free callback expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CallbackExprKind {
    /// A callback parameter reference by zero-based position.
    Param(usize),
    /// A literal value.
    Literal(Literal),
    /// A unary operation.
    Unary {
        /// Operation to apply.
        op: UnaryOp,
        /// Operand expression.
        operand: Box<CallbackExpr>,
    },
    /// A binary operation.
    Binary {
        /// Operation to apply.
        op: BinOp,
        /// Left operand.
        lhs: Box<CallbackExpr>,
        /// Right operand.
        rhs: Box<CallbackExpr>,
    },
}

/// Runtime-backed async operation represented in HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsyncOp {
    /// Wait for all futures and produce all outputs.
    All,
    /// Resolve when the first future completes.
    Race,
    /// Wait for all futures and keep settled outputs.
    AllSettled,
    /// Sleep for a duration in milliseconds.
    Sleep,
    /// Create a task from a future.
    CreateTask,
    /// Wait for a future with a timeout.
    WaitFor,
    /// Perform an HTTP GET request and return the response body as text.
    HttpGetText,
}

/// A literal value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// A boolean literal.
    Bool(bool),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A string literal.
    String(String),
    /// The None/null literal.
    None,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    /// Addition operator.
    Add,
    /// Subtraction operator.
    Sub,
    /// Multiplication operator.
    Mul,
    /// Division operator.
    Div,
    /// Equality operator.
    Eq,
    /// Inequality operator.
    NotEq,
    /// Less-than operator.
    Lt,
    /// Less-than-or-equal operator.
    Lte,
    /// Greater-than operator.
    Gt,
    /// Greater-than-or-equal operator.
    Gte,
    /// Logical AND operator.
    And,
    /// Logical OR operator.
    Or,
}

/// Returns the text representation of a binary operator.
#[must_use]
pub const fn bin_op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// Logical NOT operator.
    Not,
    /// Negation operator.
    Neg,
}
