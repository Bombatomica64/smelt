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

    /// Parse a manifest whose only interesting part is `[packages.utility]`.
    fn parse_packages(declaration: &str) -> ConfigResult<Config> {
        let source = format!(
            "[project]\nname = \"p\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n\
             [packages.utility]\n{declaration}\n\n[output]\ntarget = \"./dist\"\n\n[runtime]\nclone-strategy = \"aggressive\"\n"
        );
        toml::from_str(&source).map_err(Into::into)
    }

    /// The TOML spelling of `[packages.<name>]` maps onto the typed location.
    #[test]
    fn package_declarations_parse_into_a_location() {
        use crate::config::PackageLocation;
        let path = parse_packages("path = \"vendor/utility\"").expect("path parses");
        assert_eq!(
            path.packages()["utility"].location,
            PackageLocation::Path("vendor/utility".into())
        );
        let git = parse_packages("git = \"https://example.com/u.git\"\nrev = \"v1\"")
            .expect("git parses");
        assert_eq!(
            git.packages()["utility"].location,
            PackageLocation::Git { url: "https://example.com/u.git".to_owned(), rev: "v1".to_owned() }
        );
    }

    /// Every malformed combination is a parse error naming the problem; a
    /// `rev` on a `path` source used to be ignored silently.
    #[test]
    fn malformed_package_declarations_are_rejected() {
        for (declaration, expected) in [
            ("path = \"a\"\nrev = \"v1\"", "a `path` package has no revision"),
            ("git = \"u\"", "`git` needs a pinned `rev`"),
            ("path = \"a\"\ngit = \"u\"\nrev = \"v1\"", "exactly one of `path` and `git`"),
            ("entry = \"index.ts\"", "exactly one of `path` and `git`"),
        ] {
            let error = parse_packages(declaration).expect_err(declaration).to_string();
            assert!(error.contains(expected), "{declaration}: {error}");
        }
    }
}
