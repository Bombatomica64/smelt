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

use crate::rust::CodeWriter;

/// Emit the `SmeltHeaders` runtime type.
///
/// `needs_unknown` gates the erasure adapters (`IntoSmeltUnknown` /
/// `SmeltFromUnknown`): a program that never crosses the dynamic boundary does
/// not emit the carrier type, so the impls must not be emitted either.
pub fn emit(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_struct(writer);
    emit_inherent_impl(writer);
    emit_traits(writer, needs_unknown);
}

/// Emit the `SmeltUrlSearchParams` runtime type.
///
/// Gated exactly like [`emit`], and separately from it: a program that uses a
/// query string but no headers carries only this type.
pub fn emit_url_search_params(writer: &mut CodeWriter, needs_unknown: bool) {
    emit_params_struct(writer);
    emit_params_inherent_impl(writer);
    emit_params_traits(writer, needs_unknown);
}

/// Emit the `SmeltUrlSearchParams` struct and its comparisons.
fn emit_params_struct(writer: &mut CodeWriter) {
    writer.line("/// A WHATWG `URLSearchParams` list: ordered, case-sensitive");
    writer.line("/// name/value pairs with urlencoded serialization.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltUrlSearchParams", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// Name/value pairs in insertion order.");
        struct_writer.line("entries: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, String)>>>,");
    });
    writer.blank_line();
    // Structural equality over the ordered pairs: two query strings are the
    // same parameters when they carry the same pairs in the same order, which
    // is what the spec's serialization observes.
    writer.line(
        "impl PartialEq for SmeltUrlSearchParams { fn eq(&self, other: &Self) -> bool { *self.entries.borrow() == *other.entries.borrow() } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltUrlSearchParams { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&self.to_text()) } }",
    );
    writer.line(
        "impl Default for SmeltUrlSearchParams { fn default() -> Self { Self::new() } }",
    );
    writer.blank_line();
}

/// Emit the WHATWG `URLSearchParams` operations as inherent methods.
fn emit_params_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltUrlSearchParams", |impl_writer| {
        impl_writer.line("/// An empty parameter list with a fresh JS reference identity.");
        impl_writer.line(
            "pub fn new() -> Self { Self { id: smelt_next_object_id(), entries: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }",
        );
        impl_writer.line("/// JS reference identity of this parameter list.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// Build a parameter list from name/value pairs, in order.");
        impl_writer.line(
            "pub fn from_pairs(pairs: Vec<(String, String)>) -> Self { let params = Self::new(); params.entries.borrow_mut().extend(pairs); params }",
        );
        impl_writer.line("/// Parse a query string (with or without a leading `?`).");
        impl_writer.line("///");
        impl_writer.line("/// `url::form_urlencoded` owns the decoding: `+` is a space, `%XX` is");
        impl_writer.line("/// a byte, and a pair with no `=` has the empty string as its value.");
        impl_writer.block("pub fn from_query(query: &str) -> Self", |fn_writer| {
            fn_writer.line("let trimmed = query.strip_prefix('?').unwrap_or(query);");
            fn_writer.line(
                "let pairs = url::form_urlencoded::parse(trimmed.as_bytes()).map(|(name, value): (::std::borrow::Cow<'_, str>, ::std::borrow::Cow<'_, str>)| (name.into_owned(), value.into_owned())).collect::<Vec<(String, String)>>();",
            );
            fn_writer.line("Self::from_pairs(pairs)");
        });
        impl_writer.line("/// `get(name)`: the FIRST value for a name, or `null`.");
        impl_writer.line(
            "pub fn get(&self, name: &str) -> Option<String> { self.entries.borrow().iter().find(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()) }",
        );
        impl_writer.line("/// `getAll(name)`: every value for a name, in order.");
        impl_writer.line(
            "pub fn get_all(&self, name: &str) -> Vec<String> { self.entries.borrow().iter().filter(|(entry_name, _)| entry_name == name).map(|(_, value)| value.clone()).collect() }",
        );
        impl_writer.line("/// `has(name)`.");
        impl_writer.line(
            "pub fn has(&self, name: &str) -> bool { self.entries.borrow().iter().any(|(entry_name, _)| entry_name == name) }",
        );
        impl_writer.line("/// `append(name, value)`.");
        impl_writer.line(
            "pub fn append(&self, name: &str, value: &str) { self.entries.borrow_mut().push((name.to_owned(), value.to_owned())); }",
        );
        impl_writer.line("/// `set(name, value)`: replace the first value, drop the rest.");
        impl_writer.block("pub fn set(&self, name: &str, value: &str)", |fn_writer| {
            fn_writer.line("let mut entries = self.entries.borrow_mut();");
            fn_writer.line(
                "let position = entries.iter().position(|(entry_name, _)| entry_name == name);",
            );
            fn_writer.line(
                "let Some(index) = position else { entries.push((name.to_owned(), value.to_owned())); return; };",
            );
            fn_writer.line("entries[index] = (name.to_owned(), value.to_owned());");
            fn_writer.line("let mut kept = false;");
            fn_writer.line(
                "entries.retain(|(entry_name, _)| { if entry_name != name { return true; } let first = !kept; kept = true; first });",
            );
        });
        impl_writer.line("/// `delete(name)`: remove every pair with the name.");
        impl_writer.line(
            "pub fn delete(&self, name: &str) { self.entries.borrow_mut().retain(|(entry_name, _)| entry_name != name); }",
        );
        impl_writer.line("/// `sort()`: stable sort by name, keeping equal names in order.");
        impl_writer.line(
            "pub fn sort(&self) { self.entries.borrow_mut().sort_by(|left, right| left.0.cmp(&right.0)); }",
        );
        impl_writer.line("/// `toString()`: the urlencoded serialization.");
        impl_writer.block("pub fn to_text(&self) -> String", |fn_writer| {
            fn_writer.line("let mut serializer = url::form_urlencoded::Serializer::new(String::new());");
            fn_writer.line(
                "for (name, value) in self.entries.borrow().iter() { serializer.append_pair(name, value); }",
            );
            fn_writer.line("serializer.finish()");
        });
        impl_writer.line("/// `entries()`: the pairs in insertion order.");
        impl_writer.line(
            "pub fn entries_in_order(&self) -> Vec<(String, String)> { self.entries.borrow().clone() }",
        );
        impl_writer.line("/// `keys()`: parameter names in insertion order.");
        impl_writer.line(
            "pub fn keys(&self) -> Vec<String> { self.entries.borrow().iter().map(|(name, _)| name.clone()).collect() }",
        );
        impl_writer.line("/// `values()`: parameter values in insertion order.");
        impl_writer.line(
            "pub fn values(&self) -> Vec<String> { self.entries.borrow().iter().map(|(_, value)| value.clone()).collect() }",
        );
        impl_writer.line("/// `size`: the number of pairs.");
        impl_writer.line("pub fn size(&self) -> f64 { self.entries.borrow().len() as f64 }");
    });
    writer.blank_line();
}

