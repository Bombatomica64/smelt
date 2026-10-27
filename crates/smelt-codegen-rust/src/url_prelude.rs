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

use crate::rust::CodeWriter;
use crate::thrown::{THROW_FN, error_payload_record_expr};

/// Name of the generated adapter behind `new URL(input, base?)`.
pub const URL_NEW_FN: &str = "smelt_url_new_throwing";

/// Name of the generated adapter behind `url.href = text`.
pub const URL_SET_HREF_FN: &str = "smelt_url_set_href_throwing";

/// The component getters and setters, as `(component, getter, setter)`.
///
/// The getter is a `url::quirks` function name; the setter is the quirks
/// setter, or `None` for the read-only `origin`. `href` has its own throwing
/// setter, emitted by [`emit_throwing_support`].
const COMPONENTS: &[(&str, Option<&str>)] = &[
    ("href", None),
    ("origin", None),
    ("protocol", Some("set_protocol")),
    ("username", Some("set_username")),
    ("password", Some("set_password")),
    ("host", Some("set_host")),
    ("hostname", Some("set_hostname")),
    ("port", Some("set_port")),
    ("pathname", Some("set_pathname")),
    ("search", Some("set_search")),
    ("hash", Some("set_hash")),
];

/// Emit the `SmeltUrl` runtime type.
///
/// Requires `SmeltUrlSearchParams` (its `searchParams`), which the gate in
/// `stdlib::needs_url_search_params_runtime` guarantees, and
/// `smelt_next_object_id`.
pub fn emit(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_struct(writer);
    emit_inherent_impl(writer);
    if needs_unknown {
        emit_boundary_adapters(writer);
    }
}

/// Emit the struct and the traits every generated value carries.
fn emit_struct(writer: &mut CodeWriter) {
    writer.line("/// A WHATWG `URL`: a shared, mutable parsed URL whose `searchParams`");
    writer.line("/// is a live view of its query.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltUrl", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// The parsed URL, shared by every alias of this value.");
        struct_writer.line("url: ::std::rc::Rc<::std::cell::RefCell<::url::Url>>,");
        struct_writer.line("/// The live `searchParams` list, owned by the same URL.");
        struct_writer.line("params: SmeltUrlSearchParams,");
    });
    writer.blank_line();
    // Structural equality over the serialization, which is what
    // `expect(a).toEqual(b)` observes on two URLs; `===` compares `id()`.
    writer.line(
        "impl PartialEq for SmeltUrl { fn eq(&self, other: &Self) -> bool { self.href() == other.href() } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltUrl { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.href()) } }",
    );
    // A URL's string conversion is its `href` (`URL.prototype.toString`).
    writer.line(
        "impl ::std::fmt::Display for SmeltUrl { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.href()) } }",
    );
    writer.blank_line();
}

/// Emit construction, the component accessors and `searchParams`.
fn emit_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltUrl", |impl_writer| {
        impl_writer.line("/// Parse `input` against an optional `base`, or `None` when the");
        impl_writer.line("/// result is not a URL (the constructor's `TypeError`).");
        impl_writer.block(
            "pub fn parse(input: &str, base: Option<&str>) -> Option<Self>",
            |fn_writer| {
                fn_writer.line(
                    "let parsed = match base { Some(base) => ::url::Url::parse(base).ok()?.join(input).ok()?, None => ::url::Url::parse(input).ok()? };",
                );
                fn_writer.line("let url = ::std::rc::Rc::new(::std::cell::RefCell::new(parsed));");
                fn_writer.line("let params = SmeltUrlSearchParams::owned_by(::std::rc::Rc::clone(&url));");
                fn_writer.line("Some(Self { id: smelt_next_object_id(), url, params })");
            },
        );
        impl_writer.line("/// JS reference identity of this URL.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        for (component, setter) in COMPONENTS {
            impl_writer.line(format!("/// The `{component}` getter."));
            // `origin` is computed, every other getter borrows the parsed URL.
            let read = if *component == "origin" {
                "::url::quirks::origin(&self.url.borrow())".to_owned()
            } else {
                format!("::url::quirks::{component}(&self.url.borrow()).to_owned()")
            };
            impl_writer.line(format!("pub fn {component}(&self) -> String {{ {read} }}"));
            let Some(setter) = setter else { continue };
            impl_writer.line(format!(
                "/// The `{component}` setter; a value it cannot apply is ignored."
            ));
            // Only `search` replaces the query, so only it re-reads the live
            // params list.
            let refresh = if *component == "search" {
                " self.params.refresh_from_owner();"
            } else {
                ""
            };
            impl_writer.line(format!(
                "pub fn {setter}(&self, value: &str) {{ let _ = ::url::quirks::{setter}(&mut self.url.borrow_mut(), value);{refresh} }}"
            ));
        }
        impl_writer.line("/// The `href` setter, or `false` when `value` is not a URL (the");
        impl_writer.line("/// setter's `TypeError`), in which case nothing changes.");
        impl_writer.block("pub fn set_href(&self, value: &str) -> bool", |fn_writer| {
            fn_writer.line(
                "let applied = ::url::quirks::set_href(&mut self.url.borrow_mut(), value).is_ok();",
            );
            fn_writer.line("if applied { self.params.refresh_from_owner(); }");
            fn_writer.line("applied");
        });
        impl_writer.line("/// `searchParams`: the SAME live list on every read.");
        impl_writer.line(
            "pub fn search_params(&self) -> SmeltUrlSearchParams { self.params.clone() }",
        );
    });
    writer.blank_line();
}

