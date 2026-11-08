//! Host-runtime specialization orchestration for manifest builds.
//!
//! Frontends remain pure: this module owns detection, cache lookup, sandboxed
//! guest execution, and publication of reusable package artifacts.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest as _, Sha256};
use smelt_specialize::{
    CacheKeyInput, HashInputs, HostLanguage, ModuleInput, NodeModule, ProjectRoot,
    NodeSpecializationRequest, NodeSpecializer, PythonModule, PythonSpecializationRequest,
    PythonSpecializer, SandboxBackend, SandboxPolicyRecord, SpecializationArtifactStore,
    OciSandboxBackend, SpecializationCacheKey, SpecializationManifest, platform_backend,
    detect_specialization_modules,
};

use crate::{
    config::Config,
    manifest::ManifestSource,
    source_files::{SourceLang, exclusion_base, resolve_manifest_path},
};

/// How one language's specialization batch was resolved.
#[derive(Debug)]
pub(crate) enum Batch {
    /// The source graph has no specialization candidates in this language.
    NotNeeded,
    /// Loaded from the project cache or the emitted package artifact.
    Cached(SpecializationManifest),
    /// Produced by running the sandboxed host runtime in this build.
    Fresh(SpecializationManifest),
}

impl Batch {
    /// The materialized manifest, unless the batch was not needed.
    pub(crate) const fn manifest(&self) -> Option<&SpecializationManifest> {
        match self {
            Self::NotNeeded => None,
            Self::Cached(manifest) | Self::Fresh(manifest) => Some(manifest),
        }
    }
}

/// Per-language manifests supplied to pure frontend entry points.
///
/// Only [`prepare`] builds one, so a manifest lowering that takes it by type
/// cannot run before specialization was resolved: "no candidates" is
/// [`Batch::NotNeeded`], never a forgotten or skipped preparation.
#[derive(Debug)]
pub(crate) struct PreparedSpecialization {
    /// Materialized Python source graph.
    python: Batch,
    /// Materialized TypeScript source graph.
    typescript: Batch,
}

impl PreparedSpecialization {
    /// Nothing to specialize: a run with no manifest to cache against.
    pub(crate) const fn none() -> Self {
        Self { python: Batch::NotNeeded, typescript: Batch::NotNeeded }
    }

    /// The Python manifest, when the batch was needed.
    pub(crate) const fn python(&self) -> Option<&SpecializationManifest> {
        self.python.manifest()
    }

    /// The TypeScript manifest, when the batch was needed.
    pub(crate) const fn typescript(&self) -> Option<&SpecializationManifest> {
        self.typescript.manifest()
    }
}

/// Detects candidates and resolves each language batch through cache or sandbox.
pub(crate) fn prepare(
    config: &Config,
    manifest_path: &Path,
    sources: &[&ManifestSource],
) -> Result<PreparedSpecialization, Box<dyn std::error::Error>> {
    let inputs = detection_inputs(sources);
    let detections = detect_specialization_modules(&inputs);
    if detections.is_empty() {
        return Ok(PreparedSpecialization { python: Batch::NotNeeded, typescript: Batch::NotNeeded });
    }

    // `exclusion_base` rather than a bare `parent().canonicalize()?`: the
    // parent of a manifest named with no directory component is `""`, which
    // canonicalizes to an error and failed the whole pass.
    let manifest_dir =
        exclusion_base(manifest_path.parent().unwrap_or_else(|| Path::new(".")));
    let output_dir = resolve_manifest_path(&manifest_dir, config.output_target());
    let cache =
        SpecializationArtifactStore::new(manifest_dir.join(".smelt").join("specialization-cache"));
    let package =
        SpecializationArtifactStore::new(output_dir.join(".smelt").join("specialization"));
    let candidate_paths = detections
        .into_iter()
        .map(|detection| (detection.path, detection.language))
        .collect::<BTreeSet<_>>();

    let host = Host { project_root: &manifest_dir, fallback: oci_fallback(config) };
    let python_sources = candidates_for(HostLanguage::Python, sources, &candidate_paths);
    let typescript_sources = candidates_for(HostLanguage::TypeScript, sources, &candidate_paths);

    let python = if python_sources.is_empty() {
        Batch::NotNeeded
    } else {
        prepare_python(&host, &cache, &package, sources, &python_sources)?
    };
    let typescript = if typescript_sources.is_empty() {
        Batch::NotNeeded
    } else {
        prepare_typescript(&host, &cache, &package, sources, &typescript_sources)?
    };
    Ok(PreparedSpecialization { python, typescript })
}

