//! Shared standard-library mapping metadata.
//!
//! This crate intentionally has no frontend AST dependencies. Frontends parse
//! source syntax into local call shapes, ask this crate for a rule identity,
//! and then perform their own typed lowering.

pub mod builtin_members;
pub mod category;
pub mod classes;
pub mod deps;
pub mod diagnostics;
pub mod fields;
pub mod globals;
pub mod host_object;
pub mod recognition;
pub mod rules;
pub mod runtime_symbols;
pub mod well_known_symbols;

pub use builtin_members::{
    BUILTIN_MEMBER_FUNCTIONS, BuiltinMember, BuiltinMemberKind, builtin_member, builtin_member_key,
};
pub use category::DiagnosticCategory;
pub use classes::{
    MATCH_CLASS_NAME, MATCH_GROUPS_CLASS_NAME, StdlibClass, TYPED_ARRAY_CLASS_NAMES,
    is_typed_array_class_name, typescript_stdlib_class,
};
pub use deps::BackendDependency;
pub use diagnostics::{StdlibDiagnostic, UnsupportedForm};
pub use fields::{FieldRule, typescript_field_rule};
pub use globals::{
    ERROR_CLASS_NAMES, GlobalPresence, NODE_PROFILE_VERSION, NODE_PROFILE_VERSION_STRING,
    global_member_presence, is_error_class_name, is_javascript_global_builtin,
};
pub use host_object::{
    ByteBufferRole, HOST_OBJECTS, HostObject, TypedArrayElement, byte_buffer_host_objects,
    byte_buffer_role, host_object_by_class, host_object_marker, host_object_markers,
    typed_array_element, typed_array_host_objects,
};
pub use recognition::{
    CallRecognition, MethodRecognition, TYPESCRIPT_CALLS, TYPESCRIPT_METHODS,
    TypeScriptReceiverKind, typescript_call_rule, typescript_method_rule,
};
pub use rules::{
    ApiNamespace, ApiShape, ArgShape, EffectKind, ReceiverKind, ReturnShape, RuleId, SourceLanguage,
};

