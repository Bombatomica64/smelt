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
//! constant is the literal Rust identifier of one runtime helper, except the
//! few that name the shared type, the slot prefix, and the record or marker keys
//! the helpers use. Codegen
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

    /// `async fn` the module body's LAST statement awaits before the program
    /// exits: the run-until-idle drain, then Node's ref'd-handle rule.
    ///
    /// Separate from [`SLEEP_MS`] because only the exit drain may wait on an
    /// open handle — a mid-program `await sleep(0)` that did would never return
    /// while a server was listening.
    pub const RUN_UNTIL_EXIT: &str = "smelt_run_until_exit";

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

    /// A byte-backed host record's OWN ENUMERABLE property values, or `None`
    /// for values that are not byte-backed.
    ///
    /// Own indexed properties exist for an ELEMENT-TYPED view and for nothing
    /// else: `Object.keys(new Uint8Array([1]))` is `['0']`, while
    /// `Object.keys(new ArrayBuffer(2))` and
    /// `Object.keys(new DataView(new ArrayBuffer(2)))` are both `[]` — byte
    /// storage and a `DataView` address bytes through accessors, so they have no
    /// indexed properties of their own. [`ELEMENTS`] cannot answer this: it is
    /// the ARRAY-LIKE face (iteration, spread, `Array.from`) and deliberately
    /// decodes byte storage as its bytes.
    ///
    /// Distinguishing the two faces is what makes `Object.keys`,
    /// `Object.values`, `for...in` and `JSON.stringify` agree with Node for
    /// every byte-backed shape; before this existed a buffer enumerated its
    /// bytes and a view serialized as `{}`.
    pub const OWN_ELEMENTS: &str = "smelt_host_buffer_own_elements";

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

    /// Build a byte-backed host record at a GIVEN JavaScript reference id.
    ///
    /// The one record builder both faces share. The erased face builds records
    /// with a fresh id; the concrete family's `IntoSmeltUnknown` adapters build
    /// the SAME record at the value's own id, so erasing one value twice
    /// produces two records that are `===`. Without one builder the two shapes
    /// drifted silently: the storage adapter omitted `length` and the view
    /// adapter omitted `buffer`/`byteOffset`, which a deep-equality comparison
    /// of an erased concrete value against an erased record then failed on.
    pub const VIEW_RECORD_WITH_ID: &str = "smelt_host_buffer_view_record_with_id";

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
/// The arity is therefore recorded on the callable's shared function-identity
/// token (`SmeltFunctionIdentity`, emitted by `callable_registry`), so it is read
/// back through the canonical identity and a chain of erasure wrappers resolves to
/// the arity of the function the chain started from.
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

