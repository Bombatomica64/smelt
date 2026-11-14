//! Configuration data structures for Smelt projects.

use schemars::JsonSchema;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

/// Kind of Rust crate target emitted by Smelt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OutputKind {
    /// Generate an executable Rust program rooted at `src/main.rs`.
    Program,
    /// Generate a Rust library crate rooted at `src/lib.rs`.
    Library,
}

impl Default for OutputKind {
    /// Returns the default output kind used by existing Smelt manifests.
    fn default() -> Self {
        Self::Program
    }
}

/// Project metadata from the [project] section of Smelt.toml.
#[derive(Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Project name.
    name: String,
    /// Project version.
    version: String,
    /// Optional project description.
    description: Option<String>,
}

/// Top-level configuration for a Smelt project.
#[derive(Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Project metadata.
    project: Project,
    /// Source file configuration.
    sources: Source,
    /// Output target configuration.
    output: Output,
    /// Runtime options.
    #[serde(default)]
    runtime: Runtime,
    /// Generated-Rust options.
    #[serde(default)]
    rust: Rust,
    /// Build options.
    #[serde(default)]
    build: Build,
    /// Strict mode configuration.
    #[serde(default)]
    strict: Strict,
    /// TypeScript sources for dependencies imported by bare specifier.
    #[serde(default)]
    packages: BTreeMap<String, PackageSource>,
    /// Erasure-reason severities (`[erasure]`); see `crate::erasure`.
    #[serde(default)]
    erasure: Option<crate::erasure::ErasureConfig>,
    /// Definition-time specialization options (`[specialization]`).
    #[serde(default)]
    specialization: Specialization,
}

/// Definition-time specialization configuration from the [specialization] section.
#[derive(Default, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Specialization {
    /// A container to run specialization in when the native hard sandbox
    /// (bubblewrap on Linux) is unavailable. Without it, specialization fails
    /// closed on such a host.
    oci_fallback: Option<OciFallback>,
}

/// `[specialization.oci-fallback]`: a Docker- or Podman-compatible runtime and
/// an image pinned by digest.
#[derive(Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OciFallback {
    /// The runtime: a path, or a name looked up on `PATH` (`docker`, `podman`).
    runtime: String,
    /// The image, pinned by digest (`name@sha256:...`). The host's interpreter
    /// and libraries are bound into it read-only at their canonical paths, so
    /// the image needs nothing but a merged-`/usr` layout (`/lib -> usr/lib`),
    /// as current Debian and Ubuntu images have.
    image: String,
}

impl Config {
    /// Get the project name.
    #[must_use]
    pub fn project_name(&self) -> &str {
        &self.project.name
    }

    /// Get the project version, which the generated crate carries.
    #[must_use]
    pub fn project_version(&self) -> &str {
        &self.project.version
    }

    /// Get the project description, which the generated crate carries.
    #[must_use]
    pub fn project_description(&self) -> Option<&str> {
        self.project.description.as_deref()
    }

    /// Get the Rust edition of the generated crate.
    #[must_use]
    pub fn rust_edition(&self) -> RustEdition {
        self.rust.edition.unwrap_or_default()
    }

    /// Get `[rust.dependencies]` as `(name, TOML value text)`, in name order.
    #[must_use]
    pub fn rust_dependencies(&self) -> Vec<(String, String)> {
        self.rust
            .dependencies
            .iter()
            .map(|(name, value)| (name.clone(), value.to_string()))
            .collect()
    }

    /// The first rule of `specs/cli.md` this manifest breaks, as the message after
    /// `error[smelt::manifest]: `.
    pub(crate) fn violation(&self) -> Option<String> {
        if self.strict.typescript == Some(false) {
            return Some("strict.typescript=false is not supported in v1.0".to_owned());
        }
        if self.strict.python == Some(false) {
            return Some("strict.python=false is not supported in v1.0".to_owned());
        }
        if let Some(strategy) = self
            .runtime
            .clone_strategy
            .as_deref()
            .filter(|value| *value != "aggressive")
        {
            return Some(format!(
                "runtime.clone-strategy={strategy:?} is not supported in v1.0"
            ));
        }
        None
    }

    /// Get the list of source file entries.
    #[must_use]
    pub fn entries(&self) -> &[PathBuf] {
        &self.sources.entries
    }

    /// Get optional source root directories used for import and test discovery.
    #[must_use]
    pub fn source_roots(&self) -> Option<&[PathBuf]> {
        self.sources.roots.as_deref()
    }

