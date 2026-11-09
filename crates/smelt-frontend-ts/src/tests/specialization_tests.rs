//! TypeScript specialization-manifest frontend tests.

use std::collections::BTreeMap;

use super::*;
use smelt_specialize::{
    BindingMode, CallableProvenance, ClassDefinition, ConstructorShape, Definition,
    DescriptorDefinition, FieldDefinition, FunctionDefinition, FunctionSignature, GraphValue, GraphValueKind,
    HashInputs, HostLanguage, MANIFEST_SCHEMA_VERSION, MaterializedDefinition, ModuleRecord,
    SandboxPolicyRecord, SourceProvenance, SourceSpan, SpecializationManifest, StaticType,
    ValueGraph, ValueId,
};

const DECORATED_SOURCE: &str = r#"
@dec
class Example {
  @dec field: number = 1;
  @dec accessor count: number = 2;
  @dec static label: string = "source";
  @dec #secret: number = 3;
  @dec greet(name: string): string { return name; }
  @dec get title(): string { return "title"; }
  @dec set title(value: string) {}
}
const snapshot: string = Example.label;
"#;

#[test]
fn decorated_class_requires_specialization_manifest() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = to_hir(DECORATED_SOURCE, FileId(0), &mut ctx)
        .expect_err("decorated source must require specialization");
    let error = errors
        .first()
        .ok_or_else(|| "missing specialization diagnostic".to_owned())?;
    ensure_eq!(error.code, "smelt::specialization-required");
    Ok(())
}

