//! Declaration lowering: functions and function expressions, classes (members,
//! virtual methods, construction), arrow-const decls, type aliases, interfaces,
//! enums, and the constructor-function idiom.

mod anonymous_record;
mod arrows;
mod call_signature;
mod callable_object;
mod class_construction;
mod class_methods;
mod classes;
mod constructor;
mod enums;
pub(in crate::lowering) mod evaluation_statics;
mod function_expressions;
mod functions;
mod inferred_types;
mod super_call;
pub(in crate::lowering) mod types_iface;
mod virtual_methods;
