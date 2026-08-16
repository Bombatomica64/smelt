//! Batched Node guest orchestration for materialized TypeScript modules.

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

/// Embedded, versioned Node specialization guest.
const NODE_GUEST: &str = include_str!("../guest/node.js");

/// Monotonic suffix for Node scratch directories.
static NODE_SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One TypeScript source module and its canonical `CommonJS` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeModule {
    /// Stable source-graph module name.
    pub name: String,
    /// Canonical TypeScript source path.
    pub source_path: PathBuf,
    /// Canonical JavaScript artifact emitted by the configured TypeScript toolchain.
    pub emitted_path: PathBuf,
    /// Source map emitted by the same canonical TypeScript invocation.
    pub source_map_path: Option<PathBuf>,
}

/// Inputs for one batched sandboxed Node specialization process.
#[derive(Debug, Clone)]
pub struct NodeSpecializationRequest {
    /// Smelt compiler version.
    pub smelt_version: String,
    /// Node executable inside the read-only runtime roots.
    pub node_executable: PathBuf,
    /// Canonical project and emitted-artifact root.
    pub project_root: PathBuf,
    /// Candidate modules in dependency order.
    pub modules: Vec<NodeModule>,
    /// Canonical TypeScript compiler version used to emit the artifacts.
    pub typescript_version: String,
    /// Pinned TC39 decorators proposal revision.
    pub decorators_revision: String,
    /// Precomputed invalidation hashes.
    pub hashes: HashInputs,
    /// Effective hard-sandbox policy except the per-run scratch path.
    pub sandbox_policy: SandboxPolicyRecord,
}

/// Node specialization failure.
#[derive(Debug)]
pub enum NodeSpecializationError {
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

impl fmt::Display for NodeSpecializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Node specialization I/O failed: {error}"),
            Self::Sandbox(error) => {
                write!(formatter, "Node specialization sandbox failed: {error}")
            }
            Self::Guest { status, stderr } => write!(
                formatter,
                "Node specialization guest exited with status {}: {stderr}",
                status.map_or_else(|| "signal".to_owned(), |code| code.to_string())
            ),
            Self::Decode(error) => {
                write!(
                    formatter,
                    "Node specialization manifest was invalid JSON: {error}"
                )
            }
            Self::InvalidManifest(errors) => write!(
                formatter,
                "Node specialization manifest failed validation: {}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Self::InvalidRequest(message) => {
                write!(formatter, "invalid Node specialization request: {message}")
            }
        }
    }
}

impl std::error::Error for NodeSpecializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sandbox(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::Guest { .. } | Self::InvalidManifest(_) | Self::InvalidRequest(_) => None,
        }
    }
}

impl From<io::Error> for NodeSpecializationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SandboxError> for NodeSpecializationError {
    fn from(error: SandboxError) -> Self {
        Self::Sandbox(error)
    }
}

impl From<serde_json::Error> for NodeSpecializationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Decode(error)
    }
}

/// Runs emitted TypeScript modules through one sandboxed Node guest.
#[derive(Debug)]
pub struct NodeSpecializer<B> {
    /// Shared bounded sandbox runner.
    runner: SandboxRunner<B>,
}

impl<B: SandboxBackend> NodeSpecializer<B> {
    /// Creates a Node specializer using `backend`.
    pub const fn new(backend: B) -> Self {
        Self {
            runner: SandboxRunner::new(backend),
        }
    }

