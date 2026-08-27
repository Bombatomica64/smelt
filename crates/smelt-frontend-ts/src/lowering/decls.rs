//! Declaration lowering: functions, arrow-const decls, type aliases,
//! interfaces, enums, and the constructor-function idiom.

mod arrows;
mod callable_object;
mod constructor;
mod enums;
mod functions;
mod super_call;
pub(in crate::lowering) mod types_iface;
