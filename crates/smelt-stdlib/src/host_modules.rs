//! Registry of the *host modules* Smelt models.
//!
//! A host module is a module specifier whose implementation is not lowered from
//! TypeScript source but reimplemented in Rust — the Bun model. `node:*`
//! builtins are the obvious case; a handful of npm packages (`@date-fns/tz`,
//! the Vitest-compatible test frameworks) are modeled the same way.
//!
//! # Why this registry exists
//!
//! Before it, a module specifier that resolved to neither a source file nor a
//! recognized test framework was silently degraded: the frontend inserted the
//! imported binding as a module global of `Type::Unknown`, and every later use
//! of it collapsed into dynamic lookups on a value that is never built. An
//! Express app "transpiled with 0 blockers" into a crate that did nothing (see
//! `blocker-logs/express-v1-baseline.md`). The registry replaces that fallback
//! with a decision the compiler can defend:
//!
//! - the specifier is a **modeled host module** and the export is
//!   [`HostSurface::Modeled`] — lowering continues through the rule that models
//!   it;
//! - the specifier is a modeled host module but the export is
//!   [`HostSurface::Declared`] — the *shape* is known and the implementation is
//!   not written yet, so using it is a named blocker naming the module;
//! - the specifier is not modeled at all — using an imported value from it is a
//!   named blocker naming the package.
//!
//! Declaring a surface without implementing it is deliberate: it is how a
//! probe report can say "`node:sqlite` `DatabaseSync` is declared but not
//! implemented" instead of emitting a crate that pretends to have a database.
//!
//! # What a host module is *not*
//!
//! This registry does not describe values that only exist as ambient globals
//! (`Headers`, `Response`, `Blob`); those are recognized by
//! [`crate::globals`]. A name that is available both ways (`URL` is a global
//! *and* a `node:url` export) is declared in both places and resolves to the
//! same modeled surface, so a name has one modeled surface however it is
//! spelled.
//!
//! # Adding an entry
//!
//! Entries are per *module*, never per function-name spelling: a rule that
//! fires only for one library's spelling of a member is exactly the special
//! case `CLAUDE.md` forbids. When a host module's exports become implemented,
//! flip that export's [`HostSurface`] to [`HostSurface::Modeled`] in the same
//! commit as the implementation and its tests.

use crate::BackendDependency;

/// Whether a host-module export is implemented or only declared.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HostSurface {
    /// Smelt lowers this export to a real Rust surface.
    Modeled,
    /// The export's TypeScript shape is known but no lowering exists yet.
    ///
    /// Importing it is free; *using* it is a named blocker. The payload is the
    /// short reason a probe report shows next to the module name.
    Declared(&'static str),
}

/// Position an export may be used in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HostExportKind {
    /// A runtime value (function, object, class constructor).
    Value,
    /// A type-only export (interface, type alias).
    Type,
    /// A class-like export usable as both a value and a type.
    ValueAndType,
}

/// One exported name of a host module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct HostExport {
    /// TypeScript-visible export name (`"default"` for a default export).
    pub name: &'static str,
    /// Whether the name is a value, a type, or both.
    pub kind: HostExportKind,
    /// Whether Smelt implements this export or only declares its shape.
    pub surface: HostSurface,
}

/// A module specifier whose implementation lives in Rust rather than in source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct HostModule {
    /// Every specifier that names this module (`node:http` and bare `http`).
    pub specifiers: &'static [&'static str],
    /// The exports Smelt knows about, implemented or declared.
    pub exports: &'static [HostExport],
    /// Cargo dependencies the generated crate needs *only* when this module is
    /// actually used, so a crate that never touches it pays nothing.
    pub dependencies: &'static [BackendDependency],
}

/// Build a modeled export usable as a value.
const fn modeled_value(name: &'static str) -> HostExport {
    HostExport {
        name,
        kind: HostExportKind::Value,
        surface: HostSurface::Modeled,
    }
}

/// Build a modeled export usable as both a value and a type.
const fn modeled_class(name: &'static str) -> HostExport {
    HostExport {
        name,
        kind: HostExportKind::ValueAndType,
        surface: HostSurface::Modeled,
    }
}

/// Build a declared-but-unimplemented value export.
const fn declared_value(name: &'static str, reason: &'static str) -> HostExport {
    HostExport {
        name,
        kind: HostExportKind::Value,
        surface: HostSurface::Declared(reason),
    }
}

/// Build a declared-but-unimplemented class export.
const fn declared_class(name: &'static str, reason: &'static str) -> HostExport {
    HostExport {
        name,
        kind: HostExportKind::ValueAndType,
        surface: HostSurface::Declared(reason),
    }
}

