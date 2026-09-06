//! The codegen↔runtime helper symbol contract.
//!
//! The Rust backend (`smelt-codegen-rust`) emits generated programs that call a
//! fixed set of runtime helper functions. Those helpers are themselves emitted
//! into every generated crate as a prelude (see `lib.rs` in the codegen crate).
//! Both halves — the *definition* of a helper in the prelude and every *call
//! site* the expression emitter writes — must agree on the exact symbol name.
//! When they drift apart the mismatch is invisible until the generated crate
//! reaches `rustc`, far from the codegen change that caused it.
//!
//! This module is the single, enumerable source of truth for those names. Each
//! constant is the literal Rust identifier of one runtime helper. Codegen
//! references these constants instead of inlining string literals, so renaming
//! a helper is a one-line edit here that updates the prelude definition and all
//! call sites at once.
//!
//! Scope: this table covers only the *fixed* runtime helper set — symbols whose
//! names are baked into the runtime prelude and shared with the emitter. It
//! intentionally does **not** cover per-program or generated identifiers (e.g.
//! `smelt_object`, `smelt_args`, locals like `smelt_callback`), which are
//! synthesized per emission and are not part of the cross-crate contract.

/// Timer and microtask-queue runtime helpers.
///
/// These back the JavaScript timer surface (`setTimeout`/`clearTimeout`),
/// `await`-based sleeping, and the cooperative promise-task pump that the async
/// lowering drives. They are emitted into the prelude and called from the
/// async-op and statement emitters.
pub mod timers {
    /// `async fn` that advances virtual time and drains pending timers/tasks;
    /// backs `await sleep(...)` and the Promise busy-wait loop.
    pub const SLEEP_MS: &str = "smelt_sleep_ms";

    /// Registers a timer callback with a delay; backs `setTimeout`.
    pub const SET_TIMEOUT: &str = "smelt_set_timeout";

    /// Cancels a previously registered timer by handle; backs `clearTimeout`.
    pub const CLEAR_TIMEOUT: &str = "smelt_clear_timeout";

    /// Registers a repeating timer callback with a period; backs `setInterval`.
    ///
    /// The callback re-arms itself for the next period each time it fires, so the
    /// existing virtual-time timer queue drives it without special-casing.
    pub const SET_INTERVAL: &str = "smelt_set_interval";

    /// Cancels a previously registered repeating timer by handle; backs
    /// `clearInterval`. Intervals share the timer queue with timeouts, so this is
    /// the same cancel-by-id operation as `clearTimeout`.
    pub const CLEAR_INTERVAL: &str = "smelt_clear_interval";

    /// Resets all timer/promise-queue thread-local state; emitted at the start
    /// of generated `main`/entry wrappers so each run starts clean.
    pub const RESET_TIMERS: &str = "smelt_reset_timers";

    /// Pushes a detached future onto the cooperative promise-task queue; backs
    /// fire-and-forget async calls.
    pub const SPAWN_PROMISE_TASK: &str = "smelt_spawn_promise_task";

    /// Drains queued promise tasks by polling them to completion. Defined in
    /// the prelude and referenced only by other prelude helpers.
    pub const DRAIN_PROMISE_TASKS: &str = "smelt_drain_promise_tasks";

    /// Fires all timers whose due time has elapsed. Defined in the prelude and
    /// referenced only by other prelude helpers.
    pub const DRAIN_DUE_TIMERS: &str = "smelt_drain_due_timers";

    /// Builds a no-op `Waker` used to poll detached futures. Defined in the
    /// prelude and referenced only by other prelude helpers.
    pub const NOOP_WAKER: &str = "smelt_noop_waker";

    /// Settles with the racer that finishes FIRST; backs `Promise.race`.
    ///
    /// `tokio::select!` cannot back `Promise.race` on the virtual clock: it
    /// polls its branches in a randomized order and returns whichever branch
    /// first reports `Ready` in a poll round, while each generated promise
    /// spin-loop advances the virtual clock by one timer step per poll. Two
    /// racers therefore settle in the same round and the winner is a coin flip.
    /// This helper polls the racers in source order instead, so the racer whose
    /// timer fired earlier is observed first.
    pub const PROMISE_RACE: &str = "smelt_promise_race";
}

/// JSON / dynamic-`unknown` boundary helpers.
///
/// These bridge `serde_json` values and the runtime's tagged `SmeltUnknown`
/// dynamic value, used at JSON parse boundaries.
pub mod json {
    /// Recursively converts a `serde_json::Value` into a `SmeltUnknown`; backs
    /// `JSON.parse` and other JSON-ingest boundaries.
    pub const UNKNOWN_FROM_JSON_VALUE: &str = "smelt_unknown_from_json_value";
}

