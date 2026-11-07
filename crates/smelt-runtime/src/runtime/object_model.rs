// `structuredClone`, `typeof`, recorded prototypes and property descriptors.

// Rebuild a marker object/array with a FRESH identity while keeping its
// fields (shallow, matching `new Ctor(obj)` which copies the top level and
// shares nested references). `SmeltObject`/`SmeltArray` clones share the
// underlying `Rc` (JS reference semantics), so a genuinely new instance must
// allocate a new id over a copied entry map/vec.
// @smelt:item smelt_fresh_identity when unknown
fn smelt_fresh_identity(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => SmeltUnknown::Object(SmeltObject::with_id(smelt_next_object_id(), map.iter().collect())), SmeltUnknown::Array(array) => SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), array.into_vec())), other => other } }
// @smelt:item-end

// `structuredClone(value)` deep-copies an object graph with fresh
// identities, preserving host markers (Date/Map/Set/RegExp/Error/...). Used
// by es-toolkit `cloneDeep` (Error) and remeda `clone` (host objects it
// delegates to the platform). Primitives/functions/promises pass through.
// @smelt:item smelt_structured_clone when structured_clone
fn smelt_structured_clone(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => { let cloned: Vec<(JsString, SmeltUnknown)> = map.iter().map(|(key, field)| (key, smelt_structured_clone(field))).collect(); SmeltUnknown::Object(SmeltObject::with_id(smelt_next_object_id(), cloned)) }, SmeltUnknown::Array(array) => SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), array.into_vec().into_iter().map(smelt_structured_clone).collect())), other => other } }
// @smelt:item-end

// `Object.create(proto)` must mint a FRESH object. Two failure modes make
// "just return the prototype" wrong: a concrete prototype object would be
// aliased (so the `Object.assign(Object.create(p), o)` clone idiom mutates
// `p`), and an opaque `"__smelt_proto:*"` sentinel from
// `smelt_prototype_sentinel` is a *string*, so the caller would go on to
// index and assign fields on a string. Inherited members are modeled the
// same way the `Object.create({ ... })` literal lowering models them — the
// prototype's own keys are stored behind a `__smelt_proto:` prefix, which
// keeps them out of the created object's own-key enumeration. A class
// prototype keeps the hidden `__smelt_class` marker so the fresh object is
// still classified as a class instance rather than a plain object.
//
// Copying the members is not enough on its own, because the prototype
// CHAIN is observable: `Object.getPrototypeOf(Object.create(p))` is `p`
// itself, so `Object.getPrototypeOf(Object.getPrototypeOf(...))` is
// `Object.prototype` and not `null` — the distinction es-toolkit's
// `isPlainObject` is built on. The prototype value is therefore recorded
// in `SMELT_OBJECT_PROTOTYPES`, keyed by the created object's IDENTITY,
// and `smelt_prototype_sentinel` answers with it.
//
// Keyed by identity rather than kept as a hidden entry, for the same
// reason the array named-property table is (see `SMELT_ARRAY_PROPS`): a
// hidden entry has to be filtered out of every view that walks an
// object's entries, and that list can never be complete. Recovering a
// typed `Record<String, f64>` from an erased object maps EVERY entry
// through the value conversion, so a hidden slot came back as `NaN` — a
// prototype that is neither `null` nor `Object.prototype`, which is worse
// than recording none. Identity survives that round trip (the record and
// the object both carry the id forward), so `Object.assign`, which
// returns its target, keeps the target's prototype as JavaScript says it
// must.
// A marker costs something: it is one more key every raw-entry consumer
// has to know to skip. So record the prototype ONLY when the created
// object's own shape cannot already imply it. `Object.prototype` (the
// `"__smelt_proto:object"` sentinel) is what a fresh plain object already
// reports, and a class prototype is already carried by the
// `__smelt_class` marker stamped alongside; those two need no slot, which
// keeps the overwhelmingly common `Object.create(Object.getPrototypeOf(o))`
// clone idiom producing a marker-free object. A concrete object
// prototype, `null`, and the array/promise sentinels are not recoverable
// from the result, so they are stored.
//
// This table is the ONE prototype link in the runtime. `[[Construct]]`
// allocates its instance through this same helper (so `new F()` is linked
// to `F.prototype`), `smelt_prototype_sentinel` answers
// `Object.getPrototypeOf` from it, `smelt_proto_accessor` answers
// `v.__proto__` through that, and `smelt_instance_of_value` walks it — so
// `Object.getPrototypeOf(Object.create(p)) === p` and `x instanceof F`
// cannot disagree about the chain.
// @smelt:item smelt_prototype_slot_is_observable when unknown
/// Whether an `Object.create` prototype has to be recorded to stay observable.
fn smelt_prototype_slot_is_observable(prototype: &SmeltUnknown) -> bool { !matches!(prototype, SmeltUnknown::String(sentinel) if &**sentinel == "__smelt_proto:object" || &**sentinel == "__smelt_proto:class") }
// @smelt:item-end

