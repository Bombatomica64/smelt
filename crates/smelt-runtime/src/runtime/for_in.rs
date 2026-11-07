// `for...in`, `arguments` and own-key enumeration.

// The `arguments` exotic object. Its indexed elements are enumerable own
// properties but its `length` is not, which is what makes
// `isEqual(toArgs([1, 2, 3]), { 0: 1, 1: 2, 2: 3 })` hold: both sides
// enumerate exactly `["0", "1", "2"]`. The enumeration filters below carry
// the `length` exception, in the same shape as the `__smelt_regexp` and
// `__smelt_error` field exceptions.
// @smelt:item smelt_arguments_object when unknown
/// Build the array-like `arguments` object from a function's parameters.
///
/// Positional parameters lead; the rest parameter's list is flattened onto
/// the end, recovering the original call's argument vector. Elements are
/// stored under index keys and `length` is stored but hidden from own-key
/// enumeration, matching the exotic object's property attributes.
fn smelt_arguments_object(fixed: Vec<SmeltUnknown>, rest: Option<SmeltUnknown>) -> SmeltUnknown { let mut smelt_elements = fixed; if let Some(SmeltUnknown::Array(items)) = rest { smelt_elements.extend(items.into_vec()); } let mut fields = Vec::from([("__smelt_arguments".to_owned(), SmeltUnknown::Bool(true))]); for (index, value) in smelt_elements.iter().enumerate() { fields.push((index.to_string(), value.clone())); } fields.push(("length".to_owned(), SmeltUnknown::Number(smelt_elements.len() as f64))); SmeltUnknown::Object(SmeltObject::new(fields)) }
//~ An `arguments` object is iterable in JavaScript (its `Symbol.iterator`
//~ is `Array.prototype.values`), but the marker record above stores no
//~ `__smelt_symbol_iterator` slot, so the erased iterable-to-list coercion
//~ could not walk it: `Array.from(arguments)` and `[...arguments]` both
//~ panicked with "unknown is not iterable". This helper is the iteration
//~ door. It reads `length` and the index keys rather than the record's raw
//~ key order, so a caller that assigned an extra named property to the
//~ record cannot perturb the element sequence.
/// Extract an `arguments` object's elements, or `None` for any other value.
fn smelt_arguments_elements(object: &SmeltObject) -> Option<Vec<SmeltUnknown>> { if !object.contains_key("__smelt_arguments") { return None; } let length = match object.get("length") { Some(SmeltUnknown::Number(length)) if length >= 0.0 => length as usize, _ => 0 }; Some((0..length).map(|index| object.get(&index.to_string()).unwrap_or(SmeltUnknown::Undefined)).collect()) }
//~ `__smelt_proto:`-prefixed entries hold members INHERITED from a
//~ prototype (`Object.create(proto)`), so they are never own keys — JS
//~ `Object.keys` / `for...in` own-key enumeration must skip them.
//~
//~ A symbol-keyed property is skipped for a stronger reason: in
//~ JavaScript a symbol key NEVER appears in string-key enumeration
//~ (`Object.keys`, `Object.values`, `Object.entries`, `for...in`,
//~ `JSON.stringify`), only in `Object.getOwnPropertySymbols` and
//~ `Reflect.ownKeys`. Every storage spelling of a symbol key shares the
//~ `__smelt_symbol` stem (`smelt_stdlib::symbol_keys::SYMBOL_KEY_STEM`) —
//~ the opaque `__smelt_symbol:<description>` form, the folded registry and
//~ unique member names, and the well-known keys — so one prefix test
//~ covers all of them.
//~ The object-dependent half of the predicate (host marker, the
//~ regexp/error/arguments markers) is a property of the OBJECT, not of
//~ the key, so a walk over every key reads it once
//~ (`smelt_for_in_object_shape`) instead of re-probing ~30 marker keys per
//~ key: the serializer and the own-key walks below do exactly that. The
//~ per-key form keeps its signature for single-key callers, and both read
//~ the one key predicate, so they cannot drift apart.
/// The parts of an object's shape own-key enumeration depends on, read once per object.
#[derive(Clone, Copy)]
struct SmeltForInShape { host: bool, regexp: bool, error: bool, arguments: bool }
/// Read an object's `for...in`-relevant markers once.
fn smelt_for_in_object_shape(object: &SmeltObject) -> SmeltForInShape { if !object.store.borrow().has_internal_keys() { return SmeltForInShape { host: false, regexp: false, error: false, arguments: false }; } SmeltForInShape { host: smelt_object_has_host_marker(object), regexp: object.contains_key("__smelt_regexp"), error: object.contains_key("__smelt_error"), arguments: object.contains_key("__smelt_arguments") } }
/// Return whether `key` is visible to `for...in` on an object of this shape.
fn smelt_is_for_in_shape_key(shape: SmeltForInShape, key: &JsStr) -> bool { if !shape.host && !shape.regexp && !shape.error && !shape.arguments && !key.eq_str_prefix("__smelt") { return true; } smelt_is_for_in_shape_key_text(shape, &key.to_string_lossy()) }
/// [`smelt_is_for_in_shape_key`] over a key's UTF-8 text.
fn smelt_is_for_in_shape_key_text(shape: SmeltForInShape, key: &str) -> bool { !shape.host && !key.starts_with("__smelt_symbol") && !key.starts_with("__smelt_proto:") && !key.starts_with("__smelt_method:") && key != "__smelt_date" && key != "__smelt_timezone" && key != "__smelt_class" && key != "__smelt_map" && key != "__smelt_set" && !(shape.regexp && matches!(key, "__smelt_regexp" | "source" | "flags" | "lastIndex")) && !(shape.error && matches!(key, "__smelt_error" | "message" | "cause" | "errors" | "stack")) && !(shape.arguments && matches!(key, "__smelt_arguments" | "length")) }
/// Return whether an object key is visible to JavaScript `for...in` iteration.
fn smelt_is_for_in_object_key(object: &SmeltObject, key: &str) -> bool { smelt_is_for_in_shape_key_text(smelt_for_in_object_shape(object), key) }
/// Return whether a record key is visible to JavaScript `for...in` iteration.
fn smelt_is_for_in_record_key<V>(record: &SmeltRecord<JsString, V>, key: &JsStr) -> bool { if smelt_record_has_host_marker(record) { return false; } if !key.eq_str_prefix("__smelt") && !record.store.borrow().has_internal_keys() { return true; } let key = &*key.to_string_lossy(); !key.starts_with("__smelt_symbol") && !key.starts_with("__smelt_proto:") && !key.starts_with("__smelt_method:") && key != "__smelt_date" && key != "__smelt_timezone" && key != "__smelt_class" && !(record.has_text_key("__smelt_regexp") && matches!(key, "__smelt_regexp" | "source" | "flags" | "lastIndex")) && !(record.has_text_key("__smelt_error") && matches!(key, "__smelt_error" | "message" | "cause" | "errors" | "stack")) && !(record.has_text_key("__smelt_arguments") && matches!(key, "__smelt_arguments" | "length")) }
//~ `for...in` walks the PROTOTYPE CHAIN; `Object.keys` does not. The two
//~ therefore cannot share one key list. Inherited members live behind the
//~ `__smelt_proto:` prefix, which the own-key filters above exclude — right
//~ for `Object.keys`, wrong for `for...in`. remeda's `isEmptyish` reads
//~ exactly that difference: it uses `for (const _ in data) return false`, so
//~ `Object.create(Object.create({ a: 123 }))` must report a key even though
//~ it has no OWN key.
//~
//~ Own keys come first and shadow an inherited member of the same name,
//~ matching JavaScript's enumeration order and its shadowing rule.
//~
//~ Every `__smelt_proto:` entry here is an inherited member: the prototype
//~ itself is recorded by reference in `SMELT_OBJECT_PROTOTYPES`, keyed by
//~ object identity, so no hidden entry has to be filtered out of this walk.
/// Every key JavaScript `for...in` yields for an erased object, prototype chain included.
fn smelt_for_in_object_keys(map: &SmeltObject) -> Vec<JsString> { let mut keys = Vec::new(); let mut seen = ::std::collections::HashSet::new(); let shape = smelt_for_in_object_shape(map); for key in map.keys() { if key.eq_str_prefix("__smelt_proto:") { continue; } if smelt_is_for_in_shape_key(shape, &key) && seen.insert(key.clone()) { keys.push(key); } } for key in map.keys() { if let Some(inherited) = key.strip_str_prefix("__smelt_proto:") { let inherited = inherited.to_owned(); if seen.insert(inherited.clone()) { keys.push(inherited); } } } keys }

