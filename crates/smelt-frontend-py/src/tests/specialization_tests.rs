use std::collections::BTreeMap;

use super::*;
use smelt_specialize::{
    BindingMode, CallableProvenance, ClassDefinition, ConstructorShape, Definition,
    DescriptorDefinition, FieldDefinition, FunctionDefinition, FunctionSignature, GraphValue,
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

const PROPERTY_SOURCE: &str = r#"
class Model:
    _value: int

    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def value(self) -> int:
        return self._value

    @value.setter
    def value(self, value: int) -> None:
        self._value = value
"#;

const METACLASS_SOURCE: &str = r#"
class Meta(type):
    def __new__(cls, name, bases, namespace):
        def generated(self, value: int) -> str:
            return str(value)
        namespace["generated"] = generated
        return type.__new__(cls, name, bases, namespace)

class Model(metaclass=Meta):
    source_field: int
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

#[test]
fn manifest_materializes_property_as_typed_descriptor() -> TestResult {
    let manifest = property_manifest();
    let mut ctx = HirCtx::new();
    let module_id = to_hir_with_options(
        PROPERTY_SOURCE,
        FileId(0),
        "fixture.py",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("property lowering failed: {errors:?}"))?;
    let module = module(&ctx, module_id)?;
    let class = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Class(class) => Some(class),
            _ => None,
        })
        .ok_or_else(|| "materialized class item is missing".to_owned())?;
    ensure_eq(
        &class.fields.len(),
        &1,
        "descriptor must not occupy storage",
    )?;
    ensure_eq(
        &class.descriptors.len(),
        &1,
        "materialized descriptor count",
    )?;
    let descriptor = &class.descriptors[0];
    ensure(
        descriptor.getter.is_some() && descriptor.setter.is_some(),
        "property getter and setter must resolve to HIR functions",
    )?;
    ensure_eq(
        &ctx.krate.types.get(descriptor.read_ty),
        &Some(&Type::Int),
        "descriptor read type",
    )?;
    ensure_eq(
        &descriptor.write_ty.and_then(|ty| ctx.krate.types.get(ty)),
        &Some(&Type::Int),
        "descriptor write type",
    )?;
    ensure(
        smelt_hir::validate(&ctx.krate).is_empty(),
        "materialized descriptor HIR must validate",
    )
}

