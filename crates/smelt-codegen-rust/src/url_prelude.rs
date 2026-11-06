//! Generated runtime for the WHATWG `URL` class.
//!
//! # `SmeltUrl`
//!
//! A `URL` is a mutable reference object: a component write (`url.pathname =
//! '/x'`) is observed through every alias, and `url.searchParams` is a live
//! list whose mutations rewrite the URL's query. `SmeltUrl` is therefore a
//! handle on a shared `url::Url` (`Rc<RefCell<..>>`), and its `searchParams`
//! is a `SmeltUrlSearchParams` owned by the same cell, so the spec's update
//! steps run in both directions:
//!
//! * a params mutation serializes the list into the URL's query
//!   (`SmeltUrlSearchParams::update_owner`), and
//! * a `search` or `href` write re-reads the list from the new query
//!   (`SmeltUrlSearchParams::refresh_from_owner`).
//!
//! Every getter and setter goes through `url::quirks`, the `url` crate's
//! implementation of exactly the WHATWG `URL` API accessors, so the
//! normalization each one applies (percent-encoding a path, dropping a default
//! port, ignoring an invalid protocol) is the spec's and not a copy of it.
//!
//! # Throwing operations
//!
//! `new URL(text)` and the `href` setter throw a catchable `TypeError` on text
//! that is not a URL. Both are MIR call terminators (`BuiltinFn::UrlNew`,
//! `BuiltinFn::UrlSetHref`) for the reason every fallible stdlib operation is:
//! only a call carries an unwind edge (see
//! `blocker-logs/standards-throwing-rvalue.md`). Their adapters report through
//! the erased error channel, so they are emitted with the other throwing
//! adapters by [`emit_throwing_support`]. Every other setter ignores a value it
//! cannot apply, as the spec says.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]


/// Name of the generated adapter behind `new URL(input, base?)`.
pub(crate) const URL_NEW_FN: &str = "smelt_url_new_throwing";

/// Name of the generated adapter behind `url.href = text`.
pub(crate) const URL_SET_HREF_FN: &str = "smelt_url_set_href_throwing";
