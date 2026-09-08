//! Backend dependency metadata for standard-library mappings.

/// Backend crate required by a standard-library mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum BackendDependency {
    /// `reqwest` for generated HTTP clients.
    Reqwest,
    /// `serde_json` for generated JSON parsing and serialization.
    SerdeJson,
    /// `regex` for generated regular-expression matching.
    Regex,
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
}

impl BackendDependency {
    /// Return the generated Cargo.toml dependency line for this backend crate.
    #[must_use]
    pub const fn cargo_dependency(self) -> &'static str {
        match self {
            Self::Reqwest => {
                "reqwest = { version = \"0.12\", default-features = false, features = [\"blocking\", \"rustls-tls\"] }\n"
            }
            Self::SerdeJson => {
                "serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n"
            }
            Self::Regex => "regex = \"1\"\nfancy-regex = \"0.14\"\n",
            Self::Rand => "rand = \"0.9\"\n",
            Self::Chrono => "chrono = \"0.4\"\n",
            Self::ChronoTz => "chrono-tz = \"0.10\"\n",
            Self::Url => "url = \"2\"\n",
            Self::UnicodeNormalization => "unicode-normalization = \"0.1\"\n",
            // `server` and `http1` are the whole of what a `createServer`
            // program uses: `http1` is the wire protocol Node's own `http`
            // module speaks, and the client half of hyper is never emitted
            // (`fetch` is reqwest). `hyper-util` is taken at its `tokio` +
            // `server` features for the same reason.
            Self::Hyper => {
                "hyper = { version = \"1\", features = [\"server\", \"http1\"] }\nhyper-util = { version = \"0.1\", features = [\"tokio\", \"server\"] }\nhttp-body-util = \"0.1\"\nhttp = \"1\"\nbytes = \"1\"\n"
            }
            // `v4` is the only generator `randomUUID` names, and it is the only
            // feature taken: `uuid`'s default features add serde and formatting
            // surfaces a generated crate never spells.
            Self::Uuid => "uuid = { version = \"1\", features = [\"v4\"] }\n",
            Self::GetRandom => "getrandom = \"0.3\"\n",
            Self::Sha => "sha1 = \"0.10\"\nsha2 = \"0.10\"\n",
        }
    }
}
