//! Shared standard-library mapping metadata.
//!
//! This crate intentionally has no frontend AST dependencies. Frontends parse
//! source syntax into local call shapes, ask this crate for a rule identity,
//! and then perform their own typed lowering.

pub mod deps;
pub mod diagnostics;
pub mod recognition;
pub mod rules;

pub use deps::BackendDependency;
pub use diagnostics::{StdlibDiagnostic, UnsupportedForm};
pub use recognition::{CallRecognition, TYPESCRIPT_CALLS, typescript_call_rule};
pub use rules::{
    ApiNamespace, ApiShape, ArgShape, EffectKind, ReceiverKind, ReturnShape, RuleId, SourceLanguage,
};
