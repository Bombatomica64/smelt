//! End-to-end parity tests for CPython-specialized Python definitions.

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_py::{FrontendOptions, HirCtx, to_hir_with_options};
use smelt_hir::FileId;
use smelt_specialize::{
    BackendAvailability, HashInputs, LinuxBubblewrapBackend, PythonModule,
    PythonSpecializationRequest, PythonSpecializer, SandboxBackend, SandboxPolicyRecord,
    prereq::missing_prerequisite,
};

/// Result type shared by the parity fixtures and their helpers, defaulting to a
/// unit success value so tests can write `ParityResult` for the common case.
type ParityResult<T = ()> = Result<T, Box<dyn Error>>;

const PROPERTY_PARITY_SOURCE: &str = r"
class Counter:
    _value: int

    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def value(self) -> int:
        return self._value

    @value.setter
    def value(self, value: int) -> None:
        self._value = value

_counter: Counter = Counter(2)
print(_counter.value)
_counter.value = 5
print(_counter.value)
";

const STACKED_DECORATOR_SOURCE: &str = r#"
from functools import wraps

def decorate(prefix: str):
    def apply(function):
        print("apply:" + prefix)
        @wraps(function)
        def wrapper(value: int) -> str:
            return prefix + function(value)
        return wrapper
    return apply

@decorate("outer:")
@decorate("inner:")
def render(value: int) -> str:
    return str(value)

print(render(3))
"#;

const METACLASS_METHOD_SOURCE: &str = r#"
class Meta(type):
    def __new__(cls, name, bases, namespace):
        def generated(self, value: int) -> str:
            return "generated:" + str(value)
        namespace["generated"] = generated
        return type.__new__(cls, name, bases, namespace)

class Model(metaclass=Meta):
    pass

_model: Model = Model()
print(_model.generated(4))
"#;

const CUSTOM_DESCRIPTOR_SOURCE: &str = r"
from __future__ import annotations

class Descriptor:
    name: str

    def __set_name__(self, owner: Model, name: str) -> None:
        self.name = name

    def __get__(self, instance: Model, owner: Model) -> int:
        return 9

class Model:
    field: int = Descriptor()

_model: Model = Model()
print(_model.field)
";

const DATACLASS_SOURCE: &str = r"
from dataclasses import dataclass

@dataclass
class Point:
    x: int
    y: int

_point: Point = Point(3, 4)
print(_point.x + _point.y)
";

const SIGNATURE_CHANGING_WRAPPER_SOURCE: &str = r"
from functools import wraps

def bind_first(function):
    @wraps(function)
    def wrapper(right: int) -> str:
        return function(10, right)
    return wrapper

@bind_first
def render(left: int, right: int) -> str:
    return str(left + right)

print(render(5))
";

const INIT_SUBCLASS_SOURCE: &str = r#"
class Base:
    def __init_subclass__(cls, label: str):
        def generated(self) -> str:
            return label
        cls.generated = generated

class Child(Base, label="ready"):
    pass

_child: Child = Child()
print(_child.generated())
"#;

#[test]
fn property_fixture_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(PROPERTY_PARITY_SOURCE, "property")
}

#[test]
fn stacked_decorator_fixture_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(STACKED_DECORATOR_SOURCE, "stacked_decorators")
}

#[test]
fn metaclass_generated_method_fixture_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(METACLASS_METHOD_SOURCE, "metaclass_method")
}

#[test]
fn custom_descriptor_fixture_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(CUSTOM_DESCRIPTOR_SOURCE, "custom_descriptor")
}

#[test]
fn dataclass_fixture_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(DATACLASS_SOURCE, "dataclass")
}

#[test]
fn signature_changing_wrapper_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(SIGNATURE_CHANGING_WRAPPER_SOURCE, "signature_wrapper")
}

#[test]
fn init_subclass_generated_method_matches_cpython_output() -> ParityResult {
    assert_specialized_parity(INIT_SUBCLASS_SOURCE, "init_subclass")
}