    /// Get test file glob patterns discovered under source roots.
    #[must_use]
    pub fn test_globs(&self) -> &[String] {
        self.sources.test_globs.as_deref().unwrap_or(&[])
    }

    /// Get root-relative glob patterns for source files to exclude from lowering.
    ///
    /// Files whose root-relative path matches any of these patterns are dropped
    /// from both build discovery (`entries` plus test globs) and probe
    /// discovery. This lets a manifest opt specific files out of the whole-crate
    /// build without hiding them from the repository — useful for spec files
    /// that depend on host globals Smelt's non-DOM profile does not model.
    #[must_use]
    pub fn source_excludes(&self) -> &[String] {
        self.sources.exclude.as_deref().unwrap_or(&[])
    }

    /// The OCI fallback's runtime and image, when `[specialization.oci-fallback]`
    /// is configured.
    #[must_use]
    pub fn oci_fallback(&self) -> Option<(&str, &str)> {
        self.specialization
            .oci_fallback
            .as_ref()
            .map(|fallback| (fallback.runtime.as_str(), fallback.image.as_str()))
    }

    /// Get the output target directory path.
    #[must_use]
    pub fn output_target(&self) -> &PathBuf {
        &self.output.target
    }

    /// Get the optional output crate name override.
    #[must_use]
    pub fn output_crate_name(&self) -> Option<&str> {
        self.output.crate_name.as_deref()
    }

    /// Get the global allocator a generated program installs.
    #[must_use]
    pub fn rust_allocator(&self) -> Allocator {
        self.rust.allocator
    }

    /// Get the `[profile.release]` the generated crate carries.
    #[must_use]
    pub fn rust_release_profile(&self) -> ReleaseProfile {
        self.rust.release_profile
    }

    /// Get the generated Rust crate target kind.
    #[must_use]
    pub fn output_kind(&self) -> OutputKind {
        self.output.kind
    }

    /// Get the `[packages.<name>]` source declarations, keyed by package name.
    #[must_use]
    pub fn packages(&self) -> &BTreeMap<String, PackageSource> {
        &self.packages
    }

    /// Build-time defines as `(key, JSON text)` pairs, in key order.
    ///
    /// See [`Build::define`]. Command-line `--define` values have already been
    /// merged in by [`Config::extend_defines`].
    #[must_use]
    pub fn defines(&self) -> Vec<(String, String)> {
        self.build
            .define
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Merge command-line defines over the manifest's; a key given on the
    /// command line replaces the manifest's value, as a later `--define` does.
    pub fn extend_defines(&mut self, defines: impl IntoIterator<Item = (String, String)>) {
        self.build.define.extend(defines);
    }

    /// Get the `[erasure]` table, when the manifest has one.
    #[must_use]
    pub fn erasure(&self) -> Option<&crate::erasure::ErasureConfig> {
        self.erasure.as_ref()
    }

    /// Whether the generated output should be built automatically.
    #[must_use]
    pub fn should_build_output(&self) -> bool {
        self.output.build.unwrap_or(false)
    }

    /// `[build] type-errors`: what a build does with type-checker errors.
    #[must_use]
    pub const fn type_errors(&self) -> crate::type_gate::TypeErrorPolicy {
        self.build.type_errors
    }
}

/// Source file configuration from the [sources] section.
#[derive(Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// List of source file paths.
    entries: Vec<PathBuf>,
    /// Optional list of root directories.
    roots: Option<Vec<PathBuf>>,
    /// Optional glob patterns used to discover test source files under roots.
    #[serde(rename = "test-globs", alias = "test-prefix")]
    test_globs: Option<Vec<String>>,
    /// Optional root-relative glob patterns for source files to exclude.
    ///
    /// Matching files are skipped by both build and probe file discovery. The
    /// glob syntax matches [`test_globs`](Self::test_globs): `*` spans one path
    /// segment and `**` spans zero or more segments, always with `/` separators.
    exclude: Option<Vec<String>>,
}

