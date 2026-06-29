//! Build-time host-runtime specialization for definition-time metaprogramming.
//!
//! This crate owns the boundary between source graph resolution and frontend
//! lowering. Host runtimes materialize definition-time behavior into a
//! versioned [`SpecializationManifest`]; frontends consume that manifest as
//! ordinary typed source structure and never launch host processes.

#![expect(
    clippy::exhaustive_structs,
    clippy::exhaustive_enums,
    reason = "the manifest is a versioned interchange schema whose variants must be explicit"
)]

mod cache_key;
mod detection;
mod manifest;

pub use cache_key::{CacheKeyInput, SpecializationCacheKey};
pub use detection::{DetectionReason, ModuleDetection, ModuleInput, detect_specialization_modules};
pub use manifest::{
    AdapterRequirement, BindingMode, CallableProvenance, ClassDefinition, ConstructorShape,
    Definition, DefinitionKind, DescriptorDefinition, EffectReplay, FieldDefinition,
    FunctionDefinition, FunctionSignature, GraphValue, GraphValueKind, HashInputs, HostLanguage,
    InitializerKind, InitializerRecord, ManifestError, MaterializedDefinition, MetadataEntry,
    ModuleRecord, Parameter, ParameterKind, SandboxPolicyRecord, SourceProvenance, SourceSpan,
    SpecializationManifest, StaticType, ValueGraph, ValueId, validate_manifest,
};

/// Current specialization manifest schema version.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
