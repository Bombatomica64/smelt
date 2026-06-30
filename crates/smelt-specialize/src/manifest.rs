//! Versioned specialization manifest schema and structural validation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::MANIFEST_SCHEMA_VERSION;

/// Host language used to materialize a module batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostLanguage {
    /// `CPython` specialization.
    Python,
    /// Node/TypeScript specialization.
    TypeScript,
}

/// Versioned output of one sandboxed host-runtime specialization batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecializationManifest {
    /// Smelt compiler version that produced the manifest.
    pub smelt_version: String,
    /// Interchange schema version.
    pub schema_version: u32,
    /// Guest source language.
    pub language: HostLanguage,
    /// Exact `CPython` or Node/runtime toolchain version.
    pub host_runtime_version: String,
    /// Inputs that participate in invalidation.
    pub hashes: HashInputs,
    /// Effective hard-sandbox policy.
    pub sandbox_policy: SandboxPolicyRecord,
    /// Materialized source modules.
    pub modules: Vec<ModuleRecord>,
    /// Reference-preserving serialized values shared by all definitions.
    pub values: ValueGraph,
    /// Supported ordered definition-time effects.
    pub effects: Vec<EffectReplay>,
    /// Native specialization adapters required to consume this manifest.
    pub required_adapters: Vec<AdapterRequirement>,
}

/// Hashes covering every specialization invalidation category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashInputs {
    /// Candidate and transitive source bytes.
    pub source: String,
    /// Installed distribution or package identities.
    pub dependencies: String,
    /// Dependency lockfile bytes.
    pub lockfile: String,
    /// Allowed environment-variable names and values.
    pub environment: String,
    /// Effective sandbox policy.
    pub policy: String,
    /// Source-to-callable provenance mapping.
    pub callable_provenance: String,
}

/// Exact sandbox policy recorded into a manifest and cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPolicyRecord {
    /// Backend implementation and version.
    pub backend: String,
    /// Whether network access was allowed.
    pub network: bool,
    /// Read-only source, dependency, and runtime roots.
    pub read_only_roots: Vec<String>,
    /// Writable roots visible to the guest.
    pub writable_roots: Vec<String>,
    /// Allowlisted environment values.
    pub environment: BTreeMap<String, String>,
    /// Wall-clock deadline.
    pub wall_time_ms: u64,
    /// CPU-time limit.
    pub cpu_time_ms: u64,
    /// Address-space or job memory limit.
    pub memory_bytes: u64,
    /// Maximum guest process count.
    pub process_limit: u32,
    /// Combined captured output limit.
    pub output_bytes: u64,
    /// Whether child process creation was permitted.
    pub subprocesses: bool,
    /// Allowlisted native extension identities.
    pub native_extensions: Vec<String>,
}

/// Materialized records for one source module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleRecord {
    /// Resolved module path used by the source graph.
    pub path: String,
    /// Definition records in final binding order.
    pub definitions: Vec<MaterializedDefinition>,
    /// Final exported/module-global values.
    pub globals: BTreeMap<String, ValueId>,
}

/// A source name after all decorators and class construction hooks have run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedDefinition {
    /// Original source spelling.
    pub original_name: String,
    /// Final module binding name.
    pub binding_name: String,
    /// Source provenance for the original definition.
    pub source: SourceProvenance,
    /// Concrete final binding type.
    pub binding_type: StaticType,
    /// Materialized definition shape.
    pub definition: Definition,
}

/// Materialized definition payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Definition {
    /// Materialized class.
    Class(ClassDefinition),
    /// Materialized function or callable wrapper.
    Function(FunctionDefinition),
    /// Non-callable final value.
    Value {
        /// Materialized value graph node.
        value: ValueId,
    },
}

/// Stable definition kind for diagnostics and external tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    /// Class binding.
    Class,
    /// Callable binding.
    Function,
    /// Concrete value binding.
    Value,
}

