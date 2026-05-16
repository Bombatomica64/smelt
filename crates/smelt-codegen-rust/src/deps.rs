//! Generated Cargo dependency rendering.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and tests"
)]

use smelt_stdlib::BackendDependency;

/// Stable dependency rendering order for stdlib-backed generated crates.
///
/// Keeping the order centralized avoids Cargo.toml churn as new MIR rvalues
/// start reporting existing backend dependencies.
const STDLIB_DEPENDENCIES: [BackendDependency; 6] = [
    BackendDependency::Reqwest,
    BackendDependency::SerdeJson,
    BackendDependency::Regex,
    BackendDependency::Rand,
    BackendDependency::Chrono,
    BackendDependency::Url,
];

/// Dependency required by a generated Rust crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GeneratedDep {
    /// Tokio for generated async entrypoints and timers.
    Tokio,
    /// Standard-library backend crate.
    Stdlib(BackendDependency),
}

/// Generates Cargo.toml content for the given crate name and dependencies.
#[must_use]
pub(crate) fn cargo_toml(crate_name: &str, deps_needed: &[GeneratedDep]) -> String {
    let mut deps = String::new();
    if deps_needed.contains(&GeneratedDep::Tokio) {
        deps.push_str(
            "tokio = { version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"time\"] }\n",
        );
    }
    for dependency in STDLIB_DEPENDENCIES {
        if deps_needed.contains(&GeneratedDep::Stdlib(dependency)) {
            deps.push_str(dependency.cargo_dependency());
        }
    }
    format!(
        "[workspace]\n\n[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{deps}"
    )
}
