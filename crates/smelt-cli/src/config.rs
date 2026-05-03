//! Configuration data structures for Smelt projects.

use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

/// The language pipeline for a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipeline {
    /// TypeScript frontend pipeline.
    TypeScript,
    /// Python frontend pipeline.
    Python,
}

/// Project metadata from the [project] section of Smelt.toml.
#[derive(Deserialize, Debug, JsonSchema)]
#[allow(dead_code)]
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
#[allow(dead_code)]
pub struct Config {
    /// Project metadata.
    project: Project,
    /// Source file configuration.
    sources: Source,
    /// Output target configuration.
    output: Output,
    /// Runtime options.
    runtime: Runtime,
    /// Optional strict mode configuration.
    strict: Option<Strict>,
}

impl Config {
    /// Get the project name.
    pub fn project_name(&self) -> &str {
        &self.project.name
    }

    /// Get the list of source file entries.
    pub fn entries(&self) -> &[PathBuf] {
        &self.sources.entries
    }

    /// Get the output target directory path.
    pub fn output_target(&self) -> &PathBuf {
        &self.output.target
    }

    /// Get the optional output crate name override.
    pub fn output_crate_name(&self) -> Option<&str> {
        self.output.crate_name.as_deref()
    }

    /// Whether the generated output should be built automatically.
    pub fn should_build_output(&self) -> bool {
        self.output.build.unwrap_or(false)
    }

    /// Determine which language pipelines are needed based on source entries.
    pub fn pipelines(&self) -> Vec<Pipeline> {
        let mut out = Vec::new();
        let has_ts = self
            .sources
            .entries
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "ts"));
        let has_py = self
            .sources
            .entries
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "py"));
        if has_ts {
            out.push(Pipeline::TypeScript);
        }
        if has_py {
            out.push(Pipeline::Python);
        }
        out
    }
}

/// Source file configuration from the [sources] section.
#[derive(Deserialize, Debug, JsonSchema)]
#[allow(dead_code)]
pub struct Source {
    /// List of source file paths.
    entries: Vec<PathBuf>,
    /// Optional list of root directories.
    roots: Option<Vec<PathBuf>>,
}

/// Output configuration from the [output] section.
#[derive(Deserialize, Debug, JsonSchema)]
pub struct Output {
    /// Target output directory.
    target: PathBuf,
    /// Optional crate name override.
    #[serde(rename = "crate-name")]
    crate_name: Option<String>,
    /// Whether to build the generated crate.
    build: Option<bool>,
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
#[allow(dead_code)]
pub struct Runtime {
    /// Cloning strategy for generated code.
    #[serde(rename = "clone-strategy")]
    clone_strategy: CloneStrategy,
}

/// Strict mode configuration from the [strict] section.
#[derive(Deserialize, Debug, JsonSchema)]
#[allow(dead_code)]
pub struct Strict {
    /// Enable strict mode for TypeScript.
    typescript: Option<bool>,
    /// Enable strict mode for Python.
    python: Option<bool>,
}
