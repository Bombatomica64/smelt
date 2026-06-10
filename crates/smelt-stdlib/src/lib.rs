//! Shared standard-library mapping metadata.
//!
//! This crate intentionally has no frontend AST dependencies. Frontends parse
//! source syntax into local call shapes, ask this crate for a rule identity,
//! and then perform their own typed lowering.

pub mod classes;
pub mod deps;
pub mod diagnostics;
pub mod fields;
pub mod recognition;
pub mod rules;

pub use classes::{StdlibClass, typescript_stdlib_class};
pub use deps::BackendDependency;
pub use diagnostics::{StdlibDiagnostic, UnsupportedForm};
pub use fields::{FieldRule, typescript_field_rule};
pub use recognition::{CallRecognition, TYPESCRIPT_CALLS, typescript_call_rule};
pub use rules::{
    ApiNamespace, ApiShape, ArgShape, EffectKind, ReceiverKind, ReturnShape, RuleId, SourceLanguage,
};
