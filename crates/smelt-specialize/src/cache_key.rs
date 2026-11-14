//! Deterministic specialization cache keys.
//!
//! A key must change whenever the specialization output could, and must not
//! change when nothing that matters did. So [`CacheKeyInput`] has private
//! fields and one constructor whose arguments are the typed components, each
//! of which must be supplied:
//!
//! - the specializer itself ([`SpecializerIdentity`]): the compiler version,
//!   [`SPECIALIZER_REVISION`], and a hash of the guest script that runs in the
//!   host runtime, computed at compile time from the very bytes the binary
//!   embeds, so editing a guest invalidates every artifact it produced;
//! - the inputs, hashed against a [`ProjectRoot`]: every path in a digest is
//!   relative to the project, never absolute, and the installed packages under
//!   `node_modules` are hashed, so an artifact is portable across checkouts and
//!   stale when a dependency changes;
//! - the sandbox policy, with the project root spelled `<project>`.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{HashInputs, HostLanguage, SandboxPolicyRecord};

/// The specializer's own revision. Bump it whenever the host-side protocol
/// or the way a manifest is derived changes in a way the guest hash and the
/// compiler version do not capture.
pub const SPECIALIZER_REVISION: u32 = 1;

/// Hash of one embedded guest script, computed at compile time (64-bit FNV-1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GuestScriptHash(u64);

impl GuestScriptHash {
    /// The hash of `bytes`; a `const fn`, so the embedded guests' hashes are
    /// constants of the binary.
    #[must_use]
    pub const fn of(bytes: &[u8]) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut rest = bytes;
        while let [byte, tail @ ..] = rest {
            hash ^= u64::from_le_bytes([*byte, 0, 0, 0, 0, 0, 0, 0]);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            rest = tail;
        }
        Self(hash)
    }
}

/// The Node guest's hash.
const NODE_GUEST_HASH: GuestScriptHash = GuestScriptHash::of(crate::node::NODE_GUEST.as_bytes());

/// The `CPython` guest's hash.
const PYTHON_GUEST_HASH: GuestScriptHash =
    GuestScriptHash::of(crate::python::PYTHON_GUEST.as_bytes());

/// Which specializer produced an artifact: everything about Smelt itself that
/// can change the output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecializerIdentity {
    /// Smelt compiler version.
    smelt_version: String,
    /// [`SPECIALIZER_REVISION`].
    revision: u32,
    /// Hash of the guest script the language runs.
    guest: GuestScriptHash,
}

impl SpecializerIdentity {
    /// This build's specializer for `language`.
    #[must_use]
    pub fn of(language: HostLanguage) -> Self {
        Self {
            smelt_version: env!("CARGO_PKG_VERSION").to_owned(),
            revision: SPECIALIZER_REVISION,
            guest: match language {
                HostLanguage::Python => PYTHON_GUEST_HASH,
                HostLanguage::TypeScript => NODE_GUEST_HASH,
            },
        }
    }
}