/// Where the TypeScript sources of one dependency live: `[packages.<name>]`.
///
/// A package published to npm usually ships compiled JavaScript and `.d.ts`
/// declarations only, and Smelt lowers TypeScript SOURCE, so a bare import of
/// it (`import { Hono } from 'hono'`) has nothing to resolve to in
/// `node_modules`. This section names the sources instead: a local directory
/// (`path`) or a repository pinned to a revision (`git` + `rev`), the way a
/// Cargo dependency is pinned. Exactly one of `path` and `git` is given.
///
/// The TOML spelling is the flat table [`PackageSourceToml`] describes; it is
/// validated while the manifest is parsed, so a declaration that names both
/// sources, neither, a `git` without `rev`, or a `rev` on a `path` source is a
/// manifest parse error rather than something resolution has to re-check.
#[derive(Clone, Deserialize, Debug)]
#[serde(try_from = "PackageSourceToml")]
pub struct PackageSource {
    /// Where the package's sources come from.
    pub location: PackageLocation,
    /// Entry module, relative to the package root. Defaults to `src/index.ts`,
    /// then `index.ts`. Subpath imports (`name/sub`) resolve next to it.
    pub entry: Option<PathBuf>,
    /// Package-root-relative globs of modules left out of the crate, with the
    /// same syntax and pruning as `[sources] exclude`.
    pub exclude: Option<Vec<String>>,
}

/// The source tree a `[packages.<name>]` declaration names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageLocation {
    /// A local directory, relative to `Smelt.toml`.
    Path(PathBuf),
    /// A git repository pinned to a revision.
    Git {
        /// Repository to fetch the sources from (`git clone` URL).
        url: String,
        /// Revision to check out: a tag or a commit.
        rev: String,
    },
}

/// Where the TypeScript sources of one dependency live: `[packages.<name>]`.
///
/// A package published to npm usually ships compiled JavaScript and `.d.ts`
/// declarations only, and Smelt lowers TypeScript SOURCE, so a bare import of
/// it (`import { Hono } from 'hono'`) has nothing to resolve to in
/// `node_modules`. This section names the sources instead: a local directory
/// (`path`) or a repository pinned to a revision (`git` + `rev`), the way a
/// Cargo dependency is pinned. Exactly one of `path` and `git` is given.
//
// This is the TOML (and JSON schema) spelling; it is converted into
// `PackageSource` as the manifest is parsed. (A plain comment, so the
// generated schema description stays what `smelt-schema.json` has.)
#[derive(Clone, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PackageSource")]
pub struct PackageSourceToml {
    /// Directory holding the package's sources, relative to `Smelt.toml`.
    pub path: Option<PathBuf>,
    /// Repository to fetch the sources from (`git clone` URL).
    pub git: Option<String>,
    /// Revision to check out: a tag or a commit. Required with `git`.
    pub rev: Option<String>,
    /// Entry module, relative to the package root. Defaults to `src/index.ts`,
    /// then `index.ts`. Subpath imports (`name/sub`) resolve next to it.
    pub entry: Option<PathBuf>,
    /// Package-root-relative globs of modules left out of the crate, with the
    /// same syntax and pruning as `[sources] exclude`.
    pub exclude: Option<Vec<String>>,
}

impl TryFrom<PackageSourceToml> for PackageSource {
    type Error = String;

    /// Accept exactly `path`, or `git` with `rev`.
    fn try_from(raw: PackageSourceToml) -> Result<Self, Self::Error> {
        let location = match (raw.path, raw.git, raw.rev) {
            (Some(path), None, None) => PackageLocation::Path(path),
            (Some(_), None, Some(_)) => {
                return Err(
                    "`rev` pins a `git` source; a `path` package has no revision to pin".to_owned(),
                );
            }
            (None, Some(url), Some(rev)) => PackageLocation::Git { url, rev },
            (None, Some(_), None) => {
                return Err("`git` needs a pinned `rev` (a tag or a commit)".to_owned());
            }
            (Some(_), Some(_), _) | (None, None, _) => {
                return Err("give exactly one of `path` and `git`".to_owned());
            }
        };
        Ok(Self {
            location,
            entry: raw.entry,
            exclude: raw.exclude,
        })
    }
}

impl JsonSchema for PackageSource {
    /// The schema is the TOML spelling's.
    fn schema_name() -> std::borrow::Cow<'static, str> {
        PackageSourceToml::schema_name()
    }

    /// The schema is the TOML spelling's.
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        PackageSourceToml::json_schema(generator)
    }
}

/// Output configuration from the [output] section.
#[derive(Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Output {
    /// Target output directory.
    target: PathBuf,
    /// Optional crate name override.
    #[serde(rename = "crate-name")]
    crate_name: Option<String>,
    /// Generated Rust crate target kind.
    #[serde(default)]
    kind: OutputKind,
    /// Whether to build the generated crate.
    build: Option<bool>,
}