/// Reason text shared by the `node:http` server surface.
const HTTP_REASON: &str = "the node:http server surface is not implemented yet";

/// Reason text shared by the `node:sqlite` surface.
const SQLITE_REASON: &str = "the node:sqlite database surface is not implemented yet";

/// Reason text shared by the `WebCrypto` surface.
const CRYPTO_REASON: &str = "the node:crypto surface is not implemented yet";

/// Reason text shared by the `node:path` surface.
///
/// `path.join`/`path.resolve` had a lowering rule that returned an empty string
/// literal. That is a worse false green than erasure: `resolve(__dirname,
/// '../key.pub')` became `""` and the program went on to open it. Real path
/// joining is cheap to implement (`std::path::PathBuf`) but it is a surface
/// with semantics to get right (`..` collapsing, absolute-segment reset,
/// platform separators), so it is declared here and the stub is gone.
const PATH_REASON: &str = "the node:path surface is not implemented yet";

/// The modeled host modules, in specifier order.
pub const HOST_MODULES: &[HostModule] = &[
    // `@date-fns/tz` is modeled because Smelt already lowers `tz(zone)` to a
    // `chrono-tz` timezone value; it is here (rather than as a name test inside
    // import lowering) so package spellings live in exactly one registry.
    HostModule {
        specifiers: &["@date-fns/tz"],
        exports: &[
            modeled_value("tz"),
            declared_class(
                "TZDate",
                "the @date-fns/tz TZDate class is not implemented yet",
            ),
        ],
        dependencies: &[BackendDependency::Chrono, BackendDependency::ChronoTz],
    },
    HostModule {
        specifiers: &["node:buffer", "buffer"],
        exports: &[modeled_class("Buffer")],
        dependencies: &[],
    },
    HostModule {
        specifiers: &["node:crypto", "crypto"],
        exports: &[
            declared_value("randomUUID", CRYPTO_REASON),
            declared_value("getRandomValues", CRYPTO_REASON),
            declared_value("subtle", CRYPTO_REASON),
            declared_value("createHash", CRYPTO_REASON),
            declared_value("randomBytes", CRYPTO_REASON),
        ],
        dependencies: &[],
    },
    HostModule {
        specifiers: &["node:events", "events"],
        exports: &[
            modeled_class("EventEmitter"),
            modeled_value("default"),
        ],
        dependencies: &[],
    },
    HostModule {
        specifiers: &["node:http", "http"],
        exports: &[
            declared_value("createServer", HTTP_REASON),
            declared_class("Server", HTTP_REASON),
            declared_class("IncomingMessage", HTTP_REASON),
            declared_class("ServerResponse", HTTP_REASON),
            declared_value("request", HTTP_REASON),
            declared_value("get", HTTP_REASON),
            declared_value("default", HTTP_REASON),
        ],
        dependencies: &[],
    },
    HostModule {
        specifiers: &["node:path", "path"],
        exports: &[
            declared_value("join", PATH_REASON),
            declared_value("resolve", PATH_REASON),
            declared_value("dirname", PATH_REASON),
            declared_value("basename", PATH_REASON),
            declared_value("extname", PATH_REASON),
            declared_value("relative", PATH_REASON),
            declared_value("normalize", PATH_REASON),
            declared_value("isAbsolute", PATH_REASON),
            declared_value("parse", PATH_REASON),
            declared_value("sep", PATH_REASON),
            declared_value("default", PATH_REASON),
        ],
        dependencies: &[],
    },
    HostModule {
        specifiers: &["node:sqlite"],
        exports: &[
            declared_class("DatabaseSync", SQLITE_REASON),
            declared_class("StatementSync", SQLITE_REASON),
        ],
        dependencies: &[],
    },
    HostModule {
        specifiers: &["node:url", "url"],
        exports: &[modeled_class("URL"), modeled_class("URLSearchParams")],
        dependencies: &[BackendDependency::Url],
    },
];

/// Return the host module a specifier names, if Smelt models one.
#[must_use]
pub fn host_module(specifier: &str) -> Option<&'static HostModule> {
    HOST_MODULES
        .iter()
        .find(|module| module.specifiers.contains(&specifier))
}

/// Return whether a specifier names a modeled host module.
#[must_use]
pub fn is_host_module(specifier: &str) -> bool {
    host_module(specifier).is_some()
}

/// Return the declared export of a host module, if the module models the name.
#[must_use]
pub fn host_module_export(specifier: &str, name: &str) -> Option<&'static HostExport> {
    host_module(specifier)?
        .exports
        .iter()
        .find(|export| export.name == name)
}