#[test]
fn manifest_merges_metaclass_generated_fields() -> TestResult {
    let manifest = metaclass_manifest();
    let mut ctx = HirCtx::new();
    let module_id = to_hir_with_options(
        METACLASS_SOURCE,
        FileId(0),
        "fixture.py",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("metaclass lowering failed: {errors:?}"))?;
    let module = module(&ctx, module_id)?;
    let class = module
        .items
        .iter()
        .find_map(|item_id| match item(&ctx, *item_id).ok()? {
            Item::Class(class) => Some(class),
            _ => None,
        })
        .ok_or_else(|| "materialized class item is missing".to_owned())?;
    let fields = class
        .fields
        .iter()
        .map(|field| symbol(&ctx, field.name).map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    ensure(
        fields.iter().any(|field| field == "source_field"),
        "source field must remain in the materialized class",
    )?;
    ensure(
        fields.iter().any(|field| field == "generated_field"),
        "metaclass-generated field must be merged into HIR",
    )?;
    let generated = class
        .methods
        .iter()
        .filter_map(|item_id| item(&ctx, *item_id).ok())
        .find_map(|item| match item {
            Item::Function(function) if symbol(&ctx, function.name).ok() == Some("generated") => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| "source-defined generated method was not lifted".to_owned())?;
    ensure(
        matches!(
            generated.owner,
            smelt_hir::FunctionOwner::ClassMethod {
                class: owner_class,
                ..
            } if symbol(&ctx, owner_class).ok() == Some("Model")
        ),
        "generated method must be rebound to the materialized class",
    )
}

#[test]
fn descriptor_state_uses_the_fully_qualified_class_owner() -> TestResult {
    let manifest = qualified_descriptor_state_manifest();
    let mut ctx = HirCtx::new();
    let class_source = "class Config:\n    existing: int\n";
    let module_a = to_hir_with_options(
        class_source,
        FileId(0),
        "a.py",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("module a lowering failed: {errors:?}"))?;
    let module_b = to_hir_with_options(
        class_source,
        FileId(1),
        "b.py",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("module b lowering failed: {errors:?}"))?;
    to_hir_with_options(
        "class Owner:\n    value: int\n",
        FileId(2),
        "c.py",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("module c lowering failed: {errors:?}"))?;

    let a_fields = class_field_names(&ctx, module_a, "Config")?;
    let b_fields = class_field_names(&ctx, module_b, "Config")?;
    ensure(
        !a_fields.iter().any(|field| field == "descriptor_state"),
        "descriptor state leaked into a.Config",
    )?;
    ensure(
        b_fields.iter().any(|field| field == "descriptor_state"),
        "descriptor state was not merged into b.Config",
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

/// Builds a materialized property manifest for [`PROPERTY_SOURCE`].
fn property_manifest() -> SpecializationManifest {
    let getter = property_provenance("def value(self) -> int:");
    let setter = property_provenance("def value(self, value: int) -> None:");
    let signature = FunctionSignature {
        parameters: Vec::new(),
        return_type: StaticType::Named("fixture.Model".to_owned()),
        is_async: false,
        throws: false,
    };
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
                original_name: "Model".to_owned(),
                binding_name: "Model".to_owned(),
                source: SourceProvenance {
                    module: "fixture".to_owned(),
                    qualified_name: "Model".to_owned(),
                    span: SourceSpan {
                        file: "fixture.py".to_owned(),
                        start: source_start_in(PROPERTY_SOURCE, "class Model:"),
                        end: u32::try_from(PROPERTY_SOURCE.len()).unwrap_or(u32::MAX),
                    },
                },
                binding_type: StaticType::Named("fixture.Model".to_owned()),
                definition: Definition::Class(ClassDefinition {
                    mro: vec!["fixture.Model".to_owned()],
                    bases: Vec::new(),
                    fields: vec![FieldDefinition {
                        name: "_value".to_owned(),
                        ty: StaticType::Int,
                        default: None,
                        is_static: false,
                        is_private: false,
                        span: None,
                    }],
                    descriptors: vec![DescriptorDefinition {
                        name: "value".to_owned(),
                        value: ValueId(0),
                        read_type: StaticType::Int,
                        write_type: Some(StaticType::Int),
                        getter: Some(getter),
                        setter: Some(setter),
                        data_descriptor: true,
                        is_static: false,
                    }],
                    methods: Vec::new(),
                    static_values: BTreeMap::new(),
                    slots: Vec::new(),
                    metadata: Vec::new(),
                    constructor: ConstructorShape {
                        signature,
                        replacement: None,
                    },
                    initializers: Vec::new(),
                }),
            }],
            globals: BTreeMap::from([("Model".to_owned(), ValueId(1))]),
        }],
        values: ValueGraph {
            nodes: vec![
                GraphValue {
                    id: ValueId(0),
                    ty: StaticType::Named("builtins.property".to_owned()),
                    value: GraphValueKind::Instance {
                        class: "builtins.property".to_owned(),
                        fields: BTreeMap::new(),
                    },
                },
                GraphValue {
                    id: ValueId(1),
                    ty: StaticType::Named("fixture.Model".to_owned()),
                    value: GraphValueKind::ClassRef {
                        module: "fixture".to_owned(),
                        qualified_name: "Model".to_owned(),
                    },
                },
            ],
        },
        effects: Vec::new(),
        required_adapters: Vec::new(),
    }
}

/// Builds a manifest containing a field generated by a custom metaclass.
fn metaclass_manifest() -> SpecializationManifest {
    let mut manifest = property_manifest();
    let Definition::Class(class) = &mut manifest.modules[0].definitions[0].definition else {
        return manifest;
    };
    class.descriptors.clear();
    class.fields = vec![
        FieldDefinition {
            name: "source_field".to_owned(),
            ty: StaticType::Int,
            default: None,
            is_static: false,
            is_private: false,
            span: None,
        },
        FieldDefinition {
            name: "generated_field".to_owned(),
            ty: StaticType::String,
            default: None,
            is_static: false,
            is_private: false,
            span: None,
        },
    ];
    class.methods = vec![FunctionDefinition {
        name: "generated".to_owned(),
        signature: FunctionSignature {
            parameters: vec![
                Parameter {
                    name: "self".to_owned(),
                    ty: StaticType::Named("builtins.object".to_owned()),
                    kind: ParameterKind::Positional,
                    default: None,
                    annotation: None,
                },
                Parameter {
                    name: "value".to_owned(),
                    ty: StaticType::Int,
                    kind: ParameterKind::Positional,
                    default: None,
                    annotation: Some("int".to_owned()),
                },
            ],
            return_type: StaticType::String,
            is_async: false,
            throws: false,
        },
        callable: CallableProvenance {
            language: HostLanguage::Python,
            module: "fixture".to_owned(),
            qualified_name: "Meta.__new__.<locals>.generated".to_owned(),
            span: SourceSpan {
                file: "fixture.py".to_owned(),
                start: source_start_in(METACLASS_SOURCE, "def generated"),
                end: source_start_in(METACLASS_SOURCE, "def generated").saturating_add(64),
            },
            code_hash: "generated".to_owned(),
            captures: BTreeMap::new(),
            binding_mode: BindingMode::Instance,
        },
        wrapper_chain: Vec::new(),
        binding_mode: BindingMode::Instance,
    }];
    manifest
}