/// String runtime helpers.
///
/// These back JavaScript global string functions that need more than a direct
/// Rust std method call.
pub mod strings {
    /// Percent-encodes a string; backs `encodeURI(value)`
    /// (`Rvalue::UriTranscode` with `UriTranscodeOp::Encode`).
    ///
    /// The ECMA-262 `encodeURI` character set stays literal (ASCII
    /// alphanumerics, unreserved marks, URI reserved separators, and `#`);
    /// everything else becomes uppercase `%XX` UTF-8 triplets.
    pub const ENCODE_URI: &str = "smelt_encode_uri";

    /// Percent-encodes one URI component; backs `encodeURIComponent(value)`
    /// (`UriTranscodeOp::EncodeComponent`).
    ///
    /// Differs from [`ENCODE_URI`] by escaping the URI reserved separators
    /// `; / ? : @ & = + $ , #` as well, so the result cannot be reparsed as
    /// structure.
    pub const ENCODE_URI_COMPONENT: &str = "smelt_encode_uri_component";

    /// The shared percent-decoder both decoders call
    /// (`smelt_decode_uri` / `smelt_decode_uri_component`).
    ///
    /// Parameterized by the character set to leave escaped, and returns `None`
    /// for exactly the input ECMA-262 rejects with a `URIError`.
    pub const DECODE_URI_OCTETS: &str = "smelt_decode_uri_octets";

    /// Percent-decodes a full URI; backs `decodeURI(value)`
    /// (`UriTranscodeOp::Decode`). Leaves the URI reserved separators escaped.
    pub const DECODE_URI: &str = "smelt_decode_uri";

    /// Percent-decodes one URI component; backs `decodeURIComponent(value)`
    /// (`UriTranscodeOp::DecodeComponent`). Decodes every escape.
    pub const DECODE_URI_COMPONENT: &str = "smelt_decode_uri_component";

    /// Collates two strings; backs `a.localeCompare(b)`
    /// (`Rvalue::StringLocaleCompare`).
    ///
    /// Models the primary (case-folded) and tertiary (lowercase-first) levels
    /// of the Unicode root collation; locales and `Intl.Collator` options are
    /// not modeled.
    pub const LOCALE_COMPARE: &str = "smelt_locale_compare";
}

/// Host-object construction helpers.
///
/// These build the marker records that model JavaScript host builtins (see
/// `host_object.rs` for the marker registry itself).
pub mod host {
    /// Builds the modeled `Blob`/`File` record from its `BlobPart` contents.
    ///
    /// Backs `new Blob(parts?, options?)` and `new File(parts, name, options?)`
    /// (`Rvalue::BlobFromParts`). Concatenates part contents, stores the UTF-8
    /// byte `size`, and stamps `__smelt_file` on top of `__smelt_blob` when a
    /// file name is supplied.
    pub const BLOB_RECORD_FROM_PARTS: &str = "smelt_blob_record_from_parts";

    /// Interns the single host-constructor/namespace value for a global name.
    ///
    /// JavaScript exposes exactly one object per global builtin name, so
    /// `Blob === Blob` and `blob.constructor === Blob` both hold. Smelt models
    /// such a bare reference as a `__smelt_builtin_namespace` record, and records
    /// mint a fresh identity on construction — two references would compare
    /// `!==`. This helper caches one record per name in a thread-local so the
    /// identity is stable, and it is the value `.constructor` resolves to for a
    /// host-marker record (`Rvalue::BuiltinNamespace`).
    pub const BUILTIN_NAMESPACE: &str = "smelt_builtin_namespace";

    /// Builds a modeled host instance from constructor arguments, keyed by the
    /// host identity's *kind* (its marker with the `__smelt_` prefix stripped).
    ///
    /// One helper serves both the direct `new DataView(buf, 1, 2)` spelling
    /// (`Rvalue::HostConstruct`) and the reflected
    /// `new Object.getPrototypeOf(view).constructor(...)` spelling, so records
    /// built the two ways are indistinguishable.
    pub const REFLECTED_CONSTRUCT: &str = "smelt_reflected_construct";

    /// Maps a marker-bearing record to the class name whose constructor value its
    /// `.constructor` read resolves to.
    pub const MARKER_CONSTRUCTOR_CLASS: &str = "smelt_marker_constructor_class";

    /// Builds the array-like `arguments` exotic object from a function's
    /// positional parameters plus its flattened rest list
    /// (`Rvalue::ArgumentsObject`).
    pub const ARGUMENTS_OBJECT: &str = "smelt_arguments_object";