/// Every key JavaScript `for...in` yields for a typed record, prototype chain included.
//~ A record's own keys are already unique (its field store replaces on
//~ insert), so only an inherited `__smelt_proto:` key can repeat one;
//~ those are rare, so they are checked against the list instead of
//~ hashing every key of every record walked (this runs per response in
//~ Hono's `#newResponse`; `blocker-logs/hono-crud-perf-round2.md`).
fn smelt_for_in_record_keys<V>(record: &SmeltRecord<JsString, V>) -> Vec<JsString> { let mut keys = Vec::new(); let mut has_inherited = false; for key in record.keys() { if key.eq_str_prefix("__smelt_proto:") { has_inherited = true; continue; } if smelt_is_for_in_record_key(record, &key) { keys.push(key); } } if has_inherited { for key in record.keys() { if let Some(inherited) = key.strip_str_prefix("__smelt_proto:") { if !keys.iter().any(|existing| **existing == *inherited) { keys.push(inherited.to_owned()); } } } } keys }

//~ Own-property enumeration over a `SmeltJsMap` backing.
//~
//~ Smelt's marker convention (`__smelt_proto:` for an `Object.create`
//~ prototype member, `__smelt_method:` for a class's prototype methods,
//~ `__smelt_class` for provenance, `__smelt_symbol:` for a symbol key)
//~ is a property of the *representation*, not of the source object, so
//~ every own-key view has to honour it. `SmeltRecord` did, through
//~ `smelt_is_for_in_record_key`; the `SmeltUnknown`-keyed `SmeltJsMap`
//~ backing did not, even though the erased-object -> `SmeltJsMap`
//~ coercion copies those keys in verbatim. `Object.keys` of an
//~ `Object.create({a: 1})` therefore reported the inherited `a`.
//~
//~ This is the `SmeltJsMap` twin of that filter: it drops the markers
//~ and turns a stored `__smelt_symbol:x` key back into the
//~ `SmeltUnknown::Symbol` value it denotes, so the callers' own
//~ string-versus-symbol split (`Object.keys` excludes symbol keys,
//~ `Object.getOwnPropertySymbols` keeps only those) works on real tags.
/// Own entries of a `SmeltJsMap` backing, with representation markers removed.
///
/// Drops `__smelt_proto:` / `__smelt_method:` / `__smelt_class` keys (inherited
/// members, prototype methods and class provenance are not own properties) and
/// restores any symbol key to its `SmeltUnknown::Symbol` tag.
fn smelt_own_js_map_entries<V: Clone>(map: &SmeltJsMap<SmeltUnknown, V>) -> Vec<(SmeltUnknown, V)> { map.iter().filter_map(|(key, value)| { let SmeltUnknown::String(text) = &key else { return Some((key, value)); }; let text = text.to_string(); if text.starts_with("__smelt_proto:") || text.starts_with("__smelt_method:") || text == "__smelt_class" { return None; } if let Some(symbol) = smelt_own_symbol_key_value(&text) { return Some((symbol, value)); } Some((key, value)) }).collect() }