/// Marker keys the runtime reads and writes on erased records.
///
/// A record's `__smelt_*` keys are how erased values carry what a type would say
/// statically: which host class a record is, the callable behind an object, a
/// `Date`'s time value. The runtime's text spells them literally, because it is
/// real Rust; the emitter names them here, and `every_runtime_marker_is_registered`
/// in `smelt-codegen-rust` holds the two to one list.
pub mod markers {
    /// `__smelt_abort_follows`.
    pub const SMELT_ABORT_FOLLOWS: &str = "__smelt_abort_follows";
    /// `__smelt_abort_listeners`.
    pub const SMELT_ABORT_LISTENERS: &str = "__smelt_abort_listeners";
    /// `__smelt_abortcontroller`.
    pub const SMELT_ABORTCONTROLLER: &str = "__smelt_abortcontroller";
    /// `__smelt_abortsignal`.
    pub const SMELT_ABORTSIGNAL: &str = "__smelt_abortsignal";
    /// `__smelt_arguments`.
    pub const SMELT_ARGUMENTS: &str = "__smelt_arguments";
    /// `__smelt_arraybuffer`.
    pub const SMELT_ARRAYBUFFER: &str = "__smelt_arraybuffer";
    /// `__smelt_asymmetric`.
    pub const SMELT_ASYMMETRIC: &str = "__smelt_asymmetric";
    /// `__smelt_bigint64array`.
    pub const SMELT_BIGINT64ARRAY: &str = "__smelt_bigint64array";
    /// `__smelt_biguint64array`.
    pub const SMELT_BIGUINT64ARRAY: &str = "__smelt_biguint64array";
    /// `__smelt_blob`.
    pub const SMELT_BLOB: &str = "__smelt_blob";
    /// `__smelt_blob_bytes`.
    pub const SMELT_BLOB_BYTES: &str = "__smelt_blob_bytes";
    /// `__smelt_boolean`.
    pub const SMELT_BOOLEAN: &str = "__smelt_boolean";
    /// `__smelt_buffer`.
    pub const SMELT_BUFFER: &str = "__smelt_buffer";
    /// `__smelt_builtin_namespace`.
    pub const SMELT_BUILTIN_NAMESPACE: &str = "__smelt_builtin_namespace";
    /// `__smelt_builtin_prototype`.
    pub const SMELT_BUILTIN_PROTOTYPE: &str = "__smelt_builtin_prototype";
    /// `__smelt_call`.
    pub const SMELT_CALL: &str = "__smelt_call";
    /// `__smelt_class`.
    pub const SMELT_CLASS: &str = "__smelt_class";
    /// `__smelt_crypto_key`.
    pub const SMELT_CRYPTO_KEY: &str = "__smelt_crypto_key";
    /// `__smelt_databasesync`.
    pub const SMELT_DATABASESYNC: &str = "__smelt_databasesync";
    /// `__smelt_dataview`.
    pub const SMELT_DATAVIEW: &str = "__smelt_dataview";
    /// `__smelt_date`.
    pub const SMELT_DATE: &str = "__smelt_date";
    /// `__smelt_domexception`.
    pub const SMELT_DOMEXCEPTION: &str = "__smelt_domexception";
    /// `__smelt_error`.
    pub const SMELT_ERROR: &str = "__smelt_error";
    /// `__smelt_eventemitter`.
    pub const SMELT_EVENTEMITTER: &str = "__smelt_eventemitter";
    /// `__smelt_file`.
    pub const SMELT_FILE: &str = "__smelt_file";
    /// `__smelt_float32array`.
    pub const SMELT_FLOAT32ARRAY: &str = "__smelt_float32array";
    /// `__smelt_float64array`.
    pub const SMELT_FLOAT64ARRAY: &str = "__smelt_float64array";
    /// `__smelt_formdata`.
    pub const SMELT_FORMDATA: &str = "__smelt_formdata";
    /// `__smelt_generator`.
    pub const SMELT_GENERATOR: &str = "__smelt_generator";
    /// `__smelt_get`.
    pub const SMELT_GET: &str = "__smelt_get";
    /// `__smelt_get_callable_field`.
    pub const SMELT_GET_CALLABLE_FIELD: &str = "__smelt_get_callable_field";
    /// `__smelt_global_object`.
    pub const SMELT_GLOBAL_OBJECT: &str = "__smelt_global_object";
    /// `__smelt_hash`.
    pub const SMELT_HASH: &str = "__smelt_hash";
    /// `__smelt_headers`.
    pub const SMELT_HEADERS: &str = "__smelt_headers";
    /// `__smelt_httpserver`.
    pub const SMELT_HTTPSERVER: &str = "__smelt_httpserver";
    /// `__smelt_incomingmessage`.
    pub const SMELT_INCOMINGMESSAGE: &str = "__smelt_incomingmessage";
    /// `__smelt_int16array`.
    pub const SMELT_INT16ARRAY: &str = "__smelt_int16array";
    /// `__smelt_int32array`.
    pub const SMELT_INT32ARRAY: &str = "__smelt_int32array";
    /// `__smelt_int8array`.
    pub const SMELT_INT8ARRAY: &str = "__smelt_int8array";
    /// `__smelt_intl_collator`.
    pub const SMELT_INTL_COLLATOR: &str = "__smelt_intl_collator";
    /// `__smelt_intl_displaynames`.
    pub const SMELT_INTL_DISPLAYNAMES: &str = "__smelt_intl_displaynames";
    /// `__smelt_intl_durationformat`.
    pub const SMELT_INTL_DURATIONFORMAT: &str = "__smelt_intl_durationformat";
    /// `__smelt_intl_listformat`.
    pub const SMELT_INTL_LISTFORMAT: &str = "__smelt_intl_listformat";
    /// `__smelt_intl_locale`.
    pub const SMELT_INTL_LOCALE: &str = "__smelt_intl_locale";
    /// `__smelt_intl_numberformat`.
    pub const SMELT_INTL_NUMBERFORMAT: &str = "__smelt_intl_numberformat";
    /// `__smelt_intl_pluralrules`.
    pub const SMELT_INTL_PLURALRULES: &str = "__smelt_intl_pluralrules";
    /// `__smelt_intl_segmenter`.
    pub const SMELT_INTL_SEGMENTER: &str = "__smelt_intl_segmenter";
    /// `__smelt_map`.
    pub const SMELT_MAP: &str = "__smelt_map";
    /// `__smelt_method`.
    pub const SMELT_METHOD: &str = "__smelt_method";
    /// `__smelt_native_ctor`.
    pub const SMELT_NATIVE_CTOR: &str = "__smelt_native_ctor";
    /// `__smelt_number`.
    pub const SMELT_NUMBER: &str = "__smelt_number";
    /// `__smelt_proto`.
    pub const SMELT_PROTO: &str = "__smelt_proto";
    /// `__smelt_readablestream`.
    pub const SMELT_READABLESTREAM: &str = "__smelt_readablestream";
    /// `__smelt_regexp`.
    pub const SMELT_REGEXP: &str = "__smelt_regexp";
    /// `__smelt_request`.
    pub const SMELT_REQUEST: &str = "__smelt_request";
    /// `__smelt_response`.
    pub const SMELT_RESPONSE: &str = "__smelt_response";
    /// `__smelt_serverresponse`.
    pub const SMELT_SERVERRESPONSE: &str = "__smelt_serverresponse";
    /// `__smelt_set`.
    pub const SMELT_SET: &str = "__smelt_set";
    /// `__smelt_set_field`.
    pub const SMELT_SET_FIELD: &str = "__smelt_set_field";
    /// `__smelt_sharedarraybuffer`.
    pub const SMELT_SHAREDARRAYBUFFER: &str = "__smelt_sharedarraybuffer";
    /// `__smelt_statementsync`.
    pub const SMELT_STATEMENTSYNC: &str = "__smelt_statementsync";
    /// `__smelt_string`.
    pub const SMELT_STRING: &str = "__smelt_string";
    /// `__smelt_symbol`.
    pub const SMELT_SYMBOL: &str = "__smelt_symbol";
    /// `__smelt_symbol_async_iterator`.
    pub const SMELT_SYMBOL_ASYNC_ITERATOR: &str = "__smelt_symbol_async_iterator";
    /// `__smelt_symbol_has_instance`.
    pub const SMELT_SYMBOL_HAS_INSTANCE: &str = "__smelt_symbol_has_instance";
    /// `__smelt_symbol_is_concat_spreadable`.
    pub const SMELT_SYMBOL_IS_CONCAT_SPREADABLE: &str = "__smelt_symbol_is_concat_spreadable";
    /// `__smelt_symbol_iterator`.
    pub const SMELT_SYMBOL_ITERATOR: &str = "__smelt_symbol_iterator";
    /// `__smelt_symbol_match`.
    pub const SMELT_SYMBOL_MATCH: &str = "__smelt_symbol_match";
    /// `__smelt_symbol_match_all`.
    pub const SMELT_SYMBOL_MATCH_ALL: &str = "__smelt_symbol_match_all";
    /// `__smelt_symbol_replace`.
    pub const SMELT_SYMBOL_REPLACE: &str = "__smelt_symbol_replace";
    /// `__smelt_symbol_search`.
    pub const SMELT_SYMBOL_SEARCH: &str = "__smelt_symbol_search";
    /// `__smelt_symbol_species`.
    pub const SMELT_SYMBOL_SPECIES: &str = "__smelt_symbol_species";
    /// `__smelt_symbol_split`.
    pub const SMELT_SYMBOL_SPLIT: &str = "__smelt_symbol_split";
    /// `__smelt_symbol_to_primitive`.
    pub const SMELT_SYMBOL_TO_PRIMITIVE: &str = "__smelt_symbol_to_primitive";
    /// `__smelt_symbol_to_string_tag`.
    pub const SMELT_SYMBOL_TO_STRING_TAG: &str = "__smelt_symbol_to_string_tag";
    /// `__smelt_symbol_unscopables`.
    pub const SMELT_SYMBOL_UNSCOPABLES: &str = "__smelt_symbol_unscopables";
    /// `__smelt_textdecoder`.
    pub const SMELT_TEXTDECODER: &str = "__smelt_textdecoder";
    /// `__smelt_textencoder`.
    pub const SMELT_TEXTENCODER: &str = "__smelt_textencoder";
    /// `__smelt_timezone`.
    pub const SMELT_TIMEZONE: &str = "__smelt_timezone";
    /// `__smelt_uint16array`.
    pub const SMELT_UINT16ARRAY: &str = "__smelt_uint16array";
    /// `__smelt_uint32array`.
    pub const SMELT_UINT32ARRAY: &str = "__smelt_uint32array";
    /// `__smelt_uint8array`.
    pub const SMELT_UINT8ARRAY: &str = "__smelt_uint8array";
    /// `__smelt_uint8clampedarray`.
    pub const SMELT_UINT8CLAMPEDARRAY: &str = "__smelt_uint8clampedarray";
    /// `__smelt_url`.
    pub const SMELT_URL: &str = "__smelt_url";
    /// `__smelt_urlsearchparams`.
    pub const SMELT_URLSEARCHPARAMS: &str = "__smelt_urlsearchparams";
    /// `__smelt_vitest_mock`.
    pub const SMELT_VITEST_MOCK: &str = "__smelt_vitest_mock";
    /// `__smelt_weakmap`.
    pub const SMELT_WEAKMAP: &str = "__smelt_weakmap";
    /// `__smelt_weakset`.
    pub const SMELT_WEAKSET: &str = "__smelt_weakset";