    /// Extracts an `arguments` object's elements as a plain element vector, or
    /// `None` when the value carries no [`ARGUMENTS_MARKER`].
    ///
    /// A JavaScript `arguments` object is iterable — its `Symbol.iterator` is
    /// `Array.prototype.values` — so `Array.from(arguments)`, `[...arguments]`
    /// and `for (const a of arguments)` all walk its elements. Smelt models it as
    /// an array-like marker record with index keys and a hidden `length`, which
    /// carries no `__smelt_symbol_iterator` slot, so the erased iterable-to-list
    /// coercion used to reject it outright. This helper is what makes the marker
    /// record iterable, reading `length` and the index keys rather than the
    /// record's raw key order.
    ///
    /// Deliberately keyed on the marker rather than on "has a `length`": a bare
    /// array-like object is accepted by `Array.from` but is NOT iterable, and one
    /// emitter serves both spellings, so widening this to any `length`-bearing
    /// record would make `[...{ length: 0 }]` succeed where JavaScript throws.
    pub const ARGUMENTS_ELEMENTS: &str = "smelt_arguments_elements";

    /// The identity marker stamped onto an `arguments` object.
    ///
    /// Unlike the [`crate::host_object::HOST_OBJECTS`] markers — which hide a
    /// record's *whole* key set from enumeration — this marker hides only itself
    /// and `length`, because an `arguments` object's indexed elements are
    /// genuinely enumerable own properties while its `length` is not.
    pub const ARGUMENTS_MARKER: &str = "__smelt_arguments";
}

/// Byte-buffer host-object runtime helpers.
///
/// The binary-data host objects (`ArrayBuffer`, `SharedArrayBuffer`, Node
/// `Buffer`, `DataView` — see [`crate::host_object::ByteBufferRole`]) are modeled
/// as marker records carrying a `bytes` list. These helpers are the single place
/// that knows that layout, so `.slice()`/`.subarray()`, indexed element
/// reads/writes, array-like element extraction, and `ArrayBuffer.isView` all
/// agree on it.
pub mod byte_buffer {
    /// Returns a byte-backed host record's `bytes` as an element vector, or
    /// `None` when the value is not a byte-backed host object.
    ///
    /// Backs `new Uint8Array(arrayBuffer)` and any other array-like extraction
    /// from a byte buffer.
    pub const ELEMENTS: &str = "smelt_host_buffer_elements";

    /// Slices a byte-backed host record, returning a **fresh** record of the same
    /// host identity over the sliced bytes, or `None` for other values.
    ///
    /// Backs `arrayBuffer.slice(0)` and `buffer.subarray()`, both of which
    /// es-toolkit's clone paths rely on to produce a distinct object.
    pub const SLICE: &str = "smelt_host_buffer_slice";

    /// Reads one indexed element of a byte-backed host record (`buffer[1]`), or
    /// `None` when the receiver is not byte-backed or the key is not an index.
    pub const ELEMENT: &str = "smelt_host_buffer_element";

    /// Writes one indexed element of a byte-backed host record
    /// (`result[i] = byte`), returning whether the write was absorbed.
    pub const SET_ELEMENT: &str = "smelt_host_buffer_set_element";

    /// Whether an erased value is a *view* over byte storage; backs
    /// `ArrayBuffer.isView(x)`.
    pub const IS_VIEW: &str = "smelt_host_buffer_is_view";

    /// Builds a byte-backed host record of a given marker identity from
    /// JavaScript constructor arguments (`new Float32Array(buffer, 8, 1)`).
    ///
    /// The one construction path shared by the direct `new X(...)` lowering and
    /// the reflected `new getPrototypeOf(x).constructor(...)` path. It is where
    /// the element type decides whether a source buffer is *viewed* byte-for-byte
    /// or *converted* element-by-element.
    pub const CONSTRUCT: &str = "smelt_host_buffer_construct";

    /// A byte-backed host record's own enumerable keys — its element indices — or
    /// `None` for values that are not byte-backed.
    ///
    /// A typed array's own properties are its indexed elements; `length`,
    /// `byteLength`, `byteOffset` and `buffer` are all accessors on the
    /// prototype and never enumerate. es-toolkit's `keys(new Uint8Array(1))`
    /// asserts exactly `['0']`.
    pub const INDEX_KEYS: &str = "smelt_host_buffer_index_keys";

    /// [`INDEX_KEYS`] for a byte-backed record reached through the structural
    /// `SmeltRecord<String, SmeltUnknown>` ABI instead of a tagged `SmeltUnknown`.
    pub const RECORD_INDEX_KEYS: &str = "smelt_host_buffer_record_index_keys";

    /// [`ELEMENTS`] for a byte-backed record reached through the structural
    /// `SmeltRecord<String, SmeltUnknown>` ABI instead of a tagged `SmeltUnknown`.
    ///
    /// Backs `Object.values` / `Object.entries` over a view, which must answer its
    /// decoded elements rather than its internal storage fields.
    pub const RECORD_ELEMENTS: &str = "smelt_host_buffer_record_elements";