/// Builds a manifest whose descriptor instance belongs to `b.Config`.
fn qualified_descriptor_state_manifest() -> SpecializationManifest {
    let signature = FunctionSignature {
        parameters: Vec::new(),
        return_type: StaticType::Named("c.Owner".to_owned()),
        is_async: false,
        throws: false,
    };
    SpecializationManifest {
        smelt_version: "test".to_owned(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        language: HostLanguage::Python,
        host_runtime_version: "test".to_owned(),
        hashes: fixture_hashes(),
        sandbox_policy: fixture_policy(),
        modules: vec![ModuleRecord {
            path: "c.py".to_owned(),
            definitions: vec![MaterializedDefinition {
                original_name: "Owner".to_owned(),
                binding_name: "Owner".to_owned(),
                source: SourceProvenance {
                    module: "c".to_owned(),
                    qualified_name: "Owner".to_owned(),
                    span: SourceSpan {
                        file: "c.py".to_owned(),
                        start: 0,
                        end: 32,
                    },
                },
                binding_type: StaticType::Named("c.Owner".to_owned()),
                definition: Definition::Class(ClassDefinition {
                    mro: vec!["c.Owner".to_owned()],
                    bases: Vec::new(),
                    fields: Vec::new(),
                    descriptors: vec![DescriptorDefinition {
                        name: "value".to_owned(),
                        value: ValueId(0),
                        read_type: StaticType::Int,
                        write_type: None,
                        getter: None,
                        setter: None,
                        data_descriptor: false,
                        is_static: false,
                    }],
                    methods: Vec::new(),
                    static_values: BTreeMap::new(),
                    slots: Vec::new(),
                    metadata: Vec::new(),
                    constructor: ConstructorShape {
                        signature,
                        replacement: None,
                    },
                    initializers: Vec::new(),
                }),
            }],
            globals: BTreeMap::from([("Owner".to_owned(), ValueId(2))]),
        }],
        values: ValueGraph {
            nodes: vec![
                GraphValue {
                    id: ValueId(0),
                    ty: StaticType::Named("b.Config".to_owned()),
                    value: GraphValueKind::Instance {
                        class: "b.Config".to_owned(),
                        fields: BTreeMap::from([("descriptor_state".to_owned(), ValueId(1))]),
                    },
                },
                GraphValue {
                    id: ValueId(1),
                    ty: StaticType::Int,
                    value: GraphValueKind::Int("7".to_owned()),
                },
                GraphValue {
                    id: ValueId(2),
                    ty: StaticType::Named("c.Owner".to_owned()),
                    value: GraphValueKind::ClassRef {
                        module: "c".to_owned(),
                        qualified_name: "Owner".to_owned(),
                    },
                },
            ],
        },
        effects: Vec::new(),
        required_adapters: Vec::new(),
    }
}

/// Returns one named class's field names from a lowered module.
fn class_field_names(ctx: &HirCtx, module_id: ModuleId, name: &str) -> Result<Vec<String>, String> {
    let class = module(ctx, module_id)?
        .items
        .iter()
        .filter_map(|item_id| item(ctx, *item_id).ok())
        .find_map(|item| match item {
            Item::Class(class) if symbol(ctx, class.name).ok() == Some(name) => Some(class),
            _ => None,
        })
        .ok_or_else(|| format!("class '{name}' is absent"))?;
    class
        .fields
        .iter()
        .map(|field| symbol(ctx, field.name).map(str::to_owned))
        .collect()
}

/// Builds property accessor provenance at one source definition.
fn property_provenance(fragment: &str) -> CallableProvenance {
    CallableProvenance {
        language: HostLanguage::Python,
        module: "fixture".to_owned(),
        qualified_name: "Model.value".to_owned(),
        span: SourceSpan {
            file: "fixture.py".to_owned(),
            start: source_start_in(PROPERTY_SOURCE, fragment),
            end: source_start_in(PROPERTY_SOURCE, fragment).saturating_add(32),
        },
        code_hash: fragment.to_owned(),
        captures: BTreeMap::new(),
        binding_mode: BindingMode::Instance,
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
    source_start_in(WRAPPED_SOURCE, fragment)
}

/// Returns the byte offset for one source fragment in a fixture.
fn source_start_in(source: &str, fragment: &str) -> u32 {
    source
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
