//! Classification: which [`Category`] one generated line falls in, and the
//! prelude and adapter region state a line scan carries to decide that.
use smelt_stdlib::runtime_symbols::boundary;

use super::{Category, scan::count_occurrences};

/// How a scanned file says where its runtime prelude ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    variant_size_differences,
    reason = "a two-word value copied once per scanned file"
)]
pub(super) enum PreludeBoundary {
    /// The emitter wrote [`smelt_codegen_rust::PRELUDE_END_MARKER`] at this
    /// 0-based line: every line at or before it is prelude, every line after
    /// it is program. Authoritative; no per-line heuristic runs.
    Marker {
        /// Index of the marker line.
        line: usize,
    },
    /// A module file: it opens with
    /// [`smelt_codegen_rust::MODULE_SOURCE_HEADER`], so it is the lowering of
    /// one source file and holds no runtime prelude. Every line is program,
    /// a program item with an emitter-minted name (`smelt_global_init__x__0`)
    /// included. Authoritative; no per-line heuristic runs.
    Module,
    /// Neither marker: the coarse `smelt_*` helper tracking of
    /// [`update_prelude_helper_state`] decides line by line.
    Heuristic {
        /// Whether the scan is inside a prelude helper definition.
        in_prelude_helper: bool,
    },
}

/// Where one scanned line sits relative to the runtime prelude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineSite {
    /// At or before the prelude marker: always runtime prelude.
    Prelude,
    /// Program text; `in_prelude_helper` is the heuristic's verdict in a
    /// marker-less file and always `false` after a marker.
    Program {
        /// Whether the heuristic places the line inside a prelude helper.
        in_prelude_helper: bool,
    },
    /// Program text by provenance: after the prelude marker, or anywhere in a
    /// module file. No name heuristic applies, so a program item whose name
    /// the emitter minted (`fn smelt_global_init__x__0`) is program code from
    /// its signature on.
    ProgramByProvenance,
}

impl PreludeBoundary {
    /// The boundary of a whole file's text.
    pub(super) fn of(text: &str) -> Self {
        if let Some(line) = text
            .lines()
            .position(|line| line.trim() == smelt_codegen_rust::PRELUDE_END_MARKER)
        {
            return Self::Marker { line };
        }
        // The header sits in the file's opening comment block, before any item.
        let names_its_source = text
            .lines()
            .take_while(|line| line.starts_with("//"))
            .any(|line| line.starts_with(smelt_codegen_rust::MODULE_SOURCE_HEADER));
        if names_its_source {
            Self::Module
        } else {
            Self::Heuristic {
                in_prelude_helper: false,
            }
        }
    }

    /// Place line `index` (whose text is `line`), advancing the heuristic.
    pub(super) fn advance(&mut self, index: usize, line: &str) -> LineSite {
        match self {
            Self::Marker { line: marker } if index <= *marker => LineSite::Prelude,
            Self::Marker { .. } | Self::Module => LineSite::ProgramByProvenance,
            Self::Heuristic { in_prelude_helper } => {
                update_prelude_helper_state(line, in_prelude_helper);
                LineSite::Program {
                    in_prelude_helper: *in_prelude_helper,
                }
            }
        }
    }
}

/// Tracks whether the scan is inside a generated boundary adapter.
///
/// An adapter converts one program type to or from the erased form, so its body
/// is the sanctioned typed-to-erased crossing itself: `SmeltUnknown` there is
/// the adapter's purpose, not a value that lost its type. Classifying by the
/// enclosing item keeps the rule narrow: erasure in a program field, signature
/// or method still reads as avoidable, even beside an adapter in the same
/// `impl`.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct AdapterBody {
    /// The exact line that closes the open adapter, while inside one.
    closing: Option<&'static str>,
}

impl AdapterBody {
    /// Whether `line` belongs to an adapter, advancing past it.
    pub(super) fn advance(&mut self, line: &str) -> bool {
        if let Some(closing) = self.closing {
            if line == closing {
                self.closing = None;
            }
            return true;
        }
        let Some(closing) = adapter_item_closing(line) else {
            return false;
        };
        if opens_more_braces_than_it_closes(line) {
            self.closing = Some(closing);
        }
        true
    }
}