/// Converts the resolved source graph into centralized detector inputs.
fn detection_inputs(sources: &[&ManifestSource]) -> Vec<ModuleInput> {
    sources
        .iter()
        .filter_map(|source| {
            let language = source_language(source.lang)?;
            Some(ModuleInput {
                path: normalized_path(&source.path),
                language,
                source: source.source.clone(),
                dependencies: source
                    .dependencies
                    .iter()
                    .map(|path| normalized_path(path))
                    .collect(),
            })
        })
        .collect()
}

/// Returns candidates in source-graph order for one language.
fn candidates_for<'source>(
    language: HostLanguage,
    sources: &'source [&ManifestSource],
    candidate_paths: &BTreeSet<(String, HostLanguage)>,
) -> Vec<&'source ManifestSource> {
    sources
        .iter()
        .copied()
        .filter(|source| {
            candidate_paths.contains(&(normalized_path(&source.path), language))
                && source_language(source.lang) == Some(language)
        })
        .collect()
}

/// Where a batch runs: the project, and the container to fall back to.
struct Host<'project> {
    /// The project directory.
    project_root: &'project Path,
    /// `[specialization.oci-fallback]`, when configured.
    fallback: Option<OciSandboxBackend>,
}

impl Host<'_> {
    /// The sandbox backend batches run in (see `smelt_specialize::platform_backend`).
    fn backend(&self) -> Box<dyn SandboxBackend> {
        platform_backend(self.fallback.clone())
    }
}

/// The configured OCI fallback, its runtime resolved on `PATH` when it is a name.
fn oci_fallback(config: &Config) -> Option<OciSandboxBackend> {
    config.oci_fallback().map(|(runtime, image)| {
        let path = Path::new(runtime);
        let resolved = if path.components().count() > 1 {
            path.to_path_buf()
        } else {
            smelt_specialize::prereq::locate_executable(runtime, &[]).unwrap_or_else(|| path.to_path_buf())
        };
        OciSandboxBackend::new(resolved, image.to_owned())
    })
}

