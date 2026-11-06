//! The keys of the ambient `RequestInit` and `ResponseInit` dictionaries.
//!
//! One table per dictionary, read by the init literal validator, the ambient
//! interface declaration and utility types composing over the interface, so the
//! three surfaces cannot disagree about which keys exist.

/// The declared type of a fetch init key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchInitKeyKind {
    /// A `number` key (`status`).
    Number,
    /// A `string` key (`statusText`, `method`, the modeled `body` arm, and every
    /// stored `RequestInit` member but `keepalive`).
    Text,
    /// A `boolean` key (`keepalive`).
    Boolean,
    /// The `HeadersInit` union.
    HeadersInit,
    /// An `AbortSignal` (`RequestInit.signal`).
    AbortSignal,
}

/// One init dictionary key: its property spelling and declared type.
pub type FetchInitKey = (&'static str, FetchInitKeyKind);

/// The modeled `ResponseInit` keys.
pub const RESPONSE_INIT_KEYS: &[FetchInitKey] = &[
    ("status", FetchInitKeyKind::Number),
    ("statusText", FetchInitKeyKind::Text),
    ("headers", FetchInitKeyKind::HeadersInit),
];

/// The modeled `RequestInit` keys.
///
/// The first four feed the request's transport identity (or, for `signal`, a
/// dependent-signal follow); the rest are the members a request stores.
pub const REQUEST_INIT_KEYS: &[FetchInitKey] = &[
    ("method", FetchInitKeyKind::Text),
    ("headers", FetchInitKeyKind::HeadersInit),
    ("body", FetchInitKeyKind::Text),
    ("signal", FetchInitKeyKind::AbortSignal),
    ("cache", FetchInitKeyKind::Text),
    ("credentials", FetchInitKeyKind::Text),
    ("integrity", FetchInitKeyKind::Text),
    ("keepalive", FetchInitKeyKind::Boolean),
    ("mode", FetchInitKeyKind::Text),
    ("redirect", FetchInitKeyKind::Text),
    ("referrer", FetchInitKeyKind::Text),
    ("referrerPolicy", FetchInitKeyKind::Text),
];

/// Return the key table of the ambient fetch init dictionary called `name`.
#[must_use]
pub fn fetch_init_keys(name: &str) -> Option<&'static [FetchInitKey]> {
    match name {
        "ResponseInit" => Some(RESPONSE_INIT_KEYS),
        "RequestInit" => Some(REQUEST_INIT_KEYS),
        _ => None,
    }
}
