//! The one table of operations HIR and MIR share.
//!
//! An operation is stated once here and both IRs expand it: the [`ExprKind`]
//! and `Rvalue` variant, the child walkers, the `dump-hir`/`dump-mir` text and,
//! for the transcribed group, the lowering between them. Adding an operation is
//! an entry here, its emission in `smelt-codegen-rust`, and its recognition in a
//! frontend.
//!
//! The technique is the X-macro: [`for_each_op!`] hands every entry to a
//! callback macro, and each consumer is a `macro_rules!` that destructures the
//! same grammar. Variant names and field names are the ones the emitter and the
//! frontends match on, so nothing downstream sees a difference from a
//! hand-written enum.
//!
//! # Entry grammar
//!
//! ```text
//! /// docs
//! Name { field: kind, ... } fact(value)... => (display);
//! ```
//!
//! Field order is evaluation order. A unit operation omits the braces.
//!
//! | kind             | HIR field                 | MIR field                    |
//! |------------------|---------------------------|------------------------------|
//! | `one`            | `ExprId`                  | `Operand`                    |
//! | `opt`            | `Option<ExprId>`          | `Option<Operand>`            |
//! | `many`           | `Vec<ExprId>`             | `Vec<Operand>`               |
//! | `splice`         | `Vec<ListSpliceItem>`     | `Vec<MirListSpliceItem>`     |
//! | `symbol_pairs`   | `Vec<(Symbol, ExprId)>`   | `Vec<(Symbol, Operand)>`     |
//! | `{K}`            | `Vec<(K, ExprId)>`        | `Vec<(K, Operand)>`          |
//! | `[T]`            | `T`, carried unchanged    | `T`, carried unchanged       |
//! | `(sym "label")`  | `Symbol`                  | `Symbol`                     |
//! | `(ty)`           | `TypeId`                  | `TypeId`                     |
//!
//! # Facts
//!
//! A fact is a per-op statement written in terms of the op's fields, each bound
//! as a reference.
//!
//! * `fallible(condition)`: the op can throw a catchable error when `condition`
//!   holds. Lowering asks [`op_fallible!`] through `Rvalue::is_fallible` and
//!   gives such an op a call terminator, whose unwind edge is the only way a
//!   `try` the source wrote can see the throw; otherwise the op is a plain
//!   assignment.
//! * `call(text)`: how `dump-mir` names the op in its call-terminator form.
//! * `mutates(receiver)`: the `Option<&Operand>` the op writes through in place.
//! * `updates_state(receiver)`: the operand whose runtime-modelled state the op
//!   changes behind a shared handle (a `RegExp`'s `lastIndex`), which a field
//!   read of a value of that type observes. Unlike `mutates`, the operand is not
//!   written in place, so it stays immutable to the MIR's own analyses.
//! * `fresh(condition)`: the result is a newly allocated list no one else holds.
//!
//! A field may also state its role for list escape analysis, `name: kind @ROLE`,
//! with `ROLE` a constant of `operand_roles` or a parenthesised expression over
//! it. A field with no role is unmodelled, and the analysis treats it as an
//! escape.
//!
//! # Display
//!
//! `display` is an expression evaluated with each field rebound to its text: a
//! `String` for `one`, `(sym ..)` and `(ty)`, `Option<String>` for `opt`,
//! `Vec<String>` for `many`, `splice` and `symbol_pairs`, `Vec<(&K, String)>`
//! for `{K}`. A `[T]` field stays `&T`. The two IRs print a `(sym "label")` as
//! the symbol's name and as `label<id>` respectively. Where they print an
//! operation differently, `display` is `(hir: .., mir: ..)`.
//!
//! [`ExprKind`]: crate::ExprKind

/// Expands to the Rust type of an op field, for `operand` and `splice` the
/// IR's own operand and splice-item types.
#[macro_export]
macro_rules! op_field_ty {
    ($operand:ty, $splice:ty, one) => { $operand };
    ($operand:ty, $splice:ty, opt) => { Option<$operand> };
    ($operand:ty, $splice:ty, many) => { Vec<$operand> };
    ($operand:ty, $splice:ty, splice) => { Vec<$splice> };
    ($operand:ty, $splice:ty, symbol_pairs) => { Vec<($crate::Symbol, $operand)> };
    ($operand:ty, $splice:ty, { $key:ty }) => { Vec<($key, $operand)> };
    ($operand:ty, $splice:ty, [$data:ty]) => { $data };
    ($operand:ty, $splice:ty, (sym $label:literal)) => { $crate::Symbol };
    ($operand:ty, $splice:ty, (ty)) => { $crate::TypeId };
}

/// Selects the HIR text of an entry's display clause.
#[macro_export]
macro_rules! op_hir_display {
    ((hir: $hir:expr, mir: $mir:expr $(,)?)) => { $hir };
    (($both:expr)) => { $both };
}

/// Selects the MIR text of an entry's display clause.
#[macro_export]
macro_rules! op_mir_display {
    ((hir: $hir:expr, mir: $mir:expr $(,)?)) => { $mir };
    (($both:expr)) => { $both };
}

/// Expands to an entry's `fallible` condition; an entry without one cannot throw.
#[macro_export]
macro_rules! op_fallible {
    () => { false };
    (fallible ($condition:expr) $($rest:tt)*) => { $condition };
    ($other:ident $value:tt $($rest:tt)*) => { $crate::op_fallible!($($rest)*) };
}

/// Expands to an entry's `call` name; only an op that can throw needs one.
#[macro_export]
macro_rules! op_call_text {
    () => { "@infallible_op".to_owned() };
    (call ($text:expr) $($rest:tt)*) => { $text };
    ($other:ident $value:tt $($rest:tt)*) => { $crate::op_call_text!($($rest)*) };
}

/// Expands to an entry's `mutates` receiver; by default an op mutates nothing.
#[macro_export]
macro_rules! op_mutates {
    () => { None };
    (mutates ($receiver:expr) $($rest:tt)*) => { $receiver };
    ($other:ident $value:tt $($rest:tt)*) => { $crate::op_mutates!($($rest)*) };
}

/// Expands to an entry's `updates_state` receiver as an `Option<ExprId>` (or
/// `Option<Operand>`); by default an op changes no receiver's state.
#[macro_export]
macro_rules! op_updates_state {
    () => { None };
    (updates_state ($receiver:expr) $($rest:tt)*) => { Some(*$receiver) };
    ($other:ident $value:tt $($rest:tt)*) => { $crate::op_updates_state!($($rest)*) };
}

/// Expands to an entry's `fresh` condition; by default a result is not provably
/// fresh.
#[macro_export]
macro_rules! op_fresh {
    () => { false };
    (fresh ($condition:expr) $($rest:tt)*) => { $condition };
    ($other:ident $value:tt $($rest:tt)*) => { $crate::op_fresh!($($rest)*) };
}

/// Hands `callback` every operation, the transcribed group then the custom one.
#[macro_export]
macro_rules! for_each_op {
    ($callback:path) => {
        $crate::op_group_transcribed! { $crate::op_group_custom; $callback; }
    };
}

/// Hands `callback` the operations whose lowering is pure transcription.
#[macro_export]
macro_rules! for_each_transcribed_op {
    ($callback:path) => {
        $crate::op_group_transcribed! { $callback; }
    };
}