    /// The record key holding a byte-backed host object's storage.
    pub const BYTES_KEY: &str = "bytes";
}

/// Host-global override-slot runtime helpers.
///
/// These back the bounded whole-global reassignment of modeled host
/// constructors (`globalThis.File = ...`, `globalThis.Blob = undefined`,
/// save/restore). The generated crate emits one `thread_local!` slot per host
/// name the crate actually writes (`SMELT_HOST_OVERRIDE_<NAME>`), initialized to
/// the fixed `SmeltHostOverride::Native` state, plus the fixed enum and the
/// three helpers named here. See the `HostGlobalRead`/`HostGlobalWrite`/
/// `HostGlobalPresent` MIR rvalues.
///
/// Per-test-thread semantics: each `#[test]` runs on its own thread and gets a
/// fresh `Native` slot, matching the specs' save/restore discipline (they
/// snapshot the native handle, override the slot, then restore it within one
/// test).
pub mod host_override {
    /// Name of the fixed runtime enum modeling a host constructor's override
    /// state: `Native` (unmodified), `Absent` (set to `undefined`), or
    /// `Ctor(SmeltUnknown)` (reassigned to a constructor value).
    pub const OVERRIDE_ENUM: &str = "SmeltHostOverride";

    /// Prefix of the per-name `thread_local!` override slot
    /// (`SMELT_HOST_OVERRIDE_<NAME>`). The suffix is the upper-cased host
    /// constructor name.
    pub const SLOT_PREFIX: &str = "SMELT_HOST_OVERRIDE_";

    /// Read helper: returns the override state as a value. `Native` yields the
    /// native-handle marker record, `Absent` yields JS `undefined`, `Ctor(v)`
    /// yields the stored constructor value.
    pub const READ: &str = "smelt_host_override_read";

    /// Write helper: classifies the stored value into a slot state (`undefined`
    /// → `Absent`; native-handle marker → `Native`; function/class value →
    /// `Ctor`) and returns the stored value.
    pub const WRITE: &str = "smelt_host_override_write";

    /// Presence helper: `false` only when the slot is `Absent`.
    pub const PRESENT: &str = "smelt_host_override_present";

    /// The identity marker key stamped onto the native-handle record produced by
    /// reading a `Native` slot. Its presence classifies a written-back value as
    /// a restore-to-`Native`.
    pub const NATIVE_CTOR_MARKER: &str = "__smelt_native_ctor";
}

/// `Function.prototype.length` across the erasure boundary.
///
/// A typed callable knows its own arity, and `SmeltErasedFunction` carries it in a
/// `length` field. Erasing that value to `SmeltUnknown::Function(Rc<…>)` throws the
/// field away — an `Rc<dyn Fn>` has nowhere to put it — so a `.length` read on an
/// erased callable answered `0`. Real code branches on it: es-toolkit `rest(func)`
/// defaults its split point to `func.length - 1`, and `ary(func)` to `func.length`.
///
/// The arity is therefore recorded in a thread-local registry keyed by the erased
/// callable's allocation address, in the same shape as the sibling
/// `SMELT_FUNCTION_IDENTITIES` / `SMELT_FUNCTION_ORIGINS` registries, and read back
/// through the canonical identity so a chain of erasure wrappers resolves to the
/// arity of the function the chain started from.
pub mod function_length {
    /// Records an erased callable's source arity at the erasure site.
    pub const REGISTER: &str = "smelt_register_function_length";

    /// Reads an erased value's `Function.prototype.length`, or `0` for a
    /// non-callable — which is also what JavaScript answers for a value with no
    /// `length` property.
    pub const READ: &str = "smelt_function_length";
}

/// JavaScript numeric-semantics runtime helpers.
///
/// Rust's `f64` inherent methods and JavaScript's `Math` functions agree almost
/// everywhere, so the emitter maps most of them straight through. These are the
/// ones where the two languages genuinely disagree and a helper has to carry the
/// JavaScript rule.
pub mod math {
    /// JavaScript `Math.round`: ties round toward **+∞**, not away from zero.
    ///
    /// `Math.round(-1.5)` is `-1` in JavaScript; Rust's `f64::round` answers
    /// `-2.0`, because it rounds half away from zero. The two differ for every
    /// negative value whose fractional part is exactly `0.5`, which is what made
    /// es-toolkit's `round` specs disagree.
    ///
    /// Computed as `floor(x)` plus one when the fraction reaches `0.5`, rather
    /// than as `floor(x + 0.5)`: the ECMA-262 note on `Math.round` calls out that
    /// the naive form is wrong for very large `x`, where adding `0.5` is not
    /// representable. `floor` is exact at those magnitudes and the fraction is
    /// then `0`, so the value passes through unchanged.
    pub const ROUND: &str = "smelt_math_round";
}