// JavaScript `typeof` answers one of seven fixed spellings, decided purely
// by the erased value's tag. Every source `typeof` asks the same question,
// so the tag-to-spelling table lives here once and returns a `&'static str`
// rather than being inlined -- and re-allocated -- at each site.
// @smelt:item smelt_typeof when typeof & unknown
/// Resolve the JavaScript `typeof` spelling for an erased value.
fn smelt_typeof(value: &SmeltUnknown) -> &'static str { match value { SmeltUnknown::Undefined => "undefined", SmeltUnknown::Bool(_) => "boolean", SmeltUnknown::Number(_) => "number", /*@if bigint*/SmeltUnknown::BigInt(_) => "bigint", /*@end*/SmeltUnknown::String(_) => "string", SmeltUnknown::Symbol(_) => "symbol", SmeltUnknown::Function(_) => "function", SmeltUnknown::Object(object) if object.contains_key("__smelt_call") => "function", SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Promise(_) => "object" } }
// @smelt:item-end

// @smelt:item SMELT_OBJECT_PROTOTYPES when unknown
thread_local! {
    /// `[[Prototype]]` of objects built by `Object.create`, keyed by object identity.
    static SMELT_OBJECT_PROTOTYPES: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// Record the `[[Prototype]]` of the object with this identity.
fn smelt_register_object_prototype(id: usize, prototype: SmeltUnknown) { SMELT_OBJECT_PROTOTYPES.with(|prototypes| { prototypes.borrow_mut().insert(id, prototype); }); }

/// The recorded `[[Prototype]]` of the object with this identity, if it has one.
fn smelt_recorded_object_prototype(id: usize) -> Option<SmeltUnknown> { SMELT_OBJECT_PROTOTYPES.with(|prototypes| prototypes.borrow().get(&id).cloned()) }

/// Create a fresh erased object from a runtime prototype value (`Object.create`).
fn smelt_object_from_prototype(prototype: SmeltUnknown) -> SmeltUnknown { let mut fields: Vec<(JsString, SmeltUnknown)> = Vec::new(); match &prototype { SmeltUnknown::String(sentinel) if sentinel.eq_str("__smelt_proto:class") => { fields.push(("__smelt_class".into(), SmeltUnknown::Bool(true))); }, SmeltUnknown::Object(map) => { for (key, value) in map.iter() { if key.eq_str("__smelt_class") || key.eq_str_prefix("__smelt_proto:") { fields.push((key, value)); } else { fields.push((JsString::from(format!("__smelt_proto:{key}")), value)); } } }, _ => {} } let object = SmeltObject::new(fields); if smelt_prototype_slot_is_observable(&prototype) { smelt_register_object_prototype(object.id, prototype); } SmeltUnknown::Object(object) }
// @smelt:item-end

// `Object.defineProperty(o, k, d)` and `Object.defineProperties(o, ds)`
// both install descriptors on an object, so the frontend normalizes the
// singular form into the plural one and both land here. Previously both
// lowered to an opaque `null` and the mutation was DROPPED outright.
//
// What is modeled is the descriptor's VALUE: a data descriptor's `value`,
// or the one-shot result of an accessor descriptor's `get`. Object
// literal getters that Smelt already collapses to their return
// expression arrive as a plain value rather than a function, so both
// spellings are accepted.
//
// ENUMERABILITY is the one attribute that must be honoured rather than
// ignored, and it is honoured by NOT installing the property. An erased
// object is a flat key/value store with no per-property attribute table,
// so a key it holds is enumerable by construction: installing a
// `enumerable: false` property would make it appear in `Object.keys`,
// in spread, in `JSON.stringify` and in structural equality, all of
// which JavaScript hides it from. Leaving it out keeps every one of
// those answers right and only loses a direct `o.k` read.
// @smelt:item smelt_define_properties when define_properties
/// Install a property-descriptor table on an erased object (`Object.defineProperties`).
fn smelt_define_properties(target: SmeltUnknown, descriptors: SmeltUnknown) -> SmeltUnknown { if let (SmeltUnknown::Object(map), SmeltUnknown::Object(table)) = (&target, &descriptors) { for (key, descriptor) in table.iter() { let SmeltUnknown::Object(entry) = descriptor else { continue }; let enumerable = match entry.get("enumerable") { None | Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), Some(_) => true }; if !enumerable { continue; } let value = if let Some(value) = entry.get("value") { value } else if let Some(getter) = entry.get("get") { match getter { SmeltUnknown::Function(getter) => (getter)(Vec::new()).unwrap_or(SmeltUnknown::Undefined), other => other } } else { SmeltUnknown::Undefined }; map.insert(key, value); } } target }

