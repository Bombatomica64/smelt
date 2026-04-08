use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

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
    rust: Option<Rust>,
    strict: Option<Strict>,
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
pub struct Rust {
    edition: Option<String>,
    dependencies: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize, Debug, JsonSchema)]
pub struct Strict {
    typescript: Option<bool>,
    python: Option<bool>,
}
