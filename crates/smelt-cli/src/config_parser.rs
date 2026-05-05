//! Configuration file parsing.

#![expect(
    clippy::type_complexity,
    reason = "configuration parsing keeps boxed dynamic errors for CLI simplicity"
)]

use std::fs;

use crate::config::Config;

/// Parse a Smelt configuration file from TOML.
pub fn parse(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    fs::read_to_string(path)
        .map_err(Into::into)
        .and_then(|s| toml::from_str(&s).map_err(Into::into))
}