/// Resolves one Python batch from cache or a sandboxed `CPython` process.
fn prepare_python(
    host: &Host<'_>,
    cache: &SpecializationArtifactStore,
    package: &SpecializationArtifactStore,
    graph: &[&ManifestSource],
    candidates: &[&ManifestSource],
) -> Result<Batch, Box<dyn std::error::Error>> {
    let project_root = host.project_root;
    let python = find_executable("python3")?;
    let runtime_version = command_version(&python, &["--version"])?;
    let policy = sandbox_policy(host, &python, false);
    let project = ProjectRoot::new(project_root)?;
    let hashes = invalidation_hashes(&project, graph, &policy)?;
    let key = SpecializationCacheKey::compute(&CacheKeyInput::new(
        HostLanguage::Python,
        runtime_version,
        hashes.clone(),
        &policy,
        &project,
    ))?;
    if let Some(manifest) = load_cached(cache, package, HostLanguage::Python, &key)? {
        return Ok(Batch::Cached(manifest));
    }

    let modules = candidates
        .iter()
        .map(|source| {
            Ok(PythonModule {
                name: python_module_name(project_root, &source.path)?,
                path: source.path.canonicalize()?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let manifest = PythonSpecializer::new(host.backend()).specialize(
        &PythonSpecializationRequest {
            smelt_version: env!("CARGO_PKG_VERSION").to_owned(),
            python_executable: python,
            project_root: project_root.to_path_buf(),
            import_paths: Vec::new(),
            modules,
            hashes,
            sandbox_policy: policy,
        },
    )?;
    persist_artifact(cache, package, &key, &manifest)?;
    Ok(Batch::Fresh(manifest))
}

/// Resolves one TypeScript batch from cache or canonical tsc + sandboxed Node.
fn prepare_typescript(
    host: &Host<'_>,
    cache: &SpecializationArtifactStore,
    package: &SpecializationArtifactStore,
    graph: &[&ManifestSource],
    candidates: &[&ManifestSource],
) -> Result<Batch, Box<dyn std::error::Error>> {
    let project_root = host.project_root;
    let node = find_executable("node")?;
    let tsc = find_executable("tsc")?;
    let node_version = command_version(&node, &["--version"])?;
    let typescript_version = command_version(&tsc, &["--version"])?
        .trim_start_matches("Version ")
        .to_owned();
    let host_runtime_version =
        format!("{node_version}; TypeScript {typescript_version}; decorators 2023-11");
    let artifact_root = project_root
        .join(".smelt")
        .join("typescript-specialization");
    let mut policy = sandbox_policy(host, &node, true);
    policy
        .read_only_roots
        .push(artifact_root.display().to_string());
    policy.read_only_roots.sort();
    policy.read_only_roots.dedup();
    let project = ProjectRoot::new(project_root)?;
    let hashes = invalidation_hashes(&project, graph, &policy)?;
    let key = SpecializationCacheKey::compute(&CacheKeyInput::new(
        HostLanguage::TypeScript,
        host_runtime_version,
        hashes.clone(),
        &policy,
        &project,
    ))?;
    if let Some(manifest) = load_cached(cache, package, HostLanguage::TypeScript, &key)? {
        return Ok(Batch::Cached(manifest));
    }

    let emitted_root = artifact_root.join(key.as_str());
    compile_typescript_graph(&tsc, project_root, &emitted_root, graph)?;
    let modules = candidates
        .iter()
        .map(|source| {
            let relative = source
                .path
                .canonicalize()?
                .strip_prefix(project_root)?
                .to_path_buf();
            let emitted_path = emitted_root.join(&relative).with_extension("js");
            Ok(NodeModule {
                name: relative
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/"),
                source_path: source.path.canonicalize()?,
                source_map_path: Some(emitted_path.with_extension("js.map")),
                emitted_path,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let manifest = NodeSpecializer::new(host.backend()).specialize(
        &NodeSpecializationRequest {
            smelt_version: env!("CARGO_PKG_VERSION").to_owned(),
            node_executable: node,
            project_root: project_root.to_path_buf(),
            modules,
            typescript_version,
            decorators_revision: "2023-11".to_owned(),
            hashes,
            sandbox_policy: policy,
        },
    )?;
    persist_artifact(cache, package, &key, &manifest)?;
    Ok(Batch::Fresh(manifest))
}

/// Tries the project cache and then the emitted package artifact.
fn load_cached(
    cache: &SpecializationArtifactStore,
    package: &SpecializationArtifactStore,
    language: HostLanguage,
    key: &SpecializationCacheKey,
) -> Result<Option<SpecializationManifest>, Box<dyn std::error::Error>> {
    if let Some(manifest) = cache.load(language, key)? {
        package.store(key, &manifest)?;
        return Ok(Some(manifest));
    }
    if let Some(manifest) = package.load(language, key)? {
        cache.store(key, &manifest)?;
        return Ok(Some(manifest));
    }
    Ok(None)
}

/// Stores the cache entry and publishes byte-stable package output.
fn persist_artifact(
    cache: &SpecializationArtifactStore,
    package: &SpecializationArtifactStore,
    key: &SpecializationCacheKey,
    manifest: &SpecializationManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    cache.store(key, manifest)?;
    package.store(key, manifest)?;
    Ok(())
}

/// Compiles the resolved TypeScript graph once with standard decorators.
fn compile_typescript_graph(
    tsc: &Path,
    project_root: &Path,
    emitted_root: &Path,
    graph: &[&ManifestSource],
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(emitted_root)?;
    let mut command = Command::new(tsc);
    for source in graph.iter().filter(|source| source.lang.is_typescript()) {
        if !source.path.to_string_lossy().ends_with(".d.ts") {
            command.arg(&source.path);
        }
    }
    let output = command
        .args(["--target", "ES2022", "--module", "commonjs", "--rootDir"])
        .arg(project_root)
        .arg("--outDir")
        .arg(emitted_root)
        .args(["--sourceMap", "--skipLibCheck", "--pretty", "false"])
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "TypeScript specialization compile failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
    .into())
}

/// Computes all cache invalidation hashes from stable source and policy bytes.
///
/// Every path is hashed relative to `project` ([`ProjectRoot`]), so the
/// digests do not depend on where the checkout lives. The dependency digest
/// covers the project's dependency declarations and the packages installed
/// under its `node_modules`, which a guest may load although the source graph
/// never names them.
fn invalidation_hashes(
    project: &ProjectRoot,
    graph: &[&ManifestSource],
    policy: &SandboxPolicyRecord,
) -> Result<HashInputs, Box<dyn std::error::Error>> {
    let root = project.path();
    let source = hash_sources(project, graph);
    let declared = project.hash_files(
        ["package.json", "pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"]
            .into_iter()
            .map(|name| root.join(name)),
    )?;
    let installed = project.installed_packages_hash()?;
    let dependencies = hash_bytes(format!("{declared}\0{installed}").as_bytes());
    let lockfile = project.hash_files(
        ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "uv.lock", "poetry.lock", "Pipfile.lock"]
            .into_iter()
            .map(|name| root.join(name)),
    )?;
    let environment = hash_bytes(&serde_json::to_vec(&policy.environment)?);
    let policy_hash = hash_bytes(&serde_json::to_vec(policy)?);
    Ok(HashInputs {
        source: source.clone(),
        dependencies,
        lockfile,
        environment,
        policy: policy_hash,
        callable_provenance: source,
    })
}

/// Hashes the graph's source texts with their project-relative paths.
///
/// Hashes the text the build lowers rather than re-reading each path: a file
/// laid by a package's typed version (see [`crate::sound`]) exists only in
/// memory, or replaces an installed file at the same path, so the bytes on
/// disk are not what is compiled. For every other source the text is the
/// file's content, so the digest is the one re-reading the file gave.
fn hash_sources(project: &ProjectRoot, graph: &[&ManifestSource]) -> String {
    project.hash_texts(graph.iter().map(|source| (source.path.as_path(), source.source.as_bytes())))
}

/// Hashes one in-memory byte sequence.
fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The read-only root that must be bound for a host interpreter to be executable.
///
/// The policy cannot assume the interpreter lives under `/usr`: a Node installed
/// at `/opt/node22/bin/node` (or `/usr/local/bin/node`) is invisible inside the
/// sandbox, and the guest dies with `status 127` —
/// `prlimit: failed to execute .../node: No such file or directory`. That is a
/// silent-looking failure that only appears once a sandbox backend is actually
/// available, so it is easy to mistake for a missing prerequisite.
///
/// Binds the interpreter's *install* root — the executable's grandparent, so a
/// `bin/` layout brings its sibling `lib/` along — rather than the whole
/// filesystem prefix. For `/usr/bin/node` that is `/usr`, which the defaults
/// already cover, so this only widens the policy where it has to.
fn interpreter_read_only_root(executable: &Path) -> Option<PathBuf> {
    let canonical = executable.canonicalize().ok()?;
    let install_root = canonical.parent()?.parent().unwrap_or_else(|| Path::new("/"));
    if install_root == Path::new("/") {
        return None;
    }
    Some(install_root.to_path_buf())
}

/// Builds the default fail-closed policy for one host batch.
fn sandbox_policy(
    host: &Host<'_>,
    interpreter: &Path,
    threaded_runtime: bool,
) -> SandboxPolicyRecord {
    let project_root = host.project_root;
    let backend = host.backend();
    let interpreter_root = interpreter_read_only_root(interpreter);
    let mut read_only_roots = [
        Some(project_root.to_path_buf()),
        interpreter_root,
        Some(PathBuf::from("/usr")),
        Some(PathBuf::from("/lib")),
        Some(PathBuf::from("/lib64")),
    ]
    .into_iter()
    .flatten()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string())
    .collect::<Vec<_>>();
    read_only_roots.sort();
    read_only_roots.dedup();
    SandboxPolicyRecord {
        backend: backend.name().to_owned(),
        network: false,
        read_only_roots,
        writable_roots: vec!["<scratch>".to_owned()],
        environment: BTreeMap::new(),
        wall_time_ms: 10_000,
        cpu_time_ms: 10_000,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        process_limit: if threaded_runtime { 64 } else { 1 },
        output_bytes: 4 * 1024 * 1024,
        subprocesses: false,
        native_extensions: Vec::new(),
    }
}

/// Resolves one executable through `PATH` to an absolute path.
fn find_executable(name: &str) -> Result<PathBuf, io::Error> {
    let path = std::env::var_os("PATH")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "PATH is not set"))?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("required specialization executable '{name}' was not found in PATH"),
            )
        })
}

