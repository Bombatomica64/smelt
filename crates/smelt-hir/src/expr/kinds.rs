use super::{
    AsyncOp, BinOp, BoolFoldOp, ClosureExpr, DatePart, DictProjectionOp,
    ListCallbackOp, ListProjectionOp, ListSearchOp, Literal, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, PrimitiveCastOp, RegexMatchOp, SetBinaryOp,
    SetProjectionOp, SetRelationOp, SetRemoveOp, StringAffixOp, StringCaseOp, StringNormalizeForm,
    StringPadOp, StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, UnaryOp,
    UnknownKind, UrlField,
};
use crate::ids::{BlockId, BodyId, ExprId, ItemId, LocalId, Symbol, TypeId};
use serde::{Deserialize, Serialize};

/// A replacement argument passed to array splice-style operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSpliceItem {
    /// Replacement value or array value when `spread` is true.
    pub value: ExprId,
    /// Whether the source argument used JavaScript spread syntax.
    pub spread: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "Variant docs remain in formatter and lowering code; compact enum layout keeps this file under 600 LOC"
)]
pub enum ExprKind {
    Literal(Literal),
    Local(LocalId),
    Item(ItemId),
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Closure(ClosureExpr),
    ClosureCall {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    ClosureCallSpread {
        callee: ExprId,
        args: ExprId,
    },
    Method {
        receiver: ExprId,
        method: Symbol,
        args: Vec<ExprId>,
    },
    Field {
        receiver: ExprId,
        field: Symbol,
    },
    OptionalField {
        receiver: ExprId,
        field: Symbol,
    },
    Index {
        receiver: ExprId,
        index: ExprId,
    },
    OptionalIndex {
        receiver: ExprId,
        index: ExprId,
    },
    OptionalMethod {
        receiver: ExprId,
        method: Symbol,
        args: Vec<ExprId>,
    },
    OptionalCoalesce {
        optional: ExprId,
        fallback: ExprId,
    },
    TypeAssert {
        value: ExprId,
    },
    Len {
        operand: ExprId,
    },
    NumericAbs {
        operand: ExprId,
    },
    NumericRound {
        op: NumericRoundOp,
        operand: ExprId,
    },
    NumericExtrema {
        op: NumericExtremaOp,
        args: Vec<ExprId>,
    },
    NumericHypot {
        args: Vec<ExprId>,
    },
    NumericPredicate {
        op: NumericPredicateOp,
        operand: ExprId,
    },
    NumericUnaryFunc {
        op: NumericUnaryFuncOp,
        operand: ExprId,
    },
    NumericPow {
        base: ExprId,
        exponent: ExprId,
    },
    NumericAtan2 {
        y: ExprId,
        x: ExprId,
    },
    NumericRandom,
    NumericRandomInt {
        start: ExprId,
        end: ExprId,
    },
    NumericToStringRadix {
        operand: ExprId,
        radix: ExprId,
    },
    /// Format a numeric value as a fixed-point decimal string with a given
    /// number of fractional digits (`Number.prototype.toFixed`).
    NumericToFixed {
        /// Numeric operand being formatted.
        operand: ExprId,
        /// Number of fractional digits to render.
        digits: ExprId,
    },
    /// Parse an integer from a string with a JavaScript-style numeric radix.
    ParseIntRadix {
        operand: ExprId,
        radix: ExprId,
    },
    PrimitiveCast {
        op: PrimitiveCastOp,
        operand: ExprId,
    },
    StringCase {
        op: StringCaseOp,
        operand: ExprId,
    },
    StringNormalize {
        form: StringNormalizeForm,
        operand: ExprId,
    },
    /// JavaScript `encodeURI(operand)`: percent-encode the string, leaving the
    /// URI-reserved and unreserved characters of the `encodeURI` character set
    /// intact (see the runtime `smelt_encode_uri` helper for the exact set).
    UriEncode {
        operand: ExprId,
    },
    /// JavaScript `Object.prototype.toString.call(operand)`: the classic
    /// `"[object Tag]"` type probe. Resolves the tag from the erased value's
    /// runtime variant and host identity markers (see the runtime
    /// `smelt_object_to_string_tag` helper).
    ObjectToStringTag {
        operand: ExprId,
    },
    StringTrim {
        side: StringTrimSide,
        operand: ExprId,
    },
    StringAffix {
        op: StringAffixOp,
        haystack: ExprId,
        needle: ExprId,
    },
    StringSearch {
        op: StringSearchOp,
        haystack: ExprId,
        needle: ExprId,
        from_index: Option<ExprId>,
    },
    StringReplace {
        op: StringReplaceOp,
        haystack: ExprId,
        pattern: ExprId,
        replacement: ExprId,
    },
    StringRemoveAffix {
        op: StringAffixOp,
        haystack: ExprId,
        affix: ExprId,
    },
    StringRepeat {
        operand: ExprId,
        count: ExprId,
    },
    StringPad {
        op: StringPadOp,
        operand: ExprId,
        target_len: ExprId,
        pad: ExprId,
    },
    StringPredicate {
        op: StringPredicateOp,
        operand: ExprId,
    },
    RegexIsMatch {
        op: RegexMatchOp,
        pattern: ExprId,
        haystack: ExprId,
    },
    RegexReplace {
        op: StringReplaceOp,
        pattern: ExprId,
        haystack: ExprId,
        replacement: ExprId,
    },
    RegexReplaceCallback {
        op: StringReplaceOp,
        pattern: ExprId,
        haystack: ExprId,
        callback: ExprId,
    },
    RegexReplaceFirstMatchUppercase {
        pattern: ExprId,
        haystack: ExprId,
    },
    RegexSplit {
        pattern: ExprId,
        haystack: ExprId,
    },
    RegexFind {
        pattern: ExprId,
        haystack: ExprId,
    },
    RegexExec {
        regex: ExprId,
        haystack: ExprId,
    },
    RegexMatchAll {
        regex: ExprId,
        haystack: ExprId,
    },
    StringCharAt {
        operand: ExprId,
        index: ExprId,
    },
    StringCharCodeAt {
        operand: ExprId,
        index: ExprId,
    },
    StringContains {
        haystack: ExprId,
        needle: ExprId,
        /// Optional JavaScript `position` argument for `String.prototype.includes`.
        ///
        /// When present, the search starts at this position (truncated toward
        /// zero and clamped to the valid range). `None` means search from the start.
        from_index: Option<ExprId>,
    },
    StringSlice {
        operand: ExprId,
        start: Option<ExprId>,
        end: Option<ExprId>,
    },
    ListContains {
        list: ExprId,
        item: ExprId,
    },
    SetContains {
        set: ExprId,
        item: ExprId,
    },
    SetDisjoint {
        left: ExprId,
        right: ExprId,
    },
    SetRelation {
        op: SetRelationOp,
        left: ExprId,
        right: ExprId,
    },
    SetAdd {
        set: ExprId,
        item: ExprId,
    },
    SetRemove {
        op: SetRemoveOp,
        set: ExprId,
        item: ExprId,
    },
    SetClear {
        set: ExprId,
    },
    SetCopy {
        set: ExprId,
    },
    SetBinary {
        op: SetBinaryOp,
        left: ExprId,
        right: ExprId,
    },
    SetProjection {
        op: SetProjectionOp,
        set: ExprId,
    },
    ListConcat {
        left: ExprId,
        right: ExprId,
    },
    ListSearch {
        op: ListSearchOp,
        list: ExprId,
        item: ExprId,
        /// Optional JavaScript `fromIndex` argument for `Array.prototype.indexOf`
        /// and `lastIndexOf`.
        ///
        /// When present, the search starts at this index (truncated toward zero;
        /// negative values count back from the end). `None` searches the whole array.
        from_index: Option<ExprId>,
    },
    ListCallback {
        op: ListCallbackOp,
        list: ExprId,
        callback: ExprId,
    },
    ListFromLength {
        length: ExprId,
    },
    ListRepeat {
        value: ExprId,
        count: ExprId,
    },
    ListFromLengthMap {
        length: ExprId,
        callback: ExprId,
    },
    ListReduce {
        list: ExprId,
        initial: Option<ExprId>,
        callback: ExprId,
    },
    ListSlice {
        list: ExprId,
        start: Option<ExprId>,
        end: Option<ExprId>,
    },
    ListSplice {
        list: ExprId,
        start: ExprId,
        delete_count: Option<ExprId>,
        items: Vec<ListSpliceItem>,
        mutate: bool,
    },
    ListFill {
        list: ExprId,
        value: ExprId,
        start: Option<ExprId>,
        end: Option<ExprId>,
    },
    ListCopyWithin {
        list: ExprId,
        target: ExprId,
        start: ExprId,
        end: Option<ExprId>,
    },
    ListWith {
        list: ExprId,
        index: ExprId,
        value: ExprId,
    },
    ListFlat {
        list: ExprId,
        depth: Option<ExprId>,
    },
    ListProjection {
        op: ListProjectionOp,
        list: ExprId,
    },
    ListPush {
        list: ExprId,
        item: ExprId,
    },
    ListExtend {
        list: ExprId,
        other: ExprId,
    },
    ListInsert {
        list: ExprId,
        index: ExprId,
        item: ExprId,
    },
    ListUnshift {
        list: ExprId,
        items: Vec<ExprId>,
    },
    ListReverse {
        list: ExprId,
    },
    ListClear {
        list: ExprId,
    },
    ListCopy {
        list: ExprId,
    },
    ListCount {
        list: ExprId,
        item: ExprId,
    },
    ListSum {
        list: ExprId,
    },
    ListBoolFold {
        op: BoolFoldOp,
        list: ExprId,
    },
    ListSorted {
        list: ExprId,
        /// Optional key closure for Python `sorted(values, key=...)`.
        ///
        /// Like other callbacks, the key is a normal closure body referenced by
        /// `ExprId`; it maps one list item to a sortable value.
        key: Option<ExprId>,
        /// Whether to sort in descending order, as in Python `reverse=True`.
        reverse: bool,
    },
    ListReversed {
        list: ExprId,
    },
    ListEnumerate {
        list: ExprId,
    },
    ListZip {
        left: ExprId,
        right: ExprId,
    },
    ListRange {
        start: ExprId,
        end: ExprId,
        step: ExprId,
    },
    ListRandomChoice {
        list: ExprId,
    },
    ListIndex {
        list: ExprId,
        item: ExprId,
    },
    ListRemove {
        list: ExprId,
        item: ExprId,
    },
    ListSort {
        list: ExprId,
        /// Optional comparator closure for JavaScript `Array.prototype.sort`.
        ///
        /// Like other callbacks, the comparator is a normal closure body
        /// referenced by `ExprId`; it takes two list items and returns a number.
        comparator: Option<ExprId>,
        /// Optional key closure for Python `list.sort(key=...)`.
        ///
        /// The key is a normal closure body referenced by `ExprId`; it maps one
        /// list item to a sortable value. It is mutually exclusive with the
        /// JavaScript-style `comparator`.
        key: Option<ExprId>,
        /// Whether to sort in descending order, as in Python `reverse=True`.
        reverse: bool,
    },
    ListPop {
        list: ExprId,
    },
    ListShift {
        list: ExprId,
    },
    /// Consume the first list item and return a JavaScript iterator-result object.
    ListNext {
        list: ExprId,
    },
    /// Test whether a typed iterator result is exhausted.
    IteratorDone {
        result: ExprId,
    },
    /// Read the optional value from a typed iterator result.
    IteratorValue {
        result: ExprId,
    },
    TupleContains {
        tuple: ExprId,
        item: ExprId,
    },
    DictContainsKey {
        dict: ExprId,
        key: ExprId,
    },
    DictSet {
        dict: ExprId,
        key: ExprId,
        value: ExprId,
    },
    DictRemoveKey {
        dict: ExprId,
        key: ExprId,
    },
    DictGet {
        dict: ExprId,
        key: ExprId,
        default: Option<ExprId>,
    },
    DictSetDefault {
        dict: ExprId,
        key: ExprId,
        default: ExprId,
    },
    DictClear {
        dict: ExprId,
    },
    DictPop {
        dict: ExprId,
        key: ExprId,
        default: Option<ExprId>,
    },
    DictUpdate {
        dict: ExprId,
        other: ExprId,
    },
    DictAssign {
        target: ExprId,
        sources: Vec<ExprId>,
    },
    /// Attach object-literal properties to a callable JavaScript value.
    CallableObjectAssign {
        callable: ExprId,
        props: Vec<(Symbol, ExprId)>,
    },
    DictCopy {
        dict: ExprId,
    },
    DictProjection {
        op: DictProjectionOp,
        dict: ExprId,
    },
    StringSplit {
        haystack: ExprId,
        separator: ExprId,
        limit: Option<ExprId>,
    },
    /// Convert a string into a list of one-character strings.
    StringChars {
        haystack: ExprId,
    },
    StringJoin {
        items: ExprId,
        separator: ExprId,
    },
    JsonStringify {
        value: ExprId,
    },
    JsonParse {
        text: ExprId,
    },
    HttpGetText {
        url: ExprId,
    },
    DateNow,
    /// Configure the timestamp returned by JavaScript `Date.now()`.
    DateSetNow {
        timestamp: ExprId,
    },
    /// Restore the real JavaScript `Date.now()` clock.
    DateResetNow,
    /// Read the configured JavaScript `Date.prototype.getTimezoneOffset` value.
    DateTimezoneOffset,
    /// Configure the return value observed by `Date.prototype.getTimezoneOffset`.
    DateSetTimezoneOffset {
        offset: ExprId,
    },
    /// Restore the default `Date.prototype.getTimezoneOffset` implementation.
    DateResetTimezoneOffset,
    /// Create a date-fns-compatible date context function for an IANA time zone.
    DateTimezoneContext {
        timezone: ExprId,
    },
    DateToIsoString {
        timestamp_ms: ExprId,
    },
    /// Convert a Date timestamp to JavaScript `Date.prototype.toString()` output.
    DateToString {
        timestamp_ms: ExprId,
    },
    DateFromParts {
        parts: Vec<ExprId>,
    },
    DateFromValue {
        value: ExprId,
    },
    DateGetPart {
        part: DatePart,
        timestamp_ms: ExprId,
    },
    DateSetPart {
        part: DatePart,
        timestamp_ms: ExprId,
        values: Vec<ExprId>,
    },
    UrlField {
        field: UrlField,
        url: ExprId,
    },
    FileReadText {
        path: ExprId,
    },
    FileWriteText {
        path: ExprId,
        text: ExprId,
    },
    /// Construct a modeled host `Blob` or `File` record from constructor
    /// arguments (`new Blob(parts?, options?)` / `new File(parts, name, options?)`).
    ///
    /// `parts` is the erased `BlobPart` array (strings and other `Blob`/`File`
    /// records); `blob_type` is the resolved MIME `type` string. `name` and
    /// `last_modified` are present only for `File`, whose record additionally
    /// carries the `__smelt_file` marker on top of `__smelt_blob` so
    /// `file instanceof Blob` observes the host subtype relationship. The
    /// runtime helper concatenates part contents and stores the UTF-8 byte
    /// `size`, so `.size`/`.type`/`.name` reads observe real values.
    BlobFromParts {
        parts: ExprId,
        blob_type: ExprId,
        name: Option<ExprId>,
        last_modified: Option<ExprId>,
    },
    BinOp {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    UnaryOp {
        op: UnaryOp,
        operand: ExprId,
    },
    Conditional {
        cond: ExprId,
        then_expr: ExprId,
        else_expr: ExprId,
    },
    FunctionTableLookup {
        key: ExprId,
        cases: Vec<(String, ExprId)>,
    },
    InstanceOf {
        value: ExprId,
        class: Symbol,
    },
    UnknownIs {
        value: ExprId,
        kind: UnknownKind,
    },
    TypeofValue {
        value: ExprId,
    },
    /// Compute the opaque `Object.getPrototypeOf` sentinel for an erased value.
    ///
    /// Lowers to the `smelt_prototype_sentinel` runtime helper, which returns a
    /// distinct string for arrays, plain objects, and class instances (and `null`
    /// for null-prototype values), so prototype comparisons can tell a class
    /// instance apart from a structurally identical plain object.
    PrototypeSentinel {
        /// Value whose prototype sentinel is being computed.
        value: ExprId,
    },
    UnknownCast {
        value: ExprId,
        target: TypeId,
    },
    Block(BlockId),
    Lambda {
        body: BodyId,
        return_ty: TypeId,
    },
    ListLit(Vec<ExprId>),
    SetLit(Vec<ExprId>),
    ListToSet {
        list: ExprId,
    },
    ListPairsToDict {
        list: ExprId,
    },
    DictLit(Vec<(ExprId, ExprId)>),
    TupleLit(Vec<ExprId>),
    TupleToList {
        tuple: ExprId,
    },
    ListToTuple {
        list: ExprId,
    },
    TupleToSet {
        tuple: ExprId,
    },
    TupleIndex {
        tuple: ExprId,
        index: usize,
    },
    TupleSlice {
        tuple: ExprId,
        start: usize,
        end: usize,
    },
    New {
        class: Symbol,
        args: Vec<ExprId>,
    },
    Await(ExprId),
    AsyncOp {
        op: AsyncOp,
        args: Vec<ExprId>,
    },
}
