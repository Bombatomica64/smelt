// Runtime prelude for the WHATWG fetch types.
//
// The fetch types (`Headers` first; `Request`, `Response`, `FormData` and the
// stream/abort types follow) are modeled as **concrete generated Rust types**,
// not as marker-bearing `SmeltUnknown` records. That is the whole point of this
// module: `headers.get("content-type")` is a `string | null` in the source
// type, and a hand-writing Rust team would give it `Option<String>` and carry
// that type to runtime. Routing it through a tagged record would erase the one
// thing the caller needs.
//
// ## `SmeltHeaders`
//
// A WHATWG header list is *not* a map from name to value; it is an
// insertion-ordered list of name/value pairs with case-insensitive names, and
// the spec's observable behaviour follows from that:
//
// * `get(name)` joins **every** value for the name with `", "` — a request with
//   two `Accept` headers reads back as one comma-joined string — and answers
//   `null` (`None`) when the name is absent;
// * `append` adds a pair; `set` replaces every pair with that name, keeping the
//   position of the first one;
// * `delete` removes every pair with the name;
// * iteration (`keys`/`values`/`entries`) is **sorted by name** and combines
//   values per name, which is why it is not simply the insertion order;
// * `Set-Cookie` is the spec's one carve-out: it is never combined, so
//   `getSetCookie()` returns one entry per cookie and iteration yields each
//   `set-cookie` pair separately.
//
// Names are stored lower-cased (HTTP names are case-insensitive, and the spec
// iterates them lower-cased), and values are normalized by stripping leading
// and trailing HTTP whitespace, exactly as the constructor does.
//
// ## Why no `http::HeaderMap`
//
// The plan proposed wrapping `http::HeaderMap`. This implementation does not,
// and the reason is semantic rather than a preference: `HeaderMap`'s
// multi-value API has no comma-joining read, it validates and rejects names and
// values (a WHATWG `Headers` normalizes instead), and `Set-Cookie` would still
// need the carve-out above. Wrapping it would mean re-implementing all of the
// spec behaviour *plus* carrying a dependency, so the pair list is both smaller
// and closer to the spec. When `node:http`/`hyper` lands, the conversion to and
// from `http::HeaderMap` belongs at that boundary — one `From` impl over
// `entries()` — not in the value model.
//
// ## `SmeltUrlSearchParams`
//
// The same pair-list shape as `SmeltHeaders`, with the spec's differences
// carried honestly rather than shared away:
//
// * names are **case-sensitive** (`a` and `A` are different parameters);
// * `get(name)` answers the **first** value, not a joined one, and `getAll`
//   answers every value — the two reads a query string actually needs;
// * iteration is **insertion order**, not sorted, until `sort()` is called
//   (which is a stable sort by name, so equal names keep their relative order);
// * the value serializes to `application/x-www-form-urlencoded`, and parses
//   from it. That is delegated to `url::form_urlencoded`, which the `url`
//   crate the `URL` rules already pull in provides: percent-decoding, `+` as
//   space, and the serializer's own encode set are exactly the spec's, and
//   re-deriving them here would be a worse copy of a well-tested one.
//
// ## Identity
//
// `Headers` is a JavaScript reference object: two variables holding the same
// `Headers` observe each other's mutations, and `h1 === h2` compares identity.
// So the pair list lives behind an `Rc<RefCell<..>>` with a `smelt_next_object_id`
// identity, in the same shape as `SmeltList` and `SmeltRegExp`.

// Emit the struct definition and its identity-based equality.
// @smelt:item SmeltHeaders when headers
/// A WHATWG `Headers` list: ordered name/value pairs, case-insensitive
/// names, comma-joined reads, and the `Set-Cookie` carve-out.
#[derive(Clone)]
pub struct SmeltHeaders {
    id: usize,
    //~ Lower-cased name and normalized value, in insertion order.
    entries: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, String)>>>,
}

//~ Structural equality, not identity: `expect(a).toEqual(b)` on two header
//~ lists with the same pairs must hold, and the sorted entry projection is
//~ the spec's own notion of "the same headers".
impl PartialEq for SmeltHeaders { fn eq(&self, other: &Self) -> bool { self.entries_sorted() == other.entries_sorted() } }
impl ::std::fmt::Debug for SmeltHeaders { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_map().entries(self.entries_sorted()).finish() } }
impl Default for SmeltHeaders { fn default() -> Self { Self::new() } }

