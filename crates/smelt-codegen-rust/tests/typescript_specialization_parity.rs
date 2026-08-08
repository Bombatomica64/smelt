//! End-to-end parity tests for Node-specialized TypeScript definitions.

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_ts::{FrontendOptions, HirCtx, to_hir_with_options};
use smelt_hir::FileId;
use smelt_specialize::{
    BackendAvailability, HashInputs, LinuxBubblewrapBackend, NodeModule,
    NodeSpecializationRequest, NodeSpecializer, SandboxBackend, SandboxPolicyRecord,
    SpecializationManifest, prereq::missing_prerequisite,
};

/// Result type shared by the parity fixture and its helpers, defaulting to a
/// unit success value so tests can write `ParityResult` for the common case.
type ParityResult<T = ()> = Result<T, Box<dyn Error>>;

const DECORATED_MEMBER_SOURCE: &str = r#"
function identity(value: any, _context: any): any {
    return value;
}

function initialized(value: any, context: any): any {
    context.addInitializer(function (this: any) {
        this.initialized = "yes";
    });
    return value;
}

function wrap(value: any, _context: any): any {
    return function (this: any, name: string): string {
        return "wrapped " + value.call(this, name);
    };
}

@identity
export class Example {
    @initialized
    initialized: string = "no";

    @identity
    static label: string = "ready";

    @wrap
    greet(name: string): string {
        return "hello " + name;
    }

    @identity
    get title(): string {
        return "title";
    }
}

const example: Example = new Example();
console.log(Example.label);
console.log(example.greet("smelt"));
console.log(example.title);
console.log(example.initialized);
"#;

#[test]
fn decorated_members_match_node_output() -> ParityResult {
    const TEST: &str = "decorated_members_match_node_output";
    let Some(node) = discovered_executable("node", &["/usr/bin/node", "/usr/local/bin/node"])
    else {
        return missing_prerequisite(TEST, "node executable");
    };
    let Some(tsc) = discovered_executable("tsc", &["/usr/local/bin/tsc", "/usr/bin/tsc"]) else {
        return missing_prerequisite(TEST, "tsc (TypeScript compiler)");
    };
    let backend = LinuxBubblewrapBackend::discover();
    if backend.availability() != BackendAvailability::Available {
        return missing_prerequisite(TEST, "bubblewrap hard sandbox (bwrap + prlimit)");
    }

    let project = tempfile::tempdir()?;
    let source_path = project.path().join("fixture.ts");
    let emitted_dir = project.path().join("dist");
    fs::write(&source_path, DECORATED_MEMBER_SOURCE)?;
    compile_typescript(&tsc, &source_path, &emitted_dir)?;
    let emitted_path = emitted_dir.join("fixture.js");
    let source_map_path = emitted_dir.join("fixture.js.map");
    let expected = run_node(&node, &emitted_path)?;
    let manifest = NodeSpecializer::new(backend).specialize(&NodeSpecializationRequest {
        smelt_version: "parity-test".to_owned(),
        node_executable: node.to_path_buf(),
        project_root: project.path().to_path_buf(),
        modules: vec![NodeModule {
            name: "fixture".to_owned(),
            source_path: source_path.clone(),
            emitted_path,
            source_map_path: Some(source_map_path),
        }],
        typescript_version: "5.9.3".to_owned(),
        decorators_revision: "2023-11".to_owned(),
        hashes: fixture_hashes(),
        sandbox_policy: fixture_policy(project.path(), &node),
    })?;
    assert_manifest_invariants(&manifest)?;
    emit_and_compare(&manifest, &source_path, &expected, project.path())
}

/// Asserts the specialization manifest carries the decorator provenance the
/// parity fixture depends on: a materialized `Example` class, the wrapped
/// `greet` capture, and the field-relative `addInitializer` placement.
fn assert_manifest_invariants(manifest: &SpecializationManifest) -> ParityResult {
    if !manifest.required_adapters.is_empty() {
        return Err(format!(
            "decorated member fixture unexpectedly requires adapters: {:?}",
            manifest.required_adapters
        )
        .into());
    }
    let class = manifest
        .modules
        .first()
        .and_then(|module| {
            module
                .definitions
                .iter()
                .find(|definition| definition.binding_name == "Example")
        })
        .and_then(|definition| match &definition.definition {
            smelt_specialize::Definition::Class(class) => Some(class),
            _ => None,
        })
        .ok_or("materialized Example class is absent")?;
    if class
        .methods
        .iter()
        .find(|method| method.name == "greet")
        .is_none_or(|method| method.callable.captures.is_empty())
    {
        return Err("decorator wrapper capture provenance is absent".into());
    }
    if class.initializers.iter().all(|initializer| {
        initializer.target_kind.as_deref() != Some("field")
            || initializer.target_name.as_deref() != Some("initialized")
    }) {
        return Err("field-relative addInitializer placement is absent".into());
    }
    Ok(())
}