/// Emit the `URLSearchParams` dynamic-boundary adapters.
///
/// Same boundary shape as `SmeltHeaders`: an erased value is the marker record
/// `{ "__smelt_urlsearchparams": true, "entries": [[name, value], ..] }`. A
/// DYNAMIC BOUNDARY adapter only — the internal representation stays concrete.
fn emit_params_traits(writer: &mut CodeWriter, needs_unknown: bool) {
    if !needs_unknown {
        return;
    }
    writer.line("/// Erase a parameter list for a dynamic boundary.");
    writer.block(
        "impl IntoSmeltUnknown for SmeltUrlSearchParams",
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line(
                    "let pairs: Vec<SmeltUnknown> = self.entries_in_order().into_iter().map(|(name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(name.into()), SmeltUnknown::String(value.into())]).into())).collect();",
                );
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"__smelt_urlsearchparams\".to_owned(), SmeltUnknown::Bool(true)), (\"entries\".to_owned(), SmeltUnknown::Array(pairs.into()))])))",
                );
            });
        },
    );
    writer.blank_line();
    writer.line("/// Rebuild a parameter list from an erased value.");
    writer.block(
        "impl SmeltFromUnknown for SmeltUrlSearchParams",
        |impl_writer| {
            impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
                fn_writer.line(
                    "if let SmeltUnknown::String(query) = &value { return Self::from_query(query); }",
                );
                fn_writer.line("let SmeltUnknown::Object(map) = value else { return Self::new() };");
                fn_writer.line(
                    "let Some(SmeltUnknown::Array(pairs)) = map.get(\"entries\") else { return Self::new() };",
                );
                fn_writer.line("let params = Self::new();");
                fn_writer.block("for pair in pairs.into_vec()", |loop_writer| {
                    loop_writer.line("let SmeltUnknown::Array(pair) = pair else { continue };");
                    loop_writer.line("let pair = pair.into_vec();");
                    loop_writer.line(
                        "let (Some(SmeltUnknown::String(name)), Some(SmeltUnknown::String(entry_value))) = (pair.first().cloned(), pair.get(1).cloned()) else { continue };",
                    );
                    loop_writer.line("params.append(&name, &entry_value);");
                });
                fn_writer.line("params");
            });
        },
    );
    writer.blank_line();
}