// @smelt:item-end

// @smelt:item smelt_prototype_sentinel when unknown
fn smelt_prototype_sentinel(value: &SmeltUnknown) -> SmeltUnknown { if let SmeltUnknown::Object(map) = value { if let Some(prototype) = smelt_recorded_object_prototype(map.id) { return prototype; } } match value { SmeltUnknown::Null => SmeltUnknown::Null, SmeltUnknown::Array(_) => SmeltUnknown::String("__smelt_proto:array".into()), SmeltUnknown::Promise(_) => SmeltUnknown::String("__smelt_proto:promise".into()), SmeltUnknown::Object(map) if map.contains_key("__smelt_class") => SmeltUnknown::String("__smelt_proto:class".into()), SmeltUnknown::Object(map) => match smelt_reflected_marker_class(map) { Some(class) => smelt_reflected_prototype(class), None => SmeltUnknown::String("__smelt_proto:object".into()) }, SmeltUnknown::String(marker) if &**marker == "__smelt_proto:object" => SmeltUnknown::Null, SmeltUnknown::String(marker) if &**marker == "__smelt_proto:array" || &**marker == "__smelt_proto:promise" || &**marker == "__smelt_proto:class" => SmeltUnknown::String("__smelt_proto:object".into()), _ => SmeltUnknown::String("__smelt_proto:object".into()) } }
//~ `v.__proto__` is NOT `Object.getPrototypeOf(v)`. In JavaScript the
//~ `__proto__` accessor is inherited from `Object.prototype`, so a value
//~ whose prototype is `null` (`Object.create(null)`) does not inherit it
//~ and a `__proto__` write on such a value stores an ordinary OWN
//~ property that a later read has to answer. Smelt represents a
//~ null-prototype object as a plain erased object, so the own slot is the
//~ only observable trace of that case; everything else answers the same
//~ sentinel `Object.getPrototypeOf` does. Keeping the two spellings on
//~ separate helpers is what lets `Object.getPrototypeOf` stay blind to own
//~ properties, which the spec requires.
//~
//~ Everything past the own slot defers to `smelt_prototype_sentinel`, so
//~ the two spellings agree on the recorded `[[Prototype]]` of an
//~ `Object.create(proto)` result: that is what makes
//~ `Object.getPrototypeOf(Object.create(p))` and `Object.create(p).__proto__`
//~ both answer `p` itself rather than a sentinel, and therefore what makes
//~ an `instanceof` prototype-chain walk able to advance link by link.
/// Read the JavaScript `__proto__` accessor for an erased value.
fn smelt_proto_accessor(value: &SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) if map.contains_key("__proto__") => smelt_get_object_field(map, "__proto__"), other => smelt_prototype_sentinel(other) } }

// @smelt:item-end