//~ Emit the WHATWG operations as inherent methods.
#[allow(dead_code)]
impl SmeltHeaders {
    //~ An empty header list with a fresh JS reference identity.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), entries: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }
    //~ JS reference identity of this header list.
    pub fn id(&self) -> usize { self.id }
    //~ Build a header list from name/value pairs, appending in order.
    pub fn from_pairs<N: SmeltHostText, V: SmeltHostText>(pairs: Vec<(N, V)>) -> Self { let headers = Self::new(); for (name, value) in pairs { headers.append(&name, &value); } headers }
    //~ The spec's header-name normalization: lower-cased.
    fn normalize_name(name: &str) -> String { name.trim().to_ascii_lowercase() }
    //~ The spec's header-value normalization: strip HTTP whitespace.
    fn normalize_value(value: &str) -> String { value.trim_matches(|ch| ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n').to_owned() }
    //~ `get(name)`: every value for the name, joined with `", "`.
    //~
    //~ `None` is the source `null`: the name is not in the list. The
    //~ return type is the source type, so no caller has to re-narrow it.
    pub fn get(&self, name: &(impl SmeltHostText + ?Sized)) -> Option<String> { let name = name.host_text(); let name: &str = &name;
        let key = Self::normalize_name(name);
        let values: Vec<String> = self.entries.borrow().iter().filter(|(entry_name, _)| *entry_name == key).map(|(_, value)| value.clone()).collect();
        if values.is_empty() { None } else { Some(values.join(", ")) }
    }
    //~ `has(name)`.
    pub fn has(&self, name: &(impl SmeltHostText + ?Sized)) -> bool { let name = name.host_text(); let name: &str = &name; let key = Self::normalize_name(name); self.entries.borrow().iter().any(|(entry_name, _)| *entry_name == key) }
    //~ `append(name, value)`: add a pair, keeping existing ones.
    pub fn append(&self, name: &(impl SmeltHostText + ?Sized), value: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; let value = value.host_text(); let value: &str = &value; self.entries.borrow_mut().push((Self::normalize_name(name), Self::normalize_value(value))); }
    //~ `set(name, value)`: replace every value for the name.
    //~
    //~ The first existing pair's position is kept, matching the spec's
    //~ "set the value of the first such header and remove the others".
    pub fn set(&self, name: &(impl SmeltHostText + ?Sized), value: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; let value = value.host_text(); let value: &str = &value;
        let key = Self::normalize_name(name);
        let normalized = Self::normalize_value(value);
        let mut entries = self.entries.borrow_mut();
        let position = entries.iter().position(|(entry_name, _)| *entry_name == key);
        let Some(index) = position else { entries.push((key, normalized)); return; };
        entries[index] = (key.clone(), normalized);
        //~ Keep the first pair with this name (the one just written) and
        //~ drop the rest, as the spec's `set` does.
        let mut kept = false;
        entries.retain(|(entry_name, _)| { if *entry_name != key { return true; } let first = !kept; kept = true; first });
    }
    //~ `delete(name)`: remove every pair with the name.
    pub fn delete(&self, name: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; let key = Self::normalize_name(name); self.entries.borrow_mut().retain(|(entry_name, _)| *entry_name != key); }
    //~ The spec's iteration order: sorted by name, values combined.
    //~
    //~ `set-cookie` is the exception the spec carves out: its values are
    //~ never combined, so each cookie stays its own entry.
    pub fn entries_sorted(&self) -> Vec<(String, String)> {
        //~ Sort borrowed positions, not copies: a stable sort keeps each
        //~ name's values in insertion order, and every output pair is
        //~ copied once. (The copying version cloned the list, every
        //~ name and every value before combining: ~6 allocations per
        //~ header, twice per served response;
        //~ `hono-crud-perf-round2.md` (never committed).)
        let entries = self.entries.borrow();
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by(|left, right| entries[*left].0.cmp(&entries[*right].0));
        let mut combined: Vec<(String, String)> = Vec::with_capacity(order.len());
        let mut start = 0;
        while start < order.len() {
            let name = &entries[order[start]].0;
            let mut end = start + 1;
            while end < order.len() && entries[order[end]].0 == *name { end += 1; }
            if end - start == 1 || name == "set-cookie" {
                for position in &order[start..end] { combined.push((name.clone(), entries[*position].1.clone())); }
            }
            else {
                let mut joined = String::new();
                for (count, position) in order[start..end].iter().enumerate() { if count > 0 { joined.push_str(", "); } joined.push_str(&entries[*position].1); }
                combined.push((name.clone(), joined));
            }
            start = end;
        }
        combined
    }
    //~ The stored pairs, in insertion order, uncombined.
    //~
    //~ NOT the spec's iteration order (`entries_sorted`), which sorts by
    //~ name and comma-joins values. Copying a header list has to go
    //~ through this instead: rebuilding from the combined view would turn
    //~ two `Accept` headers into one, which a copy must not do.
    pub fn entries_in_insertion_order(&self) -> Vec<(String, String)> { self.entries.borrow().clone() }
    //~ `keys()`: header names in iteration order.
    pub fn keys(&self) -> Vec<String> { self.entries_sorted().into_iter().map(|(name, _)| name).collect() }
    //~ `values()`: header values in iteration order.
    pub fn values(&self) -> Vec<String> { self.entries_sorted().into_iter().map(|(_, value)| value).collect() }
    //~ `getSetCookie()`: each `Set-Cookie` value, uncombined.
    pub fn get_set_cookie(&self) -> Vec<String> { self.entries.borrow().iter().filter(|(name, _)| name == "set-cookie").map(|(_, value)| value.clone()).collect() }
}

// @smelt:item-end

// Emit the dynamic-boundary adapters.
//
// A `Headers` value that reaches an erased position (`console.log(headers)`,
// an `unknown`-typed sink, a JSON boundary) has to become a tagged value, and
// coming back it has to be rebuilt. Both directions go through the record
// shape `{ "__smelt_headers": true, "entries": [[name, value], ..] }`: a
// DYNAMIC BOUNDARY adapter, not the internal representation, which stays the
// concrete struct above.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltHeaders when headers & unknown
/// Erase a header list for a dynamic boundary (identity marker + pairs).
impl IntoSmeltUnknown for SmeltHeaders {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ Retain the live value under the record's id, so narrowing
        //~ the erased value back — or resolving a METHOD off it through
        //~ `smelt_host_method` — reaches the SAME header list rather
        //~ than a copy rebuilt from the pairs. A `set` through an erased
        //~ view has to be visible on the value the program still holds.
        let smelt_host_origin = self.clone();
        let pairs: Vec<SmeltUnknown> = self.entries_sorted().into_iter().map(|(name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(name.into()), SmeltUnknown::String(value.into())]).into())).collect();
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([("__smelt_headers".to_owned(), SmeltUnknown::Bool(true)), ("entries".to_owned(), SmeltUnknown::Array(pairs.into()))]))), smelt_host_origin)
    }
}

/// The modeled members of an erased `Headers` record, resolved at run time.
//~
//~ **Dynamic boundary.** The receiver is a marker-bearing record, so the
//~ member it carries is decided by the record's marker and the member NAME,
//~ both of which are runtime values here — a program reaches this only by
//~ erasing the value on purpose (`as any`, an `any`-typed field), since every
//~ ordinary spelling keeps its type through narrowing. Answering `undefined`
//~ instead, which is what a plain property read does, was a silent wrong
//~ value: `(headers as any).get('a')` gave `null` where Node gives the header.
//~
//~ The recovered value is the SAME one the record was erased from (the origin
//~ registry), so a mutating member is observed by the holder of the concrete
//~ value. Only the synchronous members are here; the async body readers are
//~ not, and they keep the erased read's `undefined`.
fn smelt_headers_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key("__smelt_headers") { return None; } if !matches!(name, "get" | "has" | "set" | "append" | "delete" | "keys" | "values" | "entries" | "getSetCookie") { return None; } let headers = <SmeltHeaders as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); let method = name.to_owned(); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let arg = |index: usize| args.get(index).cloned().map_or_else(String::new, smelt_property_key); Ok(match method.as_str() { "get" => headers.get(&arg(0)).map_or(SmeltUnknown::Null, |value| SmeltUnknown::String(value.into())), "has" => SmeltUnknown::Bool(headers.has(&arg(0))), "set" => { headers.set(&arg(0), &arg(1)); SmeltUnknown::Undefined }, "append" => { headers.append(&arg(0), &arg(1)); SmeltUnknown::Undefined }, "delete" => { headers.delete(&arg(0)); SmeltUnknown::Undefined }, "keys" => SmeltUnknown::Array(headers.keys().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), "values" => SmeltUnknown::Array(headers.values().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), "entries" => SmeltUnknown::Array(headers.entries_sorted().into_iter().map(|(entry_name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(entry_name.into()), SmeltUnknown::String(value.into())]).into())).collect::<Vec<_>>().into()), _ => SmeltUnknown::Array(headers.get_set_cookie().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), }) }))) }

/// Rebuild a header list from an erased value.
impl SmeltFromUnknown for SmeltHeaders {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        //~ The retained origin first: a record that came from an erasure
        //~ in this thread narrows back to the SAME header list, which is
        //~ what JavaScript does and what makes a `set` through an erased
        //~ view visible on the value the program still holds. The
        //~ structural rebuild below is for a record that did not come
        //~ from an erasure (hand-built, or rebuilt from JSON).
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let Some(SmeltUnknown::Array(pairs)) = map.get("entries") else { return Self::new() };
        let headers = Self::new();
        for pair in pairs.into_vec() {
            let SmeltUnknown::Array(pair) = pair else { continue };
            let pair = pair.into_vec();
            let (Some(SmeltUnknown::String(name)), Some(SmeltUnknown::String(entry_value))) = (pair.first().cloned(), pair.get(1).cloned()) else { continue };
            headers.append(&name.to_string_lossy(), &entry_value.to_string_lossy());
        }
        headers
    }
}

// @smelt:item-end