/// The line closing the adapter that `line` opens, if it opens one.
///
/// Trait impls are matched by the trait they implement: a generic bound such
/// as `impl<T: IntoSmeltUnknown> Default for Cell<T>` names the trait too, and
/// is not an adapter. `from_smelt_unknown` is matched as a method because its
/// `impl` block also holds the program's own methods.
fn adapter_item_closing(line: &str) -> Option<&'static str> {
    let implements_adapter = line.starts_with("impl")
        && (line.contains(" IntoSmeltUnknown for ") || line.contains(" SmeltFromUnknown for "));
    if implements_adapter
        || line.starts_with("fn __smelt_from_unknown_record_")
        || line.starts_with("fn smelt_decode_")
    {
        return Some("}");
    }
    line.starts_with("    fn from_smelt_unknown(")
        .then_some("    }")
}

/// Update whether we are inside a `smelt_*` runtime prelude helper definition.
///
/// The prelude helpers are emitted as free functions named `smelt_...` (or `fn
/// smelt_...`), and the `SmeltUnknown` enum/impls are prelude too. Because Rust
/// item bodies are brace-balanced we cannot cheaply track nesting line-by-line
/// without a real parser, so this uses a coarse, documented heuristic: a helper
/// definition line *starts* a prelude region, and a line that is exactly a
/// closing brace at column zero *ends* it. This is sufficient because generated
/// helpers are top-level items whose closing brace sits at the left margin.
pub(super) fn update_prelude_helper_state(line: &str, in_prelude_helper: &mut bool) {
    let trimmed = line.trim_start();
    if is_prelude_definition_start(trimmed) {
        *in_prelude_helper = true;
        // A one-line helper (`fn smelt_x(...) -> ... { ... }`) both opens and
        // closes on the same line; treat a line that ends with `}` as closed.
        if line.trim_end().ends_with('}') && !opens_more_braces_than_it_closes(line) {
            *in_prelude_helper = false;
        }
        return;
    }
    if *in_prelude_helper && line == "}" {
        *in_prelude_helper = false;
    }
}

/// Whether a trimmed line begins a runtime-prelude item definition.
fn is_prelude_definition_start(trimmed: &str) -> bool {
    trimmed.starts_with("fn smelt_")
        || trimmed.starts_with("pub fn smelt_")
        || trimmed.starts_with("async fn smelt_")
        || trimmed.starts_with("pub async fn smelt_")
        || trimmed.starts_with("pub enum SmeltUnknown")
        || trimmed.starts_with("enum SmeltUnknown")
        || trimmed.starts_with("impl SmeltUnknown")
        || trimmed.starts_with("impl Clone for SmeltUnknown")
        || trimmed.starts_with("impl PartialEq for SmeltUnknown")
        || trimmed.starts_with("impl Default for SmeltUnknown")
        || trimmed.starts_with("impl ::std::fmt::Debug for SmeltUnknown")
}

/// Whether a line opens more braces than it closes (i.e. leaves a block open).
fn opens_more_braces_than_it_closes(line: &str) -> bool {
    let opens = count_occurrences(line, "{");
    let closes = count_occurrences(line, "}");
    opens > closes
}

/// Classify one line's `SmeltUnknown` occurrences into a [`Category`].
///
/// # Heuristics (documented, ordered)
///
/// The order matters: the first matching rule wins.
///
/// 1. **Runtime prelude** — the line defines the `SmeltUnknown` enum/impls or
///    sits inside a `smelt_*` helper body (`in_prelude_helper`). This is the
///    fixed scaffolding shared by every generated crate.
/// 2. **Legitimate boundary** — the line names a genuine dynamic edge:
///    - `SmeltUnknown::Function` / `SmeltUnknown::Promise` — erased callables and
///      promises are irreducibly dynamic (JS closures/interop, async values).
///    - `IntoSmeltUnknown` / `into_smelt_unknown` / `to_smelt_unknown` — explicit
///      boundary adapters, exactly the sanctioned way to cross into erasure.
///    - `smelt_alias_erased_generic_list` / `smelt_alias_erased_generic_array` —
///      the identity adapters between two erased spellings of one generic array.
///    - runtime narrowing helpers (`js_typeof`, `smelt_typeof(`, `tag_check`,
///      `smelt_unknown_is_`) — inspecting an
///      already-dynamic value.
/// 3. **Avoidable erasure** — everything else. This deliberately captures
///    `SmeltUnknown` in program function signatures, `Vec<SmeltUnknown>` storage,
///    and concrete-tag construction (`SmeltUnknown::Number/String/Bool/...`) used
///    as ordinary data flow, because those are the cases concrete types, unions,
///    or generics are expected to remove. Ambiguous lines fall here on purpose:
///    over-reporting avoidable erasure is safe for a non-blocking gate.
pub(super) fn classify_line(line: &str, in_prelude_helper: bool) -> Category {
    if in_prelude_helper || is_prelude_definition_start(line.trim_start()) {
        return Category::RuntimePrelude;
    }
    classify_program_line(line)
}