/// Concrete class state after host-runtime construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// Qualified names in method-resolution order.
    pub mro: Vec<String>,
    /// Direct base class references.
    pub bases: Vec<ValueId>,
    /// Typed instance and class fields.
    pub fields: Vec<FieldDefinition>,
    /// Descriptor objects and protocol methods.
    pub descriptors: Vec<DescriptorDefinition>,
    /// Final bound methods.
    pub methods: Vec<FunctionDefinition>,
    /// Materialized static values.
    pub static_values: BTreeMap<String, ValueId>,
    /// Declared slot names.
    pub slots: Vec<String>,
    /// Language/framework metadata.
    pub metadata: Vec<MetadataEntry>,
    /// Final constructor call shape.
    pub constructor: ConstructorShape,
    /// Ordered class/static and instance initializers.
    pub initializers: Vec<InitializerRecord>,
}

/// Typed field materialized from a class definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Source/runtime field name.
    pub name: String,
    /// Concrete static type.
    pub ty: StaticType,
    /// Default value, when present.
    pub default: Option<ValueId>,
    /// Whether the field belongs to the class rather than instances.
    pub is_static: bool,
    /// Whether the field is private in the source language.
    pub is_private: bool,
    /// Source span, when the field maps to source.
    pub span: Option<SourceSpan>,
}

/// Typed descriptor state and callable protocol provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptorDefinition {
    /// Bound class member name.
    pub name: String,
    /// Concrete descriptor object.
    pub value: ValueId,
    /// Result type for reads.
    pub read_type: StaticType,
    /// Accepted type for writes, or `None` for read-only descriptors.
    pub write_type: Option<StaticType>,
    /// Source callable implementing `__get__` or an accessor getter.
    pub getter: Option<CallableProvenance>,
    /// Source callable implementing `__set__` or an accessor setter.
    pub setter: Option<CallableProvenance>,
    /// Whether data-descriptor precedence applies.
    pub data_descriptor: bool,
}

/// Materialized callable with source provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Final binding name.
    pub name: String,
    /// Callable signature after wrapping.
    pub signature: FunctionSignature,
    /// Source callable that executes at runtime.
    pub callable: CallableProvenance,
    /// Inner-to-outer callable wrapper chain.
    pub wrapper_chain: Vec<CallableProvenance>,
    /// Runtime binding behavior.
    pub binding_mode: BindingMode,
}

/// Callable parameter and return contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionSignature {
    /// Parameters in source call order.
    pub parameters: Vec<Parameter>,
    /// Return type.
    pub return_type: StaticType,
    /// Whether the callable is async.
    pub is_async: bool,
    /// Whether the callable may throw.
    pub throws: bool,
}

/// One callable parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
    /// Source parameter name.
    pub name: String,
    /// Concrete static type.
    pub ty: StaticType,
    /// Calling convention.
    pub kind: ParameterKind,
    /// Materialized default.
    pub default: Option<ValueId>,
    /// Source annotation spelling, when available.
    pub annotation: Option<String>,
}

/// Source-language parameter convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    /// Positional or keyword parameter.
    Positional,
    /// Positional-only parameter.
    PositionalOnly,
    /// Keyword-only parameter.
    KeywordOnly,
    /// Variadic positional parameter.
    VariadicPositional,
    /// Variadic keyword parameter.
    VariadicKeyword,
}

/// Final constructor contract for a class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstructorShape {
    /// Constructor signature.
    pub signature: FunctionSignature,
    /// Replacement constructor provenance, when a decorator replaced the class.
    pub replacement: Option<CallableProvenance>,
}

/// Metadata key/value pair materialized by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataEntry {
    /// Metadata namespace or key.
    pub key: String,
    /// Reference-preserving metadata value.
    pub value: ValueId,
}

/// Ordered static or instance initialization operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializerRecord {
    /// Stable order within the module/class boundary.
    pub order: u32,
    /// Initialization boundary.
    pub kind: InitializerKind,
    /// Source callable to lift into HIR.
    pub callable: CallableProvenance,
}

/// Initializer execution boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitializerKind {
    /// Runs during module/static initialization.
    Static,
    /// Runs for every constructed instance.
    Instance,
}

