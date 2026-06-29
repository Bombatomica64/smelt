//! Batched `CPython` guest orchestration and manifest decoding.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Serialize;

use crate::{
    HashInputs, HostLanguage, MANIFEST_SCHEMA_VERSION, SandboxBackend, SandboxError,
    SandboxPolicyRecord, SandboxRequest, SandboxRunner, SpecializationManifest, validate_manifest,
};

/// Embedded, versioned `CPython` specialization guest.
const PYTHON_GUEST: &str = include_str!("../guest/python.py");

/// Monotonic suffix for process-local scratch directories.
static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One Python module imported by its canonical source-graph name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PythonModule {
    /// Importable dotted module name.
    pub name: String,
    /// Canonical source path used for cache-key and graph validation.
    pub path: PathBuf,
}

/// Inputs for one batched `CPython` specialization process.
#[derive(Debug, Clone)]
pub struct PythonSpecializationRequest {
    /// Smelt compiler version.
    pub smelt_version: String,
    /// `CPython` executable inside the read-only runtime roots.
    pub python_executable: PathBuf,
    /// Import root for the resolved source graph.
    pub project_root: PathBuf,
    /// Candidate modules in dependency/import order.
    pub modules: Vec<PythonModule>,
    /// Precomputed invalidation hashes.
    pub hashes: HashInputs,
    /// Effective hard-sandbox policy except the per-run scratch path.
    pub sandbox_policy: SandboxPolicyRecord,
}

/// `CPython` specialization failure.
#[derive(Debug)]
pub enum PythonSpecializationError {
    /// Scratch/request filesystem operation failed.
    Io(io::Error),
    /// Hard-sandbox preparation or execution failed.
    Sandbox(SandboxError),
    /// Guest exited unsuccessfully.
    Guest {
        /// Platform exit code when available.
        status: Option<i32>,
        /// Bounded guest diagnostic output.
        stderr: String,
    },
    /// Guest output did not match the manifest schema.
    Decode(serde_json::Error),
    /// Guest emitted a structurally invalid manifest.
    InvalidManifest(Vec<crate::ManifestError>),
    /// Request paths or module graph were invalid.
    InvalidRequest(String),
}

impl fmt::Display for PythonSpecializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Python specialization I/O failed: {error}"),
            Self::Sandbox(error) => {
                write!(formatter, "Python specialization sandbox failed: {error}")
            }
            Self::Guest { status, stderr } => write!(
                formatter,
                "Python specialization guest exited with status {}: {stderr}",
                status.map_or_else(|| "signal".to_owned(), |code| code.to_string())
            ),
            Self::Decode(error) => {
                write!(
                    formatter,
                    "Python specialization manifest was invalid JSON: {error}"
                )
            }
            Self::InvalidManifest(errors) => write!(
                formatter,
                "Python specialization manifest failed validation: {}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Self::InvalidRequest(message) => {
                write!(
                    formatter,
                    "invalid Python specialization request: {message}"
                )
            }
        }
    }
}

impl std::error::Error for PythonSpecializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sandbox(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::Guest { .. } | Self::InvalidManifest(_) | Self::InvalidRequest(_) => None,
        }
    }
}

impl From<io::Error> for PythonSpecializationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SandboxError> for PythonSpecializationError {
    fn from(error: SandboxError) -> Self {
        Self::Sandbox(error)
    }
}

impl From<serde_json::Error> for PythonSpecializationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Decode(error)
    }
}

/// Runs the embedded `CPython` guest through a selected hard sandbox.
#[derive(Debug)]
pub struct PythonSpecializer<B> {
    /// Shared bounded sandbox runner.
    runner: SandboxRunner<B>,
}

impl<B: SandboxBackend> PythonSpecializer<B> {
    /// Creates a `CPython` specializer using `backend`.
    pub const fn new(backend: B) -> Self {
        Self {
            runner: SandboxRunner::new(backend),
        }
    }