/// Emit the struct definition and its identity-based equality.
fn emit_struct(writer: &mut CodeWriter) {
    writer.line("/// A WHATWG `Headers` list: ordered name/value pairs, case-insensitive");
    writer.line("/// names, comma-joined reads, and the `Set-Cookie` carve-out.");
    writer.line("#[derive(Clone)]");
    writer.block("pub struct SmeltHeaders", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// Lower-cased name and normalized value, in insertion order.");
        struct_writer.line("entries: ::std::rc::Rc<::std::cell::RefCell<Vec<(String, String)>>>,");
    });
    writer.blank_line();
    // Structural equality, not identity: `expect(a).toEqual(b)` on two header
    // lists with the same pairs must hold, and the sorted entry projection is
    // the spec's own notion of "the same headers".
    writer.line(
        "impl PartialEq for SmeltHeaders { fn eq(&self, other: &Self) -> bool { self.entries_sorted() == other.entries_sorted() } }",
    );
    writer.line(
        "impl ::std::fmt::Debug for SmeltHeaders { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_map().entries(self.entries_sorted()).finish() } }",
    );
    writer.line(
        "impl Default for SmeltHeaders { fn default() -> Self { Self::new() } }",
    );
    writer.blank_line();
}

/// Emit the WHATWG operations as inherent methods.
fn emit_inherent_impl(writer: &mut CodeWriter) {
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltHeaders", |impl_writer| {
        impl_writer.line("/// An empty header list with a fresh JS reference identity.");
        impl_writer.line(
            "pub fn new() -> Self { Self { id: smelt_next_object_id(), entries: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())) } }",
        );
        impl_writer.line("/// JS reference identity of this header list.");
        impl_writer.line("pub fn id(&self) -> usize { self.id }");
        impl_writer.line("/// Build a header list from name/value pairs, appending in order.");
        impl_writer.line(
            "pub fn from_pairs(pairs: Vec<(String, String)>) -> Self { let headers = Self::new(); for (name, value) in pairs { headers.append(&name, &value); } headers }",
        );
        impl_writer.line("/// The spec's header-name normalization: lower-cased.");
        impl_writer
            .line("fn normalize_name(name: &str) -> String { name.trim().to_ascii_lowercase() }");
        impl_writer.line("/// The spec's header-value normalization: strip HTTP whitespace.");
        impl_writer.line(
            "fn normalize_value(value: &str) -> String { value.trim_matches(|ch| ch == ' ' || ch == '\\t' || ch == '\\r' || ch == '\\n').to_owned() }",
        );
        impl_writer.line("/// `get(name)`: every value for the name, joined with `\", \"`.");
        impl_writer.line("///");
        impl_writer.line("/// `None` is the source `null`: the name is not in the list. The");
        impl_writer.line("/// return type is the source type, so no caller has to re-narrow it.");
        impl_writer.block(
            "pub fn get(&self, name: &str) -> Option<String>",
            |fn_writer| {
                fn_writer.line("let key = Self::normalize_name(name);");
                fn_writer.line(
                    "let values: Vec<String> = self.entries.borrow().iter().filter(|(entry_name, _)| *entry_name == key).map(|(_, value)| value.clone()).collect();",
                );
                fn_writer.line("if values.is_empty() { None } else { Some(values.join(\", \")) }");
            },
        );
        impl_writer.line("/// `has(name)`.");
        impl_writer.line(
            "pub fn has(&self, name: &str) -> bool { let key = Self::normalize_name(name); self.entries.borrow().iter().any(|(entry_name, _)| *entry_name == key) }",
        );
        impl_writer.line("/// `append(name, value)`: add a pair, keeping existing ones.");
        impl_writer.line(
            "pub fn append(&self, name: &str, value: &str) { self.entries.borrow_mut().push((Self::normalize_name(name), Self::normalize_value(value))); }",
        );
        impl_writer.line("/// `set(name, value)`: replace every value for the name.");
        impl_writer.line("///");
        impl_writer.line("/// The first existing pair's position is kept, matching the spec's");
        impl_writer.line("/// \"set the value of the first such header and remove the others\".");
        impl_writer.block("pub fn set(&self, name: &str, value: &str)", |fn_writer| {
            fn_writer.line("let key = Self::normalize_name(name);");
            fn_writer.line("let normalized = Self::normalize_value(value);");
            fn_writer.line("let mut entries = self.entries.borrow_mut();");
            fn_writer.line(
                "let position = entries.iter().position(|(entry_name, _)| *entry_name == key);",
            );
            fn_writer.line("let Some(index) = position else { entries.push((key, normalized)); return; };");
            fn_writer.line("entries[index] = (key.clone(), normalized);");
            fn_writer.line("// Keep the first pair with this name (the one just written) and");
            fn_writer.line("// drop the rest, as the spec's `set` does.");
            fn_writer.line("let mut kept = false;");
            fn_writer.line("entries.retain(|(entry_name, _)| { if *entry_name != key { return true; } let first = !kept; kept = true; first });");
        });
        impl_writer.line("/// `delete(name)`: remove every pair with the name.");
        impl_writer.line(
            "pub fn delete(&self, name: &str) { let key = Self::normalize_name(name); self.entries.borrow_mut().retain(|(entry_name, _)| *entry_name != key); }",
        );
        impl_writer.line("/// The spec's iteration order: sorted by name, values combined.");
        impl_writer.line("///");
        impl_writer.line("/// `set-cookie` is the exception the spec carves out: its values are");
        impl_writer.line("/// never combined, so each cookie stays its own entry.");
        impl_writer.block(
            "pub fn entries_sorted(&self) -> Vec<(String, String)>",
            |fn_writer| {
                fn_writer.line("let entries = self.entries.borrow().clone();");
                fn_writer.line("let mut names: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();");
                fn_writer.line("names.sort();");
                fn_writer.line("names.dedup();");
                fn_writer.line("let mut combined = Vec::new();");
                fn_writer.block("for name in names", |loop_writer| {
                    loop_writer.line(
                        "let values: Vec<String> = entries.iter().filter(|(entry_name, _)| *entry_name == name).map(|(_, value)| value.clone()).collect();",
                    );
                    loop_writer.block("if name == \"set-cookie\"", |arm_writer| {
                        arm_writer.line("for value in values { combined.push((name.clone(), value)); }");
                    });
                    loop_writer.block("else", |arm_writer| {
                        arm_writer.line("combined.push((name.clone(), values.join(\", \")));");
                    });
                });
                fn_writer.line("combined");
            },
        );
        impl_writer.line("/// `keys()`: header names in iteration order.");
        impl_writer.line(
            "pub fn keys(&self) -> Vec<String> { self.entries_sorted().into_iter().map(|(name, _)| name).collect() }",
        );
        impl_writer.line("/// `values()`: header values in iteration order.");
        impl_writer.line(
            "pub fn values(&self) -> Vec<String> { self.entries_sorted().into_iter().map(|(_, value)| value).collect() }",
        );
        impl_writer.line("/// `getSetCookie()`: each `Set-Cookie` value, uncombined.");
        impl_writer.line(
            "pub fn get_set_cookie(&self) -> Vec<String> { self.entries.borrow().iter().filter(|(name, _)| name == \"set-cookie\").map(|(_, value)| value.clone()).collect() }",
        );
    });
    writer.blank_line();
}