/// Emit the `URL` dynamic-boundary adapters.
///
/// An erased value is the marker record `{ "__smelt_url": true, "href": .., }`
/// carrying every component as a data property, so an erased component read
/// answers the value it had when it was erased. The live value rides in the
/// record's origin slot, and narrowing the record back recovers that same
/// object. A DYNAMIC BOUNDARY adapter only — the internal representation stays
/// the concrete `SmeltUrl`.
fn emit_boundary_adapters(writer: &mut CodeWriter) {
    writer.line("/// Erase a URL for a dynamic boundary.");
    writer.block("impl IntoSmeltUnknown for SmeltUrl", |impl_writer| {
        impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
            let fields = COMPONENTS
                .iter()
                .map(|(component, _)| {
                    format!(
                        "(\"{component}\".to_owned(), SmeltUnknown::String(self.{component}().into()))"
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            fn_writer.line("let smelt_host_origin = self.clone();");
            fn_writer.line(format!(
                "smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"__smelt_url\".to_owned(), SmeltUnknown::Bool(true)), {fields}]))), smelt_host_origin)"
            ));
        });
    });
    writer.blank_line();
    writer.line("/// Rebuild a URL from an erased value: the retained origin, else the");
    writer.line("/// `href` a hand-built record or a string carries.");
    writer.block("impl SmeltFromUnknown for SmeltUrl", |impl_writer| {
        impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
            fn_writer.line(
                "if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }",
            );
            fn_writer.line("let href = match &value { SmeltUnknown::String(text) => text.to_string(), SmeltUnknown::Object(map) => match map.get(\"href\") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() }, _ => String::new() };");
            // A value with no URL in it was never a `URL`, so reaching here is
            // a broken type boundary, not a JavaScript error a source could
            // catch: it aborts with that diagnosis rather than inventing a URL.
            fn_writer.line(
                "Self::parse(&href, None).unwrap_or_else(|| panic!(\"erased value is not a URL: {href:?}\"))",
            );
        });
    });
    writer.blank_line();
}

/// Emit the throwing `URL` adapters into the erased-error block of the prelude.
///
/// Node's message for both is `Invalid URL`, and a program can read it.
pub fn emit_throwing_support(writer: &mut CodeWriter) {
    let error = error_payload_record_expr("TypeError", "\"Invalid URL\".to_owned()");
    writer.blank_line();
    writer.line("/// `new URL(input, base?)`, throwing a catchable `TypeError` for text that is not a URL.");
    writer.line(format!(
        "fn {URL_NEW_FN}(input: &str, base: Option<&str>) -> Result<SmeltUrl, Box<dyn ::std::error::Error>> {{ \
         SmeltUrl::parse(input, base).ok_or_else(|| {THROW_FN}({error})) }}"
    ));
    writer.line("/// `url.href = value`, throwing a catchable `TypeError` for text that is not a URL.");
    writer.line(format!(
        "fn {URL_SET_HREF_FN}(url: &SmeltUrl, value: String) -> Result<String, Box<dyn ::std::error::Error>> {{ \
         if url.set_href(&value) {{ Ok(value) }} else {{ Err({THROW_FN}({error})) }} }}"
    ));
}
