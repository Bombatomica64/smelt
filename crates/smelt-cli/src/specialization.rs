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
    CacheKeyInput, HashInputs, HostLanguage, MANIFEST_SCHEMA_VERSION, ModuleInput, NodeModule,
    NodeSpecializationRequest, NodeSpecializer, PythonModule, PythonSpecializationRequest,
    PythonSpecializer, SandboxBackend, SandboxPolicyRecord, SpecializationArtifactStore,
    SpecializationCacheKey, SpecializationManifest, default_platform_backend,
    detect_specialization_modules,
};

use crate::{
    config::Config,
    lowering::SourceLang,
    manifest::{ManifestSource, resolve_manifest_path},
};

/// Per-language manifests supplied to pure frontend entry points.
#[derive(Debug, Default)]
pub(crate) struct PreparedSpecialization {
    /// Materialized Python source graph.
    pub(crate) python: Option<SpecializationManifest>,
    /// Materialized TypeScript source graph.
    pub(crate) typescript: Option<SpecializationManifest>,
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
        return Ok(PreparedSpecialization::default());
    }

    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    let output_dir = resolve_manifest_path(&manifest_dir, config.output_target());
    let cache =
        SpecializationArtifactStore::new(manifest_dir.join(".smelt").join("specialization-cache"));
    let package =
        SpecializationArtifactStore::new(output_dir.join(".smelt").join("specialization"));
    let candidate_paths = detections
        .into_iter()
        .map(|detection| (detection.path, detection.language))
        .collect::<BTreeSet<_>>();

    let python_sources = candidates_for(HostLanguage::Python, sources, &candidate_paths);
    let typescript_sources = candidates_for(HostLanguage::TypeScript, sources, &candidate_paths);

    let python = if python_sources.is_empty() {
        None
    } else {
        Some(prepare_python(
            &manifest_dir,
            &cache,
            &package,
            sources,
            &python_sources,
        )?)
    };
    let typescript = if typescript_sources.is_empty() {
        None
    } else {
        Some(prepare_typescript(
            &manifest_dir,
            &cache,
            &package,
            sources,
            &typescript_sources,
        )?)
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

/// Resolves one Python batch from cache or a sandboxed `CPython` process.
fn prepare_python(
    project_root: &Path,
    cache: &SpecializationArtifactStore,
    package: &SpecializationArtifactStore,
    graph: &[&ManifestSource],
    candidates: &[&ManifestSource],
) -> Result<SpecializationManifest, Box<dyn std::error::Error>> {
    let python = find_executable("python3")?;
    let runtime_version = command_version(&python, &["--version"])?;
    let policy = sandbox_policy(project_root, false);
    let hashes = invalidation_hashes(project_root, graph, &policy)?;
    let key = SpecializationCacheKey::compute(&CacheKeyInput {
        smelt_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        language: HostLanguage::Python,
        host_runtime_version: runtime_version,
        hashes: hashes.clone(),
        sandbox_policy: policy.clone(),
        native_adapter_versions: Vec::new(),
    })?;
    if let Some(manifest) = load_cached(cache, package, HostLanguage::Python, &key)? {
        return Ok(manifest);
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
    let manifest = PythonSpecializer::new(default_platform_backend()).specialize(
        &PythonSpecializationRequest {
            smelt_version: env!("CARGO_PKG_VERSION").to_owned(),
            python_executable: python,
            project_root: project_root.to_path_buf(),
            modules,
            hashes,
            sandbox_policy: policy,
        },
    )?;
    persist_artifact(cache, package, &key, &manifest)?;
    Ok(manifest)
}

/// Resolves one TypeScript batch from cache or canonical tsc + sandboxed Node.
fn prepare_typescript(
    project_root: &Path,
    cache: &SpecializationArtifactStore,
    package: &SpecializationArtifactStore,
    graph: &[&ManifestSource],
    candidates: &[&ManifestSource],
) -> Result<SpecializationManifest, Box<dyn std::error::Error>> {
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
    let mut policy = sandbox_policy(project_root, true);
    policy
        .read_only_roots
        .push(artifact_root.display().to_string());
    policy.read_only_roots.sort();
    policy.read_only_roots.dedup();
    let hashes = invalidation_hashes(project_root, graph, &policy)?;
    let key = SpecializationCacheKey::compute(&CacheKeyInput {
        smelt_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        language: HostLanguage::TypeScript,
        host_runtime_version,
        hashes: hashes.clone(),
        sandbox_policy: policy.clone(),
        native_adapter_versions: Vec::new(),
    })?;
    if let Some(manifest) = load_cached(cache, package, HostLanguage::TypeScript, &key)? {
        return Ok(manifest);
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
    let manifest = NodeSpecializer::new(default_platform_backend()).specialize(
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
    Ok(manifest)
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
fn invalidation_hashes(
    project_root: &Path,
    graph: &[&ManifestSource],
    policy: &SandboxPolicyRecord,
) -> Result<HashInputs, Box<dyn std::error::Error>> {
    let source = hash_files(graph.iter().map(|source| source.path.clone()))?;
    let dependency_files = [
        "package.json",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "requirements.txt",
    ]
    .map(|name| project_root.join(name));
    let dependencies = hash_existing_files(&dependency_files)?;
    let lockfiles = [
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "uv.lock",
        "poetry.lock",
        "Pipfile.lock",
    ]
    .map(|name| project_root.join(name));
    let lockfile = hash_existing_files(&lockfiles)?;
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

/// Hashes existing files in sorted path order.
fn hash_existing_files(paths: &[PathBuf]) -> Result<String, io::Error> {
    hash_files(paths.iter().filter(|path| path.is_file()).cloned())
}

/// Hashes path names and bytes in a deterministic order.
fn hash_files(paths: impl Iterator<Item = PathBuf>) -> Result<String, io::Error> {
    let mut sorted_paths = paths.collect::<Vec<_>>();
    sorted_paths.sort();
    let mut digest = Sha256::new();
    for path in sorted_paths {
        digest.update(path.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(fs::read(path)?);
        digest.update([0xff]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Hashes one in-memory byte sequence.
fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Builds the default fail-closed policy for one host batch.
fn sandbox_policy(project_root: &Path, threaded_runtime: bool) -> SandboxPolicyRecord {
    let backend = default_platform_backend();
    let mut read_only_roots = [
        project_root,
        Path::new("/usr"),
        Path::new("/lib"),
        Path::new("/lib64"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string())
    .collect::<Vec<_>>();
    read_only_roots.sort();
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
        let first = hash_files(std::iter::once(source.clone()))?;
        fs::write(&source, "value = 2\n")?;
        let second = hash_files(std::iter::once(source))?;
        if first == second {
            return Err("source changes did not invalidate the hash".into());
        }
        Ok(())
    }
}
