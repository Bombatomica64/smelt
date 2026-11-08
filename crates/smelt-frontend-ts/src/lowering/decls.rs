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
mod function_expressions;
mod functions;
mod inferred_types;
mod super_call;
mod virtual_methods;
pub(in crate::lowering) mod types_iface;