/// The operations whose lowering is pure transcription: every child is lowered
/// in field order and the result is one rvalue, assigned to a temporary or, when
/// the op can throw, carried by a call terminator.
///
/// `$($before)*` is forwarded ahead of the entries so the group can be appended
/// to entries a previous group already collected.
#[macro_export]
#[doc(hidden)]
macro_rules! op_group_transcribed {
    ($callback:path; $($before:tt)*) => {
        $callback! {
            $($before)*
            /// Resume a generator once, optionally sending a value into its suspended yield.
            GeneratorNext {
                /// Generator handle to resume.
                generator: one,
                /// Optional value sent into the suspended yield expression.
                value: opt,
                /// Protocol method used for this resumption.
                kind: [$crate::GeneratorResumeKind],
            }
                => (
                    value.map_or_else(
                        || format!("{kind:?} {generator}"),
                        |value| format!("{kind:?} {generator} with {value}"),
                    )
                );
            /// Test whether a generator resume result represents completion.
            GeneratorDone {
                /// Resume result to inspect.
                result: one,
            }
                => (format!("done {result}"));
            /// Extract the yielded or returned value from a generator resume result.
            GeneratorValue {
                /// Resume result whose payload is extracted.
                result: one,
            }
                => (format!("value {result}"));
            /// Resume a delegated generator, forwarding each suspension to the caller.
            GeneratorDelegate {
                /// Generator whose suspension points are forwarded.
                generator: one,
            }
                => (format!("yield* {generator}"));
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
            ThisRead
                => ("this".to_owned());
            /// Bind a receiver to a callable, yielding `Function.prototype.bind`'s value.
            ///
            /// The result is a callable that, when invoked, installs `receiver` as the
            /// `this` seen by [`ExprKind::ThisRead`] in the callee body and restores the
            /// previous binding afterwards. This is the single representation behind all
            /// three JavaScript spellings that supply a receiver: `object.method(..)`,
            /// `fn.call(thisArg, ..)`, and `fn.apply(thisArg, argsArray)`.
            BindThis {
                /// The callable whose receiver is being bound.
                callee: one,
                /// The receiver value the callee's `this` resolves to.
                receiver: one,
            }
                => (format!("bind_this {callee}, {receiver}"));
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
                callee: one,
                /// The spelled constructor arguments.
                args: many,
            }
                => (format!("construct {callee}({})", args.join(", ")));
            /// JavaScript `new value.constructor(args)` on an erased `value`: a new
            /// object of the kind `value` is, an array for an array, whose
            /// constructor Smelt need not hold as a value.
            ConstructLike {
                /// The value whose constructor is read.
                value: one,
                /// The spelled constructor arguments.
                args: many,
            }
                => (format!("construct_like {value}({})", args.join(", ")));
            /// Call a closure value with a runtime argument vector from spread syntax.
            ClosureCallSpread {
                /// Closure value to call.
                callee: one @CALLED,
                /// Runtime argument list.
                args: one @CALLED,
            }
                => (format!("closure_call {callee}(...{args})"));
            /// Call a function value whose whole signature is one rest parameter,
            /// with that rest list written out: `items` are its elements. Packed
            /// by a frontend; see `RestCallFusion`.
            ClosureCallRest {
                /// Closure value to call.
                callee: one @CALLED,
                /// The elements of the rest list.
                items: many @CALLED,
            }
                => (format!("closure_call {callee}(rest {})", items.join(", ")));
            /// Read a field through a TypeScript optional-chain receiver.
            OptionalField {
                /// Receiver operand, either `T` or `Option<T>`.
                receiver: one,
                /// Field name to read when the receiver is present.
                field: (sym "field"),
            }
                => (format!("{receiver}?.{field}"));
            /// Read an index through a TypeScript optional-chain receiver.
            OptionalIndex {
                /// Receiver operand, either an indexable `T` or `Option<T>`.
                receiver: one,
                /// Index operand used when the receiver is present.
                index: one,
            }
                => (format!("{receiver}?.[{index}]"));
            /// Compute the length of a value.
            Len {
                /// The value measured.
                operand: one @RECV,
            }
                => (format!("len {operand}"));
            /// Compute the absolute value of an integer or floating-point number.
            NumericAbs {
                /// The numeric value.
                operand: one,
            }
                => (format!("numeric_abs {operand}"));
            /// Round a floating-point value with a standard numeric operation.
            NumericRound {
                /// Operation to apply.
                op: [$crate::NumericRoundOp],
                /// Numeric operand to round.
                operand: one,
            }
                => (format!("numeric_{op} {operand}"));
            /// Compute the minimum or maximum of floating-point operands.
            NumericExtrema {
                /// Operation to apply.
                op: [$crate::NumericExtremaOp],
                /// Numeric operands to compare.
                args: many @READ,
                /// Optional numeric list reduced alongside `args`, produced when the
                /// source spreads a list into `Math.max`/`Math.min`
                /// (e.g. `Math.max(...values)`). The reduction folds every element of
                /// this list with the scalar `args` using the same extrema operation.
                spread: opt @RECV,
            }
                => (
                    format!(
                        "numeric_{op} {}{}",
                        args.join(", "),
                        spread.map_or_else(String::new, |spread| format!(" ...{spread}"))
                    )
                );
            /// Compute the Euclidean norm of floating-point operands.
            NumericHypot {
                /// Numeric operands to combine.
                args: many,
            }
                => (format!("numeric_hypot {}", args.join(", ")));
            /// Test a numeric value with a numeric predicate.
            NumericPredicate {
                /// Predicate to apply.
                op: [$crate::NumericPredicateOp],
                /// Numeric operand to test.
                operand: one @READ,
            }
                => (format!("numeric_{op} {operand}"));
            /// Apply a direct unary numeric function.
            NumericUnaryFunc {
                /// Operation to apply.
                op: [$crate::NumericUnaryFuncOp],
                /// Numeric operand to transform.
                operand: one,
            }
                => (format!("numeric_{op} {operand}"));
            /// Raise a floating-point base to a floating-point exponent.
            NumericPow {
                /// Base operand.
                base: one,
                /// Exponent operand.
                exponent: one,
            }
                => (format!("numeric_pow {base}, {exponent}"));
            /// Compute the arctangent of two numeric coordinates.
            NumericAtan2 {
                /// Y coordinate.
                y: one,
                /// X coordinate.
                x: one,
            }
                => (format!("numeric_atan2 {y}, {x}"));
            /// Generate a pseudo-random floating-point value in the half-open range `[0, 1)`.
            NumericRandom
                => ("numeric_random".to_owned());
            /// Generate a pseudo-random integer in an inclusive range.
            NumericRandomInt {
                /// Inclusive lower bound.
                start: one,
                /// Inclusive upper bound.
                end: one,
            }
                => (format!("numeric_random_int {start}, {end}"));
            /// Convert a numeric value to a string with a numeric radix.
            NumericToStringRadix {
                /// Numeric operand.
                operand: one,
                /// Numeric radix operand.
                radix: one,
            }
                => (format!("numeric_to_string_radix {operand}, {radix}"));
            /// Format a numeric value as a fixed-point decimal string with a given
            /// number of fractional digits (`Number.prototype.toFixed`).
            NumericToFixed {
                /// Numeric operand being formatted.
                operand: one,
                /// Number of fractional digits to render.
                digits: one,
            }
                => (format!("numeric_to_fixed {operand}, {digits}"));
            /// Parse an integer from a string with a JavaScript-style numeric radix.
            ParseIntRadix {
                /// String operand.
                operand: one,
                /// Numeric radix operand.
                radix: one,
            }
                => (format!("parse_int_radix {operand}, {radix}"));
            /// Convert a primitive value to another primitive type.
            PrimitiveCast {
                /// Conversion operation to apply.
                op: [$crate::PrimitiveCastOp],
                /// Primitive operand to convert.
                operand: one,
            }
                => (format!("primitive_cast_{op} {operand}"));
            /// Change the case of a string value.
            StringCase {
                /// Operation to apply to the string.
                op: [$crate::StringCaseOp],
                /// String operand to transform.
                operand: one,
            }
                => (format!("string_{op} {operand}"));
            /// `String.prototype.isWellFormed`: whether a JavaScript string
            /// holds no lone surrogate.
            StringIsWellFormed {
                /// String operand to test.
                operand: one,
            }
                => (format!("string_is_well_formed {operand}"));
            /// `String.prototype.toWellFormed`: a JavaScript string with each
            /// lone surrogate replaced by U+FFFD.
            StringToWellFormed {
                /// String operand to repair.
                operand: one,
            }
                => (format!("string_to_well_formed {operand}"));
            /// Normalize a Unicode string value.
            StringNormalize {
                /// Normalization form to apply.
                form: [$crate::StringNormalizeForm],
                /// String operand to normalize.
                operand: one,
            }
                => (format!("string_normalize_{form} {operand}"));
            /// JavaScript `Object.prototype.toString.call(operand)`: the classic
            /// `"[object Tag]"` type probe. Resolves the tag from the erased value's
            /// runtime variant and host identity markers (see the runtime
            /// `smelt_object_to_string_tag` helper).
            ObjectToStringTag {
                /// Erased value operand to probe.
                operand: one,
            }
                => (format!("object_to_string_tag {operand}"));
            /// Evaluate a `Symbol(d)` call: a symbol no other evaluation returns.
            ///
            /// `base` is the call site's `symbol` constant. The first
            /// evaluation answers it, so a key folded from the site statically
            /// (a module-level `const K = Symbol()`, evaluated once) still
            /// names the same property; every later evaluation answers a fresh
            /// symbol, so `mk() !== mk()` as in JavaScript.
            SymbolUnique {
                /// The call site's symbol constant.
                base: one,
            }
                => (format!("symbol_unique {base}"));
            /// JavaScript `symbol.description`: the text a symbol was created
            /// with, or `undefined` for a `Symbol()` made without one.
            SymbolDescription {
                /// Symbol operand to read.
                operand: one,
            }
                => (format!("symbol_description {operand}"));
            /// Deep-copy an erased value graph with fresh identities, preserving host
            /// markers (`structuredClone(x)`; see the runtime `smelt_structured_clone`).
            StructuredClone {
                /// Erased value operand to deep-clone.
                operand: one @READ,
            }
                => (format!("structured_clone {operand}"));
            /// Trim whitespace from a string value.
            StringTrim {
                /// Which side of the string to trim.
                side: [$crate::StringTrimSide],
                /// String operand to trim.
                operand: one,
            }
                => (format!("string_trim_{side} {operand}"));
            /// JavaScript `left.localeCompare(right)` with no locale/option arguments:
            /// collate two strings, answering a negative / zero / positive number. The
            /// modeled collation levels are documented on the runtime
            /// `smelt_locale_compare` helper.
            StringLocaleCompare {
                /// Receiver string.
                left: one,
                /// String compared against the receiver.
                right: one,
            }
                => (
                    hir: format!("string_locale_compare {left} {right}"),
                    mir: format!("string_locale_compare {left}, {right}"),
                );
            /// Test whether a string has a prefix or suffix.
            StringAffix {
                /// Affix operation to apply.
                op: [$crate::StringAffixOp],
                /// String value to search in.
                haystack: one,
                /// Affix to search for.
                needle: one,
            }
                => (format!("string_{op} {haystack}, {needle}"));
            /// Find a substring index in a string, returning -1 when absent.
            StringSearch {
                /// Search operation to apply.
                op: [$crate::StringSearchOp],
                /// String value to search in.
                haystack: one,
                /// Substring to search for.
                needle: one,
                /// Optional JavaScript `fromIndex` position.
                from_index: opt,
            }
                => (
                    {
                        let base = format!("string_{op} {haystack}, {needle}");
                        from_index.map_or_else(|| base.clone(), |index| format!("{base}, {index}"))
                    }
                );
            /// Replace literal string matches with a literal replacement string.
            StringReplace {
                /// Replacement operation to apply.
                op: [$crate::StringReplaceOp],
                /// String value to transform.
                haystack: one,
                /// Literal pattern to replace.
                pattern: one,
                /// Literal replacement value.
                replacement: one,
            }
                => (format!("string_{op} {haystack}, {pattern}, {replacement}"));
            /// Remove a literal string prefix or suffix when it is present.
            StringRemoveAffix {
                /// Affix side to remove.
                op: [$crate::StringAffixOp],
                /// String value to transform.
                haystack: one,
                /// Affix to remove.
                affix: one,
            }
                => (format!("string_{} {haystack}, {affix}", op.remove_text()));
            /// Repeat a string a numeric number of times.
            StringRepeat {
                /// String value to repeat.
                operand: one,
                /// Number of repetitions.
                count: one,
            }
                => (format!("string_repeat {operand}, {count}"));
            /// Pad a string to a target length.
            StringPad {
                /// Padding side.
                op: [$crate::StringPadOp],
                /// String value to pad.
                operand: one,
                /// Target string length.
                target_len: one,
                /// Padding string.
                pad: one,
            }
                => (format!("string_{op} {operand}, {target_len}, {pad}"));
            /// Test whether every character in a string satisfies a predicate.
            StringPredicate {
                /// Predicate to apply.
                op: [$crate::StringPredicateOp],
                /// String value to test.
                operand: one,
            }
                => (format!("string_{op} {operand}"));
            /// Test whether a regex pattern matches a string.
            RegexIsMatch {
                /// Regex match shape to apply.
                op: [$crate::RegexMatchOp],
                /// Regex pattern text.
                pattern: one,
                /// String value being matched.
                haystack: one,
            }
                => (format!("regex_{op} {pattern}, {haystack}"));
            /// Replace regex matches with a replacement string.
            RegexReplace {
                /// Replacement operation to apply.
                op: [$crate::StringReplaceOp],
                /// Regex pattern text.
                pattern: one,
                /// String value to transform.
                haystack: one,
                /// Replacement text.
                replacement: one,
            }
                => (
                    hir: format!("regex_{op} {pattern}, {haystack}, {replacement}"),
                    mir: format!("regex_{} {pattern}, {haystack}, {replacement}", op.mir_regex_text()),
                );
            /// Replace regex matches with the result of a callback.
            RegexReplaceCallback {
                /// Replacement operation to apply.
                op: [$crate::StringReplaceOp],
                /// Regex pattern text.
                pattern: one,
                /// String value to transform.
                haystack: one,
                /// Callback receiving the matched text and returning replacement text.
                callback: one,
                /// The ECMA-262 replacer arguments the callback declared, in order —
                /// `(matched, p1, …, pN, position, string)` truncated to its arity.
                /// Resolved in the frontend, where the pattern's capture-group count is
                /// known, so the emitter only has to render each role.
                args: [Vec<$crate::RegexReplaceArg>],
            }
                => (
                    hir: format!("regex_{} {pattern}, {haystack}, {callback} [{}]", op.callback_text(), regex_replace_args_text(args)),
                    mir: format!("regex_{} {pattern}, {haystack}, {callback} [{}]", op.mir_callback_text(), regex_replace_args_text(args)),
                );
            /// Split a string with a regex pattern.
            RegexSplit {
                /// Regex pattern text.
                pattern: one,
                /// String value to split.
                haystack: one,
            }
                fresh(true)
                => (format!("regex_split {pattern}, {haystack}"));
            /// Return the first regex match as a JavaScript-like match array.
            RegexFind {
                /// Regex pattern text.
                pattern: one,
                /// String value to search.
                haystack: one,
            }
                updates_state(pattern)
                => (format!("regex_find {pattern}, {haystack}"));
            /// Execute a JavaScript-like `RegExp` value and return a match object.
            RegexExec {
                /// `RegExp` value.
                regex: one,
                /// String value to search.
                haystack: one,
            }
                updates_state(regex)
                => (format!("regex_exec {regex}, {haystack}"));
            /// `regex.test(haystack)` on a CONCRETE `RegExp` receiver.
            ///
            /// Answers a `bool` directly. Distinct from [`Self::RegexIsMatch`], which
            /// takes a pattern STRING and is a pure predicate: this one is the stateful
            /// spelling, so a `/g` or `/y` receiver reads and advances its `lastIndex`
            /// exactly as `exec` does. Distinct from [`Self::RegexExec`] because the
            /// answer is the boolean, not the match — building a match object and then
            /// erasing it to compare against `null` is what this replaced.
            RegexTest {
                /// `RegExp` value.
                regex: one,
                /// String value to search.
                haystack: one,
            }
                updates_state(regex)
                => (format!("regex_test {regex} {haystack}"));
            /// Return every match index from JavaScript `String.prototype.matchAll`.
            RegexMatchAll {
                /// `RegExp` value.
                regex: one,
                /// String value to search.
                haystack: one,
            }
                => (format!("regex_match_all {regex}, {haystack}"));
            /// Read a single character from a string as a string value.
            StringCharAt {
                /// String value to index.
                operand: one,
                /// Numeric character index.
                index: one,
            }
                => (format!("string_char_at {operand}, {index}"));
            /// Read a single character code from a string as a numeric value.
            StringCharCodeAt {
                /// String value to index.
                operand: one,
                /// Numeric character index.
                index: one,
            }
                => (format!("string_char_code_at {operand}, {index}"));
            /// Read the code point starting at a string index, or `undefined`
            /// past the end (`String.prototype.codePointAt`).
            StringCodePointAt {
                /// String value to index.
                operand: one,
                /// Numeric index.
                index: one,
            }
                => (format!("string_code_point_at {operand}, {index}"));
            /// Test whether one string contains another string.
            StringContains {
                /// String value to search in.
                haystack: one,
                /// Substring to search for.
                needle: one,
                /// Optional JavaScript `position` argument for `String.prototype.includes`.
                ///
                /// When present, the search starts at this position (truncated toward
                /// zero and clamped to the valid range). `None` means search from the start.
                from_index: opt,
            }
                => (
                    {
                        let base = format!("string_contains {haystack}, {needle}");
                        from_index.map_or_else(|| base.clone(), |index| format!("{base}, {index}"))
                    }
                );
            /// Take a substring slice from a string.
            StringSlice {
                /// String value to slice.
                operand: one,
                /// Inclusive start index, or omitted for zero.
                start: opt,
                /// Exclusive end index, or omitted for string length; a
                /// length for [`StringSliceKind::Substr`](crate::StringSliceKind).
                end: opt,
                /// The bound rule.
                kind: [$crate::StringSliceKind],
            }
                => (format!("{} {}, {}, {}", kind.text(), operand, opt(&start), opt(&end)));
            /// Test whether a list contains an item.
            ListContains {
                /// List value to search in.
                list: one @RECV,
                /// Item to search for.
                item: one @READ,
            }
                => (format!("list_contains {list}, {item}"));
            /// Test whether a set contains an item.
            SetContains {
                /// Set value to search in.
                set: one,
                /// Item to search for.
                item: one,
            }
                => (format!("set_contains {set}, {item}"));
            /// Test whether two sets have no shared items.
            SetDisjoint {
                /// Left set operand.
                left: one,
                /// Right set operand.
                right: one,
            }
                => (format!("set_isdisjoint {left}, {right}"));
            /// Test a relation between two sets.
            SetRelation {
                /// Set relation to evaluate.
                op: [$crate::SetRelationOp],
                /// Left set operand.
                left: one,
                /// Right set operand.
                right: one,
            }
                => (format!("set_{op} {left}, {right}"));
            /// Insert an item into a set.
            SetAdd {
                /// Set value to mutate.
                set: one @RECV,
                /// Item to insert.
                item: one @STORED,
            }
                mutates(Some(set))
                => (format!("set_add {set}, {item}"));
            /// Remove an item from a set.
            SetRemove {
                /// Missing-item behavior.
                op: [$crate::SetRemoveOp],
                /// Set value to mutate.
                set: one,
                /// Item to remove.
                item: one,
            }
                mutates(Some(set))
                => (format!("set_{op} {set}, {item}"));
            /// Clear all items from a set.
            SetClear {
                /// Set value to mutate.
                set: one,
            }
                mutates(Some(set))
                => (format!("set_clear {set}"));
            /// Return a shallow copy of a set.
            SetCopy {
                /// Set value to copy.
                set: one,
            }
                => (format!("set_copy {set}"));
            /// Combine two sets into a new set.
            SetBinary {
                /// Set operation to apply.
                op: [$crate::SetBinaryOp],
                /// Left set operand.
                left: one,
                /// Right set operand.
                right: one,
            }
                => (format!("set_{op} {left}, {right}"));
            /// Project values or entries from a set.
            SetProjection {
                /// Projection to apply.
                op: [$crate::SetProjectionOp],
                /// Set value to project.
                set: one,
            }
                fresh(true)
                => (format!("set_{op} {set}"));
            /// `new Headers(init?)`.
            ///
            /// `init` is a record, a list of name/value pairs, or another `Headers`
            /// value; which one is decided from the initializer's static type, so the
            /// construction stays a concrete typed value with no runtime tag test.
            HeadersNew {
                /// Optional initializer expression.
                init: opt,
            }
                => (
                    init.map_or_else(
                        || "headers_new".to_owned(),
                        |init| format!("headers_new {init}"),
                    )
                );
            /// `new URLSearchParams(init?)`.
            ///
            /// `init` is a query string, a record, a list of name/value pairs, or
            /// another `URLSearchParams`; the initializer's static type selects the
            /// conversion.
            UrlSearchParamsNew {
                /// Optional initializer expression.
                init: opt,
            }
                => (
                    init.map_or_else(
                        || "url_search_params_new".to_owned(),
                        |init| format!("url_search_params_new {init}"),
                    )
                );
            /// `new TextEncoder()`.
            ///
            /// No initializer: the spec gives the constructor no arguments and fixes
            /// the encoding at UTF-8.
            TextEncoderNew
                => ("text_encoder_new".to_owned());
            /// `new TextDecoder(label?)`.
            ///
            /// `label` is the encoding label. Only the UTF-8 labels are modeled; any
            /// other label is a named blocker at lowering rather than a decoder that
            /// silently decodes as UTF-8.
            TextDecoderNew {
                /// The optional encoding-label expression.
                label: opt,
            }
                => (
                    label.map_or_else(
                        || "text_decoder_new".to_owned(),
                        |label| format!("text_decoder_new {label}"),
                    )
                );
            /// Apply a `TextEncoder` member to a concrete receiver.
            TextEncoderOp {
                /// Which member this reads or calls.
                op: [$crate::TextEncoderOp],
                /// The `TextEncoder` receiver.
                encoder: one,
                /// Operation arguments (the string to encode, or none).
                args: many,
            }
                => (format!("text_encoder_{op} {}{}", encoder, tail(" ", ", ", &args)));
            /// Apply a `TextDecoder` member to a concrete receiver.
            TextDecoderOp {
                /// Which member this reads or calls.
                op: [$crate::TextDecoderOp],
                /// The `TextDecoder` receiver.
                decoder: one,
                /// Operation arguments (the byte view to decode, or none).
                args: many,
            }
                => (format!("text_decoder_{op} {}{}", decoder, tail(" ", ", ", &args)));
            /// The ambient global object (`globalThis`, `global`), as ONE shared value.
            ///
            /// **Dynamic boundary.** JavaScript's global object is an open property bag
            /// the program and its host both write to (`Object.assign(globalThis, {..})`,
            /// `globalThis.x = v`, a host-injected `declare const X`), so its members
            /// have no static shape a struct, union or generic could describe — which
            /// names exist is decided by whatever ran before the read. Every reference
            /// therefore evaluates to the same erased object (a thread-local
            /// `SmeltObject` carrying the `__smelt_global_object` marker), and a typed
            /// read of a member goes through a checked cast at the read site.
            GlobalObject
                => ("global_object".to_owned());
            /// Construct a value of the concrete typed-array family.
            ///
            /// One node for all twelve source constructors — the eleven views and
            /// `ArrayBuffer` — because the element kind is a runtime property selected
            /// by the name, not a different operation. Codegen reads the name to pick
            /// the kind (or the storage half) and the ARGUMENT TYPES to pick the
            /// spelling: a number is a length, a list is elements, a buffer is a
            /// re-view over shared storage, another view is an element-by-element
            /// conversion, and an erased value is the one honest dynamic boundary.
            TypedArrayNew {
                /// The source constructor name — one of the eleven views, or
                /// `ArrayBuffer`.
                class_name: [String],
                /// The constructor arguments, as written.
                args: many,
            }
                => (format!("typed_array_new {class_name}{}", tail(" ", " ", &args)));
            /// Read a member of, or call a method on, the concrete typed-array family.
            ByteArrayOp {
                /// Which member this reads or calls.
                op: [$crate::ByteArrayOp],
                /// The view (or byte-storage) receiver.
                bytes: one,
                /// Operation arguments (a range, a value, a source view, or none).
                args: many,
            }
                => (format!("byte_array_{} {bytes}{}", op.name(), tail(" ", " ", &args)));
            /// Apply a `URLSearchParams` operation to a concrete receiver.
            UrlSearchParamsOp {
                /// Which operation this call performs.
                op: [$crate::UrlSearchParamsOp],
                /// The `URLSearchParams` receiver.
                params: one,
                /// Operation arguments (name, or name and value).
                args: many,
            }
                => (format!("url_search_params_{op} {}{}", params, tail(", ", ", ", &args)));
            /// An `AbortSignal` static call (`AbortSignal.abort(reason?)`,
            /// `AbortSignal.timeout(ms)`).
            ///
            /// No receiver operand: see [`AbortSignalOpKind`].
            AbortSignalOp {
                /// Which static this call performs.
                op: [$crate::AbortSignalOp],
                /// Operation arguments, in source order.
                args: many,
            }
                => (format!("abort_signal_{op}{}", tail(" ", ", ", &args)));
            /// Apply a `WebCrypto` operation.
            ///
            /// No receiver operand: the `crypto` namespace holds no state a program can
            /// observe, so the call is its arguments (see `smelt_hir::CryptoOp`).
            CryptoOp {
                /// Which operation this call performs.
                op: [$crate::CryptoOp],
                /// Operation arguments, in source order.
                args: many,
            }
                => (format!("crypto_{op}{}", tail(" ", ", ", &args)));
            /// `new FormData()`.
            ///
            /// No initializer: the spec's constructor takes an optional `HTMLFormElement`
            /// only, which is DOM and outside the profile, so a form is always built
            /// empty and filled with `append`.
            FormDataNew
                => ("form_data_new".to_owned());
            /// Apply a `FormData` operation to a concrete receiver.
            FormDataOp {
                /// Which operation this call performs.
                op: [$crate::FormDataOp],
                /// The `FormData` receiver.
                form: one,
                /// Operation arguments (name; name and value; or a callback).
                args: many,
            }
                => (format!("form_data_{op} {}{}", form, tail(", ", ", ", &args)));
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
                body: opt,
                /// `init.status`, when the init literal set it.
                status: opt,
                /// `init.statusText`, when the init literal set it.
                status_text: opt,
                /// `init.headers`, when the init literal set it.
                headers: opt,
            }
                => (
                    {
                        let mut parts = Vec::new();
                        if let Some(body) = body {
                            parts.push(format!("body={body}"));
                        }
                        if let Some(status) = status {
                            parts.push(format!("status={status}"));
                        }
                        if let Some(status_text) = status_text {
                            parts.push(format!("status_text={status_text}"));
                        }
                        if let Some(headers) = headers {
                            parts.push(format!("headers={headers}"));
                        }
                        if parts.is_empty() {
                            "response_new".to_owned()
                        } else {
                            format!("response_new {}", parts.join(" "))
                        }
                    }
                );
            /// `new EventEmitter()`.
            ///
            /// No options are modeled: the constructor's only argument is
            /// `{ captureRejections }`, which changes how a rejected promise returned
            /// by a listener is reported and has no meaning until listeners can be
            /// async here.
            EventEmitterNew
                => ("event_emitter_new".to_owned());
            /// `process.env.NAME`: the variable from the process environment, or
            /// `undefined` when it is not set. Typed `Optional<String>`.
            ProcessEnvRead {
                /// The variable's name.
                name: [String],
            }
                => (format!("process_env_read {name:?}"));
            /// `process.argv`: the executable path twice (it is both the
            /// runtime Node would name first and the script it would name
            /// second), then the arguments the program was started with, so
            /// `process.argv.slice(2)` is the user's arguments as under Node.
            /// Typed `List<String>`.
            ProcessArgv
                fresh(true)
                => ("process_argv".to_owned());
            /// An `EventEmitter` member operation on a concrete emitter receiver.
            ///
            /// The receiver is any value whose class
            /// [`has_event_emitter`](smelt_stdlib::StdlibClass::has_event_emitter) — an
            /// `EventEmitter` itself, or a `node:http` `IncomingMessage`, which holds
            /// one by composition.
            EventEmitterOp {
                /// Which operation this member performs.
                op: [$crate::EventEmitterOp],
                /// The emitter receiver.
                emitter: one,
                /// The event name, followed by the operation's own arguments.
                args: many,
            }
                => (format!("emitter_{op} {}{}", emitter, tail(" ", " ", &args)));
            /// `createServer(handler)` from `node:http`.
            ///
            /// The handler is stored CONCRETELY: unlike an event listener, its
            /// signature is fixed by the module — `(IncomingMessage, ServerResponse)` —
            /// so it needs none of the erasure the emitter's listener list needs.
            HttpCreateServer {
                /// The request handler, called once per accepted request.
                handler: one,
            }
                => (format!("http_create_server {handler}"));
            /// `new DatabaseSync(path)` (`smelt_hir::ExprKind::SqliteOpen`).
            SqliteOpen {
                /// The database file, or `":memory:"`.
                path: one,
            }
                => (format!("sqlite_open {path}"));
            /// A `node:sqlite` operation (`smelt_hir::ExprKind::SqliteOp`).
            SqliteOp {
                /// Which operation.
                op: [$crate::SqliteOp],
                /// The `DatabaseSync` or `StatementSync` receiver.
                receiver: one,
                /// The SQL text for `exec`/`prepare`; the bound parameters, erased,
                /// for `all`/`get`/`run`.
                args: many,
            }
                => (
                    {
                        let mut text = format!("sqlite_{op:?} {receiver}").to_lowercase();
                        for arg in args {
                            text.push(' ');
                            text.push_str(&arg);
                        }
                        text
                    }
                );
            /// A `node:http` server answering requests with a fetch handler.
            ///
            /// What `@hono/node-server`'s `serve` builds: every request is converted to
            /// a fetch `Request`, passed to `fetch`, and the `Response` it answers
            /// (directly or through a promise) is written back. `fetch` keeps the
            /// program's own function type; it is called with the request alone, so
            /// its further parameters are optional.
            HttpCreateFetchServer {
                /// The fetch handler, called once per accepted request.
                fetch: one,
            }
                => (format!("http_create_fetch_server {fetch}"));
            /// A `node:http` `Server` member operation.
            HttpServerOp {
                /// Which operation this member performs.
                op: [$crate::HttpServerOp],
                /// The server receiver.
                server: one,
                /// The operation's arguments: `listen` takes a port, an optional host,
                /// and an optional listening callback; the others take none.
                args: many,
            }
                => (format!("http_server_{op} {}{}", server, tail(" ", " ", &args)));
            /// A `node:http` `IncomingMessage` property read.
            IncomingMessageOp {
                /// Which property this reads.
                op: [$crate::IncomingMessageOp],
                /// The request receiver.
                message: one,
            }
                => (format!("incoming_message_{op} {message}"));
            /// A `node:http` `ServerResponse` member operation.
            ServerResponseOp {
                /// Which operation this member performs.
                op: [$crate::ServerResponseOp],
                /// The response receiver.
                response: one,
                /// The operation's arguments, in source order.
                args: many,
            }
                => (format!("server_response_{op} {}{}", response, tail(" ", " ", &args)));
            /// `new Request(input, init?)`.
            ///
            /// `input` is the request URL; `method`, `headers` and `body` come from the
            /// init literal's keys, split into typed fields for the same reason
            /// [`Self::ResponseNew`] splits its own.
            RequestNew {
                /// The URL argument.
                input: one,
                /// `init.method`, when the init literal set it.
                method: opt,
                /// `init.headers`, when the init literal set it.
                headers: opt,
                /// `init.body`, when the init literal set it.
                body: opt,
                /// `init.signal`, when the init literal set it.
                ///
                /// The request's own signal is not this value: the spec makes it a
                /// DEPENDENT signal that follows this one, so `request.signal` is a
                /// distinct object that aborts when this one does. See
                /// `blocker-logs/abort-signal-reason-and-statics.md`.
                signal: opt,
                /// The STORED `RequestInit` members the init supplied, in source order.
                ///
                /// A list rather than eight more `Option` fields because these keys
                /// differ in nothing the constructor does with them: each is a scalar
                /// that overrides one stored slot. `method`/`headers`/`body` stay their
                /// own fields because they feed the transport identity instead, and
                /// `signal` because it registers a follow rather than storing a value.
                init_members: {$crate::RequestInitMember},
            }
                => (
                    {
                        let mut parts = vec![format!("input={input}")];
                        if let Some(method) = method {
                            parts.push(format!("method={method}"));
                        }
                        if let Some(headers) = headers {
                            parts.push(format!("headers={headers}"));
                        }
                        if let Some(body) = body {
                            parts.push(format!("body={body}"));
                        }
                        if let Some(signal) = signal {
                            parts.push(format!("signal={signal}"));
                        }
                        for (member, value) in init_members {
                            parts.push(format!("{}={}", member.field_name(), value));
                        }
                        format!("request_new {}", parts.join(" "))
                    }
                );
            /// A `Request` member operation on a concrete `Request` receiver.
            RequestOp {
                /// Which operation this member performs.
                op: [$crate::RequestOp],
                /// The `Request` receiver.
                request: one,
                /// Operation arguments; every modeled member is nullary today.
                args: many,
            }
                => (format!("request_{op} {}{}", request, tail(" ", " ", &args)));
            /// A `Response` member operation on a concrete `Response` receiver.
            ResponseOp {
                /// Which operation this member performs.
                op: [$crate::ResponseOp],
                /// The `Response` receiver.
                response: one,
                /// Operation arguments; every modeled member is nullary today.
                args: many,
            }
                => (format!("response_{op} {}{}", response, tail(" ", " ", &args)));
            /// Apply a WHATWG `Headers` operation to a concrete `Headers` receiver.
            HeadersOp {
                /// Which header operation this call performs.
                op: [$crate::HeadersOp],
                /// The `Headers` receiver.
                headers: one,
                /// Operation arguments (name, or name and value).
                args: many,
            }
                => (format!("headers_{op} {}{}", headers, tail(", ", ", ", &args)));
            /// Concatenate two lists into a new list.
            ListConcat {
                /// Left list value.
                left: one @RECV,
                /// Right list value.
                right: one @RECV,
            }
                fresh(true)
                => (format!("list_concat {left}, {right}"));
            /// Normalize one `Array.prototype.concat` argument whose static type is
            /// erased into the list of items it contributes.
            ///
            /// JavaScript splices an array argument's elements into the result and
            /// appends any other value as a single element. A concretely-typed argument
            /// settles that at lowering time; an `unknown`, type-parameter, or mixed
            /// union argument can be either shape at runtime, so the choice is deferred.
            /// The node's type is the receiver's list type.
            ConcatSpread {
                /// The erased argument value.
                value: one,
            }
                => (format!("concat_spread {value}"));
            /// Find an item index in a list, returning -1 when absent.
            ListSearch {
                /// Search direction.
                op: [$crate::ListSearchOp],
                /// List value to search.
                list: one @RECV,
                /// Item to search for.
                item: one @READ,
                /// Optional JavaScript `fromIndex` argument for `Array.prototype.indexOf`
                /// and `lastIndexOf`.
                ///
                /// When present, the search starts at this index (truncated toward zero;
                /// negative values count back from the end). `None` searches the whole array.
                from_index: opt @READ,
            }
                => (
                    {
                        let base = format!("list_{op} {list}, {item}");
                        from_index.map_or_else(|| base.clone(), |index| format!("{base}, {index}"))
                    }
                );
            /// Apply a capture-free callback to a list operation.
            ListCallback {
                /// List callback operation to apply.
                op: [$crate::ListCallbackOp],
                /// List value to process.
                list: one @CALLED,
                /// Closure or callable value.
                callback: one @CALLED,
            }
                // JavaScript hands the receiver to the callback as its third argument, so
                // the callback can retain a handle on the array: the receiver escapes.
                fresh(matches!(
                    op,
                    $crate::ListCallbackOp::Map
                        | $crate::ListCallbackOp::Filter
                        | $crate::ListCallbackOp::FlatMap
                ))
                => (format!("list_{op} {list}, {callback}"));
            /// Build a sparse-like unknown list from a numeric length.
            ListFromLength {
                /// Numeric length source.
                length: one @READ,
            }
                fresh(true)
                => (format!("list_from_length {length}"));
            /// Build a list by invoking a mapper for indexes from zero to length.
            ListFromLengthMap {
                /// Numeric length source.
                length: one @READ,
                /// Closure or callable value.
                callback: one @CALLED,
            }
                fresh(true)
                => (format!("list_from_length_map {length}, {callback}"));
            /// `Array.prototype.reduce` / `reduceRight`: fold `list` through `callback`.
            ListReduce {
                /// List value to reduce.
                list: one @CALLED,
                /// Initial accumulator value, or omitted for JavaScript-style first-item seeding.
                initial: opt @CALLED,
                /// Closure or callable value.
                callback: one @CALLED,
                /// `true` for `reduceRight`: visit elements from the last index down to
                /// zero (the callback still receives each element's original index).
                from_right: [bool],
            }
                // The seed flows into the result and through the callback, and `reduce`
                // passes the array as the reducer's fourth argument.
                => (
                    format!(
                        "{} {}, {}, {}",
                        if *from_right {
                            "list_reduce_right"
                        } else {
                            "list_reduce"
                        },
                        list,
                        opt(&initial),
                        callback
                    )
                );
            /// Take a shallow slice from a list.
            ListSlice {
                /// List value to slice.
                list: one @RECV,
                /// Inclusive start index, or omitted for zero.
                start: opt @READ,
                /// Exclusive end index, or omitted for collection length.
                end: opt @READ,
            }
                fresh(true)
                => (format!("list_slice {}, {}, {}", list, opt(&start), opt(&end)));
            /// Remove and optionally insert list items.
            ListSplice {
                /// List value to mutate or copy.
                list: one @(if *mutate { RECV_MUT } else { RECV }),
                /// Start index.
                start: one @READ,
                /// Number of items to delete, or omitted to delete through the end.
                delete_count: opt @READ,
                /// Replacement items.
                items: splice @STORED,
                /// Whether to mutate the source list and return removed items.
                mutate: [bool],
            }
                mutates(if *mutate { Some(list) } else { None })
                fresh(true)
                => (
                    format!(
                        "list_splice {list}, {start}, {}, [{}], {mutate}",
                        opt(&delete_count),
                        items.join(", ")
                    )
                );
            /// Fill a list range with one value.
            ListFill {
                /// List value to mutate.
                list: one @ALIAS_MUT,
                /// Value to assign.
                value: one @STORED,
                /// Inclusive start index, or omitted for zero.
                start: opt,
                /// Exclusive end index, or omitted for collection length.
                end: opt,
            }
                mutates(Some(list))
                => (format!("list_fill {}, {}, {}, {}", list, value, opt(&start), opt(&end)));
            /// Copy a range inside a list.
            ListCopyWithin {
                /// List value to mutate.
                list: one @ALIAS_MUT,
                /// Target index.
                target: one,
                /// Source start index.
                start: one,
                /// Source end index, or omitted for collection length.
                end: opt,
            }
                mutates(Some(list))
                => (format!("list_copy_within {}, {}, {}, {}", list, target, start, opt(&end)));
            /// Return a copied list with one replaced item.
            ListWith {
                /// List value to copy.
                list: one @RECV,
                /// Index to replace.
                index: one @READ,
                /// Replacement value.
                value: one @STORED,
            }
                // `with` copies, but the replacement lands inside the copy.
                fresh(true)
                => (format!("list_with {list}, {index}, {value}"));
            /// Flatten a list to an optional JavaScript depth.
            ListFlat {
                /// List value to flatten.
                list: one @RECV,
                /// Explicit flatten depth, or omitted for JavaScript's depth of one.
                depth: opt @READ,
            }
                fresh(true)
                => (format!("list_flat {}, {}", list, opt(&depth)));
            /// Project array keys, values, or entries.
            ListProjection {
                /// Projection operation.
                op: [$crate::ListProjectionOp],
                /// List value to project.
                list: one @RECV,
            }
                fresh(true)
                => (format!("list_{op} {list}"));
            /// Return a shallow copy of a list.
            ListCopy {
                /// List value to copy.
                list: one @RECV,
            }
                fresh(true)
                => (format!("list_copy {list}"));
            /// Count list items equal to a target item.
            ListCount {
                /// List value to scan.
                list: one @RECV,
                /// Item to count.
                item: one @READ,
            }
                => (format!("list_count {list}, {item}"));
            /// Sum numeric items in a list.
            ListSum {
                /// List value to reduce.
                list: one @RECV,
            }
                => (format!("list_sum {list}"));
            /// Fold boolean items in a list with `all` or `any` semantics.
            ListBoolFold {
                /// Fold operation to apply.
                op: [$crate::BoolFoldOp],
                /// Boolean list value to reduce.
                list: one @RECV,
            }
                => (format!("list_{op} {list}"));
            /// Return a sorted copy of a list.
            ListSorted {
                /// List value to copy and sort.
                list: one @RECV,
                /// Optional key closure for Python `sorted(values, key=...)`.
                ///
                /// Like other callbacks, the key is a normal closure body referenced by
                /// `ExprId`; it maps one list item to a sortable value.
                key: opt @CALLED,
                /// Whether to sort in descending order, as in Python `reverse=True`.
                reverse: [bool],
            }
                fresh(true)
                => (
                    {
                        let key_text = if key.is_some() { " <key>" } else { "" };
                        let reverse_text = if *reverse { " reverse" } else { "" };
                        format!("list_sorted {list}{key_text}{reverse_text}")
                    }
                );
            /// Return a reversed copy of a list.
            ListReversed {
                /// List value to copy in reverse order.
                list: one @RECV,
            }
                fresh(true)
                => (format!("list_reversed {list}"));
            /// Pair list items with zero-based integer indexes.
            ListEnumerate {
                /// List value to enumerate.
                list: one @RECV,
            }
                fresh(true)
                => (format!("list_enumerate {list}"));
            /// Pair two lists elementwise, truncating to the shorter list.
            ListZip {
                /// Left list value to zip.
                left: one @RECV,
                /// Right list value to zip.
                right: one @RECV,
            }
                fresh(true)
                => (format!("list_zip {left}, {right}"));
            /// Materialize an integer range as a list.
            ListRange {
                /// Inclusive start value.
                start: one @READ,
                /// Exclusive end value.
                end: one @READ,
                /// Step value.
                step: one @READ,
            }
                fresh(true)
                => (format!("list_range {start}, {end}, {step}"));
            /// Pick one item from a list with a pseudo-random index.
            ListRandomChoice {
                /// List value to choose from.
                list: one @RECV,
            }
                => (format!("list_random_choice {list}"));
            /// Return the first index of an equal list item.
            ListIndex {
                /// List value to scan.
                list: one @RECV,
                /// Item to locate.
                item: one @READ,
            }
                => (format!("list_index {list}, {item}"));
            /// Consume the first list item and return a JavaScript iterator-result object.
            ListNext {
                /// List value to mutate.
                list: one @RECV_MUT,
            }
                mutates(Some(list))
                => (format!("list_next {list}"));
            /// Test whether a tuple contains an item.
            TupleContains {
                /// Tuple value to search in.
                tuple: one,
                /// Item to search for.
                item: one,
            }
                => (format!("tuple_contains {tuple}, {item}"));
            /// Test whether a dictionary contains a key.
            DictContainsKey {
                /// Dictionary value to search in.
                dict: one,
                /// Key to search for.
                key: one,
                /// How far the presence test may look: own properties only
                /// (`Object.hasOwn`, a typed collection probe) or the whole prototype
                /// chain (the `in` operator).
                lookup: [$crate::PropertyLookup],
            }
                => (format!("dict_contains_key {dict}, {key}, {lookup}"));
            /// Insert or replace a dictionary key-value pair.
            DictSet {
                /// Dictionary value to mutate.
                dict: one @RECV,
                /// Key to write.
                key: one @STORED,
                /// Value to write.
                value: one @STORED,
            }
                mutates(Some(dict))
                => (format!("dict_set {dict}, {key}, {value}"));
            /// Remove a dictionary key and return whether it existed.
            DictRemoveKey {
                /// Dictionary value to mutate.
                dict: one,
                /// Key to remove.
                key: one,
            }
                mutates(Some(dict))
                => (format!("dict_remove_key {dict}, {key}"));
            /// Look up a dictionary key and return an optional value or default.
            DictGet {
                /// Dictionary value to read.
                dict: one,
                /// Key to look up.
                key: one,
                /// Optional default value for missing keys.
                default: opt,
            }
                => (format!("dict_get {}, {}, {}", dict, key, opt(&default)));
            /// Return an existing dictionary value or insert and return a default.
            DictSetDefault {
                /// Dictionary value to mutate.
                dict: one,
                /// Key to look up or insert.
                key: one,
                /// Default value to insert for a missing key.
                default: one,
            }
                mutates(Some(dict))
                => (format!("dict_setdefault {dict}, {key}, {default}"));
            /// Clear all entries from a dictionary and return `None`.
            DictClear {
                /// Dictionary value to mutate.
                dict: one,
            }
                mutates(Some(dict))
                => (format!("dict_clear {dict}"));
            /// Remove a dictionary key and return its value.
            DictPop {
                /// Dictionary value to mutate.
                dict: one,
                /// Key to remove.
                key: one,
                /// Optional default value for missing keys.
                default: opt,
            }
                mutates(Some(dict))
                fallible(default.is_none())
                call("@dict_pop".to_owned())
                => (format!("dict_pop {}, {}, {}", dict, key, opt(&default)));
            /// Python `dict.popitem()`: remove and return the last-inserted
            /// `(key, value)` pair, raising `KeyError` on an empty dict.
            DictPopItem {
                /// Dictionary value to mutate.
                dict: one,
            }
                mutates(Some(dict))
                fallible(true)
                call("@dict_popitem".to_owned())
                => (format!("dict_popitem {dict}"));
            /// Extend a dictionary with entries from another dictionary and return `None`.
            DictUpdate {
                /// Dictionary value to mutate.
                dict: one,
                /// Dictionary supplying entries to copy.
                other: one,
            }
                mutates(Some(dict))
                => (format!("dict_update {dict}, {other}"));
            /// Merge source dictionaries into a target dictionary and return the target.
            DictAssign {
                /// Dictionary receiving copied entries.
                target: one,
                /// Dictionaries supplying entries to copy.
                sources: many,
            }
                mutates(Some(target))
                => (format!("dict_assign {target}, [{}]", sources.join(", ")));
            /// Attach object-literal properties to a callable JavaScript value.
            CallableObjectAssign {
                /// Callable value receiving the properties.
                callable: one,
                /// Static property values to attach.
                props: symbol_pairs,
                /// Record-typed source values whose own enumerable entries are copied
                /// onto the callable object at runtime (e.g. `Object.assign(fn, def)`
                /// where `def` is a record variable rather than an object literal).
                spreads: many,
            }
                => (
                    format!(
                        "callable_object_assign {callable}, {{{}}}{}",
                        props.join(", "),
                        tail(", ...", ", ...", &spreads)
                    )
                );
            /// Return a shallow copy of a dictionary.
            DictCopy {
                /// Dictionary value to copy.
                dict: one,
            }
                => (format!("dict_copy {dict}"));
            /// Project keys, values, or entries from a dictionary.
            DictProjection {
                /// Projection to apply.
                op: [$crate::DictProjectionOp],
                /// Dictionary value to project.
                dict: one,
            }
                fresh(true)
                => (format!("dict_{op} {dict}"));
            /// Split a string into a list of strings.
            StringSplit {
                /// String value to split.
                haystack: one,
                /// Separator string.
                separator: one,
                /// Optional maximum number of pieces.
                limit: opt,
            }
                fresh(true)
                => (format!("string_split {}, {}, {}", haystack, separator, opt(&limit)));
            /// Convert a string into a list of one-character strings.
            StringChars {
                /// String value to expand.
                haystack: one,
            }
                fresh(true)
                => (format!("string_chars {haystack}"));
            /// Join a list of strings with a separator string.
            StringJoin {
                /// String items to join.
                items: one @RECV,
                /// Separator string.
                separator: one @READ,
            }
                => (format!("string_join {items}, {separator}"));
            /// Serialize a JSON-compatible value to a JSON string.
            JsonStringify {
                /// Value to serialize.
                value: one @READ,
            }
                => (format!("json_stringify {value}"));
            /// The `space` argument of `JSON.stringify(value, replacer, space)`:
            /// re-indent compact JSON `text` the way JavaScript lays out an indented
            /// result. `space` is the source argument (a number clamped to 10, a
            /// string cut to 10 characters; anything else means no indentation).
            JsonIndent {
                /// The compact JSON text.
                text: one @READ,
                /// The source `space` argument.
                space: one @READ,
            }
                => (format!("json_indent {text}, {space}"));
            /// Perform a blocking HTTP GET request and return response text.
            HttpGetText {
                /// URL to request.
                url: one,
            }
                => (format!("http_get_text {url}"));
            /// Read the current timestamp in milliseconds.
            DateNow
                => ("date_now".to_owned());
            /// Configure the timestamp returned by JavaScript `Date.now()`.
            DateSetNow {
                /// Timestamp or Date-compatible value to use as the mocked clock.
                timestamp: one,
            }
                => (format!("date_set_now {timestamp}"));
            /// Restore the real JavaScript `Date.now()` clock.
            DateResetNow
                => ("date_reset_now".to_owned());
            /// Read the configured JavaScript `Date.prototype.getTimezoneOffset` value.
            DateTimezoneOffset
                => ("date_timezone_offset".to_owned());
            /// Configure the return value observed by `Date.prototype.getTimezoneOffset`.
            DateSetTimezoneOffset {
                /// Offset from UTC in minutes.
                offset: one,
            }
                => (format!("date_set_timezone_offset {offset}"));
            /// Restore the default `Date.prototype.getTimezoneOffset` implementation.
            DateResetTimezoneOffset
                => ("date_reset_timezone_offset".to_owned());
            /// Construct a stateful Vitest `vi.fn([impl])` mock: a callable erased
            /// object that records calls and serves configured one-shot/default
            /// outcomes (`mockReturnValue`, `mockResolvedValue`, `mockRejectedValueOnce`, ...).
            VitestMockFn {
                /// Optional wrapped implementation used as the default outcome.
                implementation: opt,
            }
                => (
                    implementation.map_or_else(
                        || "vitest_mock_fn".to_owned(),
                        |implementation| format!("vitest_mock_fn {implementation}"),
                    )
                );
            /// Whether a Vitest mock's recorded call count equals `count`
            /// (`expect(mock).toHaveBeenCalledTimes(count)`); true means the
            /// assertion holds. Non-mock values pass vacuously (documented compat).
            VitestMockCalledTimes {
                /// The (possibly non-mock) actual value under assertion.
                mock: one,
                /// Expected call count.
                count: one,
            }
                => (format!("vitest_mock_called_times {mock} {count}"));
            /// Whether a Vitest mock recorded a call whose arguments deep-equal
            /// `args` (`expect(mock).toHaveBeenCalledWith(...)`); true means the
            /// assertion holds. When `last` is set, only the most recent recorded
            /// call is compared (`toHaveBeenLastCalledWith(...)`). Non-mock values
            /// pass vacuously (documented compat).
            VitestMockCalledWith {
                /// The (possibly non-mock) actual value under assertion.
                mock: one,
                /// Expected call arguments.
                args: many,
                /// Compare only the most recent recorded call (`toHaveBeenLastCalledWith`).
                last: [bool],
            }
                => (
                    format!(
                        "vitest_mock_called_with{} {} [{}]",
                        if *last { "_last" } else { "" },
                        mock,
                        args.join(", ")
                    )
                );
            /// `vi.restoreAllMocks()`: undo every installed spy, newest first.
            VitestRestoreAllMocks
                => ("vitest_restore_all_mocks".to_owned());
            /// `vi.spyOn(target, name)`: replace `target[name]` with a recording mock
            /// that forwards to the member's current value, and evaluate to that mock.
            ///
            /// The target is a real runtime object and the replacement is a real
            /// insertion, so library code that later reads `target[name]` calls the
            /// mock (and the mock calls the original). That is what makes the recorded
            /// calls the ones the program actually made.
            VitestSpyOn {
                /// The object whose member is replaced.
                target: one,
                /// The member name.
                name: one,
            }
                => (format!("vitest_spy_on {target} {name}"));
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
                /// The actual value under assertion.
                actual: one,
                /// The expected value, possibly an asymmetric matcher or containing one.
                expected: one,
            }
                => (format!("vitest_asymmetric_equal {actual} {expected}"));
            /// Whether a Vitest mock's most recent recorded result deep-equals
            /// `expected` after flattening a resolved promise
            /// (`expect(mock).toHaveLastResolvedWith(...)`); true means the assertion
            /// holds. Non-mock values pass vacuously (documented compat).
            VitestMockLastResolvedWith {
                /// The (possibly non-mock) actual value under assertion.
                mock: one,
                /// Expected resolved value.
                expected: one,
            }
                => (format!("vitest_mock_last_resolved_with {mock} {expected}"));
            /// Create a date-fns-compatible date context function for an IANA time zone.
            DateTimezoneContext {
                /// IANA time zone name.
                timezone: one,
            }
                => (format!("date_timezone_context {timezone}"));
            /// Convert a timestamp in milliseconds to an ISO-8601 string.
            DateToIsoString {
                /// Timestamp in milliseconds.
                timestamp_ms: one,
            }
                => (
                    hir: format!("date_to_iso {timestamp_ms}"),
                    mir: format!("date_to_iso_string {timestamp_ms}"),
                );
            /// Convert a Date timestamp to JavaScript `Date.prototype.toString()` output.
            DateToString {
                /// Timestamp in milliseconds.
                timestamp_ms: one,
            }
                => (format!("date_to_string {timestamp_ms}"));
            /// Convert a Date timestamp to `Date.prototype.toUTCString()` output, the
            /// HTTP-date form (`Sun, 24 Dec 2000 10:30:59 GMT`).
            DateToUtcString {
                /// Timestamp in milliseconds.
                timestamp_ms: one,
            }
                => (format!("date_to_utc_string {timestamp_ms}"));
            /// Construct a timestamp in milliseconds from local Date constructor parts.
            DateFromParts {
                /// Date constructor parts: year, month, date, hours, minutes, seconds, milliseconds.
                parts: many,
            }
                => (format!("date_from_parts [{}]", parts.join(", ")));
            /// Construct a timestamp in milliseconds from one JavaScript Date constructor value.
            DateFromValue {
                /// Date constructor argument.
                value: one,
            }
                => (format!("date_from_value {value}"));
            /// Read a local-time date component from a timestamp.
            DateGetPart {
                /// Component to read.
                part: [$crate::DatePart],
                /// Timestamp in milliseconds.
                timestamp_ms: one,
            }
                => (format!("date_get_{part:?} {timestamp_ms}"));
            /// Return a timestamp with a local-time date component replaced.
            DateSetPart {
                /// Component to replace.
                part: [$crate::DatePart],
                /// Timestamp in milliseconds.
                timestamp_ms: one,
                /// Replacement values accepted by the corresponding JS setter.
                values: many,
            }
                => (format!("date_set_{part:?} {timestamp_ms}, [{}]", values.join(", ")));
            /// Extract a parsed URL field.
            UrlField {
                /// URL field to extract.
                field: [$crate::UrlField],
                /// URL string to parse.
                url: one,
            }
                => (
                    hir: format!("url_{field:?} {url}"),
                    mir: format!("url_{} {url}", field.text()),
                );
            /// Read a UTF-8 text file.
            FileReadText {
                /// File path to read.
                path: one,
            }
                => (format!("file_read_text {path}"));
            /// Write a UTF-8 text file.
            FileWriteText {
                /// File path to write.
                path: one,
                /// Text content to write.
                text: one,
            }
                => (format!("file_write_text {path}, {text}"));
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
                /// `BlobPart` array: string parts, blob parts, or an erased mixed array.
                parts: one,
                /// Resolved MIME `type` string.
                blob_type: one,
                /// `File` name; present only for `new File(...)`, which also stamps
                /// the `__smelt_file` marker on top of `__smelt_blob`.
                name: opt,
                /// `File` options `lastModified` milliseconds, when spelled.
                last_modified: opt,
            }
                => (
                    format!(
                        "blob_from_parts {parts}, {blob_type}, {}, {}",
                        name.unwrap_or_else(|| "none".to_owned()),
                        last_modified.unwrap_or_else(|| "none".to_owned())
                    )
                );
            /// Apply a `Blob`/`File` member to a concrete receiver.
            BlobOp {
                /// Which member this reads or calls.
                op: [$crate::BlobOp],
                /// The `Blob` receiver.
                blob: one,
                /// Operation arguments (`slice`'s range and content type, or none).
                args: many,
            }
                => (format!("blob_{op} {}{}", blob, tail(" ", ", ", &args)));
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
                /// Host constructor's registry class name (`"ArrayBuffer"`, `"DataView"`).
                class_name: [String],
                /// Spelled constructor arguments, erased.
                args: many @CALLED,
            }
                => (
                    {
                        let args_text = args;
                        format!("host_construct {class_name}({})", args_text.join(", "))
                    }
                );
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
                /// The global builtin's source name (`"Blob"`, `"Math"`).
                name: [String],
            }
                => (format!("builtin_namespace {name}"));
            /// The value of an import or global Smelt has no model for.
            ///
            /// Nothing is known of it, so it stands in for itself: every member
            /// read and every call answers it. Any other value that holds no
            /// function throws `TypeError` when called.
            UnmodeledImport
                => ("unmodeled_import".to_owned());
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
                /// Positional parameter reads, in declaration order.
                fixed: many,
                /// The rest parameter's list, flattened onto the end when present.
                rest: opt,
            }
                => (
                    format!(
                        "arguments_object [{}], {}",
                        fixed.join(", "),
                        rest.unwrap_or_else(|| "none".to_owned())
                    )
                );
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
                class: (sym "class"),
            }
                => (format!("host_global_read {class}"));
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
                class: (sym "class"),
                /// The value being stored.
                value: one,
            }
                => (format!("host_global_write {class} = {value}"));
            /// Whether a modeled host constructor's global override slot is currently
            /// *present* (`typeof <class> !== 'undefined'` folding for a reassigned host
            /// name). `false` only when the slot has been overridden to JS `undefined`
            /// (`Absent`); `true` for `Native` and any `Ctor` override. Bool-typed.
            HostGlobalPresent {
                /// Modeled host constructor whose override slot presence is tested.
                class: (sym "class"),
            }
                => (format!("host_global_present {class}"));
            /// Select a first-class function value from a static string-keyed table.
            FunctionTableLookup {
                /// String key used to select the function.
                key: one,
                /// Function values keyed by source object field text.
                cases: {String},
            }
                => (
                    format!(
                        "function_table_lookup {key} {{{}}}",
                        cases
                            .iter()
                            .map(|(case_key, value)| format!("{case_key:?}: {value}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                );
            /// Test whether a value is an instance of a class.
            InstanceOf {
                /// Value being tested. Kept as an operand so side effects are evaluated before the check.
                value: one @READ,
                /// Target class symbol.
                class: [$crate::Symbol],
            }
                => (format!("instanceof {}, class{}", value, class.0));
            /// A class used as a value: the runtime constructor object.
            ///
            /// `instanceof`, `new` and `.name` read it at run time, which is the
            /// dynamic boundary: which class a slot holds is not a static fact.
            ClassValue {
                /// The class.
                class: [$crate::Symbol],
                /// The closure constructing an instance, when the class's
                /// constructor signature is known where the value is read.
                constructor: opt,
            }
                => (format!("class_value class{} {constructor:?}", class.0));
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
                value: one,
                /// The constructor function value whose `prototype` is looked for.
                target: one,
            }
                => (format!("instanceof {value}, {target}"));
            /// Test the runtime tag of a TypeScript `unknown` value.
            UnknownIs {
                /// Value being tested.
                value: one @READ,
                /// Runtime tag to check.
                kind: [$crate::UnknownKind],
            }
                => (format!("unknown_is {kind:?} {value}"));
            /// Return the JavaScript `typeof` string for a runtime-erased value.
            TypeofValue {
                /// Value being classified.
                value: one @READ,
            }
                => (format!("typeof {value}"));
            /// Compute the opaque `Object.getPrototypeOf` sentinel for an erased value.
            ///
            /// Lowers to the `smelt_prototype_sentinel` runtime helper, which returns a
            /// distinct string for arrays, plain objects, and class instances (and `null`
            /// for null-prototype values), so prototype comparisons can tell a class
            /// instance apart from a structurally identical plain object.
            PrototypeSentinel {
                /// Value whose prototype sentinel is being computed.
                value: one,
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
                own_slot_shadows: [bool],
            }
                => (
                    if *own_slot_shadows {
                        format!("proto_accessor {value}")
                    } else {
                        format!("prototype_sentinel {value}")
                    }
                );
            /// Box a primitive the way `Object(value)` does (objects pass through).
            ///
            /// Lowers to the `smelt_box_value` runtime helper. The wrapper it builds is
            /// the same marker shape `new Number(..)` / `new Boolean(..)` /
            /// `new String(..)` build, so both spellings share one representation.
            BoxPrimitive {
                /// Value being boxed.
                value: one @ERASED,
            }
                => (format!("box_primitive {value}"));
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
                prototype: one,
            }
                => (format!("object_from_prototype {prototype}"));
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
                target: one,
                /// Erased object mapping property key to property descriptor.
                descriptors: one,
            }
                => (format!("define_properties {target} {descriptors}"));
            /// Extract a typed value from a TypeScript `unknown` value.
            UnknownCast {
                /// Value being extracted.
                value: one @ERASED,
                /// Type expected after extraction.
                target: (ty),
            }
                => (format!("unknown_cast {value} as {target}"));
            /// Convert a list into a set by collecting unique items.
            ListToSet {
                /// List value to collect into a set.
                list: one @RECV,
            }
                => (format!("list_to_set {list}"));
            /// Convert a list of key/value tuples into a dictionary.
            ListPairsToDict {
                /// List of pair tuples to collect into a dictionary.
                list: one @RECV,
            }
                => (format!("list_pairs_to_dict {list}"));
            /// Convert a homogeneous tuple into a list.
            TupleToList {
                /// Tuple value to collect into a list.
                tuple: one,
            }
                fresh(true)
                => (format!("tuple_to_list {tuple}"));
            /// Convert a homogeneous list into a statically-sized tuple.
            ListToTuple {
                /// List value to collect into a tuple.
                list: one @RECV,
            }
                => (format!("list_to_tuple {list}"));
            /// Convert a homogeneous tuple into a set by collecting unique items.
            TupleToSet {
                /// Tuple value to collect into a set.
                tuple: one,
            }
                => (format!("tuple_to_set {tuple}"));
            /// Read one statically-known tuple field by index.
            TupleIndex {
                /// Tuple value to read from.
                tuple: one,
                /// Zero-based tuple item index.
                index: [usize],
            }
                => (format!("tuple_index {tuple}, {index}"));
            /// Build a tuple from a statically-known slice of another tuple.
            TupleSlice {
                /// Tuple value to slice.
                tuple: one,
                /// Inclusive normalized start index.
                start: [usize],
                /// Exclusive normalized end index.
                end: [usize],
            }
                fresh(true)
                => (format!("tuple_slice {tuple}, {start}, {end}"));
            /// Run a runtime-backed async operation.
            AsyncOp {
                /// Operation to perform.
                op: [$crate::AsyncOp],
                /// Operation inputs.
                args: many @CALLED,
            }
                => (format!("{op}({})", args.join(", ")));
            /// One of the four ECMA-262 URI transcoding globals applied to `operand`:
            /// `encodeURI`, `encodeURIComponent`, `decodeURI`, `decodeURIComponent`.
            /// `op` carries which; see [`UriTranscodeOp`] for why one node covers all
            /// four, and the `smelt_encode_uri*` / `smelt_decode_uri*` runtime helpers
            /// for the exact character sets.
            UriTranscode {
                /// Which of the four globals this is.
                op: [$crate::UriTranscodeOp],
                /// String operand to transcode.
                operand: one,
            }
                fallible(op.is_fallible())
                call(match op {
                    $crate::UriTranscodeOp::Decode => "@decode_uri".to_owned(),
                    $crate::UriTranscodeOp::DecodeComponent => "@decode_uri_component".to_owned(),
                    $crate::UriTranscodeOp::Encode => "@encode_uri".to_owned(),
                    $crate::UriTranscodeOp::EncodeComponent => "@encode_uri_component".to_owned(),
                })
                => (format!("{op} {operand}"));
            /// A call into a modeled Node host module (`path.join(..)`,
            /// `createHash('sha256')`, `hash.update(..)`), resolved from the IMPORT the
            /// callee names rather than from its spelling — see [`HostModuleOpKind`]
            /// for each op's arguments and result type.
            HostModuleCall {
                /// Which host function this call runs.
                op: [$crate::HostModuleOp],
                /// Operation arguments, in the op's documented order.
                args: many,
            }
                fallible(op.is_fallible())
                call(format!("@{}", op.name()))
                => (format!("{} {}", op.name(), args.join(" ")));
            /// Read or write a component of a concrete `URL`, or take its
            /// `searchParams`.
            UrlOp {
                /// Which operation this is.
                op: [$crate::UrlOp],
                /// The `URL` receiver.
                url: one,
                /// The written value for a setter; empty otherwise.
                args: many,
            }
                fallible(matches!(op, $crate::UrlOp::Set($crate::UrlField::Href)))
                call("@url_set_href".to_owned())
                => (format!("url_{op:?} {url}{}", tail(", ", ", ", &args)));
            /// Python's `/`, `//` or `%` over two `int`s or two `float`s.
            ///
            /// A zero divisor raises `ZeroDivisionError`, so the op is fallible
            /// unless the frontend proved the divisor is a nonzero constant.
            PythonDivision {
                /// Which operator this is.
                op: [$crate::PythonDivisionOp],
                /// The dividend.
                lhs: one,
                /// The divisor.
                rhs: one,
                /// Whether the divisor can be zero at run time.
                divisor_checked: [bool],
            }
                fallible(*divisor_checked)
                call("@python_division".to_owned())
                => (format!("python_division {lhs} {} {rhs} checked={divisor_checked:?}", op.source_text()));
            /// Python's `d[k]` read, which raises `KeyError` on a missing key.
            PythonDictRead {
                /// The dictionary.
                dict: one,
                /// The key.
                key: one,
            }
                fallible(true)
                call("@python_dict_read".to_owned())
                => (format!("python_dict_read {dict}, {key}"));
            /// Python's `xs[i]` on a list: a negative index counts from the end,
            /// and an index out of range raises `IndexError`.
            PythonListRead {
                /// The list.
                list: one,
                /// The index, an `int`.
                index: one,
            }
                fallible(true)
                call("@python_list_read".to_owned())
                => (format!("python_list_read {list}, {index}"));
            /// A Python exception instance: an instance of the class whose own
            /// name and bases up to `BaseException` are `classes`, built from
            /// the constructor arguments `BaseException.__init__` keeps.
            PyExceptionNew {
                /// The class's own name first, then each base.
                classes: [Vec<$crate::Symbol>],
                /// The constructor arguments.
                args: many,
            }
                => (format!("py_exception_new {classes:?} [{}]", args.join(", ")));
            /// Python's `str(value)` of a value whose Python text the runtime
            /// writes: a container, an exception, `None`.
            PyStr {
                /// The value.
                value: one,
            }
                => (format!("py_str {value}"));
            /// Python's `repr(value)`.
            PyRepr {
                /// The value.
                value: one,
            }
                => (format!("py_repr {value}"));
            /// `e.args`: the constructor arguments of an exception, as the
            /// runtime's `SmeltPyExceptionArgs`.
            PyExceptionArgs {
                /// The exception.
                exception: one,
            }
                => (format!("py_exception_args {exception}"));
            /// Whether an `except` clause naming `classes` catches `exception`.
            PyExceptionCaughtBy {
                /// The caught exception.
                exception: one,
                /// The classes the clause names.
                classes: [Vec<$crate::Symbol>],
                /// Whether the clause catches an error the runtime raised
                /// without a class: true when it names only builtin classes.
                catches_unclassified: [bool],
            }
                => (format!("py_exception_caught_by {exception}, {classes:?}, {catches_unclassified}"));
            /// Python's `xs[i]` read of a `list` or `str`: a negative index
            /// counts from the end, and one past either end raises
            /// `IndexError`.
            PythonSequenceRead {
                /// The list or string.
                sequence: one,
                /// The `int` index.
                index: one,
            }
                fallible(true)
                call("@python_sequence_read".to_owned())
                => (format!("python_sequence_read {sequence}, {index}"));
            /// Python's `xs[start:end:step]` on a `list` or `str` with an explicit
            /// step: the positions `slice.indices` picks, in step order. A zero
            /// step raises `ValueError`.
            PythonStepSlice {
                /// The list or string.
                sequence: one,
                /// Optional start bound.
                start: opt,
                /// Optional end bound.
                end: opt,
                /// The step.
                step: one,
            }
                fallible(true)
                call("@python_step_slice".to_owned())
                => (format!("python_step_slice {sequence}, {}, {}, {step}", opt(&start), opt(&end)));
            /// `btoa` / `atob`: the base64 transcoders of the web platform.
            ///
            /// Fallible in both directions: `btoa` refuses a code point above U+00FF
            /// and `atob` refuses malformed base64, so both throw a catchable
            /// `InvalidCharacterError` `DOMException`.
            Base64Transcode {
                /// Which direction this is.
                op: [$crate::Base64Op],
                /// The string transcoded.
                operand: one,
            }
                fallible(true)
                call(format!("@{}", op.name()))
                => (format!("{} {operand}", op.name()));
            /// `new RegExp(pattern, flags)` over a pattern that is NOT a source literal.
            ///
            /// A literal pattern (`/a+/`, `new RegExp('a+')`) is checked by the
            /// frontend and stays an ordinary `New { class: RegExp }`. A pattern that is
            /// only known at run time can be malformed, and JavaScript answers that
            /// with a catchable `SyntaxError` AT CONSTRUCTION (`try { new RegExp(p) }
            /// catch { .. }`), so this operation is fallible rather than an infallible
            /// rvalue that could only panic at the first match.
            RegExpCompile {
                /// The pattern text, a `String`.
                pattern: one,
                /// The flags text, a `String` (`""` when the source passed none).
                flags: one,
            }
                fallible(true)
                call("@regexp_compile".to_owned())
                => (format!("regexp_compile {pattern} {flags}"));
            /// A `DataView` element accessor: `getInt16(offset, littleEndian?)` or
            /// `setFloat64(offset, value, littleEndian?)`, which throws a catchable
            /// `RangeError` when the offset is outside the view.
            ///
            /// One node for all twenty accessors, carrying the DIRECTION and the
            /// element type the registry resolved from the source member name. The
            /// element kind is not a property of the receiver here (the same view
            /// answers `getInt16` and `getFloat64`), which is why `DataView` cannot
            /// share `ByteArrayOp`'s shape, where the kind rides on the value.
            DataViewAccess {
                /// Whether this accessor WRITES (`setX`) rather than reads (`getX`).
                write: [bool],
                /// The element type, which is the accessor's width and signedness.
                element: [$crate::TypedArrayElement],
                /// The `DataView` receiver.
                view: one,
                /// The accessor arguments, as written.
                args: many,
            }
                fallible(true)
                call(format!(
                    "@data_view_{}{}",
                    if *write { "set" } else { "get" },
                    element.element_name()
                ))
                => (format!(
                    "data_view_{}{} {view} {}",
                    if *write { "set" } else { "get" },
                    element.element_name(),
                    args.join(" ")
                ));
            /// A fallible JavaScript `bigint` operation (see [`crate::BigIntOp`]):
            /// `BigInt(x)`, `BigInt.asIntN`/`asUintN`, and the bigint `/`, `%` and
            /// `**`, each of which can throw a catchable `RangeError`.
            BigIntCall {
                /// Which operation this is.
                op: [$crate::BigIntOp],
                /// The operands in source order (`[value]`, `[bits, value]`, or
                /// `[lhs, rhs]`).
                args: many,
            }
                fallible(true)
                call(format!("@{}", op.name()))
                => (format!("{} {}", op.name(), args.join(" ")));
            /// `new URL(input, base?)`: a parsed WHATWG URL value, which throws a
            /// catchable `TypeError` when the input (resolved against the base) is not
            /// a valid URL.
            ///
            /// Both operands are strings; a `URL` argument is read as its `href` by
            /// the frontend, the stringification the constructor itself performs.
            UrlNew {
                /// The URL text to parse.
                input: one,
                /// The base URL text a relative `input` resolves against.
                base: opt,
            }
                fallible(true)
                call("@url_new".to_owned())
                => (base.map_or_else(
                    || format!("url_new {input}"),
                    |base| format!("url_new {input}, {base}"),
                ));
            /// `JSON.parse(text)` / Python `json.loads(text)`, which throws a catchable
            /// `SyntaxError` on malformed text.
            ///
            /// `target` is `Some(T)` when the SOURCE declared the parse's type:
            /// `JSON.parse<T>(s)`, `JSON.parse(s) as T`, or a parse that flows straight
            /// into a `T`-annotated variable, `T`-typed parameter or `T`-returning
            /// function. Such a parse is a typed entry boundary: the backend decodes
            /// the parsed value strictly into `T` and throws a catchable `TypeError` on
            /// mismatch (design adapted from Bun's `--aot` typed entry checks,
            /// oven-sh/bun#45631). `None` keeps the dynamic parse whose type is the
            /// frontend's fallback (an untyped `JSON.parse(s)`, Python `json.loads`).
            JsonParse {
                /// The JSON text.
                text: one,
                /// The source-declared type the parsed value is decoded into.
                target: [Option<$crate::TypeId>],
            }
                fallible(true)
                call(if target.is_some() { "@json_decode" } else { "@json_parse" }.to_owned())
                => (format!(
                    "{} {text}",
                    if target.is_some() { "json_decode" } else { "json_parse" }
                ));
        }
    };
}

