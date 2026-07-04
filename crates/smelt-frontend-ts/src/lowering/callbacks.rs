//! Callback-heavy method lowering: array/set/string iteration and closures.
//!
//! This is the largest split of the lowering builder. It lowers the JavaScript
//! methods whose semantics are carried by callbacks or closures — array
//! iteration (`map`/`filter`/`reduce`/`find`/`sort`/`concat`), set and dict
//! projections, string transforms, and the closure-capture machinery that
//! turns a source arrow/function argument into an HIR [`CallbackExpr`]. The
//! helpers here classify receiver and argument surfaces, synthesize the
//! capture list, and emit the concrete list/set/string runtime call kinds.
//!
//! The implementation is split across focused submodules, each defining more
//! methods on the shared `ModuleBuilder` type:

mod body_lowering;
mod classify;
mod closures;
mod dispatch;
mod list_ops;
mod timer_wrapper;
mod transforms;