// Emit the `SmeltUrlSearchParams` struct and its comparisons.
// @smelt:item SmeltUrlSearchParams when url_search_params
/// A WHATWG `URLSearchParams` list: ordered, case-sensitive
/// name/value pairs with urlencoded serialization.
#[derive(Clone)]
pub struct SmeltUrlSearchParams {
    id: usize,
    //~ Name/value pairs in insertion order.
    entries: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, String)>>>,
    //~ The `URL` whose `searchParams` this list is, if any. The spec's
    //~ update steps write every mutation back to that URL's query.
    owner: Option<::std::rc::Rc<::std::cell::RefCell<::url::Url>>>,
}

//~ Structural equality over the ordered pairs: two query strings are the
//~ same parameters when they carry the same pairs in the same order, which
//~ is what the spec's serialization observes.
impl PartialEq for SmeltUrlSearchParams { fn eq(&self, other: &Self) -> bool { *self.entries.borrow() == *other.entries.borrow() } }
impl ::std::fmt::Debug for SmeltUrlSearchParams { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.to_text()) } }
impl Default for SmeltUrlSearchParams { fn default() -> Self { Self::new() } }

//~ Emit the WHATWG `URLSearchParams` operations as inherent methods.
#[allow(dead_code)]
impl SmeltUrlSearchParams {
    //~ An empty parameter list with a fresh JS reference identity.
    pub fn new() -> Self { Self { id: smelt_next_object_id(), entries: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), owner: None } }
    //~ The live `searchParams` list of a URL: its entries are the URL's
    //~ query, and each mutation rewrites that query.
    pub fn owned_by(owner: ::std::rc::Rc<::std::cell::RefCell<::url::Url>>) -> Self {
        let params = Self { owner: Some(owner), ..Self::new() };
        params.refresh_from_owner();
        params
    }
    //~ Re-read the entries from the owning URL's query, after a write to
    //~ the URL itself (`search` or `href`) replaced it.
    pub fn refresh_from_owner(&self) {
        let Some(owner) = &self.owner else { return };
        let pairs = owner.borrow().query_pairs().map(|(name, value)| (name.into_owned(), value.into_owned())).collect::<Vec<(String, String)>>();
        *self.entries.borrow_mut() = pairs;
    }
    //~ The spec's update steps: serialize the list into the owning URL's
    //~ query, which an empty list sets to null.
    fn update_owner(&self) {
        let Some(owner) = &self.owner else { return };
        let query = self.to_text();
        owner.borrow_mut().set_query(if query.is_empty() { None } else { Some(&query) });
    }
    //~ JS reference identity of this parameter list.
    pub fn id(&self) -> usize { self.id }
    //~ Build a parameter list from name/value pairs, in order.
    pub fn from_pairs<N: SmeltHostText, V: SmeltHostText>(pairs: Vec<(N, V)>) -> Self { let params = Self::new(); params.entries.borrow_mut().extend(pairs.into_iter().map(|(name, value)| (name.into_host_string(), value.into_host_string()))); params }
    //~ Parse a query string (with or without a leading `?`).
    //~
    //~ `url::form_urlencoded` owns the decoding: `+` is a space, `%XX` is
    //~ a byte, and a pair with no `=` has the empty string as its value.
    pub fn from_query(query: &(impl SmeltHostText + ?Sized)) -> Self { let query = query.host_text(); let query: &str = &query;
        let trimmed = query.strip_prefix('?').unwrap_or(query);
        let pairs = url::form_urlencoded::parse(trimmed.as_bytes()).map(|(name, value): (::std::borrow::Cow<'_, str>, ::std::borrow::Cow<'_, str>)| (name.into_owned(), value.into_owned())).collect::<Vec<(String, String)>>();
        Self::from_pairs(pairs)
    }
    //~ `get(name)`: the FIRST value for a name, or `null`.
    pub fn get(&self, name: &(impl SmeltHostText + ?Sized)) -> Option<String> { let name = name.host_text(); let name: &str = &name; self.entries.borrow().iter().find(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()) }
    //~ `getAll(name)`: every value for a name, in order.
    pub fn get_all(&self, name: &(impl SmeltHostText + ?Sized)) -> Vec<String> { let name = name.host_text(); let name: &str = &name; self.entries.borrow().iter().filter(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()).collect() }
    //~ `has(name)`.
    pub fn has(&self, name: &(impl SmeltHostText + ?Sized)) -> bool { let name = name.host_text(); let name: &str = &name; self.entries.borrow().iter().any(|(entry_name, _)| entry_name == name) }
    //~ `append(name, value)`.
    pub fn append(&self, name: &(impl SmeltHostText + ?Sized), value: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; let value = value.host_text(); let value: &str = &value; self.entries.borrow_mut().push((name.to_owned(), value.to_owned())); self.update_owner(); }
    //~ `set(name, value)`: replace the first value, drop the rest.
    pub fn set(&self, name: &(impl SmeltHostText + ?Sized), value: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; let value = value.host_text(); let value: &str = &value;
        let mut entries = self.entries.borrow_mut();
        let position = entries.iter().position(|(entry_name, _)| entry_name == name);
        let Some(index) = position else { entries.push((name.to_owned(), value.to_owned())); drop(entries); self.update_owner(); return; };
        entries[index] = (name.to_owned(), value.to_owned());
        let mut kept = false;
        entries.retain(|(entry_name, _)| { if entry_name != name { return true; } let first = !kept; kept = true; first });
        drop(entries);
        self.update_owner();
    }
    //~ `delete(name)`: remove every pair with the name.
    pub fn delete(&self, name: &(impl SmeltHostText + ?Sized)) { let name = name.host_text(); let name: &str = &name; self.entries.borrow_mut().retain(|(entry_name, _)| entry_name != name); self.update_owner(); }
    //~ `sort()`: stable sort by name, keeping equal names in order.
    pub fn sort(&self) { self.entries.borrow_mut().sort_by(|left, right| left.0.cmp(&right.0)); self.update_owner(); }
    //~ `toString()`: the urlencoded serialization.
    pub fn to_text(&self) -> String {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in self.entries.borrow().iter() { serializer.append_pair(name, value); }
        serializer.finish()
    }
    //~ `entries()`: the pairs in insertion order.
    pub fn entries_in_order(&self) -> Vec<(String, String)> { self.entries.borrow().clone() }
    //~ `keys()`: parameter names in insertion order.
    pub fn keys(&self) -> Vec<String> { self.entries.borrow().iter().map(|(name, _)| name.clone()).collect() }
    //~ `values()`: parameter values in insertion order.
    pub fn values(&self) -> Vec<String> { self.entries.borrow().iter().map(|(_, value)| value.clone()).collect() }
    //~ `size`: the number of pairs.
    pub fn size(&self) -> f64 { self.entries.borrow().len() as f64 }
}

// @smelt:item-end