/// Source mapping for a runtime callable created by specialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallableProvenance {
    /// Guest language.
    pub language: HostLanguage,
    /// Source module.
    pub module: String,
    /// Source qualified name.
    pub qualified_name: String,
    /// Exact source body span.
    pub span: SourceSpan,
    /// Hash of the source code object/body.
    pub code_hash: String,
    /// Concrete closure captures keyed by free-variable name.
    pub captures: BTreeMap<String, ValueId>,
    /// Receiver behavior.
    pub binding_mode: BindingMode,
}

/// Runtime receiver binding convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMode {
    /// Free/static function.
    Free,
    /// Instance receiver.
    Instance,
    /// Class/constructor receiver.
    Class,
}

/// Original source location and name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProvenance {
    /// Source module path.
    pub module: String,
    /// Original qualified name.
    pub qualified_name: String,
    /// Source span.
    pub span: SourceSpan,
}

/// Byte-oriented source range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// Source file path.
    pub file: String,
    /// Inclusive byte offset.
    pub start: u32,
    /// Exclusive byte offset.
    pub end: u32,
}

/// Concrete static type preserved across specialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StaticType {
    /// Null-like value.
    Null,
    /// Boolean.
    Bool,
    /// Arbitrary-width integer.
    Int,
    /// Floating-point number.
    Float,
    /// UTF-8 string.
    String,
    /// Byte sequence.
    Bytes,
    /// Homogeneous list.
    List(Box<Self>),
    /// Homogeneous set.
    Set(Box<Self>),
    /// Fixed tuple.
    Tuple(Vec<Self>),
    /// Homogeneous dictionary.
    Dict(Box<Self>, Box<Self>),
    /// Named class or enum.
    Named(String),
    /// Function contract.
    Function(Box<FunctionSignature>),
    /// Intentional dynamic metadata or source `unknown` boundary.
    DynamicMetadata,
}

/// Stable index into a [`ValueGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValueId(pub u32);

/// Reference-preserving graph of materialized values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueGraph {
    /// Nodes indexed by [`GraphValue::id`].
    pub nodes: Vec<GraphValue>,
}

/// One identity-bearing serialized value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphValue {
    /// Stable node identifier.
    pub id: ValueId,
    /// Concrete static type.
    pub ty: StaticType,
    /// Serialized payload and outgoing references.
    pub value: GraphValueKind,
}

/// Materialized value payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum GraphValueKind {
    /// Null-like value.
    Null,
    /// Boolean.
    Bool(bool),
    /// Integer encoded in decimal to preserve arbitrary precision.
    Int(String),
    /// IEEE-754 number.
    Float(f64),
    /// UTF-8 string.
    String(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
    /// Ordered tuple references.
    Tuple(Vec<ValueId>),
    /// Ordered list references.
    List(Vec<ValueId>),
    /// Deterministically ordered set references.
    Set(Vec<ValueId>),
    /// Ordered key/value references.
    Dict(Vec<(ValueId, ValueId)>),
    /// Enum class and member.
    Enum {
        /// Qualified enum class.
        class: String,
        /// Member name.
        member: String,
    },
    /// Typed instance fields.
    Instance {
        /// Qualified concrete class.
        class: String,
        /// Materialized instance fields.
        fields: BTreeMap<String, ValueId>,
    },
    /// Class definition reference.
    ClassRef {
        /// Defining source module.
        module: String,
        /// Qualified class name.
        qualified_name: String,
    },
    /// Function provenance reference.
    FunctionRef(CallableProvenance),
}

/// A deterministic observable effect that must be replayed in generated code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectReplay {
    /// Assign a module-global value.
    SetGlobal {
        /// Target source module.
        module: String,
        /// Global binding name.
        name: String,
        /// Final graph value.
        value: ValueId,
    },
    /// Assign a class/static value.
    SetStatic {
        /// Qualified class name.
        class: String,
        /// Static binding name.
        name: String,
        /// Final graph value.
        value: ValueId,
    },
    /// Deterministic captured output event.
    Output {
        /// Whether to replay on standard error instead of standard output.
        stderr: bool,
        /// Captured deterministic text.
        text: String,
    },
    /// Invoke a lifted source initializer.
    Initializer {
        /// Source initializer to lift and invoke.
        callable: CallableProvenance,
    },
}

