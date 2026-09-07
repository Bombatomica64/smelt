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
const STDLIB_DEPENDENCIES: [BackendDependency; 9] = [
    BackendDependency::Reqwest,
    BackendDependency::SerdeJson,
    BackendDependency::Regex,
    BackendDependency::Rand,
    BackendDependency::Chrono,
    BackendDependency::ChronoTz,
    BackendDependency::Url,
    BackendDependency::UnicodeNormalization,
    BackendDependency::Hyper,
];

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
    release_profile: crate::ReleaseProfile,
) -> String {
    let mut deps = String::new();
    // First, so the manifest reads the way the crate root does: the allocator is
    // installed before anything else runs. `default-features = false` drops
    // mimalloc's secure and stats builds, which this workload does not use.
    if allocator == crate::GeneratedAllocator::Mimalloc {
        deps.push_str("mimalloc = { version = \"0.1\", default-features = false }\n");
    }
    if deps_needed.contains(&GeneratedDep::Tokio) {
        // `rt`, not `rt-multi-thread`: a generated async `main` builds a
        // CURRENT-THREAD runtime under a `LocalSet` (see
        // `FunctionEmitter::async_main_runtime_prologue`), because every value
        // Smelt generates is `Rc`-based and so cannot cross a work-stealing
        // runtime's threads. `net` and `sync` come with it rather than with
        // `hyper` because the accept loop's listener and its shutdown channel
        // are tokio's, and a feature a dependency needs belongs on the
        // dependency that is actually being configured.
        deps.push_str(
            "tokio = { version = \"1\", features = [\"macros\", \"rt\", \"time\", \"net\", \"sync\"] }\n",
        );
    }
    if deps_needed.contains(&GeneratedDep::Genawaiter) {
        deps.push_str("genawaiter = \"0.99.1\"\n");
    }
    for dependency in STDLIB_DEPENDENCIES {
        if deps_needed.contains(&GeneratedDep::Stdlib(dependency)) {
            deps.push_str(dependency.cargo_dependency());
        }
    }
    // The generated crate declares its own `[workspace]`, so it is a workspace
    // ROOT and its `[profile.release]` is the one Cargo honours. See
    // `crate::ReleaseProfile` for why the stock profile leaves throughput on the
    // table for generated code specifically.
    //
    // NO `panic` STRATEGY MAY BE SET HERE. A generated body whose own type says
    // `may_throw: false` reports a `throw` by panicking, and an enclosing `try`
    // catches it with `std::panic::catch_unwind` (see
    // `thrown::emit_panic_route_support`). `panic = "abort"` would turn every
    // such catchable JavaScript exception into a process abort. This is pinned
    // by `no_profile_sets_panic_abort_while_the_panic_route_exists`.
    let profile = match release_profile {
        crate::ReleaseProfile::Optimized => {
            "\n[profile.release]\nlto = \"thin\"\ncodegen-units = 1\n"
        }
        crate::ReleaseProfile::Default => "",
    };
    format!(
        "[workspace]\n\n[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{deps}{profile}"
    )
}