// Emit the `URLSearchParams` dynamic-boundary adapters.
//
// An erased value is the marker record
// `{ "__smelt_urlsearchparams": true, "size": n, "entries": [[name, value], ..] }`. A
// DYNAMIC BOUNDARY adapter only — the internal representation stays concrete.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltUrlSearchParams when unknown & url_search_params
/// Erase a parameter list for a dynamic boundary.
impl IntoSmeltUnknown for SmeltUrlSearchParams {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ Same identity retention as `SmeltHeaders`: an erased view of
        //~ a parameter list is a view of the SAME list.
        let smelt_host_origin = self.clone();
        let pairs: Vec<SmeltUnknown> = self.entries_in_order().into_iter().map(|(name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(name.into()), SmeltUnknown::String(value.into())]).into())).collect();
        //~ `size` is the spec's own data property, and source code
        //~ probes it on the ERASED value (`"size" in data && typeof
        //~ data.size === "number"` is how remeda's `isEmptyish` decides
        //~ emptiness), so the record has to carry it.
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([("__smelt_urlsearchparams".to_owned(), SmeltUnknown::Bool(true)), ("size".to_owned(), SmeltUnknown::Number(pairs.len() as f64)), ("entries".to_owned(), SmeltUnknown::Array(pairs.into()))]))), smelt_host_origin)
    }
}

/// The modeled members of an erased `URLSearchParams` record, resolved at run time.
//~
//~ **Dynamic boundary.** The receiver is a marker-bearing record, so the
//~ member it carries is decided by the record's marker and the member NAME,
//~ both of which are runtime values here — a program reaches this only by
//~ erasing the value on purpose (`as any`, an `any`-typed field), since every
//~ ordinary spelling keeps its type through narrowing. Answering `undefined`
//~ instead, which is what a plain property read does, was a silent wrong
//~ value: `(headers as any).get('a')` gave `null` where Node gives the header.
//~
//~ The recovered value is the SAME one the record was erased from (the origin
//~ registry), so a mutating member is observed by the holder of the concrete
//~ value. Only the synchronous members are here; the async body readers are
//~ not, and they keep the erased read's `undefined`.
fn smelt_url_search_params_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !object.contains_key("__smelt_urlsearchparams") { return None; } if !matches!(name, "get" | "getAll" | "has" | "set" | "append" | "delete" | "toString" | "keys" | "values" | "entries" | "sort") { return None; } let params = <SmeltUrlSearchParams as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())); let method = name.to_owned(); Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let arg = |index: usize| args.get(index).cloned().map_or_else(String::new, smelt_property_key); Ok(match method.as_str() { "get" => params.get(&arg(0)).map_or(SmeltUnknown::Null, |value| SmeltUnknown::String(value.into())), "getAll" => SmeltUnknown::Array(params.get_all(&arg(0)).into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), "has" => SmeltUnknown::Bool(params.has(&arg(0))), "set" => { params.set(&arg(0), &arg(1)); SmeltUnknown::Undefined }, "append" => { params.append(&arg(0), &arg(1)); SmeltUnknown::Undefined }, "delete" => { params.delete(&arg(0)); SmeltUnknown::Undefined }, "toString" => SmeltUnknown::String(params.to_text().into()), "keys" => SmeltUnknown::Array(params.keys().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), "values" => SmeltUnknown::Array(params.values().into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>().into()), "entries" => SmeltUnknown::Array(params.entries_in_order().into_iter().map(|(entry_name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(entry_name.into()), SmeltUnknown::String(value.into())]).into())).collect::<Vec<_>>().into()), _ => { params.sort(); SmeltUnknown::Undefined }, }) }))) }

/// Rebuild a parameter list from an erased value.
impl SmeltFromUnknown for SmeltUrlSearchParams {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        if let SmeltUnknown::String(query) = &value { return Self::from_query(&query.to_string_lossy()); }
        //~ The retained origin first: a record that came from an
        //~ erasure in this thread narrows back to the same object, which
        //~ is what JavaScript does. The structural rebuild below is for a
        //~ record that did NOT (hand-built, or rebuilt from JSON).
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let Some(SmeltUnknown::Array(pairs)) = map.get("entries") else { return Self::new() };
        let params = Self::new();
        for pair in pairs.into_vec() {
            let SmeltUnknown::Array(pair) = pair else { continue };
            let pair = pair.into_vec();
            let (Some(SmeltUnknown::String(name)), Some(SmeltUnknown::String(entry_value))) = (pair.first().cloned(), pair.get(1).cloned()) else { continue };
            params.append(&name.to_string_lossy(), &entry_value.to_string_lossy());
        }
        params
    }
}

// @smelt:item-end

// Emit the `SmeltBody` enum, its identity, and its single-use flag.
// @smelt:item SmeltBodyPayload when body
/// The bytes a `Request`/`Response` carries, and whether they were read.
//~
//~ A body is **single-use**: the spec's `bodyUsed` becomes `true` on the
//~ first reader, and a second read is a `TypeError`. That is why the
//~ payload sits behind an `Rc<RefCell<..>>` with a `Cell<bool>` beside it
//~ rather than being moved out: two variables holding the same response
//~ observe one another's consumption, exactly as in JavaScript.
#[derive(Clone)]
pub enum SmeltBodyPayload {
    //~ No body at all (`new Response()`, a GET request).
    Empty,
    //~ A fully-buffered body: a string, bytes, or form data.
    Bytes(Vec<u8>),
    //~ A body still arriving in chunks, in arrival order.
    //~
    //~ This is the shape `node:http`'s `IncomingMessage` and a streamed
    //~ `fetch` response need. Reading it concatenates the chunks;
    //~ `ReadableStream` (not implemented yet) is the surface that will
    //~ expose them one at a time.
    Stream(Vec<Vec<u8>>),
}

/// A single-use body with a JS reference identity.
#[derive(Clone)]
pub struct SmeltBody {
    id: usize,
    payload: ::std::rc::Rc<::std::cell::RefCell<SmeltBodyPayload>>,
    //~ The spec's `bodyUsed`, shared by every clone of this handle.
    used: ::std::rc::Rc<::std::cell::Cell<bool>>,
    //~ The `Content-Type` this body implies, when it implies one.
    //~
    //~ The spec's "extract a body" step returns a body *and* a type, and
    //~ the type is what a `Request`/`Response` constructor appends to the
    //~ header list when the caller did not set one. A string body implies
    //~ `text/plain;charset=UTF-8`; raw bytes and a stream imply nothing,
    //~ which is why this is an `Option` rather than a default string.
    content_type: Option<String>,
}