    /// Every key above.
    pub const ALL: &[&str] = &[
        SMELT_ABORT_FOLLOWS,
        SMELT_ABORT_LISTENERS,
        SMELT_ABORTCONTROLLER,
        SMELT_ABORTSIGNAL,
        SMELT_ARGUMENTS,
        SMELT_ARRAYBUFFER,
        SMELT_ASYMMETRIC,
        SMELT_BIGINT64ARRAY,
        SMELT_BIGUINT64ARRAY,
        SMELT_BLOB,
        SMELT_BLOB_BYTES,
        SMELT_BOOLEAN,
        SMELT_BUFFER,
        SMELT_BUILTIN_NAMESPACE,
        SMELT_BUILTIN_PROTOTYPE,
        SMELT_CALL,
        SMELT_CLASS,
        SMELT_CRYPTO_KEY,
        SMELT_DATABASESYNC,
        SMELT_DATAVIEW,
        SMELT_DATE,
        SMELT_DOMEXCEPTION,
        SMELT_ERROR,
        SMELT_EVENTEMITTER,
        SMELT_FILE,
        SMELT_FLOAT32ARRAY,
        SMELT_FLOAT64ARRAY,
        SMELT_FORMDATA,
        SMELT_GENERATOR,
        SMELT_GET,
        SMELT_GET_CALLABLE_FIELD,
        SMELT_GLOBAL_OBJECT,
        SMELT_HASH,
        SMELT_HEADERS,
        SMELT_HTTPSERVER,
        SMELT_INCOMINGMESSAGE,
        SMELT_INT16ARRAY,
        SMELT_INT32ARRAY,
        SMELT_INT8ARRAY,
        SMELT_INTL_COLLATOR,
        SMELT_INTL_DISPLAYNAMES,
        SMELT_INTL_DURATIONFORMAT,
        SMELT_INTL_LISTFORMAT,
        SMELT_INTL_LOCALE,
        SMELT_INTL_NUMBERFORMAT,
        SMELT_INTL_PLURALRULES,
        SMELT_INTL_SEGMENTER,
        SMELT_MAP,
        SMELT_METHOD,
        SMELT_NATIVE_CTOR,
        SMELT_NUMBER,
        SMELT_PROTO,
        SMELT_READABLESTREAM,
        SMELT_REGEXP,
        SMELT_REQUEST,
        SMELT_RESPONSE,
        SMELT_SERVERRESPONSE,
        SMELT_SET,
        SMELT_SET_FIELD,
        SMELT_SHAREDARRAYBUFFER,
        SMELT_STATEMENTSYNC,
        SMELT_STRING,
        SMELT_SYMBOL,
        SMELT_SYMBOL_ASYNC_ITERATOR,
        SMELT_SYMBOL_HAS_INSTANCE,
        SMELT_SYMBOL_IS_CONCAT_SPREADABLE,
        SMELT_SYMBOL_ITERATOR,
        SMELT_SYMBOL_MATCH,
        SMELT_SYMBOL_MATCH_ALL,
        SMELT_SYMBOL_REPLACE,
        SMELT_SYMBOL_SEARCH,
        SMELT_SYMBOL_SPECIES,
        SMELT_SYMBOL_SPLIT,
        SMELT_SYMBOL_TO_PRIMITIVE,
        SMELT_SYMBOL_TO_STRING_TAG,
        SMELT_SYMBOL_UNSCOPABLES,
        SMELT_TEXTDECODER,
        SMELT_TEXTENCODER,
        SMELT_TIMEZONE,
        SMELT_UINT16ARRAY,
        SMELT_UINT32ARRAY,
        SMELT_UINT8ARRAY,
        SMELT_UINT8CLAMPEDARRAY,
        SMELT_URL,
        SMELT_URLSEARCHPARAMS,
        SMELT_VITEST_MOCK,
        SMELT_WEAKMAP,
        SMELT_WEAKSET,
    ];
}

