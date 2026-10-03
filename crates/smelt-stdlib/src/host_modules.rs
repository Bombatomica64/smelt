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

/// The identity of a modeled host module, whatever specifier named it.
///
/// Lowering keys a modeled export on `(identity, export name)` rather than on
/// the specifier spelling, so `node:path`, `path`, `node:path/posix` and
/// `path/posix` are one module to every rule that implements it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum HostModuleId {
    /// `@date-fns/tz`.
    DateFnsTz,
    /// `node:buffer`.
    Buffer,
    /// `node:crypto`.
    Crypto,
    /// `node:events`.
    Events,
    /// `node:http`.
    Http,
    /// `node:path` on the POSIX profile, and `node:path/posix`.
    ///
    /// One identity for both: the profile Smelt compiles for is POSIX, where
    /// Node's `path` IS `path.posix` (`require('path') === require('path').posix`
    /// on Linux and macOS), so the two specifiers name the same functions.
    Path,
    /// `node:sqlite`.
    Sqlite,
    /// `node:url`.
    Url,
}

/// A module specifier whose implementation lives in Rust rather than in source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct HostModule {
    /// Which modeled module this is, independent of the specifier spelling.
    pub id: HostModuleId,
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

/// Reason text for the `node:http` CLIENT surface.
///
/// The server half (`createServer`, `Server`, `IncomingMessage`,
/// `ServerResponse`) is modeled on hyper. `http.request`/`http.get` are the
/// client half and are a different program: they answer an `IncomingMessage`
/// driven by callbacks and events rather than taking one, and `fetch` already
/// covers what generated code needs to make a request. They stay declared, so
/// using one is a named blocker rather than a surface that half works.
const HTTP_CLIENT_REASON: &str =
    "the node:http client surface (request/get) is not implemented yet; use fetch";

/// Reason text shared by the `node:sqlite` surface.
const SQLITE_REASON: &str = "the node:sqlite database surface is not implemented yet";

/// Reason text for the `WebCrypto` members that are still declared.
///
/// The three keyless members are modeled (`randomUUID`, `getRandomValues`,
/// `subtle.digest`). What is left all takes a `CryptoKey`: `importKey` has to
/// model JWK/raw/PKCS8 key material and the algorithm identifiers that go with
/// it, and `sign`/`verify` are meaningless without it. That is a key-management
/// surface rather than three more calls, so it stays declared and Hono's
/// jwt/jwk middleware stays excluded honestly rather than half working.
const CRYPTO_KEY_REASON: &str =
    "the WebCrypto key surface (subtle.importKey/sign/verify) is not implemented yet";

/// Reason text for the `node:crypto` members outside `WebCrypto` that are
/// still declared.
///
/// `createHash` is modeled (a stateful, chainable `Hash` over the `RustCrypto`
/// hashers; see `host_module_prelude` in the Rust backend). `randomBytes`
/// answers a `Buffer` and has a callback form, so it is a surface of its own
/// rather than an alias of `getRandomValues`.
const CRYPTO_NODE_REASON: &str =
    "the node:crypto randomBytes surface is not implemented yet; use crypto.getRandomValues";

/// Reason text for the `node:path` members that are still declared.
///
/// The string-to-string surface (`join`, `resolve`, `normalize`, `dirname`,
/// `basename`, `extname`, `relative`, `isAbsolute`, `sep`, `delimiter`) is
/// modeled with Node's own POSIX algorithms. `parse`/`format` answer and take a
/// `ParsedPath` RECORD, and the `win32` flavour is a second path grammar; both
/// stay declared so using one is a named blocker rather than a guess.
const PATH_REASON: &str =
    "the node:path parse/format/win32 surface is not implemented yet";

/// The `node:path` exports, shared by `node:path` and `node:path/posix`.
///
/// `posix` is modeled as a NAMESPACE (like `crypto.subtle`): on the POSIX
/// profile `path.posix` is `path` itself, so `path.posix.join(..)` resolves to
/// the same rule as `path.join(..)`.
const PATH_EXPORTS: &[HostExport] = &[
    modeled_value("join"),
    modeled_value("resolve"),
    modeled_value("dirname"),
    modeled_value("basename"),
    modeled_value("extname"),
    modeled_value("relative"),
    modeled_value("normalize"),
    modeled_value("isAbsolute"),
    modeled_value("sep"),
    modeled_value("delimiter"),
    modeled_value("posix"),
    modeled_value("default"),
    declared_value("parse", PATH_REASON),
    declared_value("format", PATH_REASON),
    declared_value("win32", PATH_REASON),
];