/// Every key JavaScript `for...in` yields for a `SmeltJsMap` backing.
///
/// Own string keys first, then the enumerable `__smelt_proto:` members with
/// their prefix stripped -- the same order and the same prototype-chain rule as
/// `smelt_for_in_record_keys`. Symbol keys are never enumerated by `for...in`.
fn smelt_for_in_js_map_keys<V: Clone>(map: &SmeltJsMap<SmeltUnknown, V>) -> Vec<SmeltUnknown> { let mut keys = Vec::new(); let mut seen = ::std::collections::HashSet::new(); for (key, _) in smelt_own_js_map_entries(map) { if matches!(key, SmeltUnknown::Symbol(_)) { continue; } if let SmeltUnknown::String(text) = &key { if !seen.insert(text.to_string()) { continue; } } keys.push(key); } for key in map.keys() { let SmeltUnknown::String(text) = &key else { continue; }; if let Some(inherited) = text.to_string_lossy().strip_prefix("__smelt_proto:") { let inherited = inherited.to_owned(); if seen.insert(inherited.clone()) { keys.push(SmeltUnknown::String(inherited.as_str().into())); } } } keys }

//~ Emits the `Reflect.ownKeys` projection and the storage-key → symbol inverse.
//~
//~ `Reflect.ownKeys(o)` answers *every* own key: the string keys in JavaScript's
//~ own-key order, then the symbol keys. `Object.keys` answers only the string
//~ half, which is why the two need separate projections rather than one.
//~
//~ DYNAMIC BOUNDARY (`CLAUDE.md`): the element type is `SmeltUnknown` because
//~ `Reflect.ownKeys` is declared `(target: object) => (string | symbol)[]` in
//~ TypeScript's own lib. The list genuinely mixes two disjoint runtime kinds and
//~ consumers discriminate them at run time (`typeof key === 'string'`), so no
//~ concrete element type, generated union arm, or scoped generic can stand in:
//~ choosing `String` is what made the caller's `typeof` guard const-fold to
//~ `false` and lose the symbol keys entirely. The regression test
//~ `stdlib_boundary_tests::reflect_own_keys_keeps_symbol_keys_and_a_dynamic_key_type`
//~ pins that reasoning.
/// The symbol value a stored property key denotes, if it is a symbol key.
fn smelt_key_symbol_value(key: &JsStr) -> Option<SmeltUnknown> { if !key.eq_str_prefix("__smelt") { return None; } smelt_key_symbol_value_text(&key.to_string_lossy()) }
/// [`smelt_key_symbol_value`] over a key's UTF-8 text.
fn smelt_key_symbol_value_text(key: &str) -> Option<SmeltUnknown> { if let Some(description) = key.strip_prefix("__smelt_symbol:") { return Some(SmeltUnknown::Symbol(description.into())); } if let Some(spelling) = smelt_folded_symbol_key_spelling(key) { return Some(SmeltUnknown::Symbol(spelling.into())); } match key { "__smelt_symbol_iterator" => Some(SmeltUnknown::Symbol("Symbol.iterator".into())), "__smelt_symbol_async_iterator" => Some(SmeltUnknown::Symbol("Symbol.asyncIterator".into())), "__smelt_symbol_has_instance" => Some(SmeltUnknown::Symbol("Symbol.hasInstance".into())), "__smelt_symbol_is_concat_spreadable" => Some(SmeltUnknown::Symbol("Symbol.isConcatSpreadable".into())), "__smelt_symbol_match" => Some(SmeltUnknown::Symbol("Symbol.match".into())), "__smelt_symbol_match_all" => Some(SmeltUnknown::Symbol("Symbol.matchAll".into())), "__smelt_symbol_replace" => Some(SmeltUnknown::Symbol("Symbol.replace".into())), "__smelt_symbol_search" => Some(SmeltUnknown::Symbol("Symbol.search".into())), "__smelt_symbol_species" => Some(SmeltUnknown::Symbol("Symbol.species".into())), "__smelt_symbol_split" => Some(SmeltUnknown::Symbol("Symbol.split".into())), "__smelt_symbol_to_primitive" => Some(SmeltUnknown::Symbol("Symbol.toPrimitive".into())), "__smelt_symbol_to_string_tag" => Some(SmeltUnknown::Symbol("Symbol.toStringTag".into())), "__smelt_symbol_unscopables" => Some(SmeltUnknown::Symbol("Symbol.unscopables".into())), _ => None } }

