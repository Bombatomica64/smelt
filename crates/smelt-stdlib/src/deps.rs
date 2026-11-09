//! Backend dependency metadata for standard-library mappings.

/// Backend crate required by a standard-library mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum BackendDependency {
    /// `reqwest` for generated HTTP clients.
    Reqwest,
    /// `serde_json` for generated JSON parsing and serialization.
    SerdeJson,
    /// `regex` and `fancy-regex` for Python `re` matching.
    Regex,
    /// `regress` for JavaScript `RegExp`: ECMAScript syntax and semantics,
    /// matched over UTF-16 code units (`utf16` feature).
    Regress,
    /// `rand` for generated random values.
    Rand,
    /// `chrono` for generated date and datetime values.
    Chrono,
    /// `chrono-tz` for generated IANA time zone conversion.
    ChronoTz,
    /// `url` for generated URL parsing and field access.
    Url,
    /// `unicode-normalization` for generated `String.prototype.normalize`.
    UnicodeNormalization,
    /// `hyper` and its companions for the generated `node:http` server.
    ///
    /// One dependency rather than five, because the five are not separately
    /// useful. hyper 1 deliberately ships no runtime glue, so a server needs
    /// `hyper-util`'s connection builder and its `TokioIo` adapter to accept a
    /// socket at all, `http-body-util` to collect a request body and to answer
    /// with a full one, and `http`/`bytes` are the vocabulary types those two
    /// hand back and take. Splitting them into five `BackendDependency`
    /// variants would let a manifest be generated with three of the five, which
    /// cannot compile. A program that never calls `createServer` pays for none
    /// of them.
    Hyper,
    /// `uuid` for generated `crypto.randomUUID()`.
    Uuid,
    /// `getrandom` for generated `crypto.getRandomValues(view)`.
    ///
    /// Not `rand`: filling a byte view is exactly `getrandom`'s whole job, and
    /// it is the crate `rand` itself calls for its OS entropy. A program that
    /// only wants random bytes should not carry a distribution and generator
    /// framework to get them.
    GetRandom,
    /// `sha1` and `sha2` for generated `crypto.subtle.digest(..)`.
    ///
    /// One dependency rather than two, because a `digest` call picks its
    /// algorithm from a STRING at run time: `digest(name, data)` has to be able
    /// to answer SHA-1 and any SHA-2 width from the same match, so a manifest
    /// carrying one of the two crates could not compile the emitted call. The
    /// split that would matter — a program that only ever digests SHA-256 — is
    /// not knowable from the source spelling in general, and both crates are
    /// small.
    Sha,
    /// `hmac` for generated `crypto.subtle.sign`/`verify` over an HMAC
    /// `CryptoKey`.
    Hmac,
    /// `base64` for generated `btoa` / `atob`.
    ///
    /// The two globals are one dependency because they are the two directions
    /// of one codec and a program that has either almost always has both
    /// (`cookie.ts` signs with `btoa` and verifies with `atob`). Hand-rolling
    /// the alphabet was considered and rejected: the padding, the strict
    /// trailing-bit check that decides whether `atob` throws, and the
    /// whitespace rule are exactly the details a hand-written implementation
    /// gets subtly wrong, and `base64` is the crate a Rust team would reach
    /// for.
    Base64,
    /// `md-5` for generated `node:crypto` `createHash("md5")`.
    ///
    /// Separate from [`Self::Sha`] because `WebCrypto` never names MD5; only
    /// Node's `createHash` does, so a `subtle.digest` program must not carry it.
    /// It is the `RustCrypto` crate beside `sha1`/`sha2`, sharing their `digest`
    /// traits, so one `Digest` import drives all three.
    Md5,
    /// `rusqlite` for the generated `node:sqlite` database.
    ///
    /// Bundled, so the generated crate carries the `SQLite` it was tested
    /// against rather than linking whatever the build machine has, the way
    /// Node ships its own `SQLite`.
    Rusqlite,
    /// `num-bigint` (with its `num-traits`/`num-integer` companions) for
    /// JavaScript `bigint` values.
    ///
    /// One dependency rather than three because the generated bigint runtime
    /// needs all three at once: the `BigInt` type, the `ToPrimitive` /
    /// `FromPrimitive` / `Zero` traits its conversions go through, and
    /// `Integer::mod_floor` for `BigInt.asIntN`. A program with no `bigint`
    /// pays for none of them.
    BigInt,
    /// `indexmap` for the insertion-ordered dictionary.
    ///
    /// A Python `dict` iterates in insertion order (guaranteed since 3.7) and a
    /// JavaScript object or `Map` does too, while a Rust `HashMap` iterates in a
    /// per-process random order. `IndexMap` keeps the hashed lookup and adds the
    /// order; it is what `serde_json`'s `preserve_order` uses for the same reason.
    IndexMap,
}

