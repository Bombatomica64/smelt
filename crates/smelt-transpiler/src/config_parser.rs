//! Configuration file parsing.

use std::fs;

use crate::config::Config;

/// Result type used by configuration parsing.
type ConfigResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Parse a Smelt configuration file from TOML.
///
/// Both failure modes carry the offending path so a CLI user can tell a missing
/// or mistyped `--manifest-path` apart from a genuinely malformed manifest,
/// rather than seeing a bare `No such file or directory (os error 2)`.
///
/// # Errors
///
/// Returns an error when the file cannot be read (with the path quoted) or when
/// TOML deserialization fails (with the path and the underlying TOML error).
pub fn parse(path: &str) -> ConfigResult<Config> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read manifest `{path}`: {error}"))?;
    toml::from_str(&source)
        .map_err(|error| format!("failed to parse manifest `{path}`: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing manifest reports the path so a mistyped `--manifest-path` is
    /// obvious instead of surfacing a bare OS error.
    #[test]
    fn missing_manifest_error_names_the_path() {
        let error = parse("does/not/exist/Smelt.toml")
            .expect_err("a missing manifest must fail")
            .to_string();
        assert!(
            error.contains("does/not/exist/Smelt.toml"),
            "error should quote the missing path, got: {error}"
        );
        assert!(
            error.contains("failed to read manifest"),
            "error should describe the read failure, got: {error}"
        );
    }
}
