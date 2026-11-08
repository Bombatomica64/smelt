// Generated runtime for the WHATWG `URL` class.
//
// # `SmeltUrl`
//
// A `URL` is a mutable reference object: a component write (`url.pathname =
// '/x'`) is observed through every alias, and `url.searchParams` is a live
// list whose mutations rewrite the URL's query. `SmeltUrl` is therefore a
// handle on a shared `url::Url` (`Rc<RefCell<..>>`), and its `searchParams`
// is a `SmeltUrlSearchParams` owned by the same cell, so the spec's update
// steps run in both directions:
//
// * a params mutation serializes the list into the URL's query
//   (`SmeltUrlSearchParams::update_owner`), and
// * a `search` or `href` write re-reads the list from the new query
//   (`SmeltUrlSearchParams::refresh_from_owner`).
//
// Every getter and setter goes through `url::quirks`, the `url` crate's
// implementation of exactly the WHATWG `URL` API accessors, so the
// normalization each one applies (percent-encoding a path, dropping a default
// port, ignoring an invalid protocol) is the spec's and not a copy of it.
//
// # Throwing operations
//
// `new URL(text)` and the `href` setter throw a catchable `TypeError` on text
// that is not a URL. Both are MIR call terminators (`Rvalue::UrlNew`,
// the `href` setter) for the reason every fallible stdlib operation is:
// only a call carries an unwind edge (see
// `blocker-logs/standards-throwing-rvalue.md`). Their adapters report through
// the erased error channel, so they are emitted with the other throwing
// adapters by [`emit_throwing_support`]. Every other setter ignores a value it
// cannot apply, as the spec says.

// Emit the throwing `URL` adapters into the erased-error block of the prelude.
//
// Node's message for both is `Invalid URL`, and a program can read it.
// @smelt:item smelt_url_new_throwing when url_throwing
/// `new URL(input, base?)`, throwing a catchable `TypeError` for text that is not a URL.
fn smelt_url_new_throwing(input: &(impl SmeltHostText + ?Sized), base: Option<&str>) -> Result<SmeltUrl, Box<dyn ::std::error::Error>> { let input = input.host_text(); let input: &str = &input; SmeltUrl::parse(input, base).ok_or_else(|| smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("Invalid URL".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) }
/// `url.href = value`, throwing a catchable `TypeError` for text that is not a URL.
fn smelt_url_set_href_throwing(url: &SmeltUrl, value: impl SmeltHostText) -> Result<String, Box<dyn ::std::error::Error>> { let value = value.host_text().into_owned(); if url.set_href(&value) { Ok(value) } else { Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("Invalid URL".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }
// @smelt:item-end

// Emit the struct and the traits every generated value carries.
// @smelt:item SmeltUrl when url
/// A WHATWG `URL`: a shared, mutable parsed URL whose `searchParams`
/// is a live view of its query.
#[derive(Clone)]
pub struct SmeltUrl {
    id: usize,
    /// The parsed URL, shared by every alias of this value.
    url: ::std::rc::Rc<::std::cell::RefCell<::url::Url>>,
    /// The live `searchParams` list, owned by the same URL.
    params: SmeltUrlSearchParams,
}

//~ Structural equality over the serialization, which is what
//~ `expect(a).toEqual(b)` observes on two URLs; `===` compares `id()`.
impl PartialEq for SmeltUrl { fn eq(&self, other: &Self) -> bool { self.href() == other.href() } }
impl ::std::fmt::Debug for SmeltUrl { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.href()) } }
//~ A URL's string conversion is its `href` (`URL.prototype.toString`).
impl ::std::fmt::Display for SmeltUrl { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.href()) } }