/// One crate a generated manifest depends on, as the runtime text that uses it
/// requires it.
///
/// The single source both for the generated `Cargo.toml` and for
/// `smelt-runtime`'s own manifest, which a test holds equal to it: the
/// runtime's items compile against exactly the crates, versions and features a
/// generated crate links.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct CrateRequirement {
    /// The crate's name on crates.io.
    pub name: &'static str,
    /// The version requirement.
    pub version: &'static str,
    /// Whether the crate's default features stay on.
    pub default_features: bool,
    /// The features the emitted text needs.
    pub features: &'static [&'static str],
}

impl CrateRequirement {
    /// A crate at `version`, its default features on or off, plus `features`.
    #[must_use]
    pub const fn new(
        name: &'static str,
        version: &'static str,
        default_features: bool,
        features: &'static [&'static str],
    ) -> Self {
        Self { name, version, default_features, features }
    }

    /// A crate at `version` with its default features and nothing more.
    const fn plain(name: &'static str, version: &'static str) -> Self {
        Self { name, version, default_features: true, features: &[] }
    }

    /// A crate at `version` with its default features plus `features`.
    const fn with(name: &'static str, version: &'static str, features: &'static [&'static str]) -> Self {
        Self { name, version, default_features: true, features }
    }

    /// The `name = ...` line a generated `Cargo.toml` carries for this crate.
    #[must_use]
    pub fn manifest_line(&self) -> String {
        if self.default_features && self.features.is_empty() {
            return format!("{} = \"{}\"\n", self.name, self.version);
        }
        let mut entries = vec![format!("version = \"{}\"", self.version)];
        if !self.default_features {
            entries.push("default-features = false".to_owned());
        }
        if !self.features.is_empty() {
            let listed = self.features.iter().map(|feature| format!("\"{feature}\"")).collect::<Vec<_>>();
            entries.push(format!("features = [{}]", listed.join(", ")));
        }
        format!("{} = {{ {} }}\n", self.name, entries.join(", "))
    }
}

/// `tokio` for generated async entry points and timers.
///
/// `rt`, not `rt-multi-thread`: a generated async `main` builds a
/// CURRENT-THREAD runtime under a `LocalSet`, because every value Smelt
/// generates is `Rc`-based and so cannot cross a work-stealing runtime's
/// threads. `net` and `sync` come with it rather than with `hyper` because the
/// accept loop's listener and its shutdown channel are tokio's.
pub const TOKIO: CrateRequirement = CrateRequirement::with("tokio", "1", &["macros", "rt", "time", "net", "sync"]);

/// `genawaiter` for resumable synchronous generator state machines.
pub const GENAWAITER: CrateRequirement = CrateRequirement::plain("genawaiter", "0.99.1");

impl BackendDependency {
    /// Every backend dependency, in the order a generated manifest lists them.
    pub const ALL: [Self; 19] = [
        Self::Reqwest,
        Self::SerdeJson,
        Self::Regex,
        Self::Regress,
        Self::Rand,
        Self::Chrono,
        Self::ChronoTz,
        Self::Url,
        Self::UnicodeNormalization,
        Self::Hyper,
        Self::Uuid,
        Self::GetRandom,
        Self::Sha,
        Self::Hmac,
        Self::Base64,
        Self::Md5,
        Self::Rusqlite,
        Self::BigInt,
        Self::IndexMap,
    ];