/// Captures one canonical tool/runtime version string.
fn command_version(executable: &Path, args: &[&str]) -> Result<String, io::Error> {
    let output = Command::new(executable).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "'{} {}' failed",
            executable.display(),
            args.join(" ")
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    Ok(version.to_owned())
}

/// Converts one project-relative Python path into an importable dotted name.
fn python_module_name(
    project_root: &Path,
    path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let relative = path
        .canonicalize()?
        .strip_prefix(project_root)?
        .to_path_buf();
    let mut components = relative
        .with_extension("")
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if components
        .last()
        .is_some_and(|component| component == "__init__")
    {
        components.pop();
    }
    if components.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cannot derive a Python module name from '{}'",
                path.display()
            ),
        )
        .into());
    }
    Ok(components.join("."))
}

/// Maps source extensions to specialization host languages.
const fn source_language(language: SourceLang) -> Option<HostLanguage> {
    match language {
        SourceLang::Python => Some(HostLanguage::Python),
        SourceLang::TypeScript => Some(HostLanguage::TypeScript),
        SourceLang::PythonDeclaration | SourceLang::TypeScriptDeclaration => None,
    }
}

/// Produces a stable detector key for an existing source path.
fn normalized_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_module_names_preserve_package_paths() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        let package = project.path().join("pkg");
        fs::create_dir(&package)?;
        let module = package.join("model.py");
        fs::write(&module, "")?;
        let init = package.join("__init__.py");
        fs::write(&init, "")?;
        if python_module_name(project.path(), &module)? != "pkg.model" {
            return Err("module path was not converted to a dotted name".into());
        }
        if python_module_name(project.path(), &init)? != "pkg" {
            return Err("package initializer retained its __init__ segment".into());
        }
        Ok(())
    }

    #[test]
    fn hashes_change_when_source_bytes_change() -> Result<(), Box<dyn std::error::Error>> {
        let project = tempfile::tempdir()?;
        let source = project.path().join("source.py");
        fs::write(&source, "value = 1\n")?;
        let root = ProjectRoot::new(project.path())?;
        let first = root.hash_files(std::iter::once(source.clone()))?;
        fs::write(&source, "value = 2\n")?;
        let second = root.hash_files(std::iter::once(source))?;
        if first == second {
            return Err("source changes did not invalidate the hash".into());
        }
        Ok(())
    }
}
