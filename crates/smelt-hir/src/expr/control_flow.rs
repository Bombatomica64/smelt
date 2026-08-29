//! Async control-flow expression operations.

use serde::{Deserialize, Serialize};

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
    /// Produce an already-settled promise of a value (`Promise.resolve(v)`).
    ///
    /// Operands are `[duration, value?]`: the duration models JavaScript's
    /// microtask deferral (`Promise.resolve` never resolves synchronously) and
    /// the optional value is the resolution value. `Sleep` cannot stand in for
    /// this — it produces `Future<()>`, so the resolution value is lost and the
    /// `Future<()>` -> `Future<T>` coercion has to invent a `T`.
    Resolve,
    /// Schedule a callback to run after a duration in milliseconds.
    SetTimeout,
    /// Cancel a scheduled timeout callback.
    ClearTimeout,
    /// Schedule a callback to run repeatedly every N milliseconds.
    SetInterval,
    /// Cancel a scheduled repeating-interval callback.
    ClearInterval,
    /// Create a future by executing a JavaScript `Promise` executor.
    Promise,
    /// Run a Promise success continuation.
    Then,
    /// Run a Promise rejection continuation.
    Catch,
    /// Schedule an ignored future for cooperative local execution.
    SpawnLocal,
    /// Create a task from a future.
    CreateTask,
    /// Wait for a future with a timeout.
    WaitFor,
    /// Perform an HTTP GET request and return the response body as text.
    HttpGetText,
}