/// The project directory every hashed path is made relative to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    /// The project rooted at `root`, made absolute.
    ///
    /// # Errors
    ///
    /// Returns an error when the current directory cannot be read.
    pub fn new(root: &Path) -> io::Result<Self> {
        Ok(Self(std::path::absolute(root)?))
    }

    /// The root itself.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `path` relative to the project, `/`-separated; a path outside it keeps
    /// its absolute spelling, which is all it can be identified by.
    #[must_use]
    pub fn relative(&self, path: &Path) -> String {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let relative = absolute.strip_prefix(&self.0).ok().filter(|relative| {
            relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        });
        relative
            .unwrap_or(&absolute)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// A digest of `(path, text)` pairs, by project-relative path.
    #[must_use]
    pub fn hash_texts<'text>(
        &self,
        texts: impl Iterator<Item = (&'text Path, &'text [u8])>,
    ) -> String {
        let mut entries = texts
            .map(|(path, text)| (self.relative(path), text))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        let mut digest = Sha256::new();
        for (path, text) in entries {
            digest.update(path.as_bytes());
            digest.update([0]);
            digest.update(text);
            digest.update([0xff]);
        }
        hex::encode(digest.finalize())
    }

    /// A digest of the files at `paths` that exist, by project-relative path.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing file cannot be read.
    pub fn hash_files(&self, paths: impl Iterator<Item = PathBuf>) -> io::Result<String> {
        let files = paths
            .filter(|path| path.is_file())
            .map(|path| fs::read(&path).map(|bytes| (path, bytes)))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(self.hash_texts(
            files
                .iter()
                .map(|(path, bytes)| (path.as_path(), bytes.as_slice())),
        ))
    }

    /// A digest of the packages installed under the project's `node_modules`:
    /// npm's hidden lockfile and every package's (and scoped package's)
    /// `package.json`, which names its version.
    ///
    /// A guest can `require` an installed package the source graph never
    /// names, so a reinstall or an upgrade must invalidate the artifact even
    /// when no lockfile records it.
    ///
    /// # Errors
    ///
    /// Returns an error when `node_modules` exists but cannot be listed or read.
    pub fn installed_packages_hash(&self) -> io::Result<String> {
        let modules = self.0.join("node_modules");
        let mut manifests = vec![modules.join(".package-lock.json")];
        if modules.is_dir() {
            for entry in fs::read_dir(&modules)? {
                let package = entry?.path();
                let scoped = package
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('@'));
                if scoped && package.is_dir() {
                    for scoped_entry in fs::read_dir(&package)? {
                        manifests.push(scoped_entry?.path().join("package.json"));
                    }
                } else {
                    manifests.push(package.join("package.json"));
                }
            }
        }
        self.hash_files(manifests.into_iter())
    }

    /// `policy` with every path under the project spelled relative to
    /// `<project>`, so the key does not depend on where the checkout lives.
    fn portable_policy(&self, policy: &SandboxPolicyRecord) -> SandboxPolicyRecord {
        let portable = |root: &String| {
            let path = Path::new(root);
            if path.starts_with(&self.0) {
                let relative = self.relative(path);
                if relative.is_empty() {
                    "<project>".to_owned()
                } else {
                    format!("<project>/{relative}")
                }
            } else {
                root.clone()
            }
        };
        SandboxPolicyRecord {
            read_only_roots: policy.read_only_roots.iter().map(portable).collect(),
            writable_roots: policy.writable_roots.iter().map(portable).collect(),
            ..policy.clone()
        }
    }
}

/// Inputs whose exact bytes determine one host-runtime specialization result;
/// see the module docs. Only [`Self::new`] builds one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKeyInput {
    /// The specializer that produces the result.
    specializer: SpecializerIdentity,
    /// Manifest schema version.
    schema_version: u32,
    /// Guest language.
    language: HostLanguage,
    /// Exact host runtime version.
    host_runtime_version: String,
    /// Hashes of sources, dependencies, environment, and provenance.
    hashes: HashInputs,
    /// Effective hard-sandbox policy, with project paths made portable.
    sandbox_policy: SandboxPolicyRecord,
    /// Versioned native adapter identities, sorted by adapter ID.
    native_adapter_versions: Vec<(String, String)>,
}

impl CacheKeyInput {
    /// The key input of one batch in `language`, run by `host_runtime_version`
    /// over inputs `hashes` (hashed against `project`) under `policy`, with no
    /// native adapter.
    #[must_use]
    pub fn new(
        language: HostLanguage,
        host_runtime_version: String,
        hashes: HashInputs,
        policy: &SandboxPolicyRecord,
        project: &ProjectRoot,
    ) -> Self {
        Self {
            specializer: SpecializerIdentity::of(language),
            schema_version: crate::MANIFEST_SCHEMA_VERSION,
            language,
            host_runtime_version,
            hashes,
            sandbox_policy: project.portable_policy(policy),
            native_adapter_versions: Vec::new(),
        }
    }

    /// This input with the batch's native adapters, as `(id, version)`.
    #[must_use]
    pub fn with_native_adapters(mut self, mut versions: Vec<(String, String)>) -> Self {
        versions.sort();
        self.native_adapter_versions = versions;
        self
    }
}

/// Hex-encoded SHA-256 key for one specialization process and manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpecializationCacheKey(String);

impl SpecializationCacheKey {
    /// Computes a stable key from canonical schema data.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if a future schema field cannot be
    /// represented by the canonical JSON serializer.
    pub fn compute(input: &CacheKeyInput) -> Result<Self, serde_json::Error> {
        let bytes = serde_json::to_vec(input)?;
        let digest = Sha256::digest(bytes);
        Ok(Self(hex::encode(digest)))
    }