/// Emit the dynamic-boundary adapters.
///
/// A `Headers` value that reaches an erased position (`console.log(headers)`,
/// an `unknown`-typed sink, a JSON boundary) has to become a tagged value, and
/// coming back it has to be rebuilt. Both directions go through the record
/// shape `{ "__smelt_headers": true, "entries": [[name, value], ..] }`: a
/// DYNAMIC BOUNDARY adapter, not the internal representation, which stays the
/// concrete struct above.
fn emit_traits(writer: &mut CodeWriter, needs_unknown: bool) {
    if !needs_unknown {
        return;
    }
    writer.line("/// Erase a header list for a dynamic boundary (identity marker + pairs).");
    writer.block("impl IntoSmeltUnknown for SmeltHeaders", |impl_writer| {
        impl_writer.block(
            "fn into_smelt_unknown(self) -> SmeltUnknown",
            |fn_writer| {
                fn_writer.line(
                    "let pairs: Vec<SmeltUnknown> = self.entries_sorted().into_iter().map(|(name, value)| SmeltUnknown::Array(Vec::from([SmeltUnknown::String(name.into()), SmeltUnknown::String(value.into())]).into())).collect();",
                );
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([(\"__smelt_headers\".to_owned(), SmeltUnknown::Bool(true)), (\"entries\".to_owned(), SmeltUnknown::Array(pairs.into()))])))",
                );
            },
        );
    });
    writer.blank_line();
    writer.line("/// Rebuild a header list from an erased value.");
    writer.block("impl SmeltFromUnknown for SmeltHeaders", |impl_writer| {
        impl_writer.block(
            "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
            |fn_writer| {
                fn_writer.line("let SmeltUnknown::Object(map) = value else { return Self::new() };");
                fn_writer.line(
                    "let Some(SmeltUnknown::Array(pairs)) = map.get(\"entries\") else { return Self::new() };",
                );
                fn_writer.line("let headers = Self::new();");
                fn_writer.block("for pair in pairs.into_vec()", |loop_writer| {
                    loop_writer.line(
                        "let SmeltUnknown::Array(pair) = pair else { continue };",
                    );
                    loop_writer.line("let pair = pair.into_vec();");
                    loop_writer.line(
                        "let (Some(SmeltUnknown::String(name)), Some(SmeltUnknown::String(entry_value))) = (pair.first().cloned(), pair.get(1).cloned()) else { continue };",
                    );
                    loop_writer.line("headers.append(&name, &entry_value);");
                });
                fn_writer.line("headers");
            },
        );
    });
    writer.blank_line();
}
