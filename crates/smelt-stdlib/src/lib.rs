//! Shared standard-library mapping metadata.
//!
//! This crate intentionally has no frontend AST dependencies. Frontends parse
//! source syntax into local call shapes, ask this crate for a rule identity,
//! and then perform their own typed lowering.

pub mod builtin_members;
pub mod category;
pub mod classes;
pub mod deps;
pub mod erased_prototype_methods;
pub mod fetch_init;
pub mod fields;
pub mod globals;
pub mod host_modules;
pub mod host_object;
pub mod js_regex;
pub mod math;
pub mod recognition;
pub mod rules;
pub mod runtime_symbols;
pub mod symbol_keys;
pub mod well_known_symbols;

pub use builtin_members::{
    BUILTIN_MEMBER_FUNCTIONS, BuiltinMember, BuiltinMemberKind, builtin_member, builtin_member_key,
};
pub use category::DiagnosticCategory;
pub use classes::{
    ARRAY_NON_MUTATING_METHODS, HostDataProperty, HostPrimitive, InstanceOfRule, MATCH_CLASS_NAME,
    MATCH_GROUPS_CLASS_NAME, OBJECT_PROTOTYPE_MEMBERS, PY_EXCEPTION_ARGS_CLASS_NAME,
    PY_EXCEPTION_CLASS_NAME, RegExpMember, StdlibClass, TYPED_ARRAY_CLASS_NAMES, host_class_parent,
    host_data_property, instance_of_rule, is_array_non_mutating_method, is_data_view_class,
    is_object_prototype_member, is_typed_array_class_name, regexp_member,
    stdlib_class_instance_has_property, subsystem_instance_marker, typescript_stdlib_class,
};
pub use deps::{BackendDependency, CrateRequirement};
pub use erased_prototype_methods::{
    ERASED_ARRAY_PROTOTYPE_METHODS, ERASED_STRING_PROTOTYPE_METHODS, erased_array_prototype_method,
    erased_string_prototype_method, is_erased_prototype_call, is_erased_prototype_method,
    prototype_method_max_arguments,
};
pub use fetch_init::{
    FetchInitKey, FetchInitKeyKind, REQUEST_INIT_KEYS, RESPONSE_INIT_KEYS, fetch_init_keys,
};
pub use fields::{FieldRule, typescript_field_rule};
pub use globals::{
    ERROR_CLASS_NAMES, GLOBAL_NAMESPACES, GLOBAL_OBJECT_ALIASES, GlobalNamespace, GlobalPresence,
    NODE_PROFILE_TIMEZONE, NODE_PROFILE_VERSION, NODE_PROFILE_VERSION_STRING, global_is_absent,
    global_member_presence, global_namespace, global_namespace_member_is_namespace,
    is_error_class_name, is_global_function_value, is_javascript_global_builtin,
};
pub use host_modules::{
    HOST_MODULES, HostExport, HostExportKind, HostModule, HostModuleId, HostSurface, host_module,
    host_module_export, host_module_id, host_value_blocker, is_host_module,
};
pub use host_object::{
    ByteBufferRole, HOST_OBJECTS, HostKind, HostObject, TypedArrayElement,
    UNMODELED_CALL_NAMESPACES, UNMODELED_HOST_CONSTRUCTORS, builtin_base_class,
    builtin_class_extends, byte_buffer_host_objects, byte_buffer_role, data_view_accessor,
    host_markers_extending, host_object_by_class, host_object_marker, host_object_markers,
    reflectively_constructible, typed_array_element, typed_array_host_objects,
};
pub use math::{MATH_FUNCTIONS, MathFunction, math_constant, math_function, number_constant};
pub use recognition::{
    CallRecognition, MethodRecognition, TYPESCRIPT_CALLS, TYPESCRIPT_METHODS,
    TypeScriptReceiverKind, typescript_call_rule, typescript_method_rule,
};
pub use rules::RuleId;