    /// Returns the lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "a test fails fast on fixture setup")]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A fixed set of input hashes.
    fn hashes() -> HashInputs {
        HashInputs {
            source: "source".to_owned(),
            dependencies: "deps".to_owned(),
            lockfile: "lock".to_owned(),
            environment: "env".to_owned(),
            policy: "policy".to_owned(),
            callable_provenance: "callable".to_owned(),
        }
    }

    /// A policy that binds the project at `project`.
    fn policy(project: &str) -> SandboxPolicyRecord {
        SandboxPolicyRecord {
            backend: "linux-bwrap".to_owned(),
            network: false,
            read_only_roots: vec![
                project.to_owned(),
                format!("{project}/.smelt/out"),
                "/usr".to_owned(),
            ],
            writable_roots: vec!["<scratch>".to_owned()],
            environment: BTreeMap::new(),
            wall_time_ms: 10_000,
            cpu_time_ms: 5_000,
            memory_bytes: 256 * 1024 * 1024,
            process_limit: 1,
            output_bytes: 1024 * 1024,
            subprocesses: false,
            native_extensions: Vec::new(),
        }
    }

    /// The key of a Python batch of the project at `project` with `hashes`.
    fn key(project: &str, hashes: HashInputs) -> SpecializationCacheKey {
        let root = ProjectRoot::new(Path::new(project)).expect("absolute");
        let input = CacheKeyInput::new(
            HostLanguage::Python,
            "3.13.5".to_owned(),
            hashes,
            &policy(project),
            &root,
        );
        SpecializationCacheKey::compute(&input).expect("serializable")
    }

    #[test]
    fn cache_key_is_deterministic_and_sensitive_to_inputs() {
        assert_eq!(key("/project", hashes()), key("/project", hashes()));
        let changed = HashInputs {
            environment: "different".to_owned(),
            ..hashes()
        };
        assert_ne!(
            key("/project", hashes()),
            key("/project", changed),
            "environment changes must invalidate"
        );
    }

    /// The same project checked out in two places has one key.
    #[test]
    fn cache_key_is_portable_across_checkouts() {
        assert_eq!(
            key("/home/a/project", hashes()),
            key("/srv/b/project", hashes())
        );
    }

    /// The key carries the guest's hash: a different guest script is a
    /// different specializer, and the two languages' guests differ.
    #[test]
    fn the_guest_script_is_part_of_the_key() {
        assert_ne!(
            GuestScriptHash::of(b"guest v1"),
            GuestScriptHash::of(b"guest v2")
        );
        assert_ne!(
            SpecializerIdentity::of(HostLanguage::Python),
            SpecializerIdentity::of(HostLanguage::TypeScript)
        );
        assert_eq!(
            SpecializerIdentity::of(HostLanguage::Python).guest,
            PYTHON_GUEST_HASH
        );
        assert_eq!(
            SpecializerIdentity::of(HostLanguage::TypeScript).guest,
            NODE_GUEST_HASH
        );
    }

    /// Hashed paths are project-relative: the same files under two roots have
    /// one digest, and a changed byte changes it.
    #[test]
    fn hashed_texts_use_project_relative_paths() {
        let first = ProjectRoot::new(Path::new("/one/project")).expect("absolute");
        let second = ProjectRoot::new(Path::new("/two/project")).expect("absolute");
        let digest = |root: &ProjectRoot, text: &'static [u8]| {
            let path = root.path().join("src/main.ts");
            root.hash_texts(std::iter::once((path.as_path(), text)))
        };
        assert_eq!(digest(&first, b"a"), digest(&second, b"a"));
        assert_ne!(digest(&first, b"a"), digest(&first, b"b"));
    }

    /// Installed packages are hashed by their manifests: a version bump
    /// changes the digest.
    #[test]
    fn installed_packages_are_hashed() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let root = ProjectRoot::new(dir.path()).expect("absolute");
        let package = dir.path().join("node_modules/@scope/pkg");
        fs::create_dir_all(&package).expect("create package");
        fs::write(package.join("package.json"), r#"{"version":"1.0.0"}"#).expect("write manifest");
        let before = root.installed_packages_hash().expect("hash");
        fs::write(package.join("package.json"), r#"{"version":"1.0.1"}"#).expect("write manifest");
        assert_ne!(before, root.installed_packages_hash().expect("hash"));
    }
}
