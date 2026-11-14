//! Optional real-Django structural specialization probe.

use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use smelt_specialize::{
    BackendAvailability, Definition, GraphValueKind, HashInputs, LinuxBubblewrapBackend,
    PythonModule, PythonSpecializationRequest, PythonSpecializer, SandboxBackend,
    SandboxPolicyRecord, prereq::missing_prerequisite,
};

const DJANGO_PROBE_SOURCE: &str = r#"
from django.conf import settings

if not settings.configured:
    settings.configure(
        INSTALLED_APPS=[],
        DATABASES={"default": {"ENGINE": "django.db.backends.sqlite3", "NAME": ":memory:"}},
    )

import django
django.setup()

from django.db import models

class Book(models.Model):
    title = models.CharField(max_length=200)

    class Meta:
        app_label = "smelt_probe"
"#;

/// Boxed dynamic error type shared by the probe helpers.
type DynError = Box<dyn Error>;

#[test]
fn real_django_model_exposes_fields_descriptors_managers_and_metadata() -> Result<(), DynError> {
    let test = "real_django_model_exposes_fields_descriptors_managers_and_metadata";
    let Some(python) = discovered_python() else {
        return missing_prerequisite(test, "a system `python3`");
    };
    let Some(environment) = django_environment(python)? else {
        return missing_prerequisite(test, "Django importable by the system `python3`");
    };
    let backend = LinuxBubblewrapBackend::discover();
    if backend.availability() != BackendAvailability::Available {
        return missing_prerequisite(test, "the bubblewrap sandbox backend");
    }
    let project = tempfile::tempdir()?;
    let source_path = project.path().join("probe.py");
    fs::write(&source_path, DJANGO_PROBE_SOURCE)?;
    let manifest = PythonSpecializer::new(backend).specialize(&PythonSpecializationRequest {
        smelt_version: "django-probe".to_owned(),
        python_executable: python.to_path_buf(),
        project_root: project.path().to_path_buf(),
        import_paths: environment.roots.clone(),
        modules: vec![PythonModule {
            name: "probe".to_owned(),
            path: source_path,
        }],
        hashes: fixture_hashes(),
        sandbox_policy: fixture_policy(project.path(), &environment),
    })?;
    assert_book_model(&manifest)
}

/// Verifies the materialized Book model's fields, descriptors, managers, and metadata.
fn assert_book_model(manifest: &smelt_specialize::SpecializationManifest) -> Result<(), DynError> {
    let module = manifest
        .modules
        .first()
        .ok_or("Django probe emitted no module")?;
    let book = module
        .definitions
        .iter()
        .find(|definition| definition.binding_name == "Book")
        .ok_or("Django Book model was not materialized")?;
    let Definition::Class(book) = &book.definition else {
        return Err("Django Book binding is not a class".into());
    };
    if book.fields.iter().all(|field| field.name != "title") {
        return Err("Django model field 'title' is absent".into());
    }
    if book
        .descriptors
        .iter()
        .all(|descriptor| descriptor.name != "title")
    {
        return Err("Django deferred field descriptor 'title' is absent".into());
    }
    let metadata = book
        .metadata
        .iter()
        .find(|metadata| metadata.key == "python.django.model")
        .ok_or("Django model metadata is absent")?;
    let metadata_node = manifest
        .values
        .nodes
        .iter()
        .find(|node| node.id == metadata.value)
        .ok_or("Django metadata graph node is absent")?;
    let GraphValueKind::Dict(entries) = &metadata_node.value else {
        return Err("Django metadata is not a dictionary".into());
    };
    if entries.is_empty() {
        return Err("Django metadata dictionary is empty".into());
    }
    if book
        .descriptors
        .iter()
        .all(|descriptor| descriptor.name != "objects")
    {
        return Err("Django manager descriptor 'objects' is absent".into());
    }
    Ok(())
}

/// Runtime paths and native extensions needed by the installed Django package.
struct DjangoEnvironment {
    /// The directory Django is installed in: bound read-only and imported from.
    roots: Vec<PathBuf>,
    /// Explicit native-extension allowlist.
    extensions: Vec<String>,
}

/// Finds a `CPython` executable suitable for the Django probe.
fn discovered_python() -> Option<&'static Path> {
    ["/usr/bin/python3", "/usr/local/bin/python3"]
        .iter()
        .map(Path::new)
        .find(|path| path.is_file())
}

/// Discovers an installed Django distribution and the native extensions the
/// probe loads: the probe source runs on the host first, so the allowlist names
/// exactly what the guest will import.
fn django_environment(python: &Path) -> Result<Option<DjangoEnvironment>, DynError> {
    let script = r#"
import pathlib, sys
try:
    import django
except ImportError:
    raise SystemExit(3)
print(pathlib.Path(django.__file__).resolve().parents[1])
exec(compile(sys.argv[1], "probe.py", "exec"), {"__name__": "probe"})
for module in sorted(sys.modules.values(), key=lambda item: getattr(item, "__name__", "")):
    filename = getattr(module, "__file__", "")
    if filename and filename.endswith((".so", ".pyd", ".dylib")):
        print(pathlib.Path(filename).name)
"#;
    let output = Command::new(python)
        .args(["-c", script, DJANGO_PROBE_SOURCE])
        .output()?;
    if output.status.code() == Some(3_i32) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(format!(
            "Django environment discovery failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.lines().map(str::to_owned);
    let Some(root) = lines.next() else {
        return Ok(None);
    };
    Ok(Some(DjangoEnvironment {
        roots: vec![PathBuf::from(root)],
        extensions: lines.collect(),
    }))
}

/// Builds the hard-sandbox policy for the installed Django distribution.
fn fixture_policy(project: &Path, environment: &DjangoEnvironment) -> SandboxPolicyRecord {
    let mut roots = runtime_roots(project);
    roots.extend(
        environment
            .roots
            .iter()
            .map(|path| path.display().to_string()),
    );
    roots.sort();
    roots.dedup();
    SandboxPolicyRecord {
        backend: "linux-bubblewrap".to_owned(),
        network: false,
        read_only_roots: roots,
        writable_roots: Vec::new(),
        environment: BTreeMap::new(),
        wall_time_ms: 15_000,
        cpu_time_ms: 15_000,
        memory_bytes: 1024 * 1024 * 1024,
        process_limit: 1,
        output_bytes: 16 * 1024 * 1024,
        subprocesses: false,
        native_extensions: environment.extensions.clone(),
    }
}

/// Returns core interpreter and project roots mounted read-only.
fn runtime_roots(project: &Path) -> Vec<String> {
    [
        PathBuf::from(project),
        PathBuf::from("/usr"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/lib"),
        PathBuf::from("/lib64"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string())
    .collect()
}

/// Builds stable placeholder hashes for the optional structural probe.
fn fixture_hashes() -> HashInputs {
    HashInputs {
        source: "django-probe".to_owned(),
        dependencies: "installed-django".to_owned(),
        lockfile: "probe".to_owned(),
        environment: "none".to_owned(),
        policy: "django-probe".to_owned(),
        callable_provenance: "django-probe".to_owned(),
    }
}