//~ Structural equality over the bytes, matching how the other fetch types
//~ compare: `expect(a).toEqual(b)` on two responses with the same body must
//~ hold. Reading the bytes for a comparison must NOT consume the body, so
//~ this goes through `peek_bytes`, never through `take_bytes`.
impl PartialEq for SmeltBody { fn eq(&self, other: &Self) -> bool { self.peek_bytes() == other.peek_bytes() } }
impl ::std::fmt::Debug for SmeltBody { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltBody").field("used", &self.used.get()).field("len", &self.peek_bytes().len()).finish() } }
impl Default for SmeltBody { fn default() -> Self { Self::empty() } }

//~ Emit the body's constructors and its readers.
#[allow(dead_code)]
impl SmeltBody {
    //~ An unused empty body with a fresh JS reference identity.
    pub fn empty() -> Self { Self::from_payload(SmeltBodyPayload::Empty) }
    //~ A buffered body holding `text`'s UTF-8 bytes.
    //~
    //~ Carries `text/plain;charset=UTF-8` as its implied type, which the
    //~ holder's constructor appends when the caller set no `Content-Type`.
    pub fn from_text(text: &(impl SmeltHostText + ?Sized)) -> Self { let text = text.host_text(); let text: &str = &text; let mut body = Self::from_payload(SmeltBodyPayload::Bytes(text.as_bytes().to_vec())); body.content_type = Some("text/plain;charset=UTF-8".to_owned()); body }
    //~ A buffered body holding `bytes`.
    pub fn from_bytes(bytes: Vec<u8>) -> Self { Self::from_payload(SmeltBodyPayload::Bytes(bytes)) }
    //~ The spec's "extract a body" step for a `Blob` carries the blob's
    //~ `type` as the body's content type, unless the blob has none.
    //~ A body from a blob's bytes, carrying the blob's MIME type.
    pub fn from_blob(bytes: Vec<u8>, blob_type: String) -> Self { let mut body = Self::from_payload(SmeltBodyPayload::Bytes(bytes)); if !blob_type.is_empty() { body.content_type = Some(blob_type); } body }
    //~ A streaming body whose chunks arrive in order.
    pub fn from_chunks(chunks: Vec<Vec<u8>>) -> Self { Self::from_payload(SmeltBodyPayload::Stream(chunks)) }
    //~ The spec's `Request` constructor step for a REQUEST input (and for a
    //~ request at the init position): the new request reads the source's
    //~ body, and the SOURCE is disturbed immediately — measured against Node
    //~ 22, where `new Request(source)` leaves `source.bodyUsed` true and the
    //~ copy's false before either is read. So the two share the PAYLOAD but
    //~ not the used flag: sharing the flag would leave the source readable
    //~ until the copy was consumed, and copying the payload would let both
    //~ be read.
    //~ Take a source body: same payload, fresh used flag, source disturbed.
    pub fn take_from_source(source: &Self) -> Self {
        if !source.is_empty() { source.used.set(true); }
        Self { id: smelt_next_object_id(), payload: ::std::rc::Rc::clone(&source.payload), used: ::std::rc::Rc::new(::std::cell::Cell::new(false)), content_type: source.content_type.clone() }
    }
    //~ Wrap a payload, unused, with a fresh identity.
    fn from_payload(payload: SmeltBodyPayload) -> Self { Self { id: smelt_next_object_id(), payload: ::std::rc::Rc::new(::std::cell::RefCell::new(payload)), used: ::std::rc::Rc::new(::std::cell::Cell::new(false)), content_type: None } }
    //~ JS reference identity of this body.
    pub fn id(&self) -> usize { self.id }
    //~ The spec's `bodyUsed`.
    pub fn body_used(&self) -> bool { self.used.get() }
    //~ `ReadableStream.locked`: the body has no reader API in this model, so
    //~ it is locked exactly when it has been consumed (see
    //~ `smelt_stdlib::host_data_property`).
    pub fn locked(&self) -> bool { self.used.get() }
    //~ The `Content-Type` this body implies, when it implies one.
    pub fn content_type(&self) -> Option<String> { self.content_type.clone() }
    //~ Whether there is no body at all (the spec's null body).
    pub fn is_empty(&self) -> bool { matches!(&*self.payload.borrow(), SmeltBodyPayload::Empty) }
    //~ The body's bytes WITHOUT consuming it.
    //~
    //~ Only for observers that the spec does not count as readers:
    //~ equality, `Debug`, and cloning a response. Every source-visible
    //~ reader goes through `take_bytes`.
    pub fn peek_bytes(&self) -> Vec<u8> {
        match &*self.payload.borrow() {
            SmeltBodyPayload::Empty => Vec::new(),
            SmeltBodyPayload::Bytes(bytes) => bytes.clone(),
            SmeltBodyPayload::Stream(chunks) => chunks.concat(),
        }
    }
    //~ Consume the body, or fail the way the spec does.
    //~
    //~ The first reader gets the bytes and sets `bodyUsed`; a second
    //~ reader gets the spec's `TypeError: Body is unusable`. The error is
    //~ a thrown JS value rather than a Rust panic, so source-level
    //~ `try`/`catch` around a double read behaves as it does in Node.
    pub fn take_bytes(&self) -> Result<Vec<u8>, Box<dyn ::std::error::Error>> {
        if self.used.get() {
// @smelt:if unknown
            return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("Body is unusable: Body has already been read".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))));
// @smelt:else
//=             return Err(Box::<dyn ::std::error::Error>::from("TypeError: Body is unusable: Body has already been read"));
// @smelt:end
        }
        self.used.set(true);
        Ok(self.peek_bytes())
    }
    //~ `text()`: the body decoded as UTF-8, lossily, as the spec does.
    pub fn take_text(&self) -> Result<String, Box<dyn ::std::error::Error>> { Ok(String::from_utf8_lossy(&self.take_bytes()?).into_owned()) }
    //~ A clone that shares neither the bytes nor the used flag.
    //~
    //~ This is `Response.clone()`: the spec gives the clone its own
    //~ unread body, so reading one must not consume the other. `Clone`
    //~ (the Rust trait) is the *handle* copy and shares both, which is
    //~ what assigning a response to another variable does.
    //~
    //~ The spec makes `clone()` of a USED body throw a `TypeError`. A clone
    //~ here is a plain value, so the clone of a used body is itself used:
    //~ every read of it throws that same `TypeError` (`Body is unusable`),
    //~ and `try { await res.clone().json() } catch {}` takes the catch as it
    //~ does in JavaScript. The error surfaces at the first read rather than
    //~ at `clone()` itself.
    pub fn tee(&self) -> Self { let mut body = Self::from_payload(self.payload.borrow().clone()); body.content_type = self.content_type.clone(); if self.used.get() { body.used.set(true); } body }
}

// @smelt:item-end

// Emit the `ReadableStream` (`SmeltBody`) dynamic-boundary adapters.
//
// A body crossing an erased slot keeps its bytes: the record carries the
// `ReadableStream` class tag and the live handle in its origin slot, so
// narrowing it back is the SAME body, `bodyUsed` cell included, as for the
// `Request`/`Response` that hold one (Hono's `c.newResponse(res.body, res)`
// in its `HTTPException` error handler reads it).
// @smelt:item impl_IntoSmeltUnknown_for_SmeltBody when body & unknown
/// Erase a body for a dynamic boundary, retaining the live handle.
impl IntoSmeltUnknown for SmeltBody {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        let smelt_host_origin = self.clone();
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([("__smelt_class".to_owned(), SmeltUnknown::String("ReadableStream".into())), ("locked".to_owned(), SmeltUnknown::Bool(self.locked()))]))), smelt_host_origin)
    }
}

/// Recover a body from an erased value: the retained handle, else empty.
impl SmeltFromUnknown for SmeltBody {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        smelt_restore_host_origin::<Self>(&value).unwrap_or_else(Self::empty)
    }
}

// @smelt:item-end

// Emit the `SmeltResponse` struct and its comparisons.
// @smelt:item SmeltResponse when response
/// A WHATWG `Response`: a status line, a header list, and a body.
//~
//~ The status line and headers are plain fields because the spec makes
//~ them immutable on a response, so there is nothing for a shared cell to
//~ coordinate. The BODY is the mutable part — reading it is observable
//~ through every handle — and `SmeltBody` owns that sharing, which is why
//~ this struct does not wrap itself in another `Rc<RefCell<..>>`.
#[derive(Clone)]
pub struct SmeltResponse {
    id: usize,
    status: f64,
    status_text: String,
    headers: SmeltHeaders,
    body: SmeltBody,
}