    /// Materializes every requested module in one Node process.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid paths, unavailable sandboxing, failed guest
    /// execution, malformed JSON, or a structurally invalid manifest.
    pub fn specialize(
        &self,
        request: &NodeSpecializationRequest,
    ) -> Result<SpecializationManifest, NodeSpecializationError> {
        validate_node_request(request)?;
        let scratch = NodeScratchDirectory::create()?;
        let mut policy = request.sandbox_policy.clone();
        self.runner.backend_name().clone_into(&mut policy.backend);
        policy.writable_roots = vec![scratch.path().display().to_string()];
        let mut manifest_policy = policy.clone();
        manifest_policy.writable_roots = vec!["<scratch>".to_owned()];
        let input = NodeGuestInput {
            smelt_version: &request.smelt_version,
            schema_version: MANIFEST_SCHEMA_VERSION,
            project_root: &request.project_root,
            modules: &request.modules,
            typescript_version: &request.typescript_version,
            decorators_revision: &request.decorators_revision,
            hashes: &request.hashes,
            sandbox_policy: &manifest_policy,
        };
        let request_path = scratch.path().join("request.json");
        let guest_path = scratch.path().join("node-guest.js");
        write_if_changed(&request_path, &serde_json::to_vec(&input)?)?;
        write_if_changed(&guest_path, NODE_GUEST.as_bytes())?;
        let mut args = vec!["--permission".into(), "--no-addons".into()];
        for root in policy
            .read_only_roots
            .iter()
            .map(PathBuf::from)
            .chain(std::iter::once(scratch.path().to_path_buf()))
        {
            args.push(format!("--allow-fs-read={}", root.display()).into());
        }
        args.extend([
            guest_path.as_os_str().to_owned(),
            request_path.as_os_str().to_owned(),
        ]);
        let sandbox_request = SandboxRequest {
            executable: request.node_executable.clone(),
            args,
            current_dir: request.project_root.clone(),
            scratch_dir: scratch.path().to_path_buf(),
            internal_threads: true,
        };
        let output = self.runner.run(&sandbox_request, &policy)?;
        if !output.status.success() {
            return Err(NodeSpecializationError::Guest {
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        decode_manifest(&output.stdout)
    }
}

/// JSON input consumed by the embedded Node guest.
#[derive(Serialize)]
struct NodeGuestInput<'request> {
    /// Smelt compiler version.
    smelt_version: &'request str,
    /// Manifest schema version.
    schema_version: u32,
    /// Project root visible to the guest.
    project_root: &'request Path,
    /// Candidate source/artifact pairs.
    modules: &'request [NodeModule],
    /// TypeScript compiler version.
    typescript_version: &'request str,
    /// Decorators proposal revision.
    decorators_revision: &'request str,
    /// Cache hash inputs.
    hashes: &'request HashInputs,
    /// Effective sandbox policy.
    sandbox_policy: &'request SandboxPolicyRecord,
}

/// Decodes and validates Node guest output.
fn decode_manifest(bytes: &[u8]) -> Result<SpecializationManifest, NodeSpecializationError> {
    let manifest: SpecializationManifest = serde_json::from_slice(bytes)?;
    if manifest.language != HostLanguage::TypeScript {
        return Err(NodeSpecializationError::InvalidRequest(
            "Node guest emitted a non-TypeScript manifest".to_owned(),
        ));
    }
    validate_manifest(&manifest).map_err(NodeSpecializationError::InvalidManifest)?;
    Ok(manifest)
}

/// Validates Node runtime and source/artifact graph paths.
fn validate_node_request(
    request: &NodeSpecializationRequest,
) -> Result<(), NodeSpecializationError> {
    if !request.node_executable.is_file() {
        return Err(NodeSpecializationError::InvalidRequest(format!(
            "Node executable '{}' does not exist",
            request.node_executable.display()
        )));
    }
    if !request.project_root.is_dir() {
        return Err(NodeSpecializationError::InvalidRequest(format!(
            "project root '{}' is not a directory",
            request.project_root.display()
        )));
    }
    if request.modules.is_empty() {
        return Err(NodeSpecializationError::InvalidRequest(
            "at least one candidate module is required".to_owned(),
        ));
    }
    if request.typescript_version.trim().is_empty() {
        return Err(NodeSpecializationError::InvalidRequest(
            "TypeScript compiler version must be recorded".to_owned(),
        ));
    }
    if request.decorators_revision.trim().is_empty() {
        return Err(NodeSpecializationError::InvalidRequest(
            "decorators proposal revision must be pinned".to_owned(),
        ));
    }
    let canonical_root = fs::canonicalize(&request.project_root)?;
    for module in &request.modules {
        if module.name.trim().is_empty() {
            return Err(NodeSpecializationError::InvalidRequest(
                "candidate module names must be non-empty".to_owned(),
            ));
        }
        for path in [&module.source_path, &module.emitted_path]
            .into_iter()
            .chain(module.source_map_path.as_ref())
        {
            let canonical = fs::canonicalize(path)?;
            if !canonical.starts_with(&canonical_root) {
                return Err(NodeSpecializationError::InvalidRequest(format!(
                    "candidate path '{}' is outside project root '{}'",
                    path.display(),
                    request.project_root.display()
                )));
            }
        }
    }
    Ok(())
}

/// Process-local Node scratch directory removed after guest completion.
struct NodeScratchDirectory {
    /// Unique directory path.
    path: PathBuf,
}

impl NodeScratchDirectory {
    /// Creates one unique scratch directory.
    fn create() -> Result<Self, io::Error> {
        let sequence = NODE_SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "smelt-specialize-node-{}-{sequence}",
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

impl Drop for NodeScratchDirectory {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.path);
    }
}

/// Writes a Node guest input or script only when bytes differ.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    if fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, process::Command};

    use super::*;
    use crate::prereq::missing_prerequisite;

    #[test]
    fn guest_source_is_valid_javascript() -> Result<(), String> {
        let Some(node) = discovered_node() else {
            return missing_prerequisite("guest_source_is_valid_javascript", "node executable");
        };
        let scratch = NodeScratchDirectory::create().map_err(|error| error.to_string())?;
        let guest = scratch.path().join("guest.js");
        write_if_changed(&guest, NODE_GUEST.as_bytes()).map_err(|error| error.to_string())?;
        let output = Command::new(node)
            .args(["--check"])
            .arg(guest)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(())
    }

    #[test]
    fn sandboxed_guest_materializes_decorated_class_metadata_and_cycles() -> Result<(), String> {
        const TEST: &str = "sandboxed_guest_materializes_decorated_class_metadata_and_cycles";
        let Some(node) = discovered_node() else {
            return missing_prerequisite(TEST, "node executable");
        };
        let Some(tsc) = discovered_tsc() else {
            return missing_prerequisite(TEST, "tsc (TypeScript compiler)");
        };
        let Some(backend) = available_backend() else {
            return missing_prerequisite(TEST, "bubblewrap hard sandbox (bwrap + prlimit)");
        };
        let project = tempfile::tempdir().map_err(|error| error.to_string())?;
        let source = project.path().join("fixture.ts");
        fs::write(&source, DECORATED_FIXTURE).map_err(|error| error.to_string())?;
        let emitted_dir = project.path().join("dist");
        let compile = Command::new(tsc)
            .arg(&source)
            .args(["--target", "ES2022", "--module", "commonjs", "--outDir"])
            .arg(&emitted_dir)
            .args(["--sourceMap", "--skipLibCheck", "--pretty", "false"])
            .output()
            .map_err(|error| error.to_string())?;
        if !compile.status.success() {
            // tsc reports diagnostics on stdout and leaves stderr empty, so
            // reading stderr alone turns every compile failure into an
            // inscrutable empty error. Include both.
            return Err(format!(
                "TypeScript fixture failed to compile:\n{}{}",
                String::from_utf8_lossy(&compile.stdout),
                String::from_utf8_lossy(&compile.stderr)
            ));
        }
        let emitted = emitted_dir.join("fixture.js");
        let source_map = emitted_dir.join("fixture.js.map");
        let request = NodeSpecializationRequest {
            smelt_version: "test".to_owned(),
            node_executable: node.clone(),
            project_root: project.path().to_path_buf(),
            modules: vec![NodeModule {
                name: "fixture".to_owned(),
                source_path: source,
                emitted_path: emitted,
                source_map_path: Some(source_map),
            }],
            typescript_version: "5.9.3".to_owned(),
            decorators_revision: "2023-11".to_owned(),
            hashes: fixture_hashes(),
            sandbox_policy: fixture_policy(project.path(), &node),
        };
        let manifest = NodeSpecializer::new(backend)
            .specialize(&request)
            .map_err(|error| error.to_string())?;
        assert_decorated_manifest(&manifest)
    }

    #[test]
    fn sandboxed_guest_rejects_legacy_decorator_abi() -> Result<(), String> {
        const TEST: &str = "sandboxed_guest_rejects_legacy_decorator_abi";
        let Some(node) = discovered_node() else {
            return missing_prerequisite(TEST, "node executable");
        };
        let Some(backend) = available_backend() else {
            return missing_prerequisite(TEST, "bubblewrap hard sandbox (bwrap + prlimit)");
        };
        let project = tempfile::tempdir().map_err(|error| error.to_string())?;
        let source = project.path().join("legacy.ts");
        let emitted = project.path().join("legacy.js");
        fs::write(&source, "@legacy class Example {}").map_err(|error| error.to_string())?;
        fs::write(
            &emitted,
            "const Example = __decorate([], class Example {}); module.exports = { Example };",
        )
        .map_err(|error| error.to_string())?;
        let request = NodeSpecializationRequest {
            smelt_version: "test".to_owned(),
            node_executable: node.clone(),
            project_root: project.path().to_path_buf(),
            modules: vec![NodeModule {
                name: "legacy".to_owned(),
                source_path: source,
                emitted_path: emitted,
                source_map_path: None,
            }],
            typescript_version: "5.9.3".to_owned(),
            decorators_revision: "2023-11".to_owned(),
            hashes: fixture_hashes(),
            sandbox_policy: fixture_policy(project.path(), &node),
        };
        let Err(error) = NodeSpecializer::new(backend).specialize(&request) else {
            return Err("legacy decorators unexpectedly specialized".to_owned());
        };
        if error
            .to_string()
            .contains("smelt::typescript-legacy-decorators")
        {
            Ok(())
        } else {
            Err(format!("unexpected legacy decorator diagnostic: {error}"))
        }
    }

    #[test]
    fn sandboxed_guest_rejects_accessors_and_proxies_as_dynamic_objects() -> Result<(), String> {
        const TEST: &str = "sandboxed_guest_rejects_accessors_and_proxies_as_dynamic_objects";
        let Some(node) = discovered_node() else {
            return missing_prerequisite(TEST, "node executable");
        };
        let Some(backend) = available_backend() else {
            return missing_prerequisite(TEST, "bubblewrap hard sandbox (bwrap + prlimit)");
        };
        for (label, emitted_source) in [
            (
                "accessor",
                "const dynamic = {}; Object.defineProperty(dynamic, 'computed', { get() { throw new Error('boom'); } }); module.exports = { dynamic };",
            ),
            (
                "proxy",
                "const dynamic = new Proxy({}, {}); module.exports = { dynamic };",
            ),
        ] {
            let project = tempfile::tempdir().map_err(|error| error.to_string())?;
            let source = project.path().join("dynamic.ts");
            let emitted = project.path().join("dynamic.js");
            fs::write(&source, "export const dynamic: object = {};")
                .map_err(|error| error.to_string())?;
            fs::write(&emitted, emitted_source).map_err(|error| error.to_string())?;
            let request = NodeSpecializationRequest {
                smelt_version: "test".to_owned(),
                node_executable: node.clone(),
                project_root: project.path().to_path_buf(),
                modules: vec![NodeModule {
                    name: "dynamic".to_owned(),
                    source_path: source,
                    emitted_path: emitted,
                    source_map_path: None,
                }],
                typescript_version: "5.9.3".to_owned(),
                decorators_revision: "2023-11".to_owned(),
                hashes: fixture_hashes(),
                sandbox_policy: fixture_policy(project.path(), &node),
            };
            let Err(error) = NodeSpecializer::new(backend.clone()).specialize(&request) else {
                return Err(format!("{label} object unexpectedly specialized"));
            };
            if !error
                .to_string()
                .contains("smelt::dynamic-attribute-access")
            {
                return Err(format!("unexpected {label} diagnostic: {error}"));
            }
        }
        Ok(())
    }

    /// Locates the Node executable used by the guest tests.
    ///
    /// Searches `PATH` before the distro paths so a runner-installed toolchain
    /// (GitHub's `setup-node` uses the hosted tool cache) is found.
    fn discovered_node() -> Option<PathBuf> {
        crate::prereq::locate_executable("node", &["/usr/bin/node", "/usr/local/bin/node"])
    }

    /// Locates the TypeScript compiler used to build guest fixtures.
    fn discovered_tsc() -> Option<PathBuf> {
        crate::prereq::locate_executable("tsc", &["/usr/local/bin/tsc", "/usr/bin/tsc"])
    }

    /// Returns the hard-sandbox backend when it can actually enforce a policy.
    fn available_backend() -> Option<crate::LinuxBubblewrapBackend> {
        let backend = crate::LinuxBubblewrapBackend::discover();
        (backend.availability() == crate::BackendAvailability::Available).then_some(backend)
    }

    /// Representative standard-decorator TypeScript fixture.
    const DECORATED_FIXTURE: &str = r#"
(Symbol as any).metadata ??= Symbol("metadata");

function decorate(value: any, context: ClassDecoratorContext) {
    (context.metadata as any).label = "example";
    context.addInitializer(function () {
        (this as any).ready = "yes";
    });
}

function decorateMember(value: any, context: any): any {
    context.metadata[`${context.kind}:${String(context.name)}`] = true;
    context.addInitializer(function (this: any) {
        this.memberInitialized = true;
    });
    if (context.kind === "field") {
        return function (initial: any) {
            return initial;
        };
    }
    if (context.kind === "accessor") {
        return {
            get: value.get,
            set: value.set,
            init(initial: any) {
                return initial;
            },
        };
    }
    return value;
}

function decorateFactory(_label: string) {
    return decorateMember;
}

function replaceClass(_value: any, _context: ClassDecoratorContext) {
    return class Replacement {
        static marker = "replacement";
    };
}

@decorate
export class Example {
    @decorateMember
    field = 1;

    @decorateMember
    accessor count = 2;

    @decorateMember
    static label = "static";

    @decorateMember
    #secret = 3;

    @decorateMember
    @decorateFactory("stacked")
    greet(name: string): string {
        return `hello ${name}`;
    }

    @decorateMember
    #hidden(): string {
        return "hidden";
    }

    @decorateMember
    static ping(): string {
        return "pong";
    }

    @decorateMember
    accessor #privateCount = 4;

    @decorateMember
    get title(): string {
        return "title";
    }

    @decorateMember
    set title(_value: string) {}

    @decorateMember
    static get version(): string {
        return "1";
    }

    @decorateMember
    static set version(_value: string) {}
}

@replaceClass
export class Replaced {}

export const cycle: any = {};
cycle.self = cycle;
"#;