//~ Emit construction, the component accessors and `searchParams`.
#[allow(dead_code)]
impl SmeltUrl {
    /// Parse `input` against an optional `base`, or `None` when the
    /// result is not a URL (the constructor's `TypeError`).
    pub fn parse(input: &(impl SmeltHostText + ?Sized), base: Option<&str>) -> Option<Self> { let input = input.host_text(); let input: &str = &input;
        let parsed = match base { Some(base) => ::url::Url::parse(base).ok()?.join(input).ok()?, None => ::url::Url::parse(input).ok()? };
        let url = ::std::rc::Rc::new(::std::cell::RefCell::new(parsed));
        let params = SmeltUrlSearchParams::owned_by(::std::rc::Rc::clone(&url));
        Some(Self { id: smelt_next_object_id(), url, params })
    }
    /// JS reference identity of this URL.
    pub fn id(&self) -> usize { self.id }
    /// The `href` getter.
    pub fn href(&self) -> String { ::url::quirks::href(&self.url.borrow()).to_owned() }
    /// The `origin` getter.
    pub fn origin(&self) -> String { ::url::quirks::origin(&self.url.borrow()) }
    /// The `protocol` getter.
    pub fn protocol(&self) -> String { ::url::quirks::protocol(&self.url.borrow()).to_owned() }
    /// The `protocol` setter; a value it cannot apply is ignored.
    pub fn set_protocol(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_protocol(&mut self.url.borrow_mut(), value); }
    /// The `username` getter.
    pub fn username(&self) -> String { ::url::quirks::username(&self.url.borrow()).to_owned() }
    /// The `username` setter; a value it cannot apply is ignored.
    pub fn set_username(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_username(&mut self.url.borrow_mut(), value); }
    /// The `password` getter.
    pub fn password(&self) -> String { ::url::quirks::password(&self.url.borrow()).to_owned() }
    /// The `password` setter; a value it cannot apply is ignored.
    pub fn set_password(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_password(&mut self.url.borrow_mut(), value); }
    /// The `host` getter.
    pub fn host(&self) -> String { ::url::quirks::host(&self.url.borrow()).to_owned() }
    /// The `host` setter; a value it cannot apply is ignored.
    pub fn set_host(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_host(&mut self.url.borrow_mut(), value); }
    /// The `hostname` getter.
    pub fn hostname(&self) -> String { ::url::quirks::hostname(&self.url.borrow()).to_owned() }
    /// The `hostname` setter; a value it cannot apply is ignored.
    pub fn set_hostname(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_hostname(&mut self.url.borrow_mut(), value); }
    /// The `port` getter.
    pub fn port(&self) -> String { ::url::quirks::port(&self.url.borrow()).to_owned() }
    /// The `port` setter; a value it cannot apply is ignored.
    pub fn set_port(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_port(&mut self.url.borrow_mut(), value); }
    /// The `pathname` getter.
    pub fn pathname(&self) -> String { ::url::quirks::pathname(&self.url.borrow()).to_owned() }
    /// The `pathname` setter; a value it cannot apply is ignored.
    pub fn set_pathname(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_pathname(&mut self.url.borrow_mut(), value); }
    /// The `search` getter.
    pub fn search(&self) -> String { ::url::quirks::search(&self.url.borrow()).to_owned() }
    /// The `search` setter; a value it cannot apply is ignored.
    pub fn set_search(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_search(&mut self.url.borrow_mut(), value); self.params.refresh_from_owner(); }
    /// The `hash` getter.
    pub fn hash(&self) -> String { ::url::quirks::hash(&self.url.borrow()).to_owned() }
    /// The `hash` setter; a value it cannot apply is ignored.
    pub fn set_hash(&self, value: &(impl SmeltHostText + ?Sized)) { let value = value.host_text(); let value: &str = &value; let _ = ::url::quirks::set_hash(&mut self.url.borrow_mut(), value); }
    /// The `href` setter, or `false` when `value` is not a URL (the
    /// setter's `TypeError`), in which case nothing changes.
    pub fn set_href(&self, value: &(impl SmeltHostText + ?Sized)) -> bool { let value = value.host_text(); let value: &str = &value;
        let applied = ::url::quirks::set_href(&mut self.url.borrow_mut(), value).is_ok();
        if applied { self.params.refresh_from_owner(); }
        applied
    }
    /// `searchParams`: the SAME live list on every read.
    pub fn search_params(&self) -> SmeltUrlSearchParams { self.params.clone() }
}

// @smelt:item-end

// Emit the `URL` dynamic-boundary adapters.
//
// An erased value is the marker record `{ "__smelt_url": true, "href": .., }`
// carrying every component as a data property, so an erased component read
// answers the value it had when it was erased. The live value rides in the
// record's origin slot, and narrowing the record back recovers that same
// object. A DYNAMIC BOUNDARY adapter only — the internal representation stays
// the concrete `SmeltUrl`.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltUrl when unknown & url
/// Erase a URL for a dynamic boundary.
impl IntoSmeltUnknown for SmeltUrl {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let smelt_host_origin = self.clone();
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([("__smelt_url".to_owned(), SmeltUnknown::Bool(true)), ("href".to_owned(), SmeltUnknown::String(self.href().into())), ("origin".to_owned(), SmeltUnknown::String(self.origin().into())), ("protocol".to_owned(), SmeltUnknown::String(self.protocol().into())), ("username".to_owned(), SmeltUnknown::String(self.username().into())), ("password".to_owned(), SmeltUnknown::String(self.password().into())), ("host".to_owned(), SmeltUnknown::String(self.host().into())), ("hostname".to_owned(), SmeltUnknown::String(self.hostname().into())), ("port".to_owned(), SmeltUnknown::String(self.port().into())), ("pathname".to_owned(), SmeltUnknown::String(self.pathname().into())), ("search".to_owned(), SmeltUnknown::String(self.search().into())), ("hash".to_owned(), SmeltUnknown::String(self.hash().into()))]))), smelt_host_origin)
    }
}

/// Rebuild a URL from an erased value: the retained origin, else the
/// `href` a hand-built record or a string carries.
impl SmeltFromUnknown for SmeltUrl {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let href = match &value { SmeltUnknown::String(text) => text.to_string(), SmeltUnknown::Object(map) => match map.get("href") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() }, _ => String::new() };
        //~ A value with no URL in it was never a `URL`, so reaching here is
        //~ a broken type boundary, not a JavaScript error a source could
        //~ catch: it aborts with that diagnosis rather than inventing a URL.
        Self::parse(&href, None).unwrap_or_else(|| panic!("erased value is not a URL: {href:?}"))
    }
}

// @smelt:item-end