//~ Structural equality, matching `SmeltHeaders`: two responses with the same
//~ status line, headers and bytes are equal, so `expect(a).toEqual(b)`
//~ holds. Comparing must not consume either body (see `SmeltBody::eq`).
impl PartialEq for SmeltResponse { fn eq(&self, other: &Self) -> bool { self.status == other.status && self.status_text == other.status_text && self.headers == other.headers && self.body == other.body } }
impl ::std::fmt::Debug for SmeltResponse { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltResponse").field("status", &self.status).field("statusText", &self.status_text).field("headers", &self.headers).field("body", &self.body).finish() } }
impl Default for SmeltResponse { fn default() -> Self { Self::new() } }

//~ Emit the response's constructors and its member operations.
#[allow(dead_code)]
impl SmeltResponse {
    //~ `new Response()`: 200, empty reason phrase, no body.
    //~
    //~ 200 is the spec's default status, and the default reason phrase is
    //~ the EMPTY string, not `"OK"` — `new Response().statusText` is `""`
    //~ in Node. Filling in a phrase here would invent an observable value.
    pub fn new() -> Self { Self::from_parts(200.0, String::new(), SmeltHeaders::new(), SmeltBody::empty()) }
    //~ Assemble a response with a fresh JS reference identity.
    //~
    //~ A body that implies a `Content-Type` adds it to the header list
    //~ unless the caller already set one — the spec's "extract a body"
    //~ step, which is why `new Response('hi').headers.get('content-type')`
    //~ is `text/plain;charset=UTF-8` and not `null`.
    pub fn from_parts(status: f64, status_text: impl SmeltHostText, headers: SmeltHeaders, body: SmeltBody) -> Self { let status_text = status_text.host_text().into_owned();
        if let Some(content_type) = body.content_type() { if !headers.has("content-type") {
            headers.append("content-type", &content_type);
        } }
        Self { id: smelt_next_object_id(), status, status_text, headers, body }
    }
    //~ JS reference identity of this response.
    pub fn id(&self) -> usize { self.id }
    //~ `status`.
    pub fn status(&self) -> f64 { self.status }
    //~ `statusText`.
    pub fn status_text(&self) -> String { self.status_text.clone() }
    //~ `ok`: the spec derives it from the status, so this does too.
    //~
    //~ Storing it would let it drift from `status`; deriving cannot.
    pub fn ok(&self) -> bool { self.status >= 200.0 && self.status <= 299.0 }
    //~ `headers`.
    //~
    //~ The same header list, not a copy: `Headers` is a reference object,
    //~ so two reads of `response.headers` observe one another.
    pub fn headers(&self) -> SmeltHeaders { self.headers.clone() }
    //~ `bodyUsed`.
    pub fn body_used(&self) -> bool { self.body.body_used() }
    //~ The response's body handle.
    pub fn body(&self) -> SmeltBody { self.body.clone() }
    //~ `text()`: the body decoded as UTF-8, consuming it.
    //~
    //~ Fallible for the spec's reason: a second read is a `TypeError`.
    pub fn take_text(&self) -> Result<String, Box<dyn ::std::error::Error>> { self.body.take_text() }
    //~ `clone()`: a response whose body is independently readable.
    //~
    //~ The spec's `clone()` tees the body, so reading one side must not
    //~ consume the other. It is therefore NOT Rust's `Clone`, which copies
    //~ the handle and keeps one shared body — that is what assigning a
    //~ response to a second variable does, and both spellings are needed.
    //~
    //~ The header list is copied too: the clone's headers are its own.
    pub fn tee(&self) -> Self { Self::from_parts(self.status, self.status_text.clone(), SmeltHeaders::from_pairs(self.headers.entries_in_insertion_order()), self.body.tee()) }
}

// @smelt:item-end

// Emit the `Response`/`Request` dynamic-boundary adapters.
//
// **Dynamic boundary.** A response or request that reaches an erased position
// (`console.log(response)`, an `unknown`-typed sink, `isPlainObject(value)`)
// has to become a tagged value, and the tag is what carries host identity:
// `isPlainObject(new Request('http://localhost'))` is `false` *because* of
// the `__smelt_request` marker, and `instanceof` resolves through it. No
// concrete type, union, or generic can stand in for the record, because the
// receiving position's type is `unknown` — the value's shape is only known on
// the run-time branch that produced it. The internal representation stays the
// concrete struct; this is the boundary, not the storage.
//
// The es-toolkit gate tests
// (`estk_transpile_gate_tests::request_construction_carries_no_marker_record`,
// `fetch_types_tests::request_erasure_stamps_the_host_identity_marker`)
// pin the marker, which is stamped at erasure and is exactly the `isPlainObject`
// probe es-toolkit's own spec performs.
//
// The body is carried as its TEXT rather than as a handle, because an erased
// record cannot hold a single-use cell: a body that round-tripped through the
// boundary would otherwise share a used flag with a value that does not
// exist. Erasing does not consume the body (it peeks), so a response can be
// logged and still read.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltResponse when response & unknown
/// Erase a response for a dynamic boundary (identity marker + status line).
//~
//~ Retains the live response, for the reason its `Request` sibling does:
//~ erasing a value and narrowing it back is the SAME object in JavaScript,
//~ body handle and `bodyUsed` cell included.
impl IntoSmeltUnknown for SmeltResponse {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ The field record is built on first use (`lazy_view`): a response
        //~ erased only to be narrowed back (every Hono handler result) is
        //~ recovered from the origin and never reads it.
        let smelt_host_origin = self.clone();
        let smelt_id = self.id;
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::lazy_with_id(smelt_id, move || { let body_text = String::from_utf8_lossy(&self.body.peek_bytes()).into_owned(); Vec::from([("__smelt_response".to_owned(), SmeltUnknown::Bool(true)), ("status".to_owned(), SmeltUnknown::Number(self.status)), ("statusText".to_owned(), SmeltUnknown::String(self.status_text.into())), ("ok".to_owned(), SmeltUnknown::Bool(self.status >= 200.0 && self.status <= 299.0)), ("headers".to_owned(), self.headers.into_smelt_unknown()), ("body".to_owned(), SmeltUnknown::String(body_text.into()))]) })), smelt_host_origin)
    }
}

/// Recover a response from an erased value.
//~
//~ The retained origin first, exactly as for a request; a record that did
//~ not come from an erasure is rebuilt from its fields.
impl SmeltFromUnknown for SmeltResponse {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::new() };
        let status = match map.get("status") { Some(SmeltUnknown::Number(status)) => status, _ => 200.0 };
        let status_text = match map.get("statusText") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() };
        let headers = map.get("headers").map_or_else(SmeltHeaders::new, SmeltHeaders::smelt_from_unknown);
        let body = match map.get("body") { Some(SmeltUnknown::String(text)) if !text.is_empty() => SmeltBody::from_text(&text.to_string()), _ => SmeltBody::empty() };
        Self::from_parts(status, status_text, headers, body)
    }
}

// @smelt:item-end