#[test]
fn manifest_materializes_instance_private_accessor_and_static_fields() -> Result<(), String> {
    let manifest = decorated_manifest();
    let mut ctx = HirCtx::new();
    let module = to_hir_with_options(
        DECORATED_SOURCE,
        FileId(0),
        "fixture.ts",
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
            ..FrontendOptions::default()
        },
    )
    .map_err(|errors| format!("manifest-aware lowering failed: {errors:?}"))?;
    let module_index = usize::try_from(module.0).map_err(|error| error.to_string())?;
    let class = ctx.krate.modules[module_index]
        .items
        .iter()
        .filter_map(|item| usize::try_from(item.0).ok())
        .filter_map(|item| ctx.krate.items.get(item))
        .find_map(|item| match item {
            Item::Class(class) => Some(class),
            _ => None,
        })
        .ok_or_else(|| "materialized Example class is absent".to_owned())?;
    let field_names = class
        .fields
        .iter()
        .filter_map(|field| ctx.krate.symbols.get(field.name))
        .collect::<Vec<_>>();
    ensure!(field_names.contains(&"field"));
    ensure!(field_names.contains(&"count"));
    ensure!(field_names.contains(&"secret"));
    let secret = class
        .fields
        .iter()
        .find(|field| ctx.krate.symbols.get(field.name) == Some("secret"))
        .ok_or_else(|| "private field is absent".to_owned())?;
    // `#secret` is an ECMAScript private NAME, so it is not an own property:
    // `Hidden`, not the compile-time-only `Private`.
    ensure_eq!(secret.visibility, smelt_hir::Visibility::Hidden);
    let static_names = class
        .static_fields
        .iter()
        .filter_map(|field| ctx.krate.symbols.get(field.name))
        .collect::<Vec<_>>();
    ensure!(static_names.contains(&"label"));
    ensure!(static_names.contains(&"ready"));
    ensure!(class.methods.iter().any(|method| {
        usize::try_from(method.0)
            .ok()
            .and_then(|index| ctx.krate.items.get(index))
            .is_some_and(|item| {
                matches!(
                    item,
                    Item::Function(function)
                        if ctx.krate.symbols.get(function.name) == Some("greet")
                )
            })
    }));
    let title = class
        .descriptors
        .iter()
        .find(|descriptor| ctx.krate.symbols.get(descriptor.name) == Some("title"))
        .ok_or_else(|| "materialized title descriptor is absent".to_owned())?;
    ensure!(title.getter.is_some());
    ensure!(title.setter.is_some());
    for (item, expected_name) in [
        (title.getter, "__smelt_get_title"),
        (title.setter, "__smelt_set_title"),
    ] {
        let item = item
            .and_then(|item| usize::try_from(item.0).ok())
            .and_then(|item| ctx.krate.items.get(item))
            .ok_or_else(|| format!("materialized accessor item '{expected_name}' is absent"))?;
        let Item::Function(function) = item else {
            return Err(format!(
                "materialized accessor '{expected_name}' is not a function"
            ));
        };
        ensure_eq!(
            ctx.krate.symbols.get(function.name),
            Some(expected_name)
        );
    }
    ensure_eq!(ctx.krate.types.get(title.read_ty), Some(&Type::String));
    ensure!(
        title
            .write_ty
            .is_some_and(|ty| ctx.krate.types.get(ty) == Some(&Type::String))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Builds the host-materialized shape for [`DECORATED_SOURCE`].
fn decorated_manifest() -> SpecializationManifest {
    let signature = FunctionSignature {
        parameters: Vec::new(),
        return_type: StaticType::Named("fixture.Example".to_owned()),
        is_async: false,
        throws: false,
    };
    let fields = vec![
        materialized_field("field", false, false),
        materialized_field("count", false, false),
        materialized_field("label", true, false),
        materialized_field("#secret", false, true),
    ];
    SpecializationManifest {
        smelt_version: "test".to_owned(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        language: HostLanguage::TypeScript,
        host_runtime_version: "Node test".to_owned(),
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
            read_only_roots: Vec::new(),
            writable_roots: Vec::new(),
            environment: BTreeMap::new(),
            wall_time_ms: 1,
            cpu_time_ms: 1,
            memory_bytes: 1,
            process_limit: 1,
            output_bytes: 1,
            subprocesses: false,
            native_extensions: Vec::new(),
        },
        modules: vec![ModuleRecord {
            path: "fixture.ts".to_owned(),
            definitions: vec![MaterializedDefinition {
                original_name: "Example".to_owned(),
                binding_name: "Example".to_owned(),
                source: SourceProvenance {
                    module: "fixture".to_owned(),
                    qualified_name: "Example".to_owned(),
                    span: source_span(),
                },
                binding_type: StaticType::Named("fixture.Example".to_owned()),
                definition: Definition::Class(ClassDefinition {
                    mro: vec!["Example".to_owned()],
                    bases: Vec::new(),
                    fields,
                    descriptors: vec![DescriptorDefinition {
                        name: "title".to_owned(),
                        value: ValueId(0),
                        read_type: StaticType::String,
                        write_type: Some(StaticType::String),
                        getter: Some(callable_provenance("Example.title.get")),
                        setter: Some(callable_provenance("Example.title.set")),
                        data_descriptor: true,
                        is_static: false,
                    }],
                    methods: Vec::new(),
                    static_values: BTreeMap::from([
                        ("label".to_owned(), ValueId(0)),
                        ("ready".to_owned(), ValueId(1)),
                    ]),
                    slots: Vec::new(),
                    metadata: Vec::new(),
                    constructor: ConstructorShape {
                        signature,
                        replacement: None,
                    },
                    initializers: Vec::new(),
                }),
            }],
            globals: BTreeMap::from([("Example".to_owned(), ValueId(2))]),
        }],
        values: ValueGraph {
            nodes: vec![
                GraphValue {
                    id: ValueId(0),
                    ty: StaticType::String,
                    value: GraphValueKind::String("materialized".to_owned()),
                },
                GraphValue {
                    id: ValueId(1),
                    ty: StaticType::Bool,
                    value: GraphValueKind::Bool(true),
                },
                GraphValue {
                    id: ValueId(2),
                    ty: StaticType::Named("fixture.Example".to_owned()),
                    value: GraphValueKind::ClassRef {
                        module: "fixture".to_owned(),
                        qualified_name: "Example".to_owned(),
                    },
                },
            ],
        },
        effects: Vec::new(),
        required_adapters: Vec::new(),
    }
}

/// Builds source provenance for one decorated class accessor.
fn callable_provenance(qualified_name: &str) -> CallableProvenance {
    CallableProvenance {
        language: HostLanguage::TypeScript,
        module: "fixture".to_owned(),
        qualified_name: qualified_name.to_owned(),
        span: source_span(),
        code_hash: "source".to_owned(),
        captures: BTreeMap::new(),
        binding_mode: BindingMode::Instance,
    }
}

/// Builds one materialized numeric class field.
fn materialized_field(name: &str, is_static: bool, is_private: bool) -> FieldDefinition {
    FieldDefinition {
        name: name.to_owned(),
        ty: if name == "label" {
            StaticType::String
        } else {
            StaticType::Float
        },
        default: None,
        is_static,
        is_private,
        span: Some(source_span()),
    }
}

/// Returns the source span shared by this compact fixture.
fn source_span() -> SourceSpan {
    SourceSpan {
        file: "fixture.ts".to_owned(),
        start: 0,
        end: u32::try_from(DECORATED_SOURCE.len()).unwrap_or(u32::MAX),
    }
}

/// Two statics whose Rust renderings fold alike (`PI`, `pi`).
const CASE_STATICS_SOURCE: &str = r"
@dec
class Consts {
  @dec static pi: number = 1;
  @dec static PI: number = 2;
}
console.log(Consts.PI);
";

/// `decorated_manifest` reshaped for [`CASE_STATICS_SOURCE`]: `pi` holds 1
/// and `PI` holds 2.
fn case_statics_manifest() -> SpecializationManifest {
    let mut manifest = decorated_manifest();
    let span = SourceSpan {
        file: "fixture.ts".to_owned(),
        start: 0,
        end: u32::try_from(CASE_STATICS_SOURCE.len()).unwrap_or(u32::MAX),
    };
    if let Some(module) = manifest.modules.first_mut()
        && let Some(definition) = module.definitions.first_mut()
    {
        definition.original_name = "Consts".to_owned();
        definition.binding_name = "Consts".to_owned();
        definition.source.qualified_name = "Consts".to_owned();
        definition.source.span = span.clone();
        if let Definition::Class(class) = &mut definition.definition {
            class.mro = vec!["Consts".to_owned()];
            let mut pi = materialized_field("pi", true, false);
            let mut upper = materialized_field("PI", true, false);
            pi.span = Some(span.clone());
            upper.span = Some(span);
            class.fields = vec![pi, upper];
            class.descriptors = Vec::new();
            class.static_values =
                BTreeMap::from([("pi".to_owned(), ValueId(0)), ("PI".to_owned(), ValueId(1))]);
        }
        module.globals = BTreeMap::from([("Consts".to_owned(), ValueId(2))]);
    }
    manifest.values.nodes = vec![
        GraphValue { id: ValueId(0), ty: StaticType::Float, value: GraphValueKind::Float(1.0) },
        GraphValue { id: ValueId(1), ty: StaticType::Float, value: GraphValueKind::Float(2.0) },
        GraphValue {
            id: ValueId(2),
            ty: StaticType::Named("fixture.Consts".to_owned()),
            value: GraphValueKind::ClassRef { module: "fixture".to_owned(), qualified_name: "Consts".to_owned() },
        },
    ];
    manifest
}

/// A materialized static is read by its SOURCE key: `Consts.PI` is 2, never
/// the `pi` whose Rust rendering it shares (audit finding A13).
#[test]
fn materialized_static_reads_match_the_source_key() -> Result<(), String> {
    let manifest = case_statics_manifest();
    let mut ctx = HirCtx::new();
    to_hir_with_options(
        CASE_STATICS_SOURCE,
        FileId(0),
        "fixture.ts",
        &mut ctx,
        FrontendOptions { specialization: Some(&manifest), ..FrontendOptions::default() },
    )
    .map_err(|errors| format!("manifest-aware lowering failed: {errors:?}"))?;
    let floats = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter_map(|expr| match expr.kind {
            ExprKind::Literal(Literal::Float(value)) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(floats.contains(&2.0_f64), "`Consts.PI` must read PI's 2: {floats:?}");
    ensure!(!floats.contains(&1.0_f64), "`Consts.PI` read pi's 1: {floats:?}");
    Ok(())
}

/// [`DECORATED_SOURCE`] with a function expression a method's provenance
/// can name, so the lift re-parses it.
fn lifted_getter_source() -> String {
    format!("{DECORATED_SOURCE}const helper = function () {{ return \"lifted\"; }};\n")
}

/// `decorated_manifest` whose `greet` method is the function expression of
/// [`lifted_getter_source`], declared in `file`.
fn lifted_getter_manifest(source: &str, file: &str) -> SpecializationManifest {
    let mut manifest = decorated_manifest();
    let start = source.find("function ()").unwrap_or(0);
    let end = source.rfind('}').map_or(source.len(), |end| end + 1);
    let span = SourceSpan {
        file: file.to_owned(),
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    };
    for module in &mut manifest.modules {
        for definition in &mut module.definitions {
            if let Definition::Class(class) = &mut definition.definition {
                let mut callable = callable_provenance("Example.greet");
                callable.span = span.clone();
                class.methods = vec![FunctionDefinition {
                    name: "greet".to_owned(),
                    signature: FunctionSignature {
                        parameters: Vec::new(),
                        return_type: StaticType::String,
                        is_async: false,
                        throws: false,
                    },
                    callable,
                    wrapper_chain: Vec::new(),
                    binding_mode: BindingMode::Instance,
                }];
            }
        }
    }
    manifest
}

/// A callable whose provenance names another file is not re-parsed out of
/// this one (audit finding A9): the lift reports an adapter instead, while
/// the same callable named in this file is lifted.
#[test]
fn a_callable_from_another_file_is_not_lifted_from_this_one() -> Result<(), String> {
    let source = lifted_getter_source();
    let lower = |file: &str| {
        let manifest = lifted_getter_manifest(&source, file);
        let mut ctx = HirCtx::new();
        to_hir_with_options(
            &source,
            FileId(0),
            "fixture.ts",
            &mut ctx,
            FrontendOptions { specialization: Some(&manifest), ..FrontendOptions::default() },
        )
        .map(|_| ())
    };
    lower("fixture.ts").map_err(|errors| format!("the method of this file must lift: {errors:?}"))?;
    let errors = lower("elsewhere.ts").err().ok_or("a method of another file was lifted from this one")?;
    ensure!(errors.iter().any(|error| error.message.contains("elsewhere.ts")), "{errors:?}");
    Ok(())
}
