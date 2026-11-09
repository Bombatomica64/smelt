//! Options that control what the Rust backend emits.

/// Options for controlling code emission behavior.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmitOptions {
    /// The name of the Rust crate to generate.
    pub crate_name: String,
    /// The Rust crate target kind to generate.
    pub crate_kind: CrateKind,
    /// The global allocator the generated program installs.
    pub allocator: GeneratedAllocator,
    /// The `[profile.release]` the generated crate carries.
    pub release_profile: ReleaseProfile,
    /// The generated crate's `[package]` metadata.
    pub package: PackageMetadata,
    /// Dependencies the project declares, as `(name, TOML value text)`. An entry
    /// replaces what Smelt would add for that crate, and is otherwise appended.
    pub dependencies: Vec<(String, String)>,
}

/// What the generated crate's `[package]` table says beyond its name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackageMetadata {
    /// Crate version.
    pub version: String,
    /// Rust edition.
    pub edition: RustEdition,
    /// Description, as TOML string text with its quotes.
    pub description: Option<String>,
}

impl Default for PackageMetadata {
    /// Version 0.1.0, edition 2024, no description.
    fn default() -> Self {
        Self { version: "0.1.0".to_owned(), edition: RustEdition::default(), description: None }
    }
}

/// A Rust edition the generated code compiles under.
///
/// Only the editions the emitted text and the inlined runtime are valid in are
/// variants: both use let-chains and edition-2024 keyword rules, so an older
/// edition would make every generated crate a compile error. Adding a variant
/// is a promise that the emitter's output and the all-flags-on runtime
/// compile under it, which `every_accepted_edition_parses_the_runtime` checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum RustEdition {
    /// The 2024 edition.
    #[default]
    Edition2024,
}

impl RustEdition {
    /// Every edition the generated code compiles under.
    pub const ALL: [Self; 1] = [Self::Edition2024];

    /// The edition as Cargo spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Edition2024 => "2024",
        }
    }
}

impl Default for EmitOptions {
    /// Returns the default emission options with crate name "smelt_app".
    fn default() -> Self {
        Self {
            crate_name: "smelt_app".to_owned(),
            crate_kind: CrateKind::Program,
            allocator: GeneratedAllocator::default(),
            release_profile: ReleaseProfile::default(),
            package: PackageMetadata::default(),
            dependencies: Vec::new(),
        }
    }
}

impl EmitOptions {
    /// Creates program emission options for the given Rust crate name.
    pub fn new(crate_name: impl Into<String>) -> Self {
        Self {
            crate_name: crate_name.into(),
            ..Self::default()
        }
    }

    /// Sets whether Smelt emits a program or library crate root.
    #[must_use]
    pub fn with_crate_kind(mut self, crate_kind: CrateKind) -> Self {
        self.crate_kind = crate_kind;
        self
    }

    /// Sets the generated crate's `[package]` metadata.
    #[must_use]
    pub fn with_package(mut self, package: PackageMetadata) -> Self {
        self.package = package;
        self
    }

    /// Sets the dependencies the project declares; see [`Self::dependencies`].
    #[must_use]
    pub fn with_dependencies(mut self, dependencies: Vec<(String, String)>) -> Self {
        self.dependencies = dependencies;
        self
    }

    /// Sets the global allocator the generated program installs.
    #[must_use]
    pub fn with_allocator(mut self, allocator: GeneratedAllocator) -> Self {
        self.allocator = allocator;
        self
    }

    /// Sets the `[profile.release]` the generated crate carries.
    #[must_use]
    pub fn with_release_profile(mut self, release_profile: ReleaseProfile) -> Self {
        self.release_profile = release_profile;
        self
    }

    /// The allocator actually emitted for this crate.
    ///
    /// A `#[global_allocator]` is a whole-program choice, so only a generated
    /// PROGRAM may make it. A generated library is linked into someone else's
    /// binary, and that binary's author owns the decision; emitting one there
    /// would silently override it.
    pub(crate) fn effective_allocator(&self) -> GeneratedAllocator {
        match self.crate_kind {
            CrateKind::Program => self.allocator,
            CrateKind::Library => GeneratedAllocator::System,
        }
    }
}

/// The `[profile.release]` a generated crate carries.
///
/// Cargo's stock release profile builds with 16 codegen units and no LTO, so a
/// call that crosses a unit boundary is never inlined. That is the common case in
/// generated code: the runtime prelude lives in the crate root and every module
/// calls into it, so `SmeltUnknown::clone`, a list index read and a field lookup
/// are all cross-unit calls in the hot loop. `callgrind` badly under-reports this
/// — instruction counts for the es-toolkit bench crate moved less than 1% — but
/// wall clock on `sumBy`, which is a loop and nothing else, moved **1.78x**, and
/// the binary got 19% SMALLER. The cost is build time (+28% on that crate) and it
/// falls only on `--release`.
///
/// A team shipping this library by hand would set it, so `Optimized` is the
/// default; `Default` leaves Cargo's profile alone for a project that would rather
/// have the build time back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReleaseProfile {
    /// Thin LTO and one codegen unit.
    #[default]
    Optimized,
    /// Cargo's stock release profile.
    Default,
    /// Fat (whole-program) LTO and one codegen unit.
    ///
    /// Thin LTO already inlines the prelude into its callers; fat LTO also
    /// optimizes across the dependency crates (hyper, tokio, serde_json) as one
    /// module. On the generated Hono server that was +3.4% requests/s on a
    /// router-and-handler-bound route, for about 25% more release build time
    ///. That trade belongs to the
    /// application: a long-running server pays the build once, a library or a
    /// test corpus mostly pays it on every build. So it is opt-in, like the
    /// allocator.
    WholeProgram,
}

/// The global allocator a generated program installs.
///
/// Generated code allocates far more than hand-written Rust does, because every
/// JavaScript array, object and string is a separate heap value with JavaScript's
/// reference semantics. Profiling the es-toolkit corpus put the malloc family at
/// roughly 30% of `groupBy` and 43% of `partition`, which is a bigger share than
/// any single thing the emitter does. glibc's allocator is tuned for a general
/// mix; this workload is a stream of small, short-lived allocations, which is
/// exactly what a modern thread-caching allocator is built for.
///
/// A team hand-writing this library in Rust would reach for one, and generated
/// programs that care about throughput should opt in with
/// `[rust] allocator = "mimalloc"`.
///
/// The DEFAULT is nonetheless `System`, deliberately. `mimalloc` builds C, which
/// means a network fetch and a compiler at build time; making that the default
/// would silently impose it on every program Smelt emits, including the ones in
/// `examples/`, which are meant to be readable and to build offline. Choosing an
/// allocator is the application author's call, so Smelt offers it rather than
/// making it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum GeneratedAllocator {
    /// Leave the platform allocator in place.
    #[default]
    System,
    /// Install `mimalloc` as the program's global allocator.
    Mimalloc,
}

/// Rust crate target kind emitted by Smelt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CrateKind {
    /// Generate an executable Rust program rooted at `src/main.rs`.
    Program,
    /// Generate a Rust library crate rooted at `src/lib.rs`.
    Library,
}

impl CrateKind {
    /// Returns the Rust source root file name for this crate kind.
    #[must_use]
    pub(crate) fn root_file_name(self) -> &'static str {
        match self {
            Self::Program => "main.rs",
            Self::Library => "lib.rs",
        }
    }

    /// Returns the Rust source root file name for the other crate kind.
    #[must_use]
    pub(crate) fn stale_root_file_name(self) -> &'static str {
        match self {
            Self::Program => "lib.rs",
            Self::Library => "main.rs",
        }
    }
}