// Emit `SmeltRequestInit`: the stored, read-only half of a `RequestInit`.
//
// These eight members (`cache`, `credentials`, `integrity`, `keepalive`,
// `mode`, `redirect`, `referrer`, `referrerPolicy`) are scalars a request
// keeps from construction and answers unchanged — no transport behaviour reads
// them here, so they are storage plus getters, exactly as the spec describes
// them. They live in their own struct rather than as eight more `SmeltRequest`
// fields because they move together: a `Request` input copies the whole group,
// `clone()` carries the whole group, and the defaults are one value.
//
// Every field is a concrete Rust type — `String` for the WebIDL enumerations,
// `bool` for `keepalive` — so nothing here reaches `SmeltUnknown`.
// @smelt:item SmeltRequestInit when request
/// The stored, read-only members of a WHATWG `RequestInit`.
//~
//~ `Default` is the spec's own set for a request constructed from a URL,
//~ which is what Node answers for `new Request(url)`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SmeltRequestInit {
    //~ The spec's `cache`.
    pub cache: String,
    //~ The spec's `credentials`.
    pub credentials: String,
    //~ The spec's `integrity`.
    pub integrity: String,
    //~ The spec's `keepalive`.
    pub keepalive: bool,
    //~ The spec's `mode`.
    pub mode: String,
    //~ The spec's `redirect`.
    pub redirect: String,
    //~ The spec's `referrer`.
    pub referrer: String,
    //~ The spec's `referrerPolicy`.
    pub referrer_policy: String,
}

impl Default for SmeltRequestInit {
    fn default() -> Self {
        Self { cache: "default".to_owned(), credentials: "same-origin".to_owned(), integrity: String::new(), keepalive: false, mode: "cors".to_owned(), redirect: "follow".to_owned(), referrer: "about:client".to_owned(), referrer_policy: String::new() }
    }
}

thread_local! {
    //~ The spec's default `RequestInit`, shared by every request built without one.
    static SMELT_DEFAULT_REQUEST_INIT: ::std::rc::Rc<SmeltRequestInit> = ::std::rc::Rc::new(SmeltRequestInit::default());
}

/// A handle on the shared default `RequestInit` (its members are immutable).
fn smelt_default_request_init() -> ::std::rc::Rc<SmeltRequestInit> { SMELT_DEFAULT_REQUEST_INIT.with(::std::rc::Rc::clone) }

#[allow(dead_code)]
impl SmeltRequestInit {
    //~ Serialize a spelled `referrer` the way the spec stores it.
    //~
    //~ The referrer is parsed as a URL and stored as its serialization,
    //~ so `"https://c.test"` reads back as `"https://c.test/"`. The two
    //~ values that are not URLs — the empty string (no referrer) and
    //~ `about:client` — pass through, which falling back to the spelled
    //~ text gives for free. This is the same rule `from_parts` applies to
    //~ the request URL itself.
    pub fn serialize_referrer(referrer: &(impl SmeltHostText + ?Sized)) -> String { let referrer = referrer.host_text(); let referrer: &str = &referrer; ::url::Url::parse(referrer).map_or_else(|_| referrer.to_owned(), |parsed| parsed.to_string()) }
}

//~ Emit the `SmeltRequest` struct and its comparisons.
/// A WHATWG `Request`: a serialized URL, a method, headers, and a body.
//~
//~ The same split as `SmeltResponse`: the URL and method are immutable on
//~ a request, so they are plain fields, and `SmeltBody` owns the one
//~ mutable thing — whether the body has been read.
#[derive(Clone)]
pub struct SmeltRequest {
    id: usize,
    url: String,
    method: String,
    headers: SmeltHeaders,
    body: SmeltBody,
    //~ The stored `RequestInit` members; see `SmeltRequestInit`.
    //~
    //~ Shared, because they are immutable once the request exists: a request
    //~ is cloned on every handle copy, and seven owned strings per clone were
    //~ a measurable share of a served request.
    init: ::std::rc::Rc<SmeltRequestInit>,
}

