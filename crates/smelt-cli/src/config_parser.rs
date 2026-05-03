//! Configuration file parsing.

use std::fs;

use crate::config::Config;

/// Parse a Smelt configuration file from TOML.
pub fn parse(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    fs::read_to_string(path)
        .map_err(Into::into)
        .and_then(|s| toml::from_str(&s).map_err(Into::into))
}
