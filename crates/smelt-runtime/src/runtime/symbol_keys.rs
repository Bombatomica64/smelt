// Well-known and registered symbols as property keys.

// Emits the runtime symbol-value ⇄ property-key correspondence.
//
// A symbol is a value and a property key at once, and the two spellings have to
// agree: a computed key the frontend folded statically (`{ [KEY]: 1 }`,
// `class C { get [KEY]() {} }`) and a key the generated code derives from an
// erased symbol value (`obj[prop]` where `prop` arrived through a
// `SmeltUnknown` slot) must name the SAME record entry. That is why the
// derivation is owned by `smelt_stdlib::symbol_keys` and rendered here rather
// than invented twice: a well-known symbol is a language constant, a registry
// `Symbol.for('d')` is interned by description, and a unique `Symbol('d')`
// carries the source offset of the binding that created it — each folds to an
// identifier-safe synthetic member name (a symbol key can also name a *class*
// member, which has to be spellable as a Rust identifier), and every other
// spelling keeps the generic `__smelt_symbol:<description>` form so a symbol
// key never collides with the plain string key of its own description.
//
// Three functions come out of it:
//
// * `smelt_symbol_key_escape` / `smelt_symbol_key_unescape` — the reversible
//   identifier encoding (`_<hex>_` for every non-alphanumeric character), so a
//   folded key can be read back as the description it came from;
// * `smelt_symbol_property_key` — the key a symbol value indexes; and
// * `smelt_folded_symbol_key_spelling` — its inverse, which is what
//   `Object.getOwnPropertySymbols`, `Reflect.ownKeys` and erased-Map
//   enumeration hand back to the program as a symbol.
// @smelt:item smelt_symbol_key_escape when unknown
/// An identifier-safe, reversible encoding of a symbol description.
fn smelt_symbol_key_escape(text: &str) -> String { let mut escaped = String::with_capacity(text.len()); for ch in text.chars() { if ch.is_ascii_alphanumeric() { escaped.push(ch); } else { escaped.push('_'); escaped.push_str(&format!("{:x}", u32::from(ch))); escaped.push('_'); } } escaped }

/// Decodes `smelt_symbol_key_escape`; a malformed escape passes through.
fn smelt_symbol_key_unescape(escaped: &str) -> String { let mut decoded = String::with_capacity(escaped.len()); let mut rest = escaped; while let Some(index) = rest.find('_') { decoded.push_str(&rest[..index]); let tail = &rest[index + 1..]; let Some((hex, remainder)) = tail.split_once('_') else { decoded.push('_'); decoded.push_str(tail); return decoded; }; match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) { Some(ch) => decoded.push(ch), None => { decoded.push('_'); decoded.push_str(hex); decoded.push('_'); } } rest = remainder; } decoded.push_str(rest); decoded }

/// The property key a symbol value indexes.
fn smelt_symbol_property_key(description: &str) -> String { match description { "Symbol.iterator" => "__smelt_symbol_iterator".to_owned(), "Symbol.asyncIterator" => "__smelt_symbol_async_iterator".to_owned(), "Symbol.hasInstance" => "__smelt_symbol_has_instance".to_owned(), "Symbol.isConcatSpreadable" => "__smelt_symbol_is_concat_spreadable".to_owned(), "Symbol.match" => "__smelt_symbol_match".to_owned(), "Symbol.matchAll" => "__smelt_symbol_match_all".to_owned(), "Symbol.replace" => "__smelt_symbol_replace".to_owned(), "Symbol.search" => "__smelt_symbol_search".to_owned(), "Symbol.species" => "__smelt_symbol_species".to_owned(), "Symbol.split" => "__smelt_symbol_split".to_owned(), "Symbol.toPrimitive" => "__smelt_symbol_to_primitive".to_owned(), "Symbol.toStringTag" => "__smelt_symbol_to_string_tag".to_owned(), "Symbol.unscopables" => "__smelt_symbol_unscopables".to_owned(), other => { if let Some(registry) = other.strip_prefix("Symbol.for(").and_then(|rest| rest.strip_suffix(')')) { return format!("__smelt_symbol_for_{}", smelt_symbol_key_escape(registry)); } let unique = other.rsplit_once('@').filter(|(_, offset)| !offset.is_empty() && offset.bytes().all(|byte| byte.is_ascii_digit())).and_then(|(head, offset)| head.strip_prefix("Symbol(").and_then(|rest| rest.strip_suffix(')')).map(|unique| (unique, offset))); match unique { Some((unique, offset)) => format!("__smelt_symbol_unique_{}_at_{offset}", smelt_symbol_key_escape(unique)), None => format!("__smelt_symbol:{other}") } } } }

/// The symbol value spelling a folded symbol key came from.
fn smelt_folded_symbol_key_spelling(key: &str) -> Option<String> { if let Some(escaped) = key.strip_prefix("__smelt_symbol_unique_") { let (description, offset) = escaped.rsplit_once("_at_")?; return Some(format!("Symbol({})@{offset}", smelt_symbol_key_unescape(description))); } let escaped = key.strip_prefix("__smelt_symbol_for_")?; Some(format!("Symbol.for({})", smelt_symbol_key_unescape(escaped))) }

//~ A unique symbol is spelled `Symbol(d)@<site>`, a registry one
//~ `Symbol.for(d)`, and every other spelling (`Symbol.iterator`, a key decoded
//~ from an erased object) is its own description. A unique symbol created
//~ without an argument has an empty description, so `Symbol()` and
//~ `Symbol('')` both answer `undefined`.
/// `symbol.description` of the symbol a spelling names.
fn smelt_symbol_description(spelling: &str) -> Option<String> { if let Some(registry) = spelling.strip_prefix("Symbol.for(").and_then(|rest| rest.strip_suffix(')')) { return Some(registry.to_owned()); } let unique = spelling.rsplit_once('@').filter(|(_, site)| { let site = site.split_once('#').map_or(*site, |(site, _)| site); !site.is_empty() && site.bytes().all(|byte| byte.is_ascii_digit()) }).and_then(|(head, _)| head.strip_prefix("Symbol(").and_then(|rest| rest.strip_suffix(')'))); match unique { Some(unique) => (!unique.is_empty()).then(|| unique.to_owned()), None => Some(spelling.to_owned()) } }

/// `String(symbol)`: `Symbol(<description>)`, the text JavaScript prints.
fn smelt_symbol_to_string(spelling: &str) -> String { format!("Symbol({})", smelt_symbol_description(spelling).unwrap_or_default()) }

//~ `Object.getOwnPropertySymbols` reports the symbols a program itself put on
//~ an object: the opaque form and the two folded forms. A well-known key is
//~ deliberately NOT one of them here — Smelt also writes
//~ `__smelt_symbol_iterator` onto erased iterables as part of their
//~ representation, and reporting that as a source symbol would invent a
//~ property the source never wrote. `smelt_key_symbol_value` is the wider
//~ view, for `Reflect.ownKeys`, where the well-known table already applied.
/// The symbol value a program-written symbol key denotes.
fn smelt_own_symbol_key_value(key: &str) -> Option<SmeltUnknown> { if let Some(description) = key.strip_prefix("__smelt_symbol:") { return Some(SmeltUnknown::Symbol(description.into())); } smelt_folded_symbol_key_spelling(key).map(|spelling| SmeltUnknown::Symbol(spelling.into())) }

// @smelt:item-end
