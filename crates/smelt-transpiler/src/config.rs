//! Configuration data structures for Smelt projects.

use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

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
#[expect(
    dead_code,
    reason = "configuration schema includes fields not consumed yet"
)]
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
#[expect(
    dead_code,
    reason = "configuration schema includes fields not consumed yet"
)]
pub struct Config {
    /// Project metadata.
    project: Project,
    /// Source file configuration.
    sources: Source,
    /// Output target configuration.
    output: Output,
    /// Runtime options.
    runtime: Runtime,
    /// Generated-Rust options.
    #[serde(default)]
    rust: Rust,
    /// Optional strict mode configuration.
    strict: Option<Strict>,
}

impl Config {
    /// Get the project name.
    #[must_use]
    pub fn project_name(&self) -> &str {
        &self.project.name
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

    /// Whether the generated output should be built automatically.
    #[must_use]
    pub fn should_build_output(&self) -> bool {
        self.output.build.unwrap_or(false)
    }
}

/// Source file configuration from the [sources] section.
#[derive(Deserialize, Debug, JsonSchema)]
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

/// Output configuration from the [output] section.
#[derive(Deserialize, Debug, JsonSchema)]
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

/// Generated-Rust configuration from the [rust] section.
#[derive(Default, Deserialize, Debug, JsonSchema)]
pub struct Rust {
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

/// Strategy for cloning values in generated code.
#[derive(Deserialize, Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CloneStrategy {
    /// Always clone values.
    Always,
    /// Aggressively clone when beneficial.
    Aggressive,
    /// Never clone values.
    Never,
}

/// Runtime configuration from the [runtime] section.
#[derive(Deserialize, Debug, JsonSchema)]
#[expect(
    dead_code,
    reason = "configuration schema includes fields not consumed yet"
)]
pub struct Runtime {
    /// Cloning strategy for generated code.
    #[serde(rename = "clone-strategy")]
    clone_strategy: CloneStrategy,
}

/// Strict mode configuration from the [strict] section.
#[derive(Deserialize, Debug, JsonSchema)]
#[expect(
    dead_code,
    reason = "configuration schema includes fields not consumed yet"
)]
pub struct Strict {
    /// Enable strict mode for TypeScript.
    typescript: Option<bool>,
    /// Enable strict mode for Python.
    python: Option<bool>,
}
