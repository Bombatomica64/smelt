//! Generated Cargo dependency rendering.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and tests"
)]

use smelt_stdlib::deps::{GENAWAITER, TOKIO};
use smelt_stdlib::{BackendDependency, CrateRequirement};

/// Dependency required by a generated Rust crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GeneratedDep {
    /// Tokio for generated async entrypoints and timers.
    Tokio,
    /// Genawaiter for resumable synchronous generator state machines.
    Genawaiter,
    /// Standard-library backend crate.
    Stdlib(BackendDependency),
}

/// Generates Cargo.toml content for the given crate name and dependencies.
#[must_use]
pub(crate) fn cargo_toml(
    crate_name: &str,
    deps_needed: &[GeneratedDep],
    allocator: crate::GeneratedAllocator,
    options: &crate::EmitOptions,
) -> String {
    let release_profile = options.release_profile;
    let required = required_crates(deps_needed, allocator);
    // The generated crate declares its own `[workspace]`, so it is a workspace
    // ROOT and its `[profile.release]` is the one Cargo honours. See
    // `crate::ReleaseProfile` for why the stock profile leaves throughput on the
    // table for generated code specifically.
    //
    // NO `panic` STRATEGY MAY BE SET HERE. A generated body whose own type says
    // `may_throw: false` reports a `throw` by panicking, and an enclosing `try`
    // catches it with `std::panic::catch_unwind` (see
    // `runtime/thrown.rs`). `panic = "abort"` would turn every
    // such catchable JavaScript exception into a process abort. This is pinned
    // by `no_profile_sets_panic_abort_while_the_panic_route_exists`.
    let profile = match release_profile {
        crate::ReleaseProfile::Optimized => {
            "\n[profile.release]\nlto = \"thin\"\ncodegen-units = 1\n"
        }
        crate::ReleaseProfile::WholeProgram => {
            "\n[profile.release]\nlto = \"fat\"\ncodegen-units = 1\n"
        }
        crate::ReleaseProfile::Default => "",
    };
    let crate::PackageMetadata { version, edition, description } = &options.package;
    let edition_text = edition.as_str();

    let description = description
        .as_deref()
        .map_or_else(String::new, |text| format!("description = {text}\n"));
    let deps = with_declared_dependencies(&required, &options.dependencies);
    format!(
        "[workspace]\n\n[package]\nname = \"{crate_name}\"\nversion = \"{version}\"\nedition = \"{edition_text}\"\n{description}\n[dependencies]\n{deps}{profile}"
    )
}

/// `mimalloc` for a program that installs it as its global allocator.
/// `default-features = false` drops mimalloc's secure and stats builds, which
/// this workload does not use.
const MIMALLOC: CrateRequirement = CrateRequirement::new("mimalloc", "0.1", false, &[]);

/// The crates `deps_needed` requires, in manifest order.
///
/// The allocator first, so the manifest reads the way the crate root does: the
/// allocator is installed before anything else runs. Then tokio, genawaiter
/// and the backend crates in [`BackendDependency::ALL`] order, which keeps the
/// manifest stable as new MIR rvalues start reporting existing dependencies.
fn required_crates(deps_needed: &[GeneratedDep], allocator: crate::GeneratedAllocator) -> Vec<CrateRequirement> {
    let mut crates = Vec::new();
    if allocator == crate::GeneratedAllocator::Mimalloc {
        crates.push(MIMALLOC);
    }
    if deps_needed.contains(&GeneratedDep::Tokio) {
        crates.push(TOKIO);
    }
    if deps_needed.contains(&GeneratedDep::Genawaiter) {
        crates.push(GENAWAITER);
    }
    for dependency in BackendDependency::ALL {
        if deps_needed.contains(&GeneratedDep::Stdlib(dependency)) {
            crates.extend_from_slice(dependency.crates());
        }
    }
    crates
}

