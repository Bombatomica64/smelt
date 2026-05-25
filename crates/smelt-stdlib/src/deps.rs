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
        }
    }
}