/// The operations MIR lowering handles by hand, because they carry a mutation
/// receiver or short-circuiting control flow.
#[macro_export]
#[doc(hidden)]
macro_rules! op_group_custom {
    ($callback:path; $($before:tt)*) => {
        $callback! {
            $($before)*
            /// Call a closure value.
            ClosureCall {
                /// Closure value to call.
                callee: one @CALLED,
                /// Call arguments.
                args: many @CALLED,
            }
                => (format!("closure_call {callee}({})", args.join(", ")));
            /// Call a method through a TypeScript optional-chain receiver.
            OptionalMethod {
                /// Receiver operand, either `T` or `Option<T>`.
                receiver: one @CALLED,
                /// Method name to call when the receiver is present.
                method: (sym "method"),
                /// Call arguments.
                args: many @CALLED,
            }
                => (format!("{receiver}?.{method}({})", args.join(", ")));
            /// Return an optional value's content or a fallback operand.
            OptionalCoalesce {
                /// Optional operand to inspect.
                optional: one @ALIAS,
                /// Fallback operand used when the optional is empty.
                fallback: one @ALIAS,
            }
                // Both arms may become the result.
                => (format!("{optional} ?? {fallback}"));
            /// Push one item into a list and return the new length.
            ListPush {
                /// List value to mutate.
                list: one @RECV_MUT,
                /// Item to append.
                item: one @STORED,
            }
                mutates(Some(list))
                => (format!("list_push {list}, {item}"));
            /// Extend a list with items from another list and return `None`.
            ListExtend {
                /// List value to mutate.
                list: one @RECV_MUT,
                /// List supplying items to copy.
                other: one @READ,
            }
                // `extend` copies `other`'s elements out; `other` keeps its buffer.
                mutates(Some(list))
                => (format!("list_extend {list}, {other}"));
            /// Insert one item into a list at an integer index and return `None`.
            ListInsert {
                /// List value to mutate.
                list: one @RECV_MUT,
                /// Integer index where the item is inserted.
                index: one @READ,
                /// Item to insert.
                item: one @STORED,
            }
                mutates(Some(list))
                => (format!("list_insert {list}, {index}, {item}"));
            /// Insert zero or more items at the front of a list and return the new length.
            ListUnshift {
                /// List value to mutate.
                list: one @RECV_MUT,
                /// Items to insert at the front.
                items: many @STORED,
            }
                mutates(Some(list))
                => (format!("list_unshift {list} [{}]", items.join(", ")));
            /// Reverse a list in place and return the language-specific result.
            ListReverse {
                /// List value to mutate.
                list: one @ALIAS_MUT,
            }
                mutates(Some(list))
                => (format!("list_reverse {list}"));
            /// Clear all items from a list and return `None`.
            ListClear {
                /// List value to mutate.
                list: one @RECV_MUT,
            }
                mutates(Some(list))
                => (format!("list_clear {list}"));
            /// Remove the first list item equal to a target item and return `None`.
            ListRemove {
                /// List value to mutate.
                list: one @RECV_MUT,
                /// Item to remove.
                item: one @READ,
            }
                mutates(Some(list))
                => (format!("list_remove {list}, {item}"));
            /// Sort a list in place and return `None`.
            ListSort {
                /// List value to mutate.
                list: one @ALIAS_MUT,
                /// Optional comparator closure for JavaScript `Array.prototype.sort`.
                ///
                /// Like other callbacks, the comparator is a normal closure body
                /// referenced by `ExprId`; it takes two list items and returns a number.
                comparator: opt @CALLED,
                /// Optional key closure for Python `list.sort(key=...)`.
                ///
                /// The key is a normal closure body referenced by `ExprId`; it maps one
                /// list item to a sortable value. It is mutually exclusive with the
                /// JavaScript-style `comparator`.
                key: opt @CALLED,
                /// Whether to sort in descending order, as in Python `reverse=True`.
                reverse: [bool],
            }
                mutates(Some(list))
                => (
                    {
                        let comparator_text = if comparator.is_some() {
                            " <comparator>"
                        } else {
                            ""
                        };
                        let key_text = if key.is_some() { " <key>" } else { "" };
                        let reverse_text = if *reverse { " reverse" } else { "" };
                        format!("list_sort {list}{comparator_text}{key_text}{reverse_text}")
                    }
                );
            /// Pop the last item from a list.
            ListPop {
                /// List value to mutate.
                list: one @RECV_MUT,
            }
                mutates(Some(list))
                => (format!("list_pop {list}"));
            /// Remove and return the first item from a list.
            ListShift {
                /// List value to mutate.
                list: one @RECV_MUT,
            }
                mutates(Some(list))
                => (format!("list_shift {list}"));
        }
    };
}