/// Build configuration from the [build] section.
#[derive(Default, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// Build-time defines, spelled like Bun's `--define`: the key is the
    /// expression, the value is JSON text (`"process.env.NODE_ENV" =
    /// "\"production\""`).
    ///
    /// Smelt consults defines only to decide whether a package's typed version
    /// applies (its `sound.json` `when` field); it does not substitute them
    /// into the program text.
    #[serde(default)]
    define: BTreeMap<String, String>,
    /// What `smelt build` does when the type checkers (TypeScript's, ty for
    /// Python) report an error or cannot run: `"deny"` (the default) refuses to
    /// lower the program, `"allow"` prints the errors as warnings and lowers it
    /// anyway. `--allow-type-errors` allows for one run.
    #[serde(default, rename = "type-errors")]
    type_errors: crate::type_gate::TypeErrorPolicy,
}

/// The `[rust] edition` of the generated crate.
///
/// Only the editions generated code compiles under are variants (see
/// `smelt_codegen_rust::RustEdition`): the emitted text and the inlined runtime
/// use let-chains and the 2024 keyword rules, so a manifest asking for 2021 is
/// refused when it is read rather than producing a crate that cannot compile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum RustEdition {
    /// The 2024 edition.
    #[default]
    #[serde(rename = "2024")]
    Edition2024,
}

impl RustEdition {
    /// The codegen edition this one is.
    #[must_use]
    pub const fn codegen(self) -> smelt_codegen_rust::RustEdition {
        match self {
            Self::Edition2024 => smelt_codegen_rust::RustEdition::Edition2024,
        }
    }
}

/// Generated-Rust configuration from the [rust] section.
#[derive(Default, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rust {
    /// Rust edition of the generated crate; 2024 when absent, and the only
    /// edition generated code compiles under.
    edition: Option<RustEdition>,
    /// Added to the generated crate's `[dependencies]` verbatim. Same syntax as
    /// Cargo's table, and an entry here replaces what Smelt would add for that crate.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    dependencies: BTreeMap<String, toml::Value>,
    /// Global allocator a generated program installs.
    #[serde(default)]
    allocator: Allocator,
    /// `[profile.release]` the generated crate carries.
    #[serde(default, rename = "release-profile")]
    release_profile: ReleaseProfile,
}

/// `[profile.release]` the generated crate carries.
///
/// See `smelt_codegen_rust::ReleaseProfile`: Cargo's stock release profile never
/// inlines across a codegen unit, and in generated code the runtime prelude is on
/// the far side of that boundary from every hot loop.
#[derive(Clone, Copy, Default, Deserialize, Debug, Eq, JsonSchema, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseProfile {
    /// Thin LTO and one codegen unit. The default.
    #[default]
    Optimized,
    /// Cargo's stock release profile.
    Default,
    /// Fat (whole-program) LTO and one codegen unit: more release build time
    /// for more throughput. See `smelt_codegen_rust::ReleaseProfile::WholeProgram`.
    WholeProgram,
}

/// Global allocator a generated program installs.
///
/// Generated code allocates far more than hand-written Rust does — every
/// JavaScript array, object and string is a separate heap value with reference
/// semantics — so the allocator is a larger share of the profile than any single
/// emitter decision. See `smelt_codegen_rust::GeneratedAllocator`.
#[derive(Clone, Copy, Default, Deserialize, Debug, Eq, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Allocator {
    /// Leave the platform allocator in place. The default: see
    /// `smelt_codegen_rust::GeneratedAllocator` for why Smelt does not choose
    /// an allocator on the application author's behalf.
    #[default]
    System,
    /// Install `mimalloc`, which suits this workload's small short-lived values.
    Mimalloc,
}

/// Runtime configuration from the [runtime] section.
#[derive(Default, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    /// Cloning strategy for generated code; v1.0 supports only `"aggressive"`,
    /// which is also the default.
    #[serde(rename = "clone-strategy")]
    clone_strategy: Option<String>,
}

/// Strict mode configuration from the [strict] section. v1.0 is always strict, so
/// each flag is `true` or absent.
#[derive(Default, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Strict {
    /// Enable strict mode for TypeScript.
    typescript: Option<bool>,
    /// Enable strict mode for Python.
    python: Option<bool>,
}