    /// Restrictive sandbox policy for the Node fixture.
    ///
    /// `node` is passed in so its installation prefix is bound read-only.
    /// Hardcoding `/usr` only works for a distro-installed runtime, and the
    /// runtimes that matter here are not: `setup-node` uses the hosted tool
    /// cache and this repo's dev container ships Node at `/opt/node22`.
    fn fixture_policy(project: &Path, node: &Path) -> SandboxPolicyRecord {
        SandboxPolicyRecord {
            backend: "linux-bubblewrap".to_owned(),
            network: false,
            read_only_roots: [
                Some(project.to_path_buf()),
                crate::prereq::runtime_prefix(node),
                Some(PathBuf::from("/usr")),
                Some(PathBuf::from("/lib")),
                Some(PathBuf::from("/lib64")),
            ]
            .into_iter()
            .flatten()
            .filter(|path| path.exists())
            .map(|path| path.display().to_string())
            .collect(),
            writable_roots: Vec::new(),
            environment: BTreeMap::new(),
            wall_time_ms: 5_000,
            cpu_time_ms: 5_000,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            process_limit: 64,
            output_bytes: 4 * 1024 * 1024,
            subprocesses: false,
            native_extensions: Vec::new(),
        }
    }

    /// Stable placeholder hashes for the Node fixture.
    fn fixture_hashes() -> HashInputs {
        HashInputs {
            source: "node-source".to_owned(),
            dependencies: "node-dependencies".to_owned(),
            lockfile: "node-lockfile".to_owned(),
            environment: "node-environment".to_owned(),
            policy: "node-policy".to_owned(),
            callable_provenance: "node-provenance".to_owned(),
        }
    }