/// Classify a line known to be program code by where it sits (see
/// [`LineSite::ProgramByProvenance`]): boundary or avoidable, never prelude.
pub(super) fn classify_program_line(line: &str) -> Category {
    if is_legitimate_boundary_line(line) {
        return Category::LegitimateBoundary;
    }
    Category::AvoidableErasure
}

/// Spellings that put a line at a genuine dynamic boundary; each entry says why.
const BOUNDARY_MARKERS: [&str; 41] = [
    boundary::FUNCTION_TAG,
    boundary::PROMISE_TAG,
    // A JavaScript SYMBOL value. `Symbol()` mints a value whose whole
    // meaning is a unique runtime identity with no static counterpart:
    // there is no struct, no generated union arm and no scoped generic that
    // can carry "a fresh identity, distinct from every other", because the
    // distinctness is a run-time property of the evaluation, not of a type.
    // The symbol's use as a member KEY is a different thing and is static --
    // a module-level `const K = Symbol()` folds to a synthetic member name
    // and the read resolves to that member, with no erasure at all (see
    // `computed_key_symbols::unique_symbol_key`). What stays erased is only
    // the symbol as a first-class VALUE. Proven in
    // `symbol_value_is_a_boundary` below.
    boundary::SYMBOL_TAG,
    // Recovering a symbol VALUE from the property key it indexed:
    // `Object.getOwnPropertySymbols` and erased-Map enumeration hand a
    // stored key back to the program AS A SYMBOL through the emitted
    // prelude helper `smelt_own_symbol_key_value`, the inverse whose call
    // sites appear in program code. `Reflect.ownKeys` uses the wider prelude
    // helper `smelt_key_symbol_value` instead, which also maps well-known keys
    // and is called only inside the prelude. The value it produces is a
    // `SmeltUnknown::Symbol` -- the marker directly above -- for exactly the
    // reason given there: a fresh runtime identity has no static carrier.
    // The tag sits INSIDE the helper, so the call site needs its own marker.
    // Proven in `symbol_key_inverse_is_a_boundary` below.
    boundary::OWN_SYMBOL_KEY_VALUE,
    // Aliasing an erased generic array into another erased slot
    // (`crates/smelt-codegen-rust/src/erased_list_alias.rs`). A `T[]` whose
    // `T` is out of scope is rendered under the erased-generic ABI as
    // `SmeltList<SmeltUnknown>`, since no Rust generic in scope can name
    // `T`. The emitter calls these two prelude helpers only where both sides
    // render that way and the SOURCE element is such an erased `T`: the
    // value is leaving the erased-generic ABI. The erasure happened before
    // the helper; the helper is the identity adapter between two erased
    // spellings of one JavaScript array, sharing its buffer and id.
    //
    // Deliberately narrow: a source `unknown[]` (into `unknown`, or into an
    // erased `T[]` slot) keeps a plain spelling and its avoidable
    // classification. The general list conversion
    // `Into::<SmeltList<_>>::into(` is NOT a marker either: every list-typed
    // rvalue goes through it, including ones that erase a concrete value. A
    // concrete element list or an in-scope generic `SmeltList<T>` never
    // reaches these helpers (their signatures pin `SmeltUnknown` elements, so
    // the generated crate would not compile).
    // Proven in `erased_generic_list_alias_is_a_boundary` below and in
    // `crates/smelt-codegen-rust/src/tests/the `emit_*_tests.rs` files`
    // (`erased_generic_list_into_unknown_list_takes_the_alias_helper`,
    // `erased_generic_list_argument_judges_each_side_at_its_own_scope`,
    // `concrete_list_erasure_still_converts_elements`,
    // `source_unknown_list_erasure_keeps_its_direct_wrap`).
    smelt_codegen_rust::ERASED_GENERIC_LIST_ALIAS_FN,
    smelt_codegen_rust::ERASED_GENERIC_ARRAY_ALIAS_FN,
    // Reading an optional erased value at the erased carrier
    // (`Option<SmeltUnknown>` to `SmeltUnknown`, absence becoming
    // `undefined`). Same reasoning as the two list-alias helpers above:
    // the value is a `SmeltUnknown` on both sides, and the helper adapts
    // one erased spelling to the other. Proven in
    // `optional_unknown_read_is_a_boundary` below.
    smelt_codegen_rust::OPTIONAL_UNKNOWN_OR_UNDEFINED_FN,
    // The hoisted erased-record-to-class conversion
    // (`fn __smelt_from_unknown_record_<n>(smelt_value: SmeltUnknown) -> T`,
    // `EmitContext::record_conversion_helpers` in codegen). It is the
    // inverse adapter where an ERASED value is recovered as a typed class,
    // the erased side being the value that arrives, so its parameter is
    // `SmeltUnknown` by definition. Until the helper existed the identical
    // conversion was spelled inline at every site as
    // `match (v).into_smelt_unknown() { .. }` and classified here through
    // that marker; hoisting it (Hono's crate shrank 57 MB -> 12 MB) left the
    // signature as the one line newly naming `SmeltUnknown`. The prefix
    // keeps the classification the conversion had inline and no more:
    // an ordinary function taking `SmeltUnknown` stays avoidable. Proven in
    // `record_conversion_helper_is_a_boundary` below.
    smelt_codegen_rust::RECORD_CONVERSION_HELPER_PREFIX,
    // An untyped `JSON.parse(text)`: the result is whatever the TEXT holds
    // (`null`, a number, a string, an array or an object), known only when
    // the text is parsed, which is the "JSON values" boundary CLAUDE.md
    // names. A source that knows the shape writes `JSON.parse<T>` or
    // `as T`, which lower to a typed target and never reach this adapter
    // with an erased result. Proven in `untyped_json_parse_is_a_boundary`.
    boundary::JSON_PARSE_CALL,
    // A `node:sqlite` ROW read (`statement.all(..)` / `statement.get(..)`):
    // the row's columns are whatever the SQL text selects, decided when the
    // statement runs — `SELECT *` over a table another program altered
    // answers columns no compile-time type names — so a row is the same
    // run-time document `JSON.parse` returns, and a source that knows the
    // shape narrows it (`as TodoRow[]`). Statement PARAMETERS are not on
    // this list: they bind at their own Rust types (`SmeltSqliteBind`), so
    // an erased parameter still counts as avoidable. Proven in
    // `sqlite_row_read_is_a_boundary`.
    boundary::SQLITE_ALL_CALL,
    boundary::SQLITE_GET_CALL,
    boundary::INTO_TRAIT,
    boundary::INTO_METHOD,
    boundary::TO_METHOD,
    boundary::JS_TYPEOF,
    // `typeof` over an erased value reads that value's runtime tag: the same
    // runtime-narrowing boundary `js_typeof`/`tag_check` name, just factored
    // into one prelude helper instead of an inlined `match` per site (see
    // `FunctionEmitter::typeof_str_expr`). The nine-arm table it replaced was
    // classified here through its `SmeltUnknown::Function` arm; keeping the
    // call site classified the same way is what makes the two spellings
    // comparable across a baseline diff. `typeof` over a CONCRETE union does
    // not reach this marker -- it stays an inline per-member match and names
    // no `SmeltUnknown` at all. Proven in `erased_typeof_call_is_a_boundary`.
    boundary::TYPEOF_CALL,
    boundary::TAG_CHECK,
    boundary::UNKNOWN_IS_PREFIX,
    // The exception-payload ABI (`crates/smelt-codegen-rust/src/thrown.rs`).
    // `throw` accepts any JavaScript value, a `catch` binding has no static
    // type, and every fallible generated function shares one error channel
    // (`Result<T, Box<dyn Error>>`) — including erased callbacks, whose
    // signature is fixed crate-wide and so cannot mention a caller-specific
    // error type. `smelt_throw`/`smelt_thrown_value` are therefore explicit
    // boundary adapters entering and leaving that channel, exactly the
    // "erased interop" case, not avoidable program-storage erasure. Proven in
    // `crates/smelt-codegen-rust/src/tests/thrown_tests.rs`
    // (`one_channel_carries_structurally_unrelated_payloads`), which sends a
    // field-bearing record and a bare string through one function's channel on
    // a run-time branch and recovers both at a single `catch`: no concrete
    // type, generated union arm, or scoped generic can express that.
    boundary::THROW_CALL,
    boundary::THROWN_VALUE_CALL,
    // The value a `finally` holds while its clause runs on an exceptional
    // exit (`smelt_mir::PENDING_THROWN_LOCAL`, see `lower_try_catch`): the
    // same exception payload as the two adapters above, parked between
    // the handler and the rethrow, plus its `undefined` initialization
    // before the `try`. The clause is shared by every exit, so the value
    // is whatever the guarded body threw. Proven in example 181
    // (`rethrowThrough`: two throws through one
    // clause) and in `pending_thrown_value_is_a_boundary` below.
    smelt_mir::PENDING_THROWN_LOCAL,
    // The panic-recovery exception payload
    // (`crates/smelt-codegen-rust/src/thrown.rs`,
    // `runtime/thrown.rs`). When a call inside a `try` is not
    // statically known to throw, its handler is emitted as a
    // `catch_unwind`; the recovered panic message has no Smelt payload, so
    // the value the `catch` observes is rebuilt as the branded `Error`
    // record `{ __smelt_error: "Error", message }` — the SAME record
    // `smelt_thrown_value` synthesizes for a foreign error, whose ABI is
    // already a boundary above. `throw` accepts any JavaScript value and a
    // `catch` binding has no static type (TypeScript types it `unknown`), so
    // no concrete struct, generated union arm, or scoped generic can carry
    // it. Matching the *message binding* rather than the record shape keeps
    // this to the recovery site: an ordinary `new Error(msg)` record built
    // from program values is unaffected and stays classified as it was.
    // Proven in `panic_recovery_payload_is_a_boundary` below.
    boundary::THROWN_STRING,
    // `.slice()` on an erased receiver (a generic `T` / `unknown` value that
    // may be an array, string, or a typed-array/array-buffer marker at
    // runtime) emits a `smelt_slice_value` tag match that dispatches on the
    // `SmeltUnknown` variant — a genuine runtime-narrowing boundary. A
    // concrete type is unavailable precisely because the receiver is erased,
    // so this is a boundary adapter, not avoidable program-storage erasure.
    boundary::SLICE_VALUE,
    // The erased callable-object ABI. A TypeScript interface with call
    // signatures lowers to a record whose synthetic `__smelt_call` slot
    // holds the underlying callable; when the interface declares several
    // overloads, that ONE slot cannot be typed by any of them (see
    // `ModuleBuilder::overloaded_call_signature_slot_type`) — which overload
    // runs is decided by the callee inspecting the runtime argument values,
    // e.g. es-toolkit's `curry` comparing an argument against its
    // placeholder sentinel. So the slot, the adapters that read it, and the
    // values that flow through it are a genuine dynamic boundary in the
    // "values inspected through runtime narrowing" sense, not avoidable
    // program-storage erasure: no concrete Rust `Fn`, generated union arm,
    // or scoped generic can carry two overloads of different shape at once.
    // Proven in `erased_callable_object_slot_is_a_boundary` below and in
    // `crates/smelt-codegen-rust/src/tests/the `emit_*_tests.rs` files`
    // (`ambiguous_same_arity_call_signatures_answer_the_erased_call_result`,
    // `call_beyond_every_declared_overload_arity_keeps_its_arguments`,
    // `same_arity_call_signatures_are_selected_by_argument_type`).
    boundary::CALL_SLOT,
    // The JavaScript `this` receiver channel
    // (`smelt_push_this`/`smelt_this`/`smelt_bind_this`, emitted for
    // `ExprKind::ThisRead` and `ExprKind::BindThis`). `this` is supplied by
    // the CALL, not by the definition site: the same plain function sees a
    // different receiver depending on whether it was reached as
    // `object.method()`, `fn.call(thisArg, ..)`, `fn.apply(thisArg, ..)`,
    // `fn.bind(thisArg)()`, or a bare `fn()`. The source states no type for
    // it either -- every corpus site spells `this: any` or `this: unknown`
    // -- and the value is then inspected through runtime narrowing
    // (`match smelt_this() { SmeltUnknown::Object(map) => .. }`), which is
    // the "values inspected through runtime narrowing" case verbatim. No
    // concrete struct, generated union arm, or scoped generic can carry it:
    // one erased callable is reachable from arbitrarily many unrelated
    // receiver types at runtime, and the callable's Rust signature is fixed
    // when it is created, before any of them is known. Proven in
    // `this_receiver_channel_is_a_boundary` below.
    boundary::PUSH_THIS_CALL,
    boundary::PUSH_THIS_LAZY_CALL,
    boundary::THIS_CALL,
    boundary::BIND_THIS_CALL,
    // The erased class-prototype slot
    // (`crates/smelt-codegen-rust/src/class_proto.rs`). A generated class
    // keeps its concrete struct and its precisely typed inherent methods;
    // `__smelt_proto_entries` exists ONLY so that an instance which has
    // already crossed into erased code can still answer a method read.
    // That read is `smelt_get_object_field(&map, "has")` — a member looked
    // up by STRING NAME on a value whose static type is `unknown`. No
    // concrete Rust type can represent it: the set of a class's methods
    // reached by runtime name is not a struct field, not a generated union
    // arm, and not a scoped generic — the consumer (e.g. es-toolkit's
    // `memoize`, whose `cache` parameter is a dynamic bag) never names the
    // class at all. Each adapter is exactly the explicit boundary shim the
    // policy calls for: `SmeltFromUnknown` in, the real typed method in the
    // middle, an erased result out. Proven in
    // `erased_class_prototype_slot_is_a_boundary` below and executed by
    // `crates/smelt-codegen-rust/tests/erased_class_method_runtime.rs`.
    boundary::PROTO_ENTRIES_LOCAL,
    boundary::PROTO_ENTRIES,
    // The canonical erased property read (`smelt_get_unknown_field`):
    // a member looked up by STRING NAME on a value whose static type is
    // `unknown` (or an optional / union receiver that erases to it), which
    // dispatches on the `SmeltUnknown` variant at runtime — an object's own
    // entries, an array's named-property side table, a function's property
    // bag, the `Object.prototype` fallback. That is the "values inspected
    // through runtime narrowing" case verbatim: the receiver's shape is
    // decided by the value, so no concrete struct field, generated union
    // arm, or scoped generic can name the member ahead of time. Proven in
    // `erased_property_read_is_a_boundary` below.
    boundary::GET_UNKNOWN_FIELD_CALL,
    // The ambient global object (`crates/smelt-codegen-rust/src/thrown.rs`,
    // `runtime/object_model.rs`). JavaScript's global object is an open
    // property bag that the program AND its host write to
    // (`Object.assign(globalThis, {..})`, a host-injected
    // `declare const X`), so which members it holds is a run-time fact of
    // whatever ran before the read: no struct, generated union arm or
    // scoped generic can describe it. Every reference is the one shared
    // erased record, and `smelt_unknown_assign` is the in-place
    // `Object.assign` onto it. Proven in
    // `global_object_members_are_run_time_facts` below.
    boundary::GLOBAL_OBJECT_CALL,
    boundary::UNKNOWN_ASSIGN_CALL,
    // An error record's `cause` slot. ES2022 types it `cause?: unknown`,
    // and that spelling is the canonical source-level dynamic boundary: the
    // value is whatever the program chose to attach to the error, so it is
    // the thrown-value channel's payload seen from the error object rather
    // than program storage with a knowable shape. No concrete type,
    // generated union arm, or scoped generic can carry it — the same
    // reasoning that keeps `smelt_throw`/`smelt_thrown_value` a boundary
    // above, and the reason the slot is declared `Type::Unknown` at exactly
    // one place (`ERROR_MARKER_FIELDS` in
    // `smelt-frontend-ts/src/lowering/decls/super_call.rs`, whose docstring
    // spells out why the other three `Error` slots must NOT erase).
    //
    // Deliberately narrow: the marker is the field-shaped text `cause:
    // SmeltUnknown`, so it matches the slot's declaration and the value
    // written into it and nothing else. An erased local, an erased
    // parameter, or a `Vec<SmeltUnknown>` next to it stays avoidable, which
    // is what `error_cause_slot_is_a_boundary` below proves. Without this
    // rule every `class X extends Error` carried two avoidable lines it can
    // never remove (the injected slot in each constructor's `this`
    // literal), which kept `Error` subclasses out of the zero-erasure
    // examples corpus.
    boundary::ERROR_CAUSE_FIELD,
    // Handing a value to the JSON serializer. `JSON.stringify` is specified
    // by ECMA-262, not by serde: an integral number has no fraction, a
    // non-finite number is `null`, an `undefined`/function/symbol property
    // is omitted, key order is insertion order, and a byte view serializes
    // as its element indices. Every one of those rules lives in
    // `smelt_json_stringify` (mirroring `Serialize for SmeltUnknown`), so the emitter erases the value at this
    // one call site and lets that impl decide — for a union, whose arm is
    // only known at run time, it is the ONLY place the per-arm answer can
    // be decided at all.
    //
    // That makes the erasure an explicit boundary adapter into the "JSON
    // value" boundary the policy names, not program storage: the tagged
    // value exists for the duration of one call and is never read back as
    // data. The marker is the adapter call itself (`smelt_json_stringify(&`,
    // the runtime JSON writer that applies those rules and quotes strings
    // over their code units),
    // so a `Vec<SmeltUnknown>` or an erased local NEXT to it stays
    // avoidable — proven by `json_serializer_argument_is_a_boundary`.
    boundary::JSON_TO_STRING,
    // `instanceof HostClass` against a DYNAMIC or erased-class operand
    // (`crates/smelt-codegen-rust/src/emitter/instance_of.rs`, the
    // `host_instance_markers` arm). Every modeled host object whose
    // identity has to survive erasure — `Request`, `DataView`,
    // `SharedArrayBuffer`, `DOMException`, `WeakMap`, the boxed primitive
    // wrappers — carries a dedicated `__smelt_*` brand key, and this arm is
    // the one place that reads it back: the operand's static type says only
    // "some erased value", so the class is recovered from the value's own
    // brand at run time. That is the policy's "values inspected through
    // runtime narrowing" case verbatim, and it is the same guard the
    // emitter already spells through `tag_check` / `smelt_unknown_is_` for
    // the primitive tags — the host brands were the one family whose guard
    // was still inlined, so it was classified as program storage.
    //
    // The canonical erased operand is a `catch` binding: TypeScript types
    // one `unknown` by its own rule, so `catch (e) { if (e instanceof
    // DOMException) .. }` is the idiomatic — and only — way a program reads
    // a host error's brand, and no concrete type, generated union arm, or
    // scoped generic can carry it (the same reasoning that makes
    // `smelt_throw` / `smelt_thrown_value` a boundary above).
    //
    // Deliberately narrow, like the `cause` rule: the marker is the guard's
    // own shape — the match guard on the `value` binding this arm (and only
    // this arm) introduces, probing a `__smelt_`-prefixed brand key. It
    // matches both spellings the arm emits, the plain
    // `SmeltUnknown::Object(value)` and the `Option` receiver's
    // `Some(SmeltUnknown::Object(value))`, and nothing else: the prelude's
    // own brand probes bind `map`, `object`, `entries` or `record`, never
    // the guard's reserved `smelt_value`, and they are classified as prelude
    // regardless. An erased
    // local, an erased parameter, or a `Vec<SmeltUnknown>` beside the guard
    // stays avoidable, and an `instanceof` over a CONCRETE operand does not
    // reach here at all: it folds to a `true`/`false` literal naming no
    // `SmeltUnknown`. Proven by
    // `host_brand_instanceof_guard_is_a_boundary` below.
    boundary::MARKER_TEST,
    // `instanceof ErrorClass` against an erased operand — the sibling of
    // the rule directly above, and the case a `catch` binding IS. A
    // JavaScript error value carries its class in the `__smelt_error` brand
    // slot (`crates/smelt-codegen-rust/src/thrown.rs`), and
    // `error instanceof RangeError` is lowered to a guard that reads that
    // slot back and compares it against the class and its bases.
    //
    // TypeScript types a `catch` binding `unknown` by its own rule, so this
    // is not merely the idiomatic way a program identifies a thrown error —
    // it is the ONLY way, and the value it inspects arrives on the
    // exception channel whose payload ABI is already a boundary above
    // (`smelt_throw` / `smelt_thrown_value`, which carry a branded record
    // and a bare string down one `Result<_, Box<dyn Error>>` on a run-time
    // branch). No concrete type, generated union arm, or scoped generic can
    // carry that, and the class is recovered from the value rather than
    // from any static fact about it.
    //
    // Narrow like its sibling: the marker is the brand PROBE inside the
    // guard, so it matches this lowering and nothing else — an erased local
    // beside it stays avoidable, and an `instanceof` over a concrete class
    // receiver folds to a literal that names no `SmeltUnknown` at all.
    // Proven by `error_brand_instanceof_guard_is_a_boundary` below.
    boundary::ERROR_BRAND_TEST,
    // `instanceof UserClass` against an erased operand — the same guard for
    // a SOURCE class. An erased class instance records its class in the
    // hidden `__smelt_class` provenance marker (stamped by the struct
    // erasure), and `e instanceof UnsupportedPathError` is lowered to a
    // guard reading that marker back and comparing it against the class
    // and every generated class extending it
    // (`erased_user_class_instance_of_expr`). The canonical operand is
    // again a `catch` binding: `unknown` by TypeScript's own rule, arriving
    // on the exception channel, and narrowed by nothing but this runtime
    // test — no concrete type, union arm, or scoped generic can carry a
    // thrown value of a class the catch site does not name. Narrow like its
    // siblings: the marker is the probe inside the guard's own
    // `smelt_value` binding. Proven by `user_class_instanceof_guard_is_a_boundary`.
    boundary::CLASS_BRAND_TEST,
    // The write-through setter of a reference class's erased view
    // (`crates/smelt-codegen-rust/src/emitter/view_write_through.rs`). Its
    // `SmeltUnknown` parameter is the value an ERASED property write
    // (`this.match = m` with `this` erased) stored on the view: the write
    // site has only the erased carrier, and the setter is the adapter that
    // converts it back to the field's own declared type before assigning.
    // It is the inbound mirror of `into_smelt_unknown` -- a boundary
    // adapter by construction -- and no concrete type, union arm, or scoped
    // generic can carry "whatever an erased write stored under this key",
    // since one view receives writes for every field through one entry.
    // Proven by `view_write_through_setter_is_a_boundary`.
    boundary::SET_FIELD_FN,
    // The read mirror of the setter above
    // (`view_write_through::reference_callable_field_getter_method_text`):
    // the CURRENT value of a callable own field, erased, for a member read
    // through a snapshot view (a method call through an interface record
    // projected from an erased instance, whose field the callee may have
    // replaced since the view was built -- Hono's `SmartRouter.match`
    // rebinding itself). It answers a key chosen at runtime by the reader
    // with one of several differently typed fields through one entry, so
    // no concrete type, union arm, or scoped generic can carry the result;
    // it is the `into_smelt_unknown` of one field on demand. Proven by
    // `live_callable_field_getter_is_a_boundary`.
    boundary::GET_CALLABLE_FIELD_FN,
    // The live member read that asks that getter first (the prelude's
    // `smelt_live_member`), called from a receiver-bound record slot.
    boundary::LIVE_MEMBER_CALL,
];

/// Whether a line's `SmeltUnknown` usage sits at a genuine dynamic boundary.
///
/// See [`classify_line`] rule 2 for the rationale behind each marker.
fn is_legitimate_boundary_line(line: &str) -> bool {
    // A JavaScript update-expression (`x++`/`++x`) used as a value snapshots its
    // result into a `__smelt_update_tmp` local so the store can run inside the
    // current block without the enclosing expression re-reading the mutated
    // target. That temp's type is whatever the update target's type is; it is
    // `SmeltUnknown` *only* when the target is already an erased dynamic value
    // (e.g. an erased `unknown` callback parameter, as in `(v) => v++`). In that
    // case the temp is a boundary adapter carrying the dynamic value through JS's
    // prefix/postfix result semantics — a concrete type is unavailable precisely
    // because the underlying target is erased, so this is a genuine boundary, not
    // avoidable program-storage erasure. Match only the snapshot declaration
    // itself (not array-padding index uses that merely reference the temp).
    let trimmed = line.trim_start();
    if trimmed.starts_with("let __smelt_update_tmp") && trimmed.contains(": SmeltUnknown =") {
        return true;
    }

    BOUNDARY_MARKERS.iter().any(|marker| line.contains(marker))
}
