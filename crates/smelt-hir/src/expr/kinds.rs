use super::{
    AsyncOp, BinOp, BoolFoldOp, ClosureExpr, DatePart, DictProjectionOp,
    ListCallbackOp, ListProjectionOp, ListSearchOp, Literal, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, PrimitiveCastOp, RegexMatchOp, RegexReplaceArg,
    SetBinaryOp,
    SetProjectionOp, SetRelationOp, SetRemoveOp, StringAffixOp, StringCaseOp, StringNormalizeForm,
    StringPadOp, StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, UnaryOp,
    EventEmitterOp as EventEmitterOpKind, HeadersOp as HeadersOpKind,
    HttpServerOp as HttpServerOpKind, IncomingMessageOp as IncomingMessageOpKind,
    ServerResponseOp as ServerResponseOpKind,
    RequestOp as RequestOpKind, ResponseOp as ResponseOpKind,
    UnknownKind, UriTranscodeOp,
    UrlField,
    UrlSearchParamsOp as UrlSearchParamsOpKind,
};
use crate::ids::{BlockId, BodyId, ExprId, ItemId, LocalId, Symbol, TypeId};
use serde::{Deserialize, Serialize};

/// Protocol operation used to resume or abruptly complete a generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneratorResumeKind {
    /// Continue execution and make the supplied value the result of `yield`.
    Next,
    /// Complete the generator with the supplied return value.
    Return,
    /// Inject an exception at the suspended `yield` expression.
    Throw,
}