    /// Verifies decorated static state, metadata, methods, and cyclic identity.
    #[expect(
        clippy::too_many_lines,
        reason = "the assertion reports every missing decorator target in one fixture"
    )]
    fn assert_decorated_manifest(manifest: &SpecializationManifest) -> Result<(), String> {
        let module = manifest
            .modules
            .first()
            .ok_or_else(|| "Node guest emitted no module".to_owned())?;
        let example_record = module
            .definitions
            .iter()
            .find(|definition| definition.binding_name == "Example")
            .ok_or_else(|| "decorated Example class is absent".to_owned())?;
        let crate::Definition::Class(example) = &example_record.definition else {
            return Err("Example export is not a class".to_owned());
        };
        if example.methods.iter().all(|method| method.name != "greet") {
            return Err("decorated class method is absent".to_owned());
        }
        if example.methods.iter().all(|method| method.name != "#hidden") {
            return Err("decorated private method is absent".to_owned());
        }
        if example.methods.iter().all(|method| method.name != "ping") {
            return Err("decorated static method is absent".to_owned());
        }
        if example.fields.iter().all(|field| field.name != "field")
            || example.fields.iter().all(|field| field.name != "count")
            || example
                .fields
                .iter()
                .all(|field| field.name != "#secret" || !field.is_private)
            || example
                .fields
                .iter()
                .all(|field| field.name != "label" || !field.is_static)
        {
            return Err(format!(
                "decorated field targets are incomplete: {:?}",
                example.fields
            ));
        }
        if example
            .descriptors
            .iter()
            .all(|descriptor| descriptor.name != "count")
        {
            return Err("decorated auto-accessor is absent".to_owned());
        }
        if example
            .descriptors
            .iter()
            .all(|descriptor| descriptor.name != "version" || !descriptor.is_static)
        {
            return Err("decorated static accessor is absent".to_owned());
        }
        if example
            .descriptors
            .iter()
            .all(|descriptor| descriptor.name != "#privateCount")
        {
            return Err("decorated private auto-accessor is absent".to_owned());
        }
        if example.initializers.is_empty()
            || example
                .initializers
                .iter()
                .all(|initializer| initializer.kind != crate::InitializerKind::Static)
            || example
                .initializers
                .iter()
                .all(|initializer| initializer.kind != crate::InitializerKind::Instance)
        {
            return Err(format!(
                "decorator initializers are incomplete: {:?}",
                example.initializers
            ));
        }
        let source = DECORATED_FIXTURE;
        if example.initializers.iter().all(|initializer| {
            let start = usize::try_from(initializer.callable.span.start).unwrap_or(usize::MAX);
            let end = usize::try_from(initializer.callable.span.end).unwrap_or(usize::MAX);
            let line_start = source
                .get(..start)
                .and_then(|prefix| prefix.rfind('\n'))
                .map_or(0, |newline| newline.saturating_add(1));
            source.get(line_start..end).is_none_or(|text| {
                !text.contains("addInitializer")
            })
        }) {
            return Err(format!(
                "initializer provenance did not map back to its nested TypeScript source: {:?}",
                example.initializers
            ));
        }
        if !example.static_values.contains_key("ready") {
            return Err("class addInitializer static state is absent".to_owned());
        }
        if !example
            .initializers
            .windows(2)
            .all(|pair| pair[0].order < pair[1].order)
        {
            return Err(format!(
                "decorator initializer order is unstable: {:?}",
                example.initializers
            ));
        }
        if example
            .metadata
            .iter()
            .all(|metadata| metadata.key != "typescript.decorators")
        {
            return Err("decorator metadata is absent".to_owned());
        }
        let cycle = module
            .globals
            .get("cycle")
            .and_then(|value| usize::try_from(value.0).ok())
            .and_then(|index| manifest.values.nodes.get(index))
            .ok_or_else(|| "cycle export graph node is absent".to_owned())?;
        let crate::GraphValueKind::Instance { fields, .. } = &cycle.value else {
            return Err("cycle export is not an object instance".to_owned());
        };
        if fields.get("self") != Some(&cycle.id) {
            return Err("cycle export did not preserve self identity".to_owned());
        }
        if !manifest.required_adapters.is_empty() {
            return Err(format!(
                "source-defined decorated fixture requested adapters: {:?}",
                manifest.required_adapters
            ));
        }
        let replaced = module
            .definitions
            .iter()
            .find(|definition| definition.binding_name == "Replaced")
            .ok_or_else(|| "replacement constructor binding is absent".to_owned())?;
        let crate::Definition::Class(replaced) = &replaced.definition else {
            return Err("replacement constructor did not materialize a class".to_owned());
        };
        if !replaced.static_values.contains_key("marker")
            || replaced.constructor.replacement.is_none()
        {
            return Err("replacement constructor state or provenance is absent".to_owned());
        }
        Ok(())
    }
}