/// The spellings `smelt smelt-unknown-report` treats as a genuine dynamic boundary.
///
/// A generated line holding one of these is classified as a legitimate use of the
/// erased carrier. Why each one is a real boundary, and the test proving a concrete
/// type cannot replace it, is recorded where the report lists them. They live here
/// because the report reads text the emitter writes and the runtime defines, and a
/// rename on either side must not silently change what the report counts.
pub mod boundary {
    /// An erased callable.
    pub const FUNCTION_TAG: &str = "SmeltUnknown::Function";
    /// An erased promise.
    pub const PROMISE_TAG: &str = "SmeltUnknown::Promise";
    /// An erased symbol value.
    pub const SYMBOL_TAG: &str = "SmeltUnknown::Symbol";
    /// A symbol recovered from its property key.
    pub const OWN_SYMBOL_KEY_VALUE: &str = "smelt_own_symbol_key_value";
    /// `JSON.parse`.
    pub const JSON_PARSE_CALL: &str = "smelt_json_parse(";
    /// A `node:sqlite` multi-row read.
    pub const SQLITE_ALL_CALL: &str = "smelt_sqlite_all(";
    /// A `node:sqlite` single-row read.
    pub const SQLITE_GET_CALL: &str = "smelt_sqlite_get(";
    /// The erasure trait.
    pub const INTO_TRAIT: &str = "IntoSmeltUnknown";
    /// The erasure trait's method.
    pub const INTO_METHOD: &str = "into_smelt_unknown";
    /// The by-reference erasure method.
    pub const TO_METHOD: &str = "to_smelt_unknown";
    /// The `typeof` type-test method.
    pub const JS_TYPEOF: &str = "js_typeof";
    /// The `typeof` helper.
    pub const TYPEOF_CALL: &str = "smelt_typeof(";
    /// A runtime tag narrowing.
    pub const TAG_CHECK: &str = "tag_check";
    /// The `smelt_unknown_is_*` tag predicates.
    pub const UNKNOWN_IS_PREFIX: &str = "smelt_unknown_is_";
    /// A throw.
    pub const THROW_CALL: &str = "smelt_throw(";
    /// A caught value.
    pub const THROWN_VALUE_CALL: &str = "smelt_thrown_value(";
    /// A thrown string.
    pub const THROWN_STRING: &str = "SmeltUnknown::String(__smelt_error";
    /// An erased slice value.
    pub const SLICE_VALUE: &str = "smelt_slice_value";
    /// A callable object's slot.
    pub const CALL_SLOT: &str = "__smelt_call";
    /// Installing a dynamic `this`.
    pub const PUSH_THIS_CALL: &str = "smelt_push_this(";
    /// Installing a lazily built dynamic `this`.
    pub const PUSH_THIS_LAZY_CALL: &str = "smelt_push_this_lazy(";
    /// Reading the dynamic `this`.
    pub const THIS_CALL: &str = "smelt_this()";
    /// Binding a receiver.
    pub const BIND_THIS_CALL: &str = "smelt_bind_this(";
    /// A class's prototype entries, as the generated local.
    pub const PROTO_ENTRIES_LOCAL: &str = "__smelt_proto_entries";
    /// A class's prototype entries.
    pub const PROTO_ENTRIES: &str = "smelt_proto_entries";
    /// An erased member read.
    pub const GET_UNKNOWN_FIELD_CALL: &str = "smelt_get_unknown_field(";
    /// `globalThis`.
    pub const GLOBAL_OBJECT_CALL: &str = "smelt_global_object(";
    /// A write through an erased target.
    pub const UNKNOWN_ASSIGN_CALL: &str = "smelt_unknown_assign(";
    /// An erased `Error.cause` field.
    pub const ERROR_CAUSE_FIELD: &str = "cause: SmeltUnknown";
    /// `JSON.stringify` through the carrier.
    pub const JSON_TO_STRING: &str = "serde_json::to_string(&";
    /// A generated conversion testing for a marker.
    pub const MARKER_TEST: &str = "if value.contains_key(\"__smelt_";
    /// A generated conversion testing an error brand.
    pub const ERROR_BRAND_TEST: &str = "if matches!(value.get(\"__smelt_error\")";
    /// A generated conversion testing a class brand.
    pub const CLASS_BRAND_TEST: &str = "if matches!(value.get(\"__smelt_class\")";
    /// A class's generated keyed field write.
    pub const SET_FIELD_FN: &str = "fn __smelt_set_field(";
    /// A class's generated keyed callable read.
    pub const GET_CALLABLE_FIELD_FN: &str = "fn __smelt_get_callable_field(";
    /// A live member read off an erased view.
    pub const LIVE_MEMBER_CALL: &str = "smelt_live_member(";
}
