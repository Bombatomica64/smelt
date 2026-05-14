//! Configuration file parsing.

use std::fs;

use crate::config::Config;

/// Result type used by configuration parsing.
type ConfigResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Parse a Smelt configuration file from TOML.
///
/// # Errors
///
/// Returns an error when the file cannot be read or when TOML deserialization fails.
pub fn parse(path: &str) -> ConfigResult<Config> {
    fs::read_to_string(path)
        .map_err(Into::into)
        .and_then(|s| toml::from_str(&s).map_err(Into::into))
}
