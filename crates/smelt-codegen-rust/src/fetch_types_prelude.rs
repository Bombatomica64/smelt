//! Runtime prelude for the WHATWG fetch types.
//!
//! The fetch types (`Headers` first; `Request`, `Response`, `FormData` and the
//! stream/abort types follow) are modeled as **concrete generated Rust types**,
//! not as marker-bearing `SmeltUnknown` records. That is the whole point of this
//! module: `headers.get("content-type")` is a `string | null` in the source
//! type, and a hand-writing Rust team would give it `Option<String>` and carry
//! that type to runtime. Routing it through a tagged record would erase the one
//! thing the caller needs.
//!
//! ## `SmeltHeaders`
//!
//! A WHATWG header list is *not* a map from name to value; it is an
//! insertion-ordered list of name/value pairs with case-insensitive names, and
//! the spec's observable behaviour follows from that:
//!
//! * `get(name)` joins **every** value for the name with `", "` — a request with
//!   two `Accept` headers reads back as one comma-joined string — and answers
//!   `null` (`None`) when the name is absent;
//! * `append` adds a pair; `set` replaces every pair with that name, keeping the
//!   position of the first one;
//! * `delete` removes every pair with the name;
//! * iteration (`keys`/`values`/`entries`) is **sorted by name** and combines
//!   values per name, which is why it is not simply the insertion order;
//! * `Set-Cookie` is the spec's one carve-out: it is never combined, so
//!   `getSetCookie()` returns one entry per cookie and iteration yields each
//!   `set-cookie` pair separately.
//!
//! Names are stored lower-cased (HTTP names are case-insensitive, and the spec
//! iterates them lower-cased), and values are normalized by stripping leading
//! and trailing HTTP whitespace, exactly as the constructor does.
//!
//! ## Why no `http::HeaderMap`
//!
//! The plan proposed wrapping `http::HeaderMap`. This implementation does not,
//! and the reason is semantic rather than a preference: `HeaderMap`'s
//! multi-value API has no comma-joining read, it validates and rejects names and
//! values (a WHATWG `Headers` normalizes instead), and `Set-Cookie` would still
//! need the carve-out above. Wrapping it would mean re-implementing all of the
//! spec behaviour *plus* carrying a dependency, so the pair list is both smaller
//! and closer to the spec. When `node:http`/`hyper` lands, the conversion to and
//! from `http::HeaderMap` belongs at that boundary — one `From` impl over
//! `entries()` — not in the value model.
//!
//! ## `SmeltUrlSearchParams`
//!
//! The same pair-list shape as `SmeltHeaders`, with the spec's differences
//! carried honestly rather than shared away:
//!
//! * names are **case-sensitive** (`a` and `A` are different parameters);
//! * `get(name)` answers the **first** value, not a joined one, and `getAll`
//!   answers every value — the two reads a query string actually needs;
//! * iteration is **insertion order**, not sorted, until `sort()` is called
//!   (which is a stable sort by name, so equal names keep their relative order);
//! * the value serializes to `application/x-www-form-urlencoded`, and parses
//!   from it. That is delegated to `url::form_urlencoded`, which the `url`
//!   crate the `URL` rules already pull in provides: percent-decoding, `+` as
//!   space, and the serializer's own encode set are exactly the spec's, and
//!   re-deriving them here would be a worse copy of a well-tested one.
//!
//! ## Identity
//!
//! `Headers` is a JavaScript reference object: two variables holding the same
//! `Headers` observe each other's mutations, and `h1 === h2` compares identity.
//! So the pair list lives behind an `Rc<RefCell<..>>` with a `smelt_next_object_id`
//! identity, in the same shape as `SmeltList` and `SmeltRegExp`.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