/// `items` joined by `separator` after `lead`, or nothing when `items` is empty:
/// the optional argument tail of a `dump` line.
#[must_use]
pub fn tail(lead: &str, separator: &str, items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("{lead}{}", items.join(separator))
    }
}

/// An absent optional child's text is `_`.
#[must_use]
pub fn opt(child: &Option<String>) -> String {
    child.clone().unwrap_or_else(|| "_".to_owned())
}

/// A regex replacer's resolved argument roles as `matched,p1,position`, so a
/// dump shows WHICH spec arguments a callback receives, not merely that it has
/// some.
#[must_use]
pub fn regex_replace_args_text(args: &[crate::RegexReplaceArg]) -> String {
    args.iter()
        .map(|arg| match arg {
            crate::RegexReplaceArg::Matched => "matched".to_owned(),
            crate::RegexReplaceArg::Capture(index) => format!("p{index}"),
            crate::RegexReplaceArg::Position => "position".to_owned(),
            crate::RegexReplaceArg::Source => "source".to_owned(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    //! The text helpers every display clause leans on.

    use super::{opt, tail};

    #[test]
    fn tail_is_nothing_for_no_items_and_leads_otherwise() {
        assert_eq!(tail(", ", ", ", &[]), "");
        let items = ["a".to_owned(), "b".to_owned()];
        assert_eq!(tail(" ", ", ", &items), " a, b");
    }

    #[test]
    fn an_absent_child_prints_as_a_blank() {
        assert_eq!(opt(&None), "_");
        assert_eq!(opt(&Some("x".to_owned())), "x");
    }
}