impl PartialEq for SmeltRequest { fn eq(&self, other: &Self) -> bool { self.url == other.url && self.method == other.method && self.headers == other.headers && self.body == other.body && self.init == other.init } }
impl ::std::fmt::Debug for SmeltRequest { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltRequest").field("method", &self.method).field("url", &self.url).field("headers", &self.headers).field("body", &self.body).finish() } }
//~ A class holding a `Request` field derives `Default` for its storage
//~ struct, like every other modeled fetch value (`SmeltResponse`,
//~ `SmeltHeaders`); the constructor always assigns the real request.
impl Default for SmeltRequest { fn default() -> Self { let headers = SmeltHeaders::new(); let body = SmeltBody::empty(); Self { id: smelt_next_object_id(), url: String::new(), method: "GET".to_owned(), headers, body, init: smelt_default_request_init() } } }

//~ Emit [`URL_FAST_PATH`].
/// Whether `input` is an http(s) URL already in its WHATWG serialization (see `fetch_types_prelude`).
#[allow(dead_code)]
fn smelt_url_is_serialized_http(input: &str) -> bool {
    let (rest, default_port) = if let Some(rest) = input.strip_prefix("http://") { (rest, 80_u32) } else if let Some(rest) = input.strip_prefix("https://") { (rest, 443_u32) } else { return false };
    let Some(path_start) = rest.find('/') else { return false };
    let (authority, path) = rest.split_at(path_start);
    let (host, port) = match authority.split_once(':') { Some((host, port)) => (host, Some(port)), None => (authority, None) };
    if let Some(port) = port {
        if port.is_empty() || port.len() > 5 || port.starts_with('0') || !port.bytes().all(|byte| byte.is_ascii_digit()) { return false; }
        match port.parse::<u32>() { Ok(value) if value <= 65535 && value != default_port => {} _ => return false }
    }
    if host.is_empty() { return false; }
    let labels: Vec<&str> = host.split('.').collect();
    let dotted_quad = labels.len() == 4 && labels.iter().all(|label| !label.is_empty() && label.len() <= 3 && label.bytes().all(|byte| byte.is_ascii_digit()) && (label.len() == 1 || !label.starts_with('0')) && label.parse::<u32>().is_ok_and(|octet| octet <= 255));
    if !dotted_quad {
        if labels.iter().any(|label| label.is_empty() || label.starts_with("xn--") || label.starts_with('-') || label.ends_with('-') || !label.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')) { return false; }
        let last = labels[labels.len() - 1];
        if !last.bytes().any(|byte| byte.is_ascii_lowercase()) || last.starts_with("0x") { return false; }
    }
    let bytes = path.as_bytes();
    if !bytes.iter().all(|byte| (0x21..=0x7e).contains(byte) && !b"\"#'<>\\^`{|}".contains(byte)) { return false; }
    !bytes.windows(2).any(|pair| pair == b"/.") && !bytes.windows(3).any(|triple| triple[0] == b'%' && triple[1] == b'2' && (triple[2] | 0x20) == b'e')
}

//~ Emit the request's constructor and its member operations.
#[allow(dead_code)]
impl SmeltRequest {
    //~ Assemble a request with a fresh JS reference identity.
    //~
    //~ `input` is SERIALIZED, not stored as written: the spec parses it
    //~ into a URL record and `request.url` reads back the serialization,
    //~ so `https://a.test` answers `https://a.test/`. A body that implies
    //~ a `Content-Type` adds it unless the caller set one, exactly as on a
    //~ response — the pairing belongs to the body, so both holders get it
    //~ from the same place.
    pub fn from_parts(input: &(impl SmeltHostText + ?Sized), method: String, headers: SmeltHeaders, body: SmeltBody) -> Self { let input = input.host_text(); let input: &str = &input; Self::assemble(input, method, headers, body, smelt_default_request_init()) }
    //~ The same assembly, carrying the stored `RequestInit` members.
    pub fn from_parts_with_init(input: &(impl SmeltHostText + ?Sized), method: String, headers: SmeltHeaders, body: SmeltBody, init: SmeltRequestInit) -> Self { let input = input.host_text(); let input: &str = &input; Self::assemble(input, method, headers, body, ::std::rc::Rc::new(init)) }
    //~ The assembly both constructors share, over an already-shared `init`.
    fn assemble(input: &str, method: String, headers: SmeltHeaders, body: SmeltBody, init: ::std::rc::Rc<SmeltRequestInit>) -> Self {
        let url = if input == "about:blank" || smelt_url_is_serialized_http(input) { input.to_owned() } else { ::url::Url::parse(input).map_or_else(|_| input.to_owned(), |parsed| parsed.to_string()) };
        if let Some(content_type) = body.content_type() { if !headers.has("content-type") {
            headers.append("content-type", &content_type);
        } }
        Self { id: smelt_next_object_id(), url, method: Self::normalize_method(&method), headers, body, init }
    }
    //~ The spec's method normalization.
    //~
    //~ Exactly the spec's list is upper-cased when a method is given in
    //~ any case: `DELETE GET HEAD OPTIONS POST PUT`. Every other token is
    //~ left as written — `patch` stays `patch`, which is observable and is
    //~ what Node does, so upper-casing everything would be wrong.
    fn normalize_method(method: &str) -> String {
        let upper = method.to_ascii_uppercase();
        if matches!(upper.as_str(), "DELETE" | "GET" | "HEAD" | "OPTIONS" | "POST" | "PUT") {
            upper
        }
        else {
            method.to_owned()
        }
    }
    //~ JS reference identity of this request.
    pub fn id(&self) -> usize { self.id }
    //~ `url`: the serialized request URL.
    pub fn url(&self) -> String { self.url.clone() }
    //~ `method`.
    pub fn method(&self) -> String { self.method.clone() }
    //~ `headers`: the same list, not a copy.
    pub fn headers(&self) -> SmeltHeaders { self.headers.clone() }
    //~ `bodyUsed`.
    pub fn body_used(&self) -> bool { self.body.body_used() }
    //~ The request's body handle.
    pub fn body(&self) -> SmeltBody { self.body.clone() }
    //~ `text()`: the body decoded as UTF-8, consuming it.
    pub fn take_text(&self) -> Result<String, Box<dyn ::std::error::Error>> { self.body.take_text() }
    //~ `clone()`: a request whose body is independently readable.
    //~
    //~ Same distinction as `SmeltResponse::tee`: the spec's `clone()` tees
    //~ the body, while Rust's `Clone` copies the handle and shares it.
    //~
    //~ The url is already serialized, so it is passed through `from_parts`
    //~ unchanged; re-parsing a serialization is a no-op.
    pub fn tee(&self) -> Self { Self::from_parts_with_init(&self.url, self.method.clone(), SmeltHeaders::from_pairs(self.headers.entries_in_insertion_order()), self.body.tee(), (*self.init).clone()) }
    //~ The stored `RequestInit` members, as a copy.
    pub fn init(&self) -> SmeltRequestInit { (*self.init).clone() }
    //~ `cache`: a stored `RequestInit` member.
    pub fn cache(&self) -> String { self.init.cache.clone() }
    //~ `credentials`: a stored `RequestInit` member.
    pub fn credentials(&self) -> String { self.init.credentials.clone() }
    //~ `integrity`: a stored `RequestInit` member.
    pub fn integrity(&self) -> String { self.init.integrity.clone() }
    //~ `keepalive`: a stored `RequestInit` member.
    pub fn keepalive(&self) -> bool { self.init.keepalive }
    //~ `mode`: a stored `RequestInit` member.
    pub fn mode(&self) -> String { self.init.mode.clone() }
    //~ `redirect`: a stored `RequestInit` member.
    pub fn redirect(&self) -> String { self.init.redirect.clone() }
    //~ `referrer`: a stored `RequestInit` member.
    pub fn referrer(&self) -> String { self.init.referrer.clone() }
    //~ `referrerPolicy`: a stored `RequestInit` member.
    pub fn referrer_policy(&self) -> String { self.init.referrer_policy.clone() }
}

// @smelt:item-end

// Emit the `Request` dynamic-boundary adapters.
//
// Same boundary and the same reasoning as `SmeltResponse`'s erasure; the record
// carries a url and a method where a response's carries a status line.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltRequest when request & unknown
/// Erase a request for a dynamic boundary (identity marker + url/method).
//~
//~ Retains the live request under the record's object id, so narrowing the
//~ erased value back hands out the SAME request rather than a rebuilt copy:
//~ `new Request(input)` on an erased input must disturb the ORIGINAL's body
//~ (Node leaves `input.bodyUsed` true at once), which a rebuilt copy cannot
//~ do. Same rule, and the same registry, as `host_value_erasure`'s pair.
impl IntoSmeltUnknown for SmeltRequest {
    fn into_smelt_unknown(self) -> SmeltUnknown {
        //~ Built on first use, as the response's record is.
        let smelt_host_origin = self.clone();
        let smelt_id = self.id;
        smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::lazy_with_id(smelt_id, move || { let body_text = String::from_utf8_lossy(&self.body.peek_bytes()).into_owned(); Vec::from([("__smelt_request".to_owned(), SmeltUnknown::Bool(true)), ("url".to_owned(), SmeltUnknown::String(self.url.into())), ("method".to_owned(), SmeltUnknown::String(self.method.into())), ("headers".to_owned(), self.headers.into_smelt_unknown()), ("body".to_owned(), SmeltUnknown::String(body_text.into()))]) })), smelt_host_origin)
    }
}

/// Recover a request from an erased value.
//~
//~ A record that came from an erasure in this thread hands back the SAME
//~ request, body handle and `bodyUsed` cell included. Only a record that did
//~ not (one rebuilt from JSON, say) is reconstructed from its fields.
impl SmeltFromUnknown for SmeltRequest {
    fn smelt_from_unknown(value: SmeltUnknown) -> Self {
        if let Some(origin) = smelt_restore_host_origin::<Self>(&value) { return origin; }
        let SmeltUnknown::Object(map) = value else { return Self::from_parts("about:blank", "GET".to_owned(), SmeltHeaders::new(), SmeltBody::empty()) };
        let url = match map.get("url") { Some(SmeltUnknown::String(url)) => url.to_string(), _ => "about:blank".to_owned() };
        let method = match map.get("method") { Some(SmeltUnknown::String(method)) => method.to_string(), _ => "GET".to_owned() };
        let headers = map.get("headers").map_or_else(SmeltHeaders::new, SmeltHeaders::smelt_from_unknown);
        let body = match map.get("body") { Some(SmeltUnknown::String(text)) if !text.is_empty() => SmeltBody::from_text(&text.to_string()), _ => SmeltBody::empty() };
        Self::from_parts(&url, method, headers, body)
    }
}

// @smelt:item-end
