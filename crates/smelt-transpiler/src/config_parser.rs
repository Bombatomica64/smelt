//! Configuration file parsing.

use std::{fs, path::Path};

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
/// Returns an error when the file cannot be read (with the path quoted), when
/// TOML deserialization fails (with the path and the underlying TOML error), or
/// when the manifest breaks a rule of `specs/cli.md`.
pub fn parse(path: &str) -> ConfigResult<Config> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read manifest `{path}`: {error}"))?;
    let config: Config = toml::from_str(&source)
        .map_err(|error| format!("failed to parse manifest `{path}`: {error}"))?;
    config.violation().map_or_else(
        || Ok(config),
        |message| Err(format!("{path}: error[smelt::manifest]: {message}").into()),
    )
}

/// Parse the manifest at `path`, as [`parse`] does for a `Path`.
///
/// # Errors
///
/// Returns an error when `path` is not valid UTF-8, or as [`parse`] does.
pub fn parse_path(path: &Path) -> ConfigResult<Config> {
    parse(path.to_str().ok_or("manifest path contains invalid UTF-8")?)
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

    /// Parse a one-file manifest with `extra` appended, through the real parser.
    fn parse_with(extra: &str) -> ConfigResult<Config> {
        let directory = std::env::temp_dir().join(format!(
            "smelt-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&directory)?;
        let path = directory.join("Smelt.toml");
        fs::write(
            &path,
            format!(
                "[project]\nname = \"p\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n[output]\ntarget = \"./dist\"\n\n{extra}\n"
            ),
        )?;
        let parsed = parse_path(&path);
        fs::remove_dir_all(&directory)?;
        parsed
    }

    /// `[strict]` and `[runtime]` are optional, and default to what v1.0 supports.
    #[test]
    fn strict_and_runtime_default_when_absent() {
        parse_with("").expect("a manifest without them parses");
        parse_with("[strict]\ntypescript = true\npython = true\n\n[runtime]\nclone-strategy = \"aggressive\"")
            .expect("the supported values parse");
    }

    /// Anything else is a hard error in the spelling `specs/cli.md` gives.
    #[test]
    fn unsupported_settings_are_manifest_errors() {
        for (extra, expected) in [
            ("[strict]\ntypescript = false", "error[smelt::manifest]: strict.typescript=false is not supported in v1.0"),
            ("[strict]\npython = false", "error[smelt::manifest]: strict.python=false is not supported in v1.0"),
            ("[runtime]\nclone-strategy = \"never\"", "error[smelt::manifest]: runtime.clone-strategy=\"never\" is not supported in v1.0"),
            ("[runtime]\nclone-strategy = \"always\"", "error[smelt::manifest]: runtime.clone-strategy=\"always\" is not supported in v1.0"),
            ("[rust]\nedition = \"2099\"", "error[smelt::manifest]: rust.edition=\"2099\" is not a Rust edition"),
        ] {
            let error = parse_with(extra).expect_err(extra).to_string();
            assert!(error.contains(expected), "{extra}: {error}");
        }
    }

    /// `[specialization.oci-fallback]` names a runtime and an image; without it
    /// there is no fallback, and a misspelt key is rejected.
    #[test]
    fn the_oci_fallback_is_configured_in_specialization() {
        let configured =
            parse_with("[specialization.oci-fallback]\nruntime = \"docker\"\nimage = \"ubuntu@sha256:00\"");
        assert_eq!(
            configured.as_ref().ok().and_then(Config::oci_fallback),
            Some(("docker", "ubuntu@sha256:00"))
        );
        let absent = parse_with("");
        assert!(absent.as_ref().is_ok_and(|config| config.oci_fallback().is_none()));
        let misspelt = parse_with("[specialization.oci-fallback]\nruntime = \"docker\"\nimg = \"x\"");
        assert!(misspelt.is_err_and(|error| error.to_string().contains("unknown")));
    }

    /// A misspelt key is an error, in a table and at the top level.
    #[test]
    fn unknown_keys_are_rejected() {
        for extra in ["[rust]\neditoin = \"2021\"", "[runtim]\nclone-strategy = \"aggressive\"", "[strict]\ntypescrpt = true"] {
            let error = parse_with(extra).expect_err(extra).to_string();
            assert!(error.contains("unknown"), "{extra}: {error}");
        }
    }

    /// `test-prefix` stays an alias of `test-globs`.
    #[test]
    fn test_prefix_is_an_alias_of_test_globs() {
        let source = "[project]\nname = \"p\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"a.ts\"]\ntest-prefix = [\"**/*.test.ts\"]\n\n[output]\ntarget = \"./dist\"\n";
        let config: Config = toml::from_str(source).expect("the alias parses");
        assert_eq!(config.test_globs(), ["**/*.test.ts".to_owned()]);
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
