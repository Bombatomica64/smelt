#![allow(dead_code)]

use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipeline {
    TypeScript,
    Python,
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Project {
    name: String,
    version: String,
    description: Option<String>,
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Config {
    project: Project,
    sources: Source,
    output: Output,
    runtime: Runtime,
    strict: Option<Strict>,
}

impl Config {
    pub fn project_name(&self) -> &str {
        &self.project.name
    }

    pub fn entries(&self) -> &[PathBuf] {
        &self.sources.entries
    }

    pub fn output_target(&self) -> &PathBuf {
        &self.output.target
    }

    pub fn output_crate_name(&self) -> Option<&str> {
        self.output.crate_name.as_deref()
    }

    pub fn should_build_output(&self) -> bool {
        self.output.build.unwrap_or(false)
    }

    pub fn pipelines(&self) -> Vec<Pipeline> {
        let mut out = Vec::new();
        let has_ts = self
            .sources
            .entries
            .iter()
            .any(|p| p.extension().map_or(false, |e| e == "ts"));
        let has_py = self
            .sources
            .entries
            .iter()
            .any(|p| p.extension().map_or(false, |e| e == "py"));
        if has_ts {
            out.push(Pipeline::TypeScript);
        }
        if has_py {
            out.push(Pipeline::Python);
        }
        out
    }
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Source {
    entries: Vec<PathBuf>,
    roots: Option<Vec<PathBuf>>,
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Output {
    target: PathBuf,
    #[serde(rename = "crate-name")]
    crate_name: Option<String>,
    build: Option<bool>,
}

#[derive(Deserialize, Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CloneStrategy {
    Always,
    Aggressive,
    Never,
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Runtime {
    #[serde(rename = "clone-strategy")]
    clone_strategy: CloneStrategy,
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Strict {
    typescript: Option<bool>,
    python: Option<bool>,
}