/// The `[dependencies]` table: every `required` crate, one per line, and the
/// project's `declared` entries.
///
/// A declared crate the program does not require is appended as written. A
/// declared crate it does require takes the place of the required line with
/// the project's own version, source and keys, and with the required features
/// added to the ones it lists: the emitted runtime text calls those features'
/// APIs, so an override that dropped them (`tokio = "1.40"` loses `rt` and
/// `macros`) would leave the crate unable to compile. No key appears twice.
fn with_declared_dependencies(required: &[CrateRequirement], declared: &[(String, String)]) -> String {
    let mut lines = Vec::new();
    for requirement in required {
        match declared.iter().find(|(name, _)| name == requirement.name) {
            Some((name, value)) => lines.push(format!("{name} = {}", merged_override(requirement, value))),
            None => lines.push(requirement.manifest_line().trim_end().to_owned()),
        }
    }
    lines.extend(
        declared
            .iter()
            .filter(|(name, _)| !required.iter().any(|requirement| requirement.name == name))
            .map(|(name, value)| format!("{name} = {value}")),
    );
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// A project's override `value` (TOML value text) of a crate the program
/// requires, with the requirement's features merged in.
///
/// The override keeps every key it states, its own order and its version; a
/// required feature it does not list is appended to its `features`. An
/// override already listing them all, or one that is not a version string or
/// an inline table, is kept as written.
fn merged_override(requirement: &CrateRequirement, value: &str) -> String {
    let Ok(parsed) = format!("value = {value}").parse::<toml::Table>() else {
        return value.to_owned();
    };
    let mut table = match parsed.get("value") {
        Some(toml::Value::String(version)) => {
            let mut table = toml::Table::new();
            table.insert("version".to_owned(), toml::Value::String(version.clone()));
            table
        }
        Some(toml::Value::Table(table)) => table.clone(),
        _ => return value.to_owned(),
    };
    let mut features = match table.get("features") {
        Some(toml::Value::Array(features)) => features.clone(),
        _ => Vec::new(),
    };
    let missing = requirement
        .features
        .iter()
        .filter(|feature| !features.iter().any(|listed| listed.as_str() == Some(feature)))
        .map(|feature| toml::Value::String((*feature).to_owned()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return value.to_owned();
    }
    features.extend(missing);
    table.insert("features".to_owned(), toml::Value::Array(features));
    // `version` leads, as Cargo's own examples spell a dependency table.
    let mut ordered = table.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(key, _)| key.as_str() != "version");
    let rendered = ordered
        .into_iter()
        .map(|(key, entry)| format!("{} = {}", inline_key(key), inline_toml(entry)))
        .collect::<Vec<_>>();
    format!("{{ {} }}", rendered.join(", "))
}

/// `value` as one line of TOML: tables inline (`{ k = v }`), arrays inline.
fn inline_toml(value: &toml::Value) -> String {
    match value {
        toml::Value::Table(table) => {
            let entries = table
                .iter()
                .map(|(key, entry)| format!("{} = {}", inline_key(key), inline_toml(entry)))
                .collect::<Vec<_>>();
            format!("{{ {} }}", entries.join(", "))
        }
        toml::Value::Array(items) => {
            format!("[{}]", items.iter().map(inline_toml).collect::<Vec<_>>().join(", "))
        }
        other => other.to_string(),
    }
}

/// `key` as a TOML key: bare when it can be, quoted otherwise.
fn inline_key(key: &str) -> String {
    if !key.is_empty() && key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        key.to_owned()
    } else {
        toml::Value::String(key.to_owned()).to_string()
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the project's dependencies in the generated manifest.

    use super::{GeneratedDep, required_crates, with_declared_dependencies};
    use smelt_stdlib::BackendDependency;
    use smelt_stdlib::deps::{GENAWAITER, TOKIO};

    /// A version-only override keeps its version and gains the features the
    /// runtime text needs; its place in the table is the required crate's.
    #[test]
    fn a_version_override_keeps_the_required_features() {
        let required = [TOKIO, smelt_stdlib::CrateRequirement::new("regex", "1", true, &[])];
        let declared = vec![("tokio".to_owned(), "\"1.40\"".to_owned())];
        assert_eq!(
            with_declared_dependencies(&required, &declared),
            "tokio = { version = \"1.40\", features = [\"macros\", \"rt\", \"time\", \"net\", \"sync\"] }\nregex = \"1\"\n"
        );
    }

    /// A table override keeps its own keys and features and gains only the
    /// required features it lacks; one listing them all is kept as written.
    #[test]
    fn a_table_override_gains_only_missing_features() {
        let declared = vec![(
            "tokio".to_owned(),
            "{ version = \"1.40\", default-features = false, features = [\"fs\", \"rt\"] }".to_owned(),
        )];
        assert_eq!(
            with_declared_dependencies(&[TOKIO], &declared),
            "tokio = { version = \"1.40\", default-features = false, features = [\"fs\", \"rt\", \"macros\", \"time\", \"net\", \"sync\"] }\n"
        );
        let complete = vec![("genawaiter".to_owned(), "{ path = \"../genawaiter\" }".to_owned())];
        assert_eq!(with_declared_dependencies(&[GENAWAITER], &complete), "genawaiter = { path = \"../genawaiter\" }\n");
    }

    /// A declared crate the program does not require is appended as written.
    #[test]
    fn an_unrequired_declaration_is_appended() {
        let declared = vec![("anyhow".to_owned(), "\"1\"".to_owned())];
        assert_eq!(with_declared_dependencies(&[], &declared), "anyhow = \"1\"\n");
        assert_eq!(with_declared_dependencies(&[], &[]), "");
    }

    /// `smelt-runtime`'s manifest declares exactly the crates a generated
    /// crate can link, at the same versions, default features and features:
    /// the runtime items are compiled and tested against that manifest and
    /// inlined into crates built from this one, so a drift between the two
    /// ships text tested against crates the user never gets.
    #[test]
    fn the_runtime_manifest_matches_the_generated_dependencies() {
        let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../smelt-runtime/Cargo.toml");
        let Ok(text) = std::fs::read_to_string(manifest_path) else { panic!("smelt-runtime/Cargo.toml is readable") };
        let Ok(manifest) = text.parse::<toml::Table>() else { panic!("smelt-runtime/Cargo.toml parses") };
        let Some(runtime_deps) = manifest.get("dependencies").and_then(toml::Value::as_table) else {
            panic!("smelt-runtime declares [dependencies]")
        };
        let all = BackendDependency::ALL.map(GeneratedDep::Stdlib);
        let mut needed = vec![GeneratedDep::Tokio, GeneratedDep::Genawaiter];
        needed.extend(all);
        let generated = required_crates(&needed, crate::GeneratedAllocator::System);
        for requirement in &generated {
            let entry = runtime_deps
                .get(requirement.name)
                .unwrap_or_else(|| panic!("smelt-runtime does not declare `{}`", requirement.name));
            let field = |key: &str| entry.as_table().and_then(|table| table.get(key));
            let version = entry.as_str().or_else(|| field("version").and_then(toml::Value::as_str));
            assert_eq!(version, Some(requirement.version), "`{}` version", requirement.name);
            let default_features = field("default-features").and_then(toml::Value::as_bool).unwrap_or(true);
            assert_eq!(default_features, requirement.default_features, "`{}` default features", requirement.name);
            let features = field("features")
                .and_then(toml::Value::as_array)
                .map(|features| features.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            assert_eq!(features, requirement.features, "`{}` features", requirement.name);
            assert_eq!(field("optional").and_then(toml::Value::as_bool), Some(true), "`{}` is optional", requirement.name);
        }
        for name in runtime_deps.keys() {
            assert!(
                generated.iter().any(|requirement| requirement.name == name),
                "smelt-runtime declares `{name}`, which no generated crate links"
            );
        }
    }
}
