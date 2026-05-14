//! Recognition layer for Python `asyncio` APIs.
//!
//! This crate intentionally does not know about Tokio or Rust code generation.
//! It classifies Python `asyncio` attribute calls so the Python frontend can
//! rewrite supported APIs into Smelt's runtime-neutral async operations.

/// Classification for an `asyncio.<name>` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AsyncioApi {
    /// `asyncio.create_task`.
    CreateTask,
    /// `asyncio.gather`.
    Gather,
    /// `asyncio.sleep`.
    Sleep,
    /// `asyncio.wait_for`.
    WaitFor,
    /// `asyncio.Queue`.
    Queue,
    /// `asyncio.Lock`.
    Lock,
    /// Lower-level event-loop API rejected in the first async slice.
    LowerLevelLoop,
    /// Unknown `asyncio` API.
    Unknown,
}

/// Classify an attribute name from `asyncio.<name>`.
#[must_use]
pub fn classify_attr(name: &str) -> AsyncioApi {
    match name {
        "create_task" => AsyncioApi::CreateTask,
        "gather" => AsyncioApi::Gather,
        "sleep" => AsyncioApi::Sleep,
        "wait_for" => AsyncioApi::WaitFor,
        "Queue" => AsyncioApi::Queue,
        "Lock" => AsyncioApi::Lock,
        "get_event_loop" | "get_running_loop" | "call_soon" => AsyncioApi::LowerLevelLoop,
        _ => AsyncioApi::Unknown,
    }
}