/// The modeled host modules, in specifier order.
pub const HOST_MODULES: &[HostModule] = &[
    // `@date-fns/tz` is modeled because Smelt already lowers `tz(zone)` to a
    // `chrono-tz` timezone value; it is here (rather than as a name test inside
    // import lowering) so package spellings live in exactly one registry.
    HostModule {
        id: HostModuleId::DateFnsTz,
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
        id: HostModuleId::Buffer,
        specifiers: &["node:buffer", "buffer"],
        exports: &[modeled_class("Buffer")],
        dependencies: &[],
    },
    HostModule {
        id: HostModuleId::Crypto,
        specifiers: &["node:crypto", "crypto"],
        exports: &[
            modeled_value("randomUUID"),
            modeled_value("getRandomValues"),
            // `subtle` is modeled as a NAMESPACE, not as a value: the only way
            // to reach it is `subtle.digest(..)`, whose whole dotted spelling
            // is one recognized call. Reading `subtle` itself, or calling a
            // member that is not `digest`, does not resolve to this and reports
            // the key-surface blocker.
            modeled_value("subtle"),
            // Node's stateful hasher: `createHash(name)` answers a `Hash`
            // whose `update` chains and whose `digest` finalizes it.
            modeled_value("createHash"),
            modeled_class("Hash"),
            // `import crypto from 'node:crypto'` is the module object: its
            // members resolve through the same rules as the named imports
            // (`crypto.createHash(..)`, `crypto.randomUUID()`).
            modeled_value("default"),
            declared_value("randomBytes", CRYPTO_NODE_REASON),
            declared_value("importKey", CRYPTO_KEY_REASON),
            declared_value("sign", CRYPTO_KEY_REASON),
            declared_value("verify", CRYPTO_KEY_REASON),
        ],
        // The three modeled members each pull their own crate, and each is
        // reported by its own rule (`RuleId::backend_dependency`) so a program
        // that only calls `randomUUID` gets `uuid` and neither hash crate.
        dependencies: &[],
    },
    HostModule {
        id: HostModuleId::Events,
        specifiers: &["node:events", "events"],
        exports: &[
            modeled_class("EventEmitter"),
            modeled_value("default"),
        ],
        dependencies: &[],
    },
    HostModule {
        id: HostModuleId::Http,
        specifiers: &["node:http", "http"],
        exports: &[
            modeled_value("createServer"),
            modeled_class("Server"),
            modeled_class("IncomingMessage"),
            modeled_class("ServerResponse"),
            declared_value("request", HTTP_CLIENT_REASON),
            declared_value("get", HTTP_CLIENT_REASON),
            // `import http from 'node:http'` then `http.createServer(..)` is a
            // namespace read, which the modeled path does not resolve; the
            // named import is the shape that works and the reason says so.
            declared_value(
                "default",
                "import { createServer } from 'node:http' rather than the default export",
            ),
        ],
        dependencies: &[BackendDependency::Hyper],
    },
    HostModule {
        id: HostModuleId::Path,
        specifiers: &["node:path", "path", "node:path/posix", "path/posix"],
        exports: PATH_EXPORTS,
        dependencies: &[],
    },
    HostModule {
        id: HostModuleId::Sqlite,
        specifiers: &["node:sqlite"],
        exports: &[
            declared_class("DatabaseSync", SQLITE_REASON),
            declared_class("StatementSync", SQLITE_REASON),
        ],
        dependencies: &[],
    },
    HostModule {
        id: HostModuleId::Url,
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

/// Return the identity of the host module a specifier names, if modeled.
#[must_use]
pub fn host_module_id(specifier: &str) -> Option<HostModuleId> {
    host_module(specifier).map(|module| module.id)
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
        // `node:http`'s CLIENT half. Its server half is modeled, so a module
        // can be half implemented and this asserts the declared half still
        // reports rather than quietly erasing.
        let blocker =
            host_value_blocker("node:http", "request").expect("a declared export must block");
        assert!(blocker.contains("node:http"), "{blocker}");
        assert!(blocker.contains("not implemented"), "{blocker}");
    }

    /// A modeled export does not block.
    #[test]
    fn modeled_export_does_not_block() {
        assert!(host_value_blocker("@date-fns/tz", "tz").is_none());
        assert!(host_value_blocker("node:url", "URL").is_none());
        // The `node:http` server surface, modeled on hyper.
        assert!(host_value_blocker("node:http", "createServer").is_none());
        assert!(host_value_blocker("node:http", "ServerResponse").is_none());
    }

    /// A name a modeled module does not export blocks naming the export.
    #[test]
    fn unknown_export_of_modeled_module_blocks() {
        let blocker = host_value_blocker("node:url", "fileURLToPath")
            .expect("an unmodeled export must block");
        assert!(blocker.contains("fileURLToPath"), "{blocker}");
    }

    /// Every `node:path` spelling is the one POSIX path module, and its
    /// string surface is modeled while the record/win32 half stays declared.
    #[test]
    fn path_specifiers_share_one_modeled_surface() {
        for specifier in ["node:path", "path", "node:path/posix", "path/posix"] {
            assert_eq!(host_module_id(specifier), Some(HostModuleId::Path), "{specifier}");
            assert!(host_value_blocker(specifier, "join").is_none(), "{specifier}");
            assert!(host_value_blocker(specifier, "sep").is_none(), "{specifier}");
        }
        assert!(host_value_blocker("node:path", "parse").is_some());
        assert!(host_value_blocker("node:path", "win32").is_some());
    }

    /// `createHash` is modeled; `randomBytes` still reports its blocker.
    #[test]
    fn crypto_create_hash_is_modeled() {
        assert!(host_value_blocker("node:crypto", "createHash").is_none());
        assert!(host_value_blocker("crypto", "Hash").is_none());
        assert!(host_value_blocker("node:crypto", "randomBytes").is_some());
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
