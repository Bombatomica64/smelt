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
//~ The copy is a new object: the write-through hook naming the source instance stays behind.
fn smelt_structured_clone(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => { let cloned: Vec<(JsString, SmeltUnknown)> = map.iter().filter(|(key, _)| key.to_string_lossy() != "__smelt_method:__smelt_set_field").map(|(key, field)| (key, smelt_structured_clone(field))).collect(); SmeltUnknown::Object(SmeltObject::with_id(smelt_next_object_id(), cloned)) }, SmeltUnknown::Array(array) => SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), array.into_vec().into_iter().map(smelt_structured_clone).collect())), other => other } }
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
    //~ `[[Prototype]]` of objects built by `Object.create`, keyed by object identity.
    static SMELT_OBJECT_PROTOTYPES: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// Record the `[[Prototype]]` of the object with this identity.
fn smelt_register_object_prototype(id: usize, prototype: SmeltUnknown) { SMELT_OBJECT_PROTOTYPES.with(|prototypes| { prototypes.borrow_mut().insert(id, prototype); }); }

/// The recorded `[[Prototype]]` of the object with this identity, if it has one.
fn smelt_recorded_object_prototype(id: usize) -> Option<SmeltUnknown> { SMELT_OBJECT_PROTOTYPES.with(|prototypes| prototypes.borrow().get(&id).cloned()) }

/// Create a fresh erased object from a runtime prototype value (`Object.create`).
fn smelt_object_from_prototype(prototype: SmeltUnknown) -> SmeltUnknown { let mut fields: Vec<(JsString, SmeltUnknown)> = Vec::new(); match &prototype { SmeltUnknown::String(sentinel) if sentinel.eq_str("__smelt_proto:class") => { fields.push(("__smelt_class".into(), SmeltUnknown::Bool(true))); }, SmeltUnknown::Object(map) if smelt_class_prototype_tag(map).is_some() => { if let Some(tag) = smelt_class_prototype_tag(map) { fields.push(("__smelt_class".into(), SmeltUnknown::String(tag))); } }, SmeltUnknown::Object(map) => { for (key, value) in map.iter() { if key.eq_str("__smelt_class") || key.eq_str_prefix("__smelt_proto:") { fields.push((key, value)); } else { fields.push((JsString::from(format!("__smelt_proto:{key}")), value)); } } }, _ => {} } let object = SmeltObject::new(fields); if smelt_prototype_slot_is_observable(&prototype) { smelt_register_object_prototype(object.id, prototype); } SmeltUnknown::Object(object) }
// @smelt:item-end

// `Object.defineProperty(o, k, d)` and `Object.defineProperties(o, ds)`
// both install descriptors on an object, so the frontend normalizes the
// singular form into the plural one and both land here.
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
// @smelt:item smelt_class_prototype when unknown
//~ A class's prototype is ONE erased object per class, interned by the class's
//~ tag (the name its instances carry in `__smelt_class`), so
//~ `Object.getPrototypeOf(a) === Object.getPrototypeOf(b)` holds exactly when
//~ `a` and `b` are instances of one class, and both are `C.prototype`. Its own
//~ `[[Prototype]]` is the base class's prototype, or `Object.prototype` for a
//~ root class, recorded in `SMELT_OBJECT_PROTOTYPES` like every other link, so
//~ every chain walk reads one table. It stores only its tag, under the
//~ `__smelt_method:` prefix every enumeration and equality filter hides; its
//~ `constructor` is answered on read from the interned class value
//~ (`smelt_get_object_field`), so no value holds a cycle. See
//~ docs/design/class-prototypes.md.
thread_local! {
    static SMELT_CLASS_PROTOTYPES: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
    static SMELT_CLASS_VALUES: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
    static SMELT_CLASS_BUILDERS: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

//~ An instance of a class the program builds reflectively carries the class's
//~ erased constructor (`__smelt_method:constructor`). A prototype read off it
//~ records that constructor for the class's tag, so the per-tag stand-in
//~ `.constructor` answers before the program reads the class as a value
//~ (`smelt_class_constructor`) runs the real constructor.
/// Record the erased constructor an instance of the class tagged `tag` carries.
fn smelt_note_class_builder(map: &SmeltObject, tag: &JsString) {
    let Some(builder @ SmeltUnknown::Function(_)) = map.get("__smelt_method:constructor") else { return };
    SMELT_CLASS_BUILDERS.with(|builders| { builders.borrow_mut().entry(tag.to_string_lossy()).or_insert(builder); });
}

/// The erased constructor recorded for the class tagged `tag`, if any.
fn smelt_class_builder(tag: &str) -> Option<SmeltUnknown> { SMELT_CLASS_BUILDERS.with(|builders| builders.borrow().get(tag).cloned()) }

/// The key a class prototype stores its class tag under.
const SMELT_CLASS_PROTO_KEY: &str = "__smelt_method:__smelt_class_proto";

/// The program class tagged `tag`, as its base's tag (`""` for a root
/// class); `None` when no class, or more than one, carries the tag.
//~ Two classes of one source name in different modules share a tag, so a
//~ table keyed by it cannot tell them apart: such a tag is neither interned
//~ nor linked to a base.
fn smelt_program_class(tag: &str) -> Option<&'static str> {
    const SMELT_PROGRAM_CLASSES: &[(&str, &str)] = &[
// @smelt:slot program_classes
    ];
    SMELT_PROGRAM_CLASSES.iter().find(|(name, _)| *name == tag).map(|(_, base)| *base)
}