/// Versioned native adapter requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterRequirement {
    /// Stable adapter registry ID.
    pub id: String,
    /// Required adapter version.
    pub version: String,
    /// Human-readable reason specialization could not stay source-mappable.
    pub reason: String,
    /// Source location that introduced the opaque behavior.
    pub span: Option<SourceSpan>,
}

/// Structural manifest validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    /// Machine-readable validation code.
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ManifestError {}

/// Validates schema compatibility, unique identities, graph references, source
/// ranges, initializer order, and adapter IDs.
///
/// # Errors
///
/// Returns every structural error found so callers can present one actionable
/// report instead of repeatedly invoking a host process.
pub fn validate_manifest(manifest: &SpecializationManifest) -> Result<(), Vec<ManifestError>> {
    let mut errors = Vec::new();
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        errors.push(error(
            "smelt::specialization-schema-version",
            format!(
                "manifest schema {} is incompatible with supported schema {}",
                manifest.schema_version, MANIFEST_SCHEMA_VERSION
            ),
        ));
    }

    let mut value_ids = BTreeSet::new();
    for node in &manifest.values.nodes {
        if !value_ids.insert(node.id) {
            errors.push(error(
                "smelt::specialization-duplicate-value",
                format!("value graph ID {} appears more than once", node.id.0),
            ));
        }
    }

    let mut references = Vec::new();
    collect_manifest_references(manifest, &mut references);
    for reference in references {
        if !value_ids.contains(&reference) {
            errors.push(error(
                "smelt::specialization-missing-value",
                format!(
                    "value graph ID {} is referenced but not defined",
                    reference.0
                ),
            ));
        }
    }

    let mut modules = BTreeSet::new();
    for module in &manifest.modules {
        if !modules.insert(module.path.as_str()) {
            errors.push(error(
                "smelt::specialization-duplicate-module",
                format!("module '{}' appears more than once", module.path),
            ));
        }
        validate_module(module, &mut errors);
    }

    validate_adapters(&manifest.required_adapters, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates native adapter identities and uniqueness.
fn validate_adapters(adapters: &[AdapterRequirement], errors: &mut Vec<ManifestError>) {
    let mut seen = BTreeSet::new();
    for adapter in adapters {
        if adapter.id.trim().is_empty() || adapter.version.trim().is_empty() {
            errors.push(error(
                "smelt::specialization-invalid-adapter",
                "native adapter IDs and versions must be non-empty".to_owned(),
            ));
        } else if adapter.id != adapter.id.trim() || adapter.version != adapter.version.trim() {
            errors.push(error(
                "smelt::specialization-invalid-adapter",
                format!(
                    "native adapter '{}' has surrounding whitespace in its ID or version",
                    adapter.id
                ),
            ));
        } else if !seen.insert(adapter.id.as_str()) {
            errors.push(error(
                "smelt::specialization-duplicate-adapter",
                format!("native adapter '{}' appears more than once", adapter.id),
            ));
        }
    }
}

/// Validates module-local identities and source ranges.
fn validate_module(module: &ModuleRecord, errors: &mut Vec<ManifestError>) {
    let mut bindings = BTreeSet::new();
    for definition in &module.definitions {
        if !bindings.insert(definition.binding_name.as_str()) {
            errors.push(error(
                "smelt::specialization-duplicate-binding",
                format!(
                    "module '{}' defines binding '{}' more than once",
                    module.path, definition.binding_name
                ),
            ));
        }
        validate_span(&definition.source.span, errors);
        if let Definition::Class(class) = &definition.definition {
            let mut previous = None;
            for initializer in &class.initializers {
                if previous.is_some_and(|order| initializer.order <= order) {
                    errors.push(error(
                        "smelt::specialization-initializer-order",
                        format!(
                            "initializers for '{}.{}' are not strictly ordered",
                            module.path, definition.binding_name
                        ),
                    ));
                    break;
                }
                previous = Some(initializer.order);
            }
        }
    }
}

/// Validates one half-open source span.
fn validate_span(span: &SourceSpan, errors: &mut Vec<ManifestError>) {
    if span.start > span.end {
        errors.push(error(
            "smelt::specialization-invalid-span",
            format!(
                "source span {}:{}..{} has an inverted range",
                span.file, span.start, span.end
            ),
        ));
    }
}

/// Collects all value graph references reachable from manifest records.
fn collect_manifest_references(manifest: &SpecializationManifest, output: &mut Vec<ValueId>) {
    for node in &manifest.values.nodes {
        collect_value_references(&node.value, output);
        if let StaticType::Function(signature) = &node.ty {
            collect_signature_references(signature, output);
        }
    }
    for module in &manifest.modules {
        output.extend(module.globals.values().copied());
        for definition in &module.definitions {
            collect_definition_references(&definition.definition, output);
        }
    }
    for effect in &manifest.effects {
        match effect {
            EffectReplay::SetGlobal { value, .. } | EffectReplay::SetStatic { value, .. } => {
                output.push(*value);
            }
            EffectReplay::Initializer { callable } => {
                output.extend(callable.captures.values().copied());
            }
            EffectReplay::Output { .. } => {}
        }
    }
}

/// Collects outgoing references from one graph node.
fn collect_value_references(value: &GraphValueKind, output: &mut Vec<ValueId>) {
    match value {
        GraphValueKind::Tuple(values)
        | GraphValueKind::List(values)
        | GraphValueKind::Set(values) => output.extend(values.iter().copied()),
        GraphValueKind::Dict(entries) => {
            output.extend(entries.iter().flat_map(|(key, item)| [*key, *item]));
        }
        GraphValueKind::Instance { fields, .. } => output.extend(fields.values().copied()),
        GraphValueKind::FunctionRef(callable) => {
            output.extend(callable.captures.values().copied());
        }
        GraphValueKind::Null
        | GraphValueKind::Bool(_)
        | GraphValueKind::Int(_)
        | GraphValueKind::Float(_)
        | GraphValueKind::String(_)
        | GraphValueKind::Bytes(_)
        | GraphValueKind::Enum { .. }
        | GraphValueKind::ClassRef { .. } => {}
    }
}

/// Collects references from one definition payload.
fn collect_definition_references(definition: &Definition, output: &mut Vec<ValueId>) {
    match definition {
        Definition::Value { value } => output.push(*value),
        Definition::Function(function) => collect_function_references(function, output),
        Definition::Class(class) => {
            output.extend(class.bases.iter().copied());
            output.extend(class.static_values.values().copied());
            output.extend(class.metadata.iter().map(|entry| entry.value));
            for field in &class.fields {
                output.extend(field.default);
            }
            for descriptor in &class.descriptors {
                output.push(descriptor.value);
                if let Some(getter) = &descriptor.getter {
                    output.extend(getter.captures.values().copied());
                }
                if let Some(setter) = &descriptor.setter {
                    output.extend(setter.captures.values().copied());
                }
            }
            for method in &class.methods {
                collect_function_references(method, output);
            }
            collect_signature_references(&class.constructor.signature, output);
            if let Some(replacement) = &class.constructor.replacement {
                output.extend(replacement.captures.values().copied());
            }
            for initializer in &class.initializers {
                output.extend(initializer.callable.captures.values().copied());
            }
        }
    }
}

/// Collects references from a function and its wrapper chain.
fn collect_function_references(function: &FunctionDefinition, output: &mut Vec<ValueId>) {
    collect_signature_references(&function.signature, output);
    output.extend(function.callable.captures.values().copied());
    for wrapper in &function.wrapper_chain {
        output.extend(wrapper.captures.values().copied());
    }
}

/// Collects default values from a function signature.
fn collect_signature_references(signature: &FunctionSignature, output: &mut Vec<ValueId>) {
    output.extend(
        signature
            .parameters
            .iter()
            .filter_map(|parameter| parameter.default),
    );
}

/// Constructs one manifest validation error.
const fn error(code: &'static str, message: String) -> ManifestError {
    ManifestError { code, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_manifest() -> SpecializationManifest {
        SpecializationManifest {
            smelt_version: "0.1.0".to_owned(),
            schema_version: MANIFEST_SCHEMA_VERSION,
            language: HostLanguage::Python,
            host_runtime_version: "3.13.5".to_owned(),
            hashes: HashInputs {
                source: "source".to_owned(),
                dependencies: "dependencies".to_owned(),
                lockfile: "lockfile".to_owned(),
                environment: "environment".to_owned(),
                policy: "policy".to_owned(),
                callable_provenance: "provenance".to_owned(),
            },
            sandbox_policy: SandboxPolicyRecord {
                backend: "test".to_owned(),
                network: false,
                read_only_roots: vec!["source".to_owned()],
                writable_roots: vec!["scratch".to_owned()],
                environment: BTreeMap::new(),
                wall_time_ms: 100,
                cpu_time_ms: 100,
                memory_bytes: 1024,
                process_limit: 1,
                output_bytes: 1024,
                subprocesses: false,
                native_extensions: Vec::new(),
            },
            modules: Vec::new(),
            values: ValueGraph { nodes: Vec::new() },
            effects: Vec::new(),
            required_adapters: Vec::new(),
        }
    }

    #[test]
    fn identity_cycles_round_trip_and_validate() -> Result<(), String> {
        let mut manifest = empty_manifest();
        manifest.values.nodes = vec![
            GraphValue {
                id: ValueId(0),
                ty: StaticType::List(Box::new(StaticType::DynamicMetadata)),
                value: GraphValueKind::List(vec![ValueId(1)]),
            },
            GraphValue {
                id: ValueId(1),
                ty: StaticType::List(Box::new(StaticType::DynamicMetadata)),
                value: GraphValueKind::List(vec![ValueId(0), ValueId(0)]),
            },
        ];
        let json = serde_json::to_string(&manifest).map_err(|error| error.to_string())?;
        let decoded: SpecializationManifest =
            serde_json::from_str(&json).map_err(|error| error.to_string())?;
        if decoded != manifest {
            return Err("manifest graph identity did not round-trip".to_owned());
        }
        validate_manifest(&decoded).map_err(|errors| format!("{errors:?}"))
    }

    #[test]
    fn missing_graph_references_are_reported() -> Result<(), String> {
        let mut manifest = empty_manifest();
        manifest.effects.push(EffectReplay::SetGlobal {
            module: "model".to_owned(),
            name: "registry".to_owned(),
            value: ValueId(7),
        });
        let Err(errors) = validate_manifest(&manifest) else {
            return Err("missing values did not fail validation".to_owned());
        };
        if !errors
            .iter()
            .any(|error| error.code == "smelt::specialization-missing-value")
        {
            return Err("validation did not identify missing graph references".to_owned());
        }
        Ok(())
    }

    #[test]
    fn schema_version_mismatch_is_precise() -> Result<(), String> {
        let mut manifest = empty_manifest();
        manifest.schema_version = MANIFEST_SCHEMA_VERSION + 1;
        let Err(errors) = validate_manifest(&manifest) else {
            return Err("future schemas did not fail validation".to_owned());
        };
        if !errors
            .iter()
            .any(|error| error.code == "smelt::specialization-schema-version")
        {
            return Err("schema mismatch did not use its stable diagnostic".to_owned());
        }
        Ok(())
    }

    #[test]
    fn adapter_ids_reject_surrounding_whitespace() -> Result<(), String> {
        let mut manifest = empty_manifest();
        manifest.required_adapters = vec![
            AdapterRequirement {
                id: "adapter.example".to_owned(),
                version: "1".to_owned(),
                reason: "first".to_owned(),
                span: None,
            },
            AdapterRequirement {
                id: " adapter.example ".to_owned(),
                version: "1".to_owned(),
                reason: "duplicate".to_owned(),
                span: None,
            },
        ];
        let Err(errors) = validate_manifest(&manifest) else {
            return Err("whitespace-variant adapter IDs were accepted".to_owned());
        };
        if errors
            .iter()
            .any(|error| error.code == "smelt::specialization-invalid-adapter")
        {
            Ok(())
        } else {
            Err(format!("invalid adapter diagnostic is absent: {errors:?}"))
        }
    }
}
