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
    /// Percent-encodes a string; backs `encodeURI(value)` (`Rvalue::UriEncode`).
    ///
    /// The ECMA-262 `encodeURI` character set stays literal (ASCII
    /// alphanumerics, unreserved marks, URI reserved separators, and `#`);
    /// everything else becomes uppercase `%XX` UTF-8 triplets.
    pub const ENCODE_URI: &str = "smelt_encode_uri";
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