/// `Reflect.ownKeys` over a string-keyed record: string keys, then symbol keys.
///
/// DYNAMIC BOUNDARY: a key is `string | symbol` in the source type of
/// `Reflect.ownKeys`, and callers discriminate with `typeof`.
fn smelt_own_keys<V>(record: &SmeltRecord<JsString, V>) -> Vec<SmeltUnknown> { let mut strings = Vec::new(); let mut symbols = Vec::new(); for key in record.keys() { if let Some(symbol) = smelt_key_symbol_value(&key) { symbols.push(symbol); } else if smelt_is_for_in_record_key(record, &key) && !key.eq_str_prefix("__smelt_proto:") { strings.push(SmeltUnknown::String(key.clone())); } } strings.extend(symbols); strings }

/// `Reflect.ownKeys` over an erased object: string keys, then symbol keys.
fn smelt_own_object_keys(map: &SmeltObject) -> Vec<SmeltUnknown> { let mut strings = Vec::new(); let mut symbols = Vec::new(); let shape = smelt_for_in_object_shape(map); for key in map.keys() { if let Some(symbol) = smelt_key_symbol_value(&key) { symbols.push(symbol); } else if smelt_is_for_in_shape_key(shape, &key) && !key.eq_str_prefix("__smelt_proto:") { strings.push(SmeltUnknown::String(key.clone())); } } strings.extend(symbols); strings }

/// `Reflect.ownKeys` over a `SmeltJsMap` backing: string keys, then symbol keys.
fn smelt_own_js_map_keys<V: Clone>(map: &SmeltJsMap<SmeltUnknown, V>) -> Vec<SmeltUnknown> { let mut strings = Vec::new(); let mut symbols = Vec::new(); for (key, _) in smelt_own_js_map_entries(map) { if matches!(key, SmeltUnknown::Symbol(_)) { symbols.push(key); } else { strings.push(key); } } strings.extend(symbols); strings }

// @smelt:item-end