    /// Runs all requested modules in one `CPython` process.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid graph paths, unavailable hard sandboxing,
    /// guest rejection/failure, malformed JSON, or manifest validation errors.
    pub fn specialize(
        &self,
        request: &PythonSpecializationRequest,
    ) -> Result<SpecializationManifest, PythonSpecializationError> {
        validate_python_request(request)?;
        let scratch = ScratchDirectory::create()?;
        let mut policy = request.sandbox_policy.clone();
        self.runner.backend_name().clone_into(&mut policy.backend);
        policy.writable_roots = vec![scratch.path().display().to_string()];
        let input = PythonGuestInput {
            smelt_version: &request.smelt_version,
            schema_version: MANIFEST_SCHEMA_VERSION,
            project_root: &request.project_root,
            modules: &request.modules,
            hashes: &request.hashes,
            sandbox_policy: &policy,
        };
        let request_path = scratch.path().join("request.json");
        write_if_changed(&request_path, &serde_json::to_vec(&input)?)?;
        let sandbox_request = SandboxRequest {
            executable: request.python_executable.clone(),
            args: vec![
                "-I".into(),
                "-S".into(),
                "-c".into(),
                PYTHON_GUEST.into(),
                request_path.as_os_str().to_owned(),
            ],
            current_dir: request.project_root.clone(),
            scratch_dir: scratch.path().to_path_buf(),
        };
        let output = self.runner.run(&sandbox_request, &policy)?;
        if !output.status.success() {
            return Err(PythonSpecializationError::Guest {
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        decode_manifest(&output.stdout)
    }
}

/// JSON input consumed by the embedded guest.
#[derive(Serialize)]
struct PythonGuestInput<'a> {
    /// Smelt compiler version.
    smelt_version: &'a str,
    /// Manifest schema version.
    schema_version: u32,
    /// Canonical source import root.
    project_root: &'a Path,
    /// Candidate modules in import order.
    modules: &'a [PythonModule],
    /// Cache invalidation hashes.
    hashes: &'a HashInputs,
    /// Exact effective sandbox policy.
    sandbox_policy: &'a SandboxPolicyRecord,
}

/// Decodes and structurally validates guest output.
fn decode_manifest(bytes: &[u8]) -> Result<SpecializationManifest, PythonSpecializationError> {
    let manifest: SpecializationManifest = serde_json::from_slice(bytes)?;
    if manifest.language != HostLanguage::Python {
        return Err(PythonSpecializationError::InvalidRequest(
            "CPython guest emitted a non-Python manifest".to_owned(),
        ));
    }
    validate_manifest(&manifest).map_err(PythonSpecializationError::InvalidManifest)?;
    Ok(manifest)
}

/// Validates source graph paths before any host process starts.
fn validate_python_request(
    request: &PythonSpecializationRequest,
) -> Result<(), PythonSpecializationError> {
    if !request.python_executable.is_file() {
        return Err(PythonSpecializationError::InvalidRequest(format!(
            "CPython executable '{}' does not exist",
            request.python_executable.display()
        )));
    }
    if !request.project_root.is_dir() {
        return Err(PythonSpecializationError::InvalidRequest(format!(
            "project root '{}' is not a directory",
            request.project_root.display()
        )));
    }
    if request.modules.is_empty() {
        return Err(PythonSpecializationError::InvalidRequest(
            "at least one candidate module is required".to_owned(),
        ));
    }
    let canonical_root = fs::canonicalize(&request.project_root)?;
    for module in &request.modules {
        if module.name.trim().is_empty() {
            return Err(PythonSpecializationError::InvalidRequest(
                "candidate module names must be non-empty".to_owned(),
            ));
        }
        let canonical_module = fs::canonicalize(&module.path)?;
        if !canonical_module.starts_with(&canonical_root) {
            return Err(PythonSpecializationError::InvalidRequest(format!(
                "candidate module '{}' is outside project root '{}'",
                module.path.display(),
                request.project_root.display()
            )));
        }
    }
    Ok(())
}

/// Process-local scratch directory removed after guest completion.
struct ScratchDirectory {
    /// Unique directory path.
    path: PathBuf,
}

impl ScratchDirectory {
    /// Creates one unique scratch directory.
    fn create() -> Result<Self, io::Error> {
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "smelt-specialize-python-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    /// Returns the scratch path.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.path);
    }
}

/// Writes a file only when its bytes differ.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    if fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn guest_source_is_valid_python() -> Result<(), String> {
        let discovered = ["/usr/bin/python3", "/usr/local/bin/python3"]
            .iter()
            .map(Path::new)
            .find(|path| path.is_file());
        let Some(python) = discovered else {
            return Ok(());
        };
        let status = std::process::Command::new(python)
            .args([
                "-I",
                "-S",
                "-c",
                "compile(__import__('sys').stdin.read(), '<guest>', 'exec')",
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(PYTHON_GUEST.as_bytes())?;
                }
                child.wait()
            })
            .map_err(|error| error.to_string())?;
        if !status.success() {
            return Err("embedded CPython guest did not compile".to_owned());
        }
        Ok(())
    }

    #[test]
    fn guest_materializes_wrappers_descriptors_and_cycles() -> Result<(), String> {
        let Some(python) = discovered_python() else {
            return Ok(());
        };
        let manifest = run_fixture_guest(python)?;
        assert_fixture_manifest(&manifest)
    }

    #[test]
    fn sandboxed_specializer_runs_when_backend_is_available() -> Result<(), String> {
        let Some(python) = discovered_python() else {
            return Ok(());
        };
        let backend = crate::LinuxBubblewrapBackend::discover();
        if !matches!(
            backend.availability(),
            crate::BackendAvailability::Available
        ) {
            return Ok(());
        }
        let scratch = ScratchDirectory::create().map_err(|error| error.to_string())?;
        let project = scratch.path().join("sandbox-project");
        fs::create_dir(&project).map_err(|error| error.to_string())?;
        let module_path = project.join("fixture.py");
        fs::write(&module_path, "value: int = 7\n").map_err(|error| error.to_string())?;
        let mut policy = fixture_policy(&project, scratch.path());
        policy.read_only_roots = runtime_roots(&project);
        policy.writable_roots.clear();
        policy.memory_bytes = 512 * 1024 * 1024;
        let request = PythonSpecializationRequest {
            smelt_version: "test".to_owned(),
            python_executable: python.to_path_buf(),
            project_root: project,
            modules: vec![PythonModule {
                name: "fixture".to_owned(),
                path: module_path,
            }],
            hashes: fixture_hashes(),
            sandbox_policy: policy,
        };
        let manifest = PythonSpecializer::new(backend)
            .specialize(&request)
            .map_err(|error| error.to_string())?;
        if manifest.modules.len() != 1 {
            return Err("sandboxed guest did not emit one module".to_owned());
        }
        Ok(())
    }

    /// Returns existing `CPython` runtime roots plus the fixture project.
    fn runtime_roots(project: &Path) -> Vec<String> {
        [
            Some(project),
            Some(Path::new("/usr")),
            Some(Path::new("/lib")),
            Some(Path::new("/lib64")),
        ]
        .into_iter()
        .flatten()
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect()
    }

    /// Finds a test `CPython` executable.
    fn discovered_python() -> Option<&'static Path> {
        ["/usr/bin/python3", "/usr/local/bin/python3"]
            .iter()
            .map(Path::new)
            .find(|path| path.is_file())
    }

    /// Runs the embedded guest against a representative fixture.
    fn run_fixture_guest(python: &Path) -> Result<SpecializationManifest, String> {
        let scratch = ScratchDirectory::create().map_err(|error| error.to_string())?;
        let project = scratch.path().join("project");
        fs::create_dir(&project).map_err(|error| error.to_string())?;
        let module_path = project.join("fixture.py");
        fs::write(&module_path, fixture_source()).map_err(|error| error.to_string())?;
        let policy = fixture_policy(&project, scratch.path());
        let hashes = fixture_hashes();
        let modules = vec![PythonModule {
            name: "fixture".to_owned(),
            path: module_path,
        }];
        let input = PythonGuestInput {
            smelt_version: "test",
            schema_version: MANIFEST_SCHEMA_VERSION,
            project_root: &project,
            modules: &modules,
            hashes: &hashes,
            sandbox_policy: &policy,
        };
        let request_path = scratch.path().join("request.json");
        write_if_changed(
            &request_path,
            &serde_json::to_vec(&input).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let output = std::process::Command::new(python)
            .args(["-I", "-S", "-c"])
            .arg(PYTHON_GUEST)
            .arg(&request_path)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        decode_manifest(&output.stdout).map_err(|error| error.to_string())
    }

    /// Returns source covering wrappers, descriptors, and cyclic state.
    fn fixture_source() -> &'static str {
        r#"
def decorate(function):
    prefix = "value="
    def wrapper(value: int) -> str:
        return prefix + function(value)
    wrapper.__wrapped__ = function
    return wrapper

@decorate
def render(value: int) -> str:
    return str(value)

class Descriptor:
    def __set_name__(self, owner, name):
        self.name = name
    def __get__(self, instance, owner) -> int:
        return 1

class Model:
    field: int = Descriptor()
    _value: int = 1

    @property
    def value(self) -> int:
        return self._value

    @value.setter
    def value(self, value: int) -> None:
        self._value = value

cycle = []
cycle.append(cycle)
"#
    }

    /// Builds a representative restrictive policy.
    fn fixture_policy(project: &Path, scratch: &Path) -> SandboxPolicyRecord {
        SandboxPolicyRecord {
            backend: "test".to_owned(),
            network: false,
            read_only_roots: vec![project.display().to_string(), "/usr".to_owned()],
            writable_roots: vec![scratch.display().to_string()],
            environment: BTreeMap::new(),
            wall_time_ms: 1_000,
            cpu_time_ms: 1_000,
            memory_bytes: 128 * 1024 * 1024,
            process_limit: 1,
            output_bytes: 1024 * 1024,
            subprocesses: false,
            native_extensions: Vec::new(),
        }
    }

    /// Builds stable placeholder hashes for guest behavior tests.
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

    /// Verifies the representative fixture's materialized shape.
    fn assert_fixture_manifest(manifest: &SpecializationManifest) -> Result<(), String> {
        let module = manifest
            .modules
            .first()
            .ok_or_else(|| "guest emitted no module record".to_owned())?;
        let render = module
            .definitions
            .iter()
            .find(|definition| definition.binding_name == "render")
            .ok_or_else(|| "decorated binding was not materialized".to_owned())?;
        let crate::Definition::Function(function) = &render.definition else {
            return Err("decorated binding did not remain callable".to_owned());
        };
        if function.wrapper_chain.is_empty() {
            return Err("decorator wrapper chain was not preserved".to_owned());
        }
        let model = module
            .definitions
            .iter()
            .find(|definition| definition.binding_name == "Model")
            .ok_or_else(|| "class binding was not materialized".to_owned())?;
        let crate::Definition::Class(class) = &model.definition else {
            return Err("Model binding was not a class".to_owned());
        };
        if class
            .descriptors
            .iter()
            .all(|descriptor| descriptor.name != "field")
        {
            return Err("typed descriptor was not materialized".to_owned());
        }
        assert_property_descriptor(manifest, class)?;
        let cycle_id = module
            .globals
            .get("cycle")
            .ok_or_else(|| "cycle global was not serialized".to_owned())?;
        let cycle = manifest
            .values
            .nodes
            .iter()
            .find(|node| node.id == *cycle_id)
            .ok_or_else(|| "cycle graph node was missing".to_owned())?;
        if !matches!(
            &cycle.value,
            crate::GraphValueKind::List(values) if values == &vec![*cycle_id]
        ) {
            return Err("self-referential list identity was not preserved".to_owned());
        }
        Ok(())
    }

    /// Verifies property accessor provenance and non-opaque graph state.
    fn assert_property_descriptor(
        manifest: &SpecializationManifest,
        class: &crate::ClassDefinition,
    ) -> Result<(), String> {
        let property = class
            .descriptors
            .iter()
            .find(|descriptor| descriptor.name == "value")
            .ok_or_else(|| "property descriptor was not materialized".to_owned())?;
        if property.getter.is_none() || property.setter.is_none() {
            return Err("property accessor provenance was not preserved".to_owned());
        }
        let property_value = manifest
            .values
            .nodes
            .iter()
            .find(|node| node.id == property.value)
            .ok_or_else(|| "property value graph node was missing".to_owned())?;
        if !matches!(
            &property_value.value,
            crate::GraphValueKind::Instance {
                class: property_class,
                fields,
            } if property_class == "builtins.property" && fields.is_empty()
        ) {
            return Err("property value must not retain opaque CPython callables".to_owned());
        }
        Ok(())
    }
}