/// How far a property-presence test is allowed to look for the key.
///
/// JavaScript has two presence tests over one property name, and they answer
/// differently for anything a value inherits: `key in value` walks the
/// prototype chain, so `'toString' in {}` is `true`, while
/// `Object.hasOwn(value, key)` (and `hasOwnProperty` / `propertyIsEnumerable`)
/// stop at the value's own properties, so `Object.hasOwn({}, 'toString')` is
/// `false`. Both lower to `DictContainsKey`, so the containment expression has
/// to carry which of the two it is; without it the emitter had to pick one
/// reach for both spellings and was wrong about the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyLookup {
    /// Own properties only — `Object.hasOwn`, `hasOwnProperty`, and every
    /// typed-collection membership test (`Map.has`, a `Dict` key probe), whose
    /// keys are all own by construction.
    Own,
    /// Own properties and everything inherited — the `in` operator.
    PrototypeChain,
}

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
    /// Read of a module-level mutable global binding.
    ///
    /// `item` references an [`crate::Item::MutableGlobal`]; the expression is
    /// typed as that binding's declared type. Reads of a lifted module `let`
    /// lower to this instead of being const-inlined.
    GlobalGet {
        /// The mutable-global item being read.
        item: ItemId,
    },
    /// Store to a module-level mutable global binding.
    ///
    /// Evaluates `value`, stores it into the global referenced by `item`, and
    /// evaluates to the stored value so that `++`/`--` and compound assignments
    /// compose as expressions. `item` references an
    /// [`crate::Item::MutableGlobal`].
    GlobalSet {
        /// The mutable-global item being written.
        item: ItemId,
        /// The value expression to store.
        value: ExprId,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// Suspend the current generator and expose `value` to its caller.
    GeneratorYield {
        /// Value exposed by the suspension point.
        value: ExprId,
    },
    /// Resume a generator once, optionally sending a value into its suspended yield.
    GeneratorNext {
        generator: ExprId,
        value: Option<ExprId>,
        kind: GeneratorResumeKind,
    },
    /// Test whether a resume result represents completion.
    GeneratorDone { result: ExprId },
    /// Extract the yielded or returned value from a resume result.
    GeneratorValue { result: ExprId },
    /// Resume a delegated generator, forwarding each suspension to the caller.
    GeneratorDelegate { generator: ExprId },
    Closure(ClosureExpr),
    /// Read the JavaScript `this` receiver installed by the active call.
    ///
    /// A plain function or function expression has no lexical receiver: in
    /// JavaScript its `this` is supplied by the CALL, not by the definition
    /// site. Smelt models that with a dynamically scoped receiver channel --
    /// [`ExprKind::BindThis`] installs a receiver for the duration of one call
    /// and this expression reads whatever the innermost active call installed,
    /// answering `undefined` for a plain (receiver-less) invocation. Arrow
    /// functions never produce this expression: they capture the enclosing
    /// function's `this` local lexically, exactly as the source does.
    ThisRead,
    /// Bind a receiver to a callable, yielding `Function.prototype.bind`'s value.
    ///
    /// The result is a callable that, when invoked, installs `receiver` as the
    /// `this` seen by [`ExprKind::ThisRead`] in the callee body and restores the
    /// previous binding afterwards. This is the single representation behind all
    /// three JavaScript spellings that supply a receiver: `object.method(..)`,
    /// `fn.call(thisArg, ..)`, and `fn.apply(thisArg, argsArray)`.
    BindThis {
        /// The callable whose receiver is being bound.
        callee: ExprId,
        /// The value the callee's `this` resolves to.
        receiver: ExprId,
    },
    ClosureCall {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// JavaScript `new callee(args)` where `callee` is a function VALUE — the
    /// `[[Construct]]` operation, not a call.
    ///
    /// Every non-arrow JavaScript function is a constructor, and constructing
    /// through one is observably different from calling it: an object is
    /// allocated whose prototype link is the callee's own `prototype` property,
    /// the callee runs with that object as its `this`, and the result is the
    /// callee's return value only when that value is an object — otherwise the
    /// allocated object. All three are what make `new f() instanceof g` answer
    /// anything but `false`, so a plain [`ExprKind::ClosureCall`] cannot stand
    /// in for this node.
    Construct {
        /// The function value being constructed through.
        callee: ExprId,
        /// The spelled constructor arguments.
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
        /// Optional numeric list reduced alongside `args`, produced when the
        /// source spreads a list into `Math.max`/`Math.min`
        /// (e.g. `Math.max(...values)`). The reduction folds every element of
        /// this list with the scalar `args` using the same extrema operation.
        spread: Option<ExprId>,
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
    /// One of the four ECMA-262 URI transcoding globals applied to `operand`:
    /// `encodeURI`, `encodeURIComponent`, `decodeURI`, `decodeURIComponent`.
    /// `op` carries which; see [`UriTranscodeOp`] for why one node covers all
    /// four, and the `smelt_encode_uri*` / `smelt_decode_uri*` runtime helpers
    /// for the exact character sets.
    UriTranscode {
        op: UriTranscodeOp,
        operand: ExprId,
    },
    /// JavaScript `Object.prototype.toString.call(operand)`: the classic
    /// `"[object Tag]"` type probe. Resolves the tag from the erased value's
    /// runtime variant and host identity markers (see the runtime
    /// `smelt_object_to_string_tag` helper).
    ObjectToStringTag {
        operand: ExprId,
    },
    /// Deep-copy an erased value graph with fresh identities, preserving host
    /// markers (`structuredClone(x)`; see the runtime `smelt_structured_clone`).
    StructuredClone {
        operand: ExprId,
    },
    StringTrim {
        side: StringTrimSide,
        operand: ExprId,
    },
    /// JavaScript `left.localeCompare(right)` with no locale/option arguments:
    /// collate two strings, answering a negative / zero / positive number. The
    /// modeled collation levels are documented on the runtime
    /// `smelt_locale_compare` helper.
    StringLocaleCompare {
        left: ExprId,
        right: ExprId,
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
        /// The ECMA-262 replacer arguments the callback declared, in order.
        /// Resolved in the frontend from the pattern's capture-group count and
        /// the callback's arity; see [`RegexReplaceArg`].
        args: Vec<RegexReplaceArg>,
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
    /// `new Headers(init?)`.
    ///
    /// `init` is a record, a list of name/value pairs, or another `Headers`
    /// value; which one is decided from the initializer's static type, so the
    /// construction stays a concrete typed value with no runtime tag test.
    HeadersNew {
        /// Optional initializer expression.
        init: Option<ExprId>,
    },
    /// `new URLSearchParams(init?)`.
    ///
    /// `init` is a query string, a record, a list of name/value pairs, or
    /// another `URLSearchParams`; the initializer's static type selects the
    /// conversion.
    UrlSearchParamsNew {
        /// Optional initializer expression.
        init: Option<ExprId>,
    },
    /// A `URLSearchParams` method call on a concrete receiver.
    UrlSearchParamsOp {
        /// Which operation this call performs.
        op: UrlSearchParamsOpKind,
        /// The `URLSearchParams` receiver.
        params: ExprId,
        /// Operation arguments (name, or name and value).
        args: Vec<ExprId>,
    },
    /// `new Response(body?, init?)`.
    ///
    /// The init object's keys are lowered to their own fields rather than kept
    /// as a record: `status`, `statusText` and `headers` have exact source types
    /// (`number`, `string`, `HeadersInit`), and keeping them typed here is what
    /// lets codegen build the concrete `SmeltResponse` without re-deriving a
    /// shape from an erased record at run time. An init that is not an object
    /// literal cannot be split this way and is a named blocker in the frontend.
    ResponseNew {
        /// The body argument, when the source passed one.
        body: Option<ExprId>,
        /// `init.status`, when the init literal set it.
        status: Option<ExprId>,
        /// `init.statusText`, when the init literal set it.
        status_text: Option<ExprId>,
        /// `init.headers`, when the init literal set it.
        headers: Option<ExprId>,
    },
    /// `new EventEmitter()`.
    ///
    /// No options are modeled: the constructor's only argument is
    /// `{ captureRejections }`, which changes how a rejected promise returned
    /// by a listener is reported and has no meaning until listeners can be
    /// async here.
    EventEmitterNew,
    /// An `EventEmitter` member operation on a concrete emitter receiver.
    ///
    /// The receiver is any value whose class
    /// [`has_event_emitter`](smelt_stdlib::StdlibClass::has_event_emitter) — an
    /// `EventEmitter` itself, or a `node:http` `IncomingMessage`, which holds
    /// one by composition.
    EventEmitterOp {
        /// Which operation this member performs.
        op: EventEmitterOpKind,
        /// The emitter receiver.
        emitter: ExprId,
        /// The event name, followed by the operation's own arguments.
        args: Vec<ExprId>,
    },
    /// `createServer(handler)` from `node:http`.
    ///
    /// The handler is stored CONCRETELY: unlike an event listener, its
    /// signature is fixed by the module — `(IncomingMessage, ServerResponse)` —
    /// so it needs none of the erasure the emitter's listener list needs.
    HttpCreateServer {
        /// The request handler, called once per accepted request.
        handler: ExprId,
    },
    /// A `node:http` `Server` member operation.
    HttpServerOp {
        /// Which operation this member performs.
        op: HttpServerOpKind,
        /// The server receiver.
        server: ExprId,
        /// The operation's arguments: `listen` takes a port, an optional host,
        /// and an optional listening callback; the others take none.
        args: Vec<ExprId>,
    },
    /// A `node:http` `IncomingMessage` property read.
    IncomingMessageOp {
        /// Which property this reads.
        op: IncomingMessageOpKind,
        /// The request receiver.
        message: ExprId,
    },
    /// A `node:http` `ServerResponse` member operation.
    ServerResponseOp {
        /// Which operation this member performs.
        op: ServerResponseOpKind,
        /// The response receiver.
        response: ExprId,
        /// The operation's arguments, in source order.
        args: Vec<ExprId>,
    },
    /// `new Request(input, init?)`.
    ///
    /// `input` is the request URL; `method`, `headers` and `body` come from the
    /// init literal's keys, split into typed fields for the same reason
    /// [`Self::ResponseNew`] splits its own.
    RequestNew {
        /// The URL argument.
        input: ExprId,
        /// `init.method`, when the init literal set it.
        method: Option<ExprId>,
        /// `init.headers`, when the init literal set it.
        headers: Option<ExprId>,
        /// `init.body`, when the init literal set it.
        body: Option<ExprId>,
    },
    /// A `Request` member operation on a concrete `Request` receiver.
    RequestOp {
        /// Which operation this member performs.
        op: RequestOpKind,
        /// The `Request` receiver.
        request: ExprId,
        /// Operation arguments; every modeled member is nullary today.
        args: Vec<ExprId>,
    },
    /// A `Response` member operation on a concrete `Response` receiver.
    ResponseOp {
        /// Which operation this member performs.
        op: ResponseOpKind,
        /// The `Response` receiver.
        response: ExprId,
        /// Operation arguments; every modeled member is nullary today.
        args: Vec<ExprId>,
    },
    /// A WHATWG `Headers` method call on a concrete `Headers` receiver.
    HeadersOp {
        /// Which header operation this call performs.
        op: HeadersOpKind,
        /// The `Headers` receiver.
        headers: ExprId,
        /// Operation arguments (name, or name and value).
        args: Vec<ExprId>,
    },
    ListConcat {
        left: ExprId,
        right: ExprId,
    },
    /// Normalize one `Array.prototype.concat` argument whose static type is
    /// erased into the list of items it contributes.
    ///
    /// JavaScript splices an array argument's elements into the result and
    /// appends any other value as a single element. A concretely-typed argument
    /// settles that at lowering time; an `unknown`, type-parameter, or mixed
    /// union argument can be either shape at runtime, so the choice is deferred.
    /// The node's type is the receiver's list type.
    ConcatSpread {
        value: ExprId,
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
        lookup: PropertyLookup,
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
        /// Record-typed source values whose own enumerable entries are copied
        /// onto the callable object at runtime (e.g. `Object.assign(fn, def)`
        /// where `def` is a record variable rather than an object literal).
        spreads: Vec<ExprId>,
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
    /// Construct a stateful Vitest `vi.fn([impl])` mock: a callable erased
    /// object that records calls and serves configured one-shot/default
    /// outcomes (`mockReturnValue`, `mockResolvedValue`, `mockRejectedValueOnce`, ...).
    VitestMockFn {
        /// Optional wrapped implementation used as the default outcome.
        implementation: Option<ExprId>,
    },
    /// Whether a Vitest mock's recorded call count equals `count`
    /// (`expect(mock).toHaveBeenCalledTimes(count)`); true means the
    /// assertion holds. Non-mock values pass vacuously (documented compat).
    VitestMockCalledTimes {
        mock: ExprId,
        count: ExprId,
    },
    /// Whether a Vitest mock recorded a call whose arguments deep-equal
    /// `args` (`expect(mock).toHaveBeenCalledWith(...)`); true means the
    /// assertion holds. When `last` is set, only the most recent recorded
    /// call is compared (`toHaveBeenLastCalledWith(...)`). Non-mock values
    /// pass vacuously (documented compat).
    VitestMockCalledWith {
        mock: ExprId,
        args: Vec<ExprId>,
        last: bool,
    },
    /// `vi.restoreAllMocks()`: undo every installed spy, newest first.
    VitestRestoreAllMocks,
    /// `vi.spyOn(target, name)`: replace `target[name]` with a recording mock
    /// that forwards to the member's current value, and evaluate to that mock.
    ///
    /// The target is a real runtime object and the replacement is a real
    /// insertion, so library code that later reads `target[name]` calls the
    /// mock (and the mock calls the original). That is what makes the recorded
    /// calls the ones the program actually made.
    VitestSpyOn {
        target: ExprId,
        name: ExprId,
    },
    /// Whether two values are deep-equal under the vitest matcher rules
    /// (`expect(a).toEqual(b)` where either side may hold an ASYMMETRIC
    /// matcher).
    ///
    /// Distinct from an ordinary structural comparison because an asymmetric
    /// matcher answers for itself: at every level of the walk, a value branded
    /// `__smelt_asymmetric` is asked whether it matches its counterpart. That
    /// rule belongs to the test harness only, so it must not reach
    /// `SmeltUnknown`'s own `PartialEq`, which library code observes.
    VitestAsymmetricEqual {
        actual: ExprId,
        expected: ExprId,
    },
    /// Whether a Vitest mock's most recent recorded result deep-equals
    /// `expected` after flattening a resolved promise
    /// (`expect(mock).toHaveLastResolvedWith(...)`); true means the assertion
    /// holds. Non-mock values pass vacuously (documented compat).
    VitestMockLastResolvedWith {
        mock: ExprId,
        expected: ExprId,
    },
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
    /// Construct a modeled host object of a *registry* identity from its
    /// constructor arguments (`new ArrayBuffer(8)`, `new DataView(buf, 1, 2)`).
    ///
    /// The distinguishing feature is that this lowers to the *same* runtime
    /// constructor the reflected `Object.getPrototypeOf(x).constructor` path
    /// calls. JavaScript clone idioms reach a host constructor both ways —
    /// directly (`new DataView(v.buffer.slice(0), v.byteOffset, v.byteLength)` in
    /// es-toolkit's `cloneDeepWith`) and reflectively (`new Constructor(...)` in
    /// its `clone`) — so a record built one way has to be indistinguishable from
    /// one built the other. Routing both through one runtime helper is what makes
    /// that true by construction instead of by two hand-matched lowerings.
    ///
    /// `class_name` is the host constructor's registry class name (`"DataView"`);
    /// `args` are the spelled constructor arguments, erased.
    HostConstruct {
        class_name: String,
        args: Vec<ExprId>,
    },
    /// The single interned value for a global builtin *name* used as a value
    /// (`Blob`, `ArrayBuffer`, `Math`, `JSON`).
    ///
    /// JavaScript exposes one object per global name, so `Blob === Blob` and
    /// `blob.constructor === Blob` both hold. Building a fresh record per
    /// reference — which is what a plain object literal does, since records mint
    /// an identity on construction — makes both comparisons `false`. The runtime
    /// helper caches one record per name, and it is the value a host-marker
    /// record's `.constructor` resolves to, so the two spellings meet.
    ///
    /// For a name that also names a modeled host *constructor*, the interned
    /// record additionally carries a `__smelt_call` slot, so `new Ctor(...)`
    /// through a captured reference constructs rather than answering `null`.
    BuiltinNamespace {
        name: String,
    },
    /// The JavaScript `arguments` object of the enclosing non-arrow function,
    /// built from that function's own parameters.
    ///
    /// `arguments` is an array-like exotic object: its *elements* are the actual
    /// call arguments under index keys, and its `length` is non-enumerable, so
    /// `Object.keys(arguments)` is `["0", "1", ...]` with no `"length"`. Code that
    /// compares an `arguments` object against a plain object
    /// (`isEqual(toArgs([1, 2, 3]), { 0: 1, 1: 2, 2: 3 })`) depends on exactly
    /// that key set, so a `{ length: n }` stand-in cannot stand in.
    ///
    /// Smelt reconstructs the object from the parameters the enclosing function
    /// declared: `fixed` are the positional parameter reads, and `rest` is the
    /// rest parameter's list, flattened onto the end. A function whose parameters
    /// fully describe its call arguments — which is every function whose
    /// `arguments` Smelt can see — reproduces the object exactly.
    ArgumentsObject {
        fixed: Vec<ExprId>,
        rest: Option<ExprId>,
    },
    /// Read the current value of a modeled host constructor's global override
    /// slot (`globalThis.<class>`), for a host name the crate reassigns
    /// somewhere (see the host-global override plan).
    ///
    /// When the slot is `Native` the read yields a *native-handle* marker record
    /// (`{ "__smelt_native_ctor": true, "name": "<class>" }`) — an identity token
    /// used for save/restore, not a callable. When the slot has been overridden
    /// the read yields the stored value (a constructor value, or JS `undefined`
    /// when set absent). Evaluates to a tagged dynamic value.
    HostGlobalRead {
        /// Modeled host constructor whose override slot is read.
        class: Symbol,
    },
    /// Write `value` into a modeled host constructor's global override slot
    /// (`globalThis.<class> = value`). Evaluates to the stored value so the
    /// assignment composes as an expression.
    ///
    /// The runtime write helper classifies the stored value: JS `undefined`
    /// makes the slot `Absent`; a value carrying the native-handle marker
    /// restores the slot to `Native`; a function / class-constructor value makes
    /// the slot hold that constructor (`Ctor`).
    HostGlobalWrite {
        /// Modeled host constructor whose override slot is written.
        class: Symbol,
        /// The value being stored.
        value: ExprId,
    },
    /// Whether a modeled host constructor's global override slot is currently
    /// *present* (`typeof <class> !== 'undefined'` folding for a reassigned host
    /// name). `false` only when the slot has been overridden to JS `undefined`
    /// (`Absent`); `true` for `Native` and any `Ctor` override. Bool-typed.
    HostGlobalPresent {
        /// Modeled host constructor whose override slot presence is tested.
        class: Symbol,
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
    /// JavaScript `value instanceof target` where `target` is a function VALUE
    /// rather than a nominal class name (`OrdinaryHasInstance`).
    ///
    /// The answer is a prototype-chain walk: `value`'s chain is followed link by
    /// link and each link compared by reference against the target's own
    /// `prototype` property. A nominal class target keeps
    /// [`ExprKind::InstanceOf`], whose marker probe already knows the class
    /// identity at compile time; this node is for the case where the constructor
    /// is only known at runtime, which is every plain function used as one.
    InstanceOfValue {
        /// The value whose prototype chain is walked.
        value: ExprId,
        /// The constructor function value whose `prototype` is looked for.
        target: ExprId,
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
        /// Whether an own `__proto__` slot on the receiver shadows the answer.
        ///
        /// `Object.getPrototypeOf(v)` never consults own properties, so it sets
        /// this to `false`. The `v.__proto__` accessor does: in JavaScript
        /// `__proto__` is an accessor inherited from `Object.prototype`, so a
        /// value with a null prototype (`Object.create(null)`) does not inherit
        /// it and a `__proto__` write there stores an ordinary own property that
        /// a later read must answer. Smelt represents a null-prototype object as
        /// a plain erased object, so the own slot is the only observable trace of
        /// that case and the accessor has to prefer it.
        own_slot_shadows: bool,
    },
    /// Box a primitive the way `Object(value)` does (objects pass through).
    ///
    /// Lowers to the `smelt_box_value` runtime helper. The wrapper it builds is
    /// the same marker shape `new Number(..)` / `new Boolean(..)` /
    /// `new String(..)` build, so both spellings share one representation.
    BoxPrimitive {
        /// Value being boxed.
        value: ExprId,
    },
    /// Create a fresh erased object from a runtime prototype value.
    ///
    /// Lowers to the `smelt_object_from_prototype` runtime helper. `Object.create`
    /// must yield a NEW object, never the prototype it was handed: returning the
    /// argument aliases the prototype (so `Object.assign(Object.create(p), o)`
    /// would mutate `p`) and, when the prototype is one of the opaque
    /// `"__smelt_proto:*"` sentinels produced by `PrototypeSentinel`, would hand
    /// back a string where the source expects an object.
    ObjectFromPrototype {
        /// Prototype the fresh object inherits from.
        prototype: ExprId,
    },
    /// Install a property-descriptor table onto an erased object.
    ///
    /// Lowers to the `smelt_define_properties` runtime helper, which is the
    /// shared body of `Object.defineProperty` and `Object.defineProperties`:
    /// both spellings hand a target object a map from property key to
    /// descriptor, and differ only in whether that map has one entry or many.
    /// Before this existed the two calls lowered to an opaque `null` and the
    /// mutation was DROPPED, so a `cloneDeep` of a `defineProperties` result
    /// came back missing every defined key.
    DefineProperties {
        /// Object the properties are installed on; also the result value.
        target: ExprId,
        /// Erased object mapping property key to property descriptor.
        descriptors: ExprId,
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