/// The interned prototype object of the class tagged `tag`.
fn smelt_class_prototype(tag: &str) -> SmeltUnknown {
    if let Some(found) = SMELT_CLASS_PROTOTYPES.with(|prototypes| prototypes.borrow().get(tag).cloned()) { return found; }
    let prototype = SmeltObject::new(Vec::from([(SMELT_CLASS_PROTO_KEY.to_owned(), SmeltUnknown::String(tag.into()))]));
    let value = SmeltUnknown::Object(prototype.clone());
    SMELT_CLASS_PROTOTYPES.with(|prototypes| { prototypes.borrow_mut().insert(tag.to_owned(), value.clone()); });
    let base = match smelt_program_class(tag) { Some(base) if !base.is_empty() && base != tag => smelt_class_prototype(base), _ => SmeltUnknown::String("__smelt_proto:object".into()) };
    smelt_register_object_prototype(prototype.id, base);
    value
}

/// The class tag a class prototype object carries, if `map` is one.
fn smelt_class_prototype_tag(map: &SmeltObject) -> Option<JsString> { match map.get(SMELT_CLASS_PROTO_KEY) { Some(SmeltUnknown::String(tag)) => Some(tag), _ => None } }

/// Intern the class value built for `tag`: the first one built is the class
/// from then on, so every read of the class is the same value.
//~ A later build can only fill in a constructor the first one lacked.
fn smelt_intern_class_value(tag: &str, value: SmeltUnknown) -> SmeltUnknown {
    SMELT_CLASS_VALUES.with(|values| {
        let mut values = values.borrow_mut();
        if let Some(found) = values.get(tag) {
            if let (SmeltUnknown::Object(found), SmeltUnknown::Object(built)) = (found, &value) && matches!(found.get("__smelt_call"), None | Some(SmeltUnknown::Undefined)) && let Some(call) = built.get("__smelt_call") { found.insert("__smelt_call", call); }
            return found.clone();
        }
        values.insert(tag.to_owned(), value.clone());
        value
    })
}

/// The class value of the class tagged `tag`: the interned one once the
/// program has read the class, else the per-tag constructor stand-in.
fn smelt_class_value_of(tag: &str) -> SmeltUnknown { SMELT_CLASS_VALUES.with(|values| values.borrow().get(tag).cloned()).unwrap_or_else(|| smelt_class_constructor(tag.to_owned())) }
// @smelt:item-end

// @smelt:item smelt_set_prototype_of when set_prototype_of
/// `Object.setPrototypeOf(target, prototype)` on an erased object.
//~ The link goes into `SMELT_OBJECT_PROTOTYPES`, which every prototype read and
//~ chain walk consults first. A class prototype also stamps its class tag, so
//~ name-based `instanceof` and `.constructor` agree with the chain; any other
//~ prototype takes a class tag away. Not an object: no link to record, as
//~ JavaScript ignores the call for a primitive target.
fn smelt_set_prototype_of(target: SmeltUnknown, prototype: SmeltUnknown) -> SmeltUnknown {
    if let SmeltUnknown::Object(map) = &target {
        match &prototype {
            SmeltUnknown::Object(proto) if let Some(tag) = smelt_class_prototype_tag(proto) => { map.insert("__smelt_class", SmeltUnknown::String(tag)); }
            SmeltUnknown::Object(_) | SmeltUnknown::Null => { map.remove("__smelt_class"); }
            _ => return target,
        }
        smelt_register_object_prototype(map.id, prototype);
    }
    target
}
// @smelt:item-end

// @smelt:item smelt_define_properties when define_properties
/// Install a property-descriptor table on an erased object (`Object.defineProperties`).
fn smelt_define_properties(target: SmeltUnknown, descriptors: SmeltUnknown) -> SmeltUnknown { if let (SmeltUnknown::Object(map), SmeltUnknown::Object(table)) = (&target, &descriptors) { for (key, descriptor) in table.iter() { let SmeltUnknown::Object(entry) = descriptor else { continue }; let enumerable = match entry.get("enumerable") { None | Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), Some(_) => true }; if !enumerable { continue; } let value = if let Some(value) = entry.get("value") { value } else if let Some(getter) = entry.get("get") { match getter { SmeltUnknown::Function(getter) => (getter)(Vec::new()).unwrap_or(SmeltUnknown::Undefined), other => other } } else { SmeltUnknown::Undefined }; map.insert(key, value); } } target }

// @smelt:item-end

// @smelt:item smelt_prototype_sentinel when unknown
fn smelt_prototype_sentinel(value: &SmeltUnknown) -> SmeltUnknown { if let SmeltUnknown::Object(map) = value { if let Some(prototype) = smelt_recorded_object_prototype(map.id) { return prototype; } } match value { SmeltUnknown::Null => SmeltUnknown::Null, SmeltUnknown::Array(_) => SmeltUnknown::String("__smelt_proto:array".into()), SmeltUnknown::Promise(_) => SmeltUnknown::String("__smelt_proto:promise".into()), SmeltUnknown::Object(map) if let Some(SmeltUnknown::String(tag)) = map.get("__smelt_class") => { smelt_note_class_builder(map, &tag); smelt_class_prototype(&tag.to_string_lossy()) }, SmeltUnknown::Object(map) if map.contains_key("__smelt_class") => SmeltUnknown::String("__smelt_proto:class".into()), SmeltUnknown::Object(map) => match smelt_reflected_marker_class(map) { Some(class) => smelt_reflected_prototype(class), None => SmeltUnknown::String("__smelt_proto:object".into()) }, SmeltUnknown::String(marker) if &**marker == "__smelt_proto:object" => SmeltUnknown::Null, SmeltUnknown::String(marker) if &**marker == "__smelt_proto:array" || &**marker == "__smelt_proto:promise" || &**marker == "__smelt_proto:class" => SmeltUnknown::String("__smelt_proto:object".into()), _ => SmeltUnknown::String("__smelt_proto:object".into()) } }
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