/// Return the blocker reason for using an imported host-module value.
///
/// `None` means the export is implemented and lowering may continue. `Some`
/// carries the message a diagnostic should show, covering the three cases the
/// module docs list: an unmodeled package, a modeled module that does not
/// export the name, and a declared-but-unimplemented export.
#[must_use]
pub fn host_value_blocker(specifier: &str, name: &str) -> Option<String> {
    let Some(module) = host_module(specifier) else {
        return Some(format!(
            "unresolved package `{specifier}`: not a source file and not a modeled host module"
        ));
    };
    let Some(export) = module
        .exports
        .iter()
        .find(|export| export.name == name)
    else {
        return Some(format!(
            "modeled host module `{specifier}` does not model the export `{name}`"
        ));
    };
    match export.surface {
        HostSurface::Modeled => None,
        HostSurface::Declared(reason) => Some(format!(
            "`{name}` from `{specifier}` is declared but not implemented: {reason}"
        )),
    }
}

/// Whether *using* a value imported from an unmodeled bare package blocks.
///
/// Both halves of the unresolved-import policy are enabled:
///
/// - a **modeled** host module whose export is [`HostSurface::Declared`] blocks.
///   Smelt knows the module and knows it has no implementation, so erasing the
///   binding would be a false green with no upside;
/// - an **unmodeled** bare package (`express`, `lodash`, `yup`) blocks too, at
///   the point its imported value is *used* as a value.
///
/// The second half returned `false` for one pass, because erased-library
/// interop is a real capability and turning it on had to be a deliberate
/// decision rather than a side effect. It is now `true`: a framework import
/// that drives a program is precisely the false green this classification
/// exists to stop, and a program that lowers to a crate with the framework
/// silently erased is not a program a hand-writing Rust team would ship.
///
/// Two carve-outs keep the honest cases honest, and they are what make the flip
/// safe (see `classify_pending_host_imports`): a **relative specifier** never
/// blocks, because it names a source file the manifest resolver owns; and a
/// **test module** never blocks, because assertion and fixture libraries
/// (`chai`, `yup`) only ever flow into already-erased matchers, which
/// `CLAUDE.md` sanctions explicitly.
///
/// This stays a named constant rather than being inlined: it is the one place
/// the policy is stated, and the doc above is the argument for it.
#[must_use]
pub const fn unmodeled_package_use_blocks() -> bool {
    true
}

/// Return the Cargo dependencies a host module's generated code needs.
#[must_use]
pub fn host_module_dependencies(specifier: &str) -> &'static [BackendDependency] {
    host_module(specifier).map_or(&[], |module| module.dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both the `node:`-prefixed and bare spellings name the same module.
    #[test]
    fn resolves_both_node_specifier_spellings() {
        assert_eq!(host_module("node:http"), host_module("http"));
        assert!(is_host_module("node:sqlite"));
        assert!(!is_host_module("express"));
    }

    /// An unmodeled package reports the package, not a member name.
    #[test]
    fn unmodeled_package_blocks_by_package_name() {
        let blocker = host_value_blocker("express", "default")
            .expect("an unmodeled package must block");
        assert!(blocker.contains("unresolved package `express`"), "{blocker}");
    }

    /// A declared-but-unimplemented export blocks with its module's reason.
    #[test]
    fn declared_export_blocks_with_reason() {
        let blocker = host_value_blocker("node:http", "createServer")
            .expect("a declared export must block");
        assert!(blocker.contains("node:http"), "{blocker}");
        assert!(blocker.contains("not implemented"), "{blocker}");
    }

    /// A modeled export does not block.
    #[test]
    fn modeled_export_does_not_block() {
        assert!(host_value_blocker("@date-fns/tz", "tz").is_none());
        assert!(host_value_blocker("node:url", "URL").is_none());
    }

    /// A name a modeled module does not export blocks naming the export.
    #[test]
    fn unknown_export_of_modeled_module_blocks() {
        let blocker = host_value_blocker("node:url", "fileURLToPath")
            .expect("an unmodeled export must block");
        assert!(blocker.contains("fileURLToPath"), "{blocker}");
    }

    /// Dependencies are declared per module so use stays pay-for-use.
    #[test]
    fn modules_declare_their_dependencies() {
        assert!(host_module_dependencies("node:url").contains(&BackendDependency::Url));
        assert!(host_module_dependencies("express").is_empty());
    }

    /// Every registry entry declares at least one specifier and export.
    #[test]
    fn registry_entries_are_well_formed() {
        for module in HOST_MODULES {
            assert!(
                !module.specifiers.is_empty(),
                "a host module must have a specifier"
            );
            assert!(
                !module.exports.is_empty(),
                "host module {:?} must declare exports",
                module.specifiers
            );
        }
    }
}