/// Specializes one fixture, generates Rust, and compares process stdout.
fn assert_specialized_parity(source: &str, fixture_name: &str) -> ParityResult {
    let Some(python) = discovered_python() else {
        return missing_prerequisite(fixture_name, "python3 interpreter");
    };
    let backend = LinuxBubblewrapBackend::discover();
    if backend.availability() != BackendAvailability::Available {
        return missing_prerequisite(fixture_name, "bubblewrap hard sandbox (bwrap + prlimit)");
    }
    let project = tempfile::tempdir()?;
    let source_path = project.path().join("fixture.py");
    fs::write(&source_path, source)?;
    let expected = run_python(&python, &source_path)?;
    let manifest = PythonSpecializer::new(backend).specialize(&PythonSpecializationRequest {
        smelt_version: "parity-test".to_owned(),
        python_executable: python.clone(),
        project_root: project.path().to_path_buf(),
        modules: vec![PythonModule {
            name: "fixture".to_owned(),
            path: source_path.clone(),
        }],
        hashes: fixture_hashes(fixture_name),
        sandbox_policy: fixture_policy(project.path()),
    })?;
    if !manifest.required_adapters.is_empty() {
        return Err(format!(
            "{fixture_name} fixture unexpectedly requires adapters: {:?}",
            manifest.required_adapters
        )
        .into());
    }

    let mut ctx = HirCtx::new();
    to_hir_with_options(
        source,
        FileId(0),
        source_path.to_string_lossy().as_ref(),
        &mut ctx,
        FrontendOptions {
            specialization: Some(&manifest),
        },
    )
    .map_err(|errors| format!("manifest-aware HIR lowering failed: {errors:?}"))?;
    let mut mir =
        smelt_mir::lower_hir(&ctx.krate).map_err(|errors| format!("MIR lowering: {errors:?}"))?;
    smelt_mir::opt::optimize(&mut mir);
    let generated = project.path().join("generated");
    emit_crate(
        &mir,
        &generated,
        &EmitOptions::new(format!("python_specialization_{fixture_name}"))
            .with_crate_kind(CrateKind::Program),
    )?;
    let actual = run_generated(&generated.join("Cargo.toml"), project.path())?;
    if actual != expected {
        let generated_source =
            fs::read_to_string(generated.join("src/main.rs")).unwrap_or_default();
        return Err(format!(
            "CPython and generated Rust output differ\nCPython:\n{expected}\nRust:\n{actual}\nGenerated:\n{generated_source}"
        )
        .into());
    }
    Ok(())
}

/// Finds a `CPython` executable suitable for the parity fixture.
fn discovered_python() -> Option<PathBuf> {
    smelt_specialize::prereq::locate_executable(
        "python3",
        &["/usr/bin/python3", "/usr/local/bin/python3"],
    )
}

/// Runs the source fixture directly and returns its stdout.
fn run_python(python: &Path, source: &Path) -> ParityResult<String> {
    let output = Command::new(python).arg(source).output()?;
    if !output.status.success() {
        return Err(format!(
            "CPython fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Runs the generated Rust program with an isolated Cargo target directory.
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

/// Builds deterministic placeholder cache hashes for a parity run.
fn fixture_hashes(fixture_name: &str) -> HashInputs {
    HashInputs {
        source: fixture_name.to_owned(),
        dependencies: "none".to_owned(),
        lockfile: "none".to_owned(),
        environment: "none".to_owned(),
        policy: "parity".to_owned(),
        callable_provenance: fixture_name.to_owned(),
    }
}

/// Builds the restrictive policy used by the sandboxed parity run.
fn fixture_policy(project: &Path) -> SandboxPolicyRecord {
    SandboxPolicyRecord {
        backend: "linux-bubblewrap".to_owned(),
        network: false,
        read_only_roots: runtime_roots(project),
        writable_roots: Vec::new(),
        environment: BTreeMap::new(),
        wall_time_ms: 5_000,
        cpu_time_ms: 5_000,
        memory_bytes: 512 * 1024 * 1024,
        process_limit: 1,
        output_bytes: 1024 * 1024,
        subprocesses: false,
        native_extensions: Vec::new(),
    }
}

/// Returns the source and `CPython` runtime roots mounted read-only.
fn runtime_roots(project: &Path) -> Vec<String> {
    [
        PathBuf::from(project),
        PathBuf::from("/usr"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string())
    .collect()
}
