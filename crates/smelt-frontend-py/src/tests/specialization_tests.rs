use std::collections::BTreeMap;

use super::*;
use smelt_specialize::{
    BindingMode, CallableProvenance, Definition, FunctionDefinition, FunctionSignature, GraphValue,
    GraphValueKind, HashInputs, HostLanguage, MANIFEST_SCHEMA_VERSION, MaterializedDefinition,
    ModuleRecord, Parameter, ParameterKind, SandboxPolicyRecord, SourceProvenance, SourceSpan,
    SpecializationManifest, StaticType, ValueGraph, ValueId,
};

const WRAPPED_SOURCE: &str = r#"
from typing import Callable

def decorate(function: Callable[[int], str]) -> Callable[[int], str]:
    prefix = "value="
    def wrapper(value: int) -> str:
        return prefix + function(value)
    return wrapper

@decorate
def render(value: int) -> str:
    return str(value)
"#;

#[test]
fn decorated_function_without_manifest_requires_specialization() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(WRAPPED_SOURCE, &mut ctx)?;
    ensure_eq(
        &first_error(&errors)?.code,
        &"smelt::specialization-required",
        "missing specialization diagnostic",
    )
}

#[test]
fn manifest_lifts_source_wrapper_and_concrete_captures() -> TestResult {
    let manifest = wrapper_manifest();
    let mut ctx = HirCtx::new();
    let module_id = to_hir_with_options(
        WRAPPED_SOURCE,
        FileId(0),
        "fixture.py",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("manifest-aware lowering failed: {errors:?}"))?;
    let module = module(&ctx, module_id)?;
    ensure_eq(&module.items.len(), &2, "original and wrapper item count")?;
    let names = module
        .items
        .iter()
        .map(|item_id| match item(&ctx, *item_id)? {
            Item::Function(function) => Ok(symbol(&ctx, function.name)?.to_owned()),
            _ => Err("expected function item".to_owned()),
        })
        .collect::<Result<Vec<_>, String>>()?;
    ensure(
        names.iter().any(|name| name == "__smelt_original_render"),
        "original callable must remain available to its wrapper capture",
    )?;
    ensure(
        names.iter().any(|name| name == "render"),
        "final module binding must use the decorated name",
    )?;
    ensure(
        smelt_hir::validate(&ctx.krate).is_empty(),
        "lifted wrapper HIR must validate",
    )
}

/// Builds a materialized wrapper manifest for [`WRAPPED_SOURCE`].
fn wrapper_manifest() -> SpecializationManifest {
    let original = provenance("render", source_start("def render"), BTreeMap::new());
    let captures = BTreeMap::from([
        ("prefix".to_owned(), ValueId(0)),
        ("function".to_owned(), ValueId(1)),
    ]);
    let wrapper = provenance(
        "decorate.<locals>.wrapper",
        source_start("def wrapper"),
        captures,
    );
    let signature = string_function_signature();
    SpecializationManifest {
        smelt_version: "test".to_owned(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        language: HostLanguage::Python,
        host_runtime_version: "test".to_owned(),
        hashes: fixture_hashes(),
        sandbox_policy: fixture_policy(),
        modules: vec![ModuleRecord {
            path: "fixture.py".to_owned(),
            definitions: vec![MaterializedDefinition {
                original_name: "render".to_owned(),
                binding_name: "render".to_owned(),
                source: SourceProvenance {
                    module: "fixture".to_owned(),
                    qualified_name: "render".to_owned(),
                    span: original.span.clone(),
                },
                binding_type: StaticType::Function(Box::new(signature.clone())),
                definition: Definition::Function(FunctionDefinition {
                    name: "render".to_owned(),
                    signature,
                    callable: wrapper.clone(),
                    wrapper_chain: vec![original.clone()],
                    binding_mode: BindingMode::Free,
                }),
            }],
            globals: BTreeMap::from([("render".to_owned(), ValueId(2))]),
        }],
        values: ValueGraph {
            nodes: vec![
                GraphValue {
                    id: ValueId(0),
                    ty: StaticType::String,
                    value: GraphValueKind::String("value=".to_owned()),
                },
                GraphValue {
                    id: ValueId(1),
                    ty: StaticType::Function(Box::new(string_function_signature())),
                    value: GraphValueKind::FunctionRef(original),
                },
                GraphValue {
                    id: ValueId(2),
                    ty: StaticType::Function(Box::new(string_function_signature())),
                    value: GraphValueKind::FunctionRef(wrapper),
                },
            ],
        },
        effects: Vec::new(),
        required_adapters: Vec::new(),
    }
}

/// Builds callable provenance against the in-memory fixture.
fn provenance(
    qualified_name: &str,
    start: u32,
    captures: BTreeMap<String, ValueId>,
) -> CallableProvenance {
    CallableProvenance {
        language: HostLanguage::Python,
        module: "fixture".to_owned(),
        qualified_name: qualified_name.to_owned(),
        span: SourceSpan {
            file: "fixture.py".to_owned(),
            start,
            end: start.saturating_add(32),
        },
        code_hash: qualified_name.to_owned(),
        captures,
        binding_mode: BindingMode::Free,
    }
}

/// Returns the byte offset for one source fragment.
fn source_start(fragment: &str) -> u32 {
    WRAPPED_SOURCE
        .find(fragment)
        .and_then(|offset| u32::try_from(offset).ok())
        .unwrap_or(0)
}

/// Builds the wrapper's concrete callable contract.
fn string_function_signature() -> FunctionSignature {
    FunctionSignature {
        parameters: vec![Parameter {
            name: "value".to_owned(),
            ty: StaticType::Int,
            kind: ParameterKind::Positional,
            default: None,
            annotation: Some("int".to_owned()),
        }],
        return_type: StaticType::String,
        is_async: false,
        throws: false,
    }
}

/// Builds stable placeholder cache hashes.
fn fixture_hashes() -> HashInputs {
    HashInputs {
        source: "source".to_owned(),
        dependencies: "dependencies".to_owned(),
        lockfile: "lockfile".to_owned(),
        environment: "environment".to_owned(),
        policy: "policy".to_owned(),
        callable_provenance: "provenance".to_owned(),
    }
}

/// Builds a non-executing fixture sandbox policy.
fn fixture_policy() -> SandboxPolicyRecord {
    SandboxPolicyRecord {
        backend: "test".to_owned(),
        network: false,
        read_only_roots: vec!["fixture.py".to_owned()],
        writable_roots: vec!["scratch".to_owned()],
        environment: BTreeMap::new(),
        wall_time_ms: 1,
        cpu_time_ms: 1,
        memory_bytes: 1,
        process_limit: 1,
        output_bytes: 1,
        subprocesses: false,
        native_extensions: Vec::new(),
    }
}