/// Lowers the fixture through the manifest-aware pipeline, emits a program
/// crate, runs it, and asserts its stdout matches the canonical Node output.
fn emit_and_compare(
    manifest: &SpecializationManifest,
    source_path: &Path,
    expected: &str,
    project: &Path,
) -> ParityResult {
    let mut ctx = HirCtx::new();
    to_hir_with_options(
        DECORATED_MEMBER_SOURCE,
        FileId(0),
        source_path.to_string_lossy().as_ref(),
        &mut ctx,
        FrontendOptions {
            specialization: Some(manifest),
        },
    )
    .map_err(|errors| format!("manifest-aware HIR lowering failed: {errors:?}"))?;
    let mut mir =
        smelt_mir::lower_hir(&ctx.krate).map_err(|errors| format!("MIR lowering: {errors:?}"))?;
    smelt_mir::opt::optimize(&mut mir);
    let generated = project.join("generated");
    emit_crate(
        &mir,
        &generated,
        &EmitOptions::new("typescript_specialization_members")
            .with_crate_kind(CrateKind::Program),
    )?;
    let actual = run_generated(&generated.join("Cargo.toml"), project)?;
    if actual != expected {
        let generated_source =
            fs::read_to_string(generated.join("src/main.rs")).unwrap_or_default();
        return Err(format!(
            "Node and generated Rust output differ\nNode:\n{expected}\nRust:\n{actual}\nGenerated:\n{generated_source}"
        )
        .into());
    }
    Ok(())
}

/// Compiles one standard-decorator fixture with canonical source maps.
fn compile_typescript(tsc: &Path, source: &Path, output: &Path) -> ParityResult {
    let result = Command::new(tsc)
        .arg(source)
        .args(["--target", "ES2022", "--module", "commonjs", "--outDir"])
        .arg(output)
        .args(["--sourceMap", "--skipLibCheck", "--pretty", "false"])
        .output()?;
    if !result.status.success() {
        // tsc reports diagnostics on stdout and leaves stderr empty.
        return Err(format!(
            "TypeScript fixture failed to compile:\n{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}

/// Runs canonical JavaScript output and captures stdout.
fn run_node(node: &Path, emitted: &Path) -> ParityResult<String> {
    let output = Command::new(node).arg(emitted).output()?;
    if !output.status.success() {
        return Err(format!(
            "Node fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Runs generated Rust with an isolated target directory.
fn run_generated(manifest: &Path, scratch: &Path) -> ParityResult<String> {
    let output = Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", scratch.join("generated-target"))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "generated Rust fixture failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Finds the first available executable.
fn discovered_executable(name: &str, fallbacks: &[&str]) -> Option<PathBuf> {
    smelt_specialize::prereq::locate_executable(name, fallbacks)
}

/// Builds deterministic placeholder hashes for the parity run.
fn fixture_hashes() -> HashInputs {
    HashInputs {
        source: "typescript-members".to_owned(),
        dependencies: "none".to_owned(),
        lockfile: "none".to_owned(),
        environment: "none".to_owned(),
        policy: "parity".to_owned(),
        callable_provenance: "typescript-members".to_owned(),
    }
}

/// Builds the restrictive policy used by the sandboxed parity run.
fn fixture_policy(project: &Path, node: &Path) -> SandboxPolicyRecord {
    SandboxPolicyRecord {
        backend: "linux-bubblewrap".to_owned(),
        network: false,
        read_only_roots: runtime_roots(project, node),
        writable_roots: Vec::new(),
        environment: BTreeMap::new(),
        wall_time_ms: 5_000,
        cpu_time_ms: 5_000,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        process_limit: 64,
        output_bytes: 1024 * 1024,
        subprocesses: false,
        native_extensions: Vec::new(),
    }
}

/// Returns the source and Node runtime roots mounted read-only.
fn runtime_roots(project: &Path, node: &Path) -> Vec<String> {
    [
        Some(PathBuf::from(project)),
        // Bind Node's own install prefix: hardcoding /usr only works for a
        // distro-installed runtime, and `setup-node` uses the hosted tool cache.
        smelt_specialize::prereq::runtime_prefix(node),
        Some(PathBuf::from("/usr")),
        Some(PathBuf::from("/lib")),
        Some(PathBuf::from("/lib64")),
    ]
    .into_iter()
    .flatten()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string())
    .collect()
}