    /// The crates this dependency brings into a generated manifest.
    #[must_use]
    #[expect(clippy::too_many_lines, reason = "one table entry per dependency, each with its reason")]
    pub const fn crates(self) -> &'static [CrateRequirement] {
        match self {
            Self::Reqwest => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement {
                name: "reqwest",
                version: "0.12",
                default_features: false,
                features: &["blocking", "rustls-tls"],
            }];
                CRATES
            }
            // `preserve_order`: a parsed JSON object keeps its source key
            // order, as a JavaScript object does. Without it `serde_json::Map`
            // is a `BTreeMap` and `JSON.stringify(JSON.parse(s))` came back
            // with its keys sorted.
            Self::SerdeJson => {
                const CRATES: &[CrateRequirement] = &[
                CrateRequirement::with("serde", "1", &["derive"]),
                CrateRequirement::with("serde_json", "1", &["preserve_order"]),
            ];
                CRATES
            }
            Self::Regex => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("regex", "1"), CrateRequirement::plain("fancy-regex", "0.14")];
                CRATES
            }
            Self::Regress => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::with("regress", "0.10", &["utf16"])];
                CRATES
            }
            Self::Rand => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("rand", "0.9")];
                CRATES
            }
            Self::Chrono => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("chrono", "0.4")];
                CRATES
            }
            Self::ChronoTz => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("chrono-tz", "0.10")];
                CRATES
            }
            Self::Url => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("url", "2")];
                CRATES
            }
            Self::UnicodeNormalization => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("unicode-normalization", "0.1")];
                CRATES
            }
            // `server` and `http1` are the whole of what a `createServer`
            // program uses: `http1` is the wire protocol Node's own `http`
            // module speaks, and the client half of hyper is never emitted
            // (`fetch` is reqwest). `hyper-util` is taken at its `tokio` +
            // `server` features for the same reason.
            Self::Hyper => {
                const CRATES: &[CrateRequirement] = &[
                CrateRequirement::with("hyper", "1", &["server", "http1"]),
                CrateRequirement::with("hyper-util", "0.1", &["tokio", "server"]),
                CrateRequirement::plain("http-body-util", "0.1"),
                CrateRequirement::plain("http", "1"),
                CrateRequirement::plain("bytes", "1"),
            ];
                CRATES
            }
            // `v4` is the only generator `randomUUID` names, and it is the only
            // feature taken: `uuid`'s default features add serde and formatting
            // surfaces a generated crate never spells.
            Self::Uuid => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::with("uuid", "1", &["v4"])];
                CRATES
            }
            Self::GetRandom => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("getrandom", "0.3")];
                CRATES
            }
            Self::Sha => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("sha1", "0.10"), CrateRequirement::plain("sha2", "0.10")];
                CRATES
            }
            // The MAC over the same `digest`-trait hashers `Sha` brings; a
            // program needing it is given `Sha` as well.
            Self::Hmac => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("hmac", "0.12")];
                CRATES
            }
            Self::Base64 => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("base64", "0.22")];
                CRATES
            }
            Self::Md5 => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("md-5", "0.10")];
                CRATES
            }
            Self::Rusqlite => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::with("rusqlite", "0.32", &["bundled"])];
                CRATES
            }
            Self::BigInt => {
                const CRATES: &[CrateRequirement] = &[
                CrateRequirement::plain("num-bigint", "0.4"),
                CrateRequirement::plain("num-traits", "0.2"),
                CrateRequirement::plain("num-integer", "0.1"),
            ];
                CRATES
            }
            Self::IndexMap => {
                const CRATES: &[CrateRequirement] = &[CrateRequirement::plain("indexmap", "2")];
                CRATES
            }
        }
    }

    /// The generated `Cargo.toml` dependency lines for this backend crate.
    #[must_use]
    pub fn cargo_dependency(self) -> String {
        self.crates().iter().map(CrateRequirement::manifest_line).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::BackendDependency;

    /// `ALL` lists each dependency once.
    #[test]
    fn every_dependency_is_listed_once() {
        for (index, dependency) in BackendDependency::ALL.iter().enumerate() {
            assert!(
                !BackendDependency::ALL.iter().take(index).any(|earlier| earlier == dependency),
                "{dependency:?} is listed twice"
            );
        }
    }

    /// The manifest lines keep the spellings generated crates have always had.
    #[test]
    fn manifest_lines_spell_each_shape() {
        assert_eq!(
            BackendDependency::Reqwest.cargo_dependency(),
            "reqwest = { version = \"0.12\", default-features = false, features = [\"blocking\", \"rustls-tls\"] }\n"
        );
        assert_eq!(BackendDependency::Regex.cargo_dependency(), "regex = \"1\"\nfancy-regex = \"0.14\"\n");
        assert_eq!(BackendDependency::Uuid.cargo_dependency(), "uuid = { version = \"1\", features = [\"v4\"] }\n");
    }
}
