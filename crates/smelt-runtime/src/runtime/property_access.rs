// Reading and writing properties of erased values.

// Erased dynamic indexed assignment `target[key] = value`. Mirrors JS:
// an object gets a string property; an array with a numeric index sets
// (and extends) that element; a value other than an array or a
// function becomes a fresh object holding the single property; an array with a
// non-index key keeps its identity and stores that key in the named-property table.
// Centralizing this keeps each call site a single call instead of an
// inline `match` that repeats `SmeltUnknown` at every assignment.
// @smelt:item smelt_index_assign when unknown
fn smelt_index_assign(target: &mut SmeltUnknown, key: String, value: SmeltUnknown) {
    match target {
        //~ A byte-backed host object's indexed slots are its bytes, not ordinary
        //~ record properties: `view[i] = byte` has to land in `bytes` so a later
        //~ `view[i]` read and a `slice()` observe it. Writes to any other key, or to
        //~ a record without byte storage, fall through to the property insert.
        SmeltUnknown::Object(map) if smelt_host_buffer_set_element(map, &key, value.clone()) => {}
        SmeltUnknown::Object(map) => { map.insert(key, value); }
        //~ A JavaScript function is an object, so a keyed write onto one is a
        //~ real store into its own-property bag (`function_object_prelude`) --
        //~ not a reason to replace the function with a record, which is what the
        //~ fallback arm below would do and which loses the callable.
        SmeltUnknown::Function(function) => { smelt_set_function_property(function, &key, value); }
        //~ A JS array keeps being an array when a non-index key is written to it:
        //~ `a[0] = x` sets an element, `a.x = v` sets a named property in the
        //~ array's side table (`SmeltArray::props`). Replacing the array with a
        //~ one-property object — what this used to do — lost every element and made
        //~ `Array.isArray(a)` answer `false`.
        SmeltUnknown::Array(array) => {
            if let Ok(index) = key.parse::<usize>() { array.set_index(index, value); }
            else { array.set_named_property(key, value); }
        }
        other => { *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([(key, value)]))); }
    }
}

// @smelt:item-end

// JavaScript property-key coercion: `obj[key]` stringifies whatever `key`
// is. Lives in the prelude because it was previously emitted as a full
// nested `fn` definition at EVERY dynamic property-key site. es-toolkit's
// `compat/object/mergeWith` key loop alone repeated it enough to make one
// generated file 2.6 MB of the crate's 14 MB.
//
// A symbol key keeps its `__smelt_symbol:` prefix, which is the storage
// form symbol-keyed properties use, so a symbol round-trips as a distinct
// key rather than colliding with its own description string.
// @smelt:item smelt_property_key when unknown
fn smelt_property_key(value: SmeltUnknown) -> String { match value { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), SmeltUnknown::Number(value) => smelt_number_to_string(value), /*@if bigint*/SmeltUnknown::BigInt(value) => value.to_string(), /*@end*/SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => "undefined".to_owned(), SmeltUnknown::Array(values) => values.into_vec().into_iter().map(smelt_property_key).collect::<Vec<_>>().join(","), SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() } }

// @smelt:item-end

// JavaScript `Array.prototype.concat` normalizes each argument with
// `IsConcatSpreadable`: an array contributes its elements, and any other
// value contributes itself as one element. When the argument's static
// type is erased the frontend cannot pick a side, so it emits
// `Rvalue::ConcatSpread` and the decision lands here, at runtime.
// @smelt:item smelt_concat_spread when concat_spread & unknown
fn smelt_concat_spread(value: SmeltUnknown) -> Vec<SmeltUnknown> { match value { SmeltUnknown::Array(values) => values.into_vec(), other => ::std::vec![other] } }

// @smelt:item-end

// One key test shared by every erased-Map accessor, so a read and a write
// cannot disagree about which entry a key names. A `{ __smelt_map: [..] }`
// entry is a two-element array, and Map key identity is SameValueZero.
// @smelt:item smelt_map_entry_key_is when unknown
/// Whether an erased Map entry (a `[key, value]` pair) is keyed by `key`.
fn smelt_map_entry_key_is(entry: &SmeltUnknown, key: &SmeltUnknown) -> bool {
    let SmeltUnknown::Array(pair) = entry else { return false };
    pair.get(0).is_some_and(|entry_key| entry_key.same_js_key(key))
}

/// A property missing from a record's own entries, read through the
/// prototype entries an erased object carries (`__smelt_proto:` for
/// `Object.create`, `__smelt_method:` for a class method), else `undefined`.
#[allow(dead_code)]
fn smelt_record_inherited(record: &SmeltRecord<String, SmeltUnknown>, key: &str) -> SmeltUnknown { record.get(&format!("__smelt_proto:{key}")).or_else(|| record.get(&format!("__smelt_method:{key}"))).unwrap_or(SmeltUnknown::Undefined) }
// @smelt:item-end

// @smelt:item smelt_get_object_field
// @smelt:if unknown
fn smelt_get_object_field(map: &SmeltObject, field: &str) -> SmeltUnknown {
// @smelt:end
// @smelt:if vitest_mock
    //~ Vitest exposes a `.mock` accessor on every mock function carrying
    //~ its recorded activity (`mockFn.mock.calls`, `mockFn.mock.results`).
    //~ The erased mock object only stores the `__smelt_vitest_mock` marker
    //~ and its configuration methods, so synthesize the `.mock` object
    //~ from the live registry state on read. `calls` is an array of the
    //~ recorded argument arrays; `results` mirrors the recorded return
    //~ values, so `mockFn.mock.calls.length` flows through the ordinary
    //~ array-length path.
    if field == "mock" { if let Some(state) = smelt_vitest_mock_state(&SmeltUnknown::Object(map.clone())) { let state = state.borrow(); let calls = state.calls.iter().map(|call| SmeltUnknown::Array(call.clone().into())).collect::<Vec<_>>(); let results = state.results.clone(); let mock = SmeltObject::new(Vec::new()); mock.insert("calls".to_owned(), SmeltUnknown::Array(calls.into())); mock.insert("results".to_owned(), SmeltUnknown::Array(results.into())); return SmeltUnknown::Object(mock); } }
// @smelt:end
// @smelt:if unknown
    //~ An erased `Map` is a marker object `{ __smelt_map: [[k, v], ...] }`.
    //~ Real Maps expose `.size` through `Map.prototype`, which the marker
    //~ object does not store as an own field, so synthesize it from the entry
    //~ count when a `.size` read reaches an erased Map. This keeps generic
    //~ `unknown`-typed code that probes `value.size` (e.g. `isEmptyish`)
    //~ correct without materializing the typed `SmeltJsMap`.
    //~ Error markers do not store `name` as an own field (the base
    //~ `new Error(msg)` shape is just `{ __smelt_error, message }`). Real errors
    //~ inherit `name` from their prototype, so synthesize it when absent — from
    //~ the class name the marker records, which is what makes
    //~ `new TypeError('t').name` read `"TypeError"` rather than `"Error"`.
    if field == "name" && !map.contains_key("name") { if let Some(SmeltUnknown::String(class_name)) = map.get("__smelt_error") { return SmeltUnknown::String(class_name); } }
    //~ A class instance answers `.constructor` with its interned per-class value.
    //~ An own `constructor` field still wins (source code can assign one), and a
    //~ plain object keeps `undefined` — which is what lets two plain objects pass
    //~ the `isPlainObject(a) && isPlainObject(b)` arm instead.
    if field == "constructor" && !map.contains_key("constructor") { if let Some(SmeltUnknown::String(class_name)) = map.get("__smelt_class") { return smelt_class_constructor(class_name.to_string()); } }
    //~ Byte-backed host objects (`ArrayBuffer`, `Buffer`, `DataView`, ...) hold
    //~ their storage in a `bytes` list, so an indexed read (`buffer[1]`, and the
    //~ `a[i]` walk es-toolkit's `isEqualWith` runs over a typed-array-tagged
    //~ value) must resolve against those bytes rather than answering
    //~ `undefined`. Non-index fields and non-byte-backed records are untouched.
    if let Some(element) = smelt_host_buffer_element(map, field) { return element; }
    //~ `blob.constructor === Blob` holds in JavaScript, and the clone specs
    //~ assert it on a cloned host object. A marker-bearing record therefore
    //~ answers `.constructor` with the interned constructor value for its
    //~ identity — the very object a bare `Blob` reference evaluates to. An own
    //~ `constructor` field still wins (source code can assign one), and an
    //~ unmarked record keeps `undefined`, which is what makes two plain objects
    //~ compare as equal instances.
    if field == "constructor" && !map.contains_key("constructor") { if let Some(class) = smelt_marker_constructor_class(map) { return smelt_builtin_namespace(class); } }
    //~ The ambient global object's properties ARE the modeled JavaScript
    //~ globals. `globalThis.Error` already resolved to the modeled
    //~ constructor through the static-member normalization in the frontend,
    //~ but the same read spelled with a runtime key (`globalThis[type]`, the
    //~ shape every lodash-derived typed-array/error spec loop uses) answered
    //~ `undefined`, so `new (globalThis[type])(...)` fabricated a
    //~ null-returning closure call and every value it produced compared
    //~ equal to every other. Resolving the name against the interned
    //~ builtin-constructor registry — the same table the static spelling
    //~ uses — makes the two spellings agree. A name this profile models no
    //~ constructor for is still genuinely absent, and an own field set on
    //~ the global object still wins.
    if map.contains_key("__smelt_global_object") && !map.contains_key(field) && smelt_builtin_construct_kind(field).is_some() { return smelt_builtin_namespace(field); }
    //~ A boxed string (`new String('ab')`) is an exotic String object: its
    //~ payload's own properties — `length` and the indexed characters — are
    //~ properties of the WRAPPER too, so `new String('ab').length` is 2 and
    //~ `[0]` is `"a"`. They are not stored as own entries (that would make
    //~ them enumerable and would double them in a clone), so they are
    //~ synthesized on read, exactly like the erased `Map`'s `.size` below. An
    //~ own field still wins, and the payload's *methods* stay on the
    //~ prototype rather than becoming own members.
    if !map.contains_key(field) { if let Some(SmeltUnknown::String(text)) = map.get("__smelt_string").and(map.get("value")) { if field == "length" { return SmeltUnknown::Number(smelt_utf16_len(&text) as f64); } if let Ok(index) = field.parse::<usize>() { return smelt_utf16_char_at(&text, index).map_or(SmeltUnknown::Undefined, |character| SmeltUnknown::String(character.into())); } } }
    //~ A builtin read as a value is a marker record (`smelt_builtin_namespace`),
    //~ and JavaScript answers property reads on it: `Array.prototype` is the
    //~ builtin's prototype object and `Array.isArray` is a function value. Both
    //~ resolve through the shared modeled-member registry, so the value spelling
    //~ of a member and its call spelling agree. An own field still wins, and an
    //~ unmodeled member stays `undefined` rather than becoming a callable that
    //~ cannot run.
    if !map.contains_key(field) { if let Some(SmeltUnknown::String(class)) = map.get("__smelt_builtin_namespace").and(map.get("name")) { if field == "prototype" { return smelt_builtin_prototype_object(&class); } if let Some(member) = smelt_builtin_member_value(&class, "static", field) { return member; } } }
    if let Some(SmeltUnknown::String(class)) = map.get("__smelt_builtin_prototype") { if let Some(member) = smelt_builtin_member_value(&class, "prototype", field) { return member; } }
    if field == "size" { if let Some(SmeltUnknown::Array(pairs)) = map.get("__smelt_map") { return SmeltUnknown::Number(pairs.len() as f64); } }
    //~ Same synthesis for an erased `Set` (`{ __smelt_set: [members...] }`):
    //~ real Sets expose `.size` through `Set.prototype`, absent from the marker
    //~ object's own fields, so derive it from the member count.
    if field == "size" { if let Some(SmeltUnknown::Array(members)) = map.get("__smelt_set") { return SmeltUnknown::Number(members.len() as f64); } }
    //~ Erased `Map` prototype methods, the exact counterpart of the erased-Set
    //~ block below. Only `.size` used to be synthesized, so every other
    //~ `Map.prototype` read on an erased Map answered `undefined` — and a
    //~ *call* of that `undefined` collapsed to a null callback rather than
    //~ failing, which made the miss silent. es-toolkit `isEqualWith` walks a
    //~ Map with `for (const [key, value] of a.entries())` after checking
    //~ `a.size !== b.size`: with `entries()` yielding nothing, the loop body
    //~ never ran and two same-size Maps with completely different contents
    //~ compared EQUAL.
    //~
    //~ Entries are read from the `{ __smelt_map: [[k, v], ...] }` marker array.
    //~ `get`/`has` apply SameValueZero (`same_js_key`) and take the FIRST
    //~ matching pair, which is the only one a Map can hold. `forEach` invokes
    //~ the callback with `(value, key, map)` per JS semantics — note the
    //~ argument order is the reverse of the stored pair. As in the erased-Set
    //~ block, the direct `m.forEach(cb)` spelling never reaches here: the
    //~ erased-iterable coercion claims it first and walks the marker array,
    //~ handing the callback `(entry, index)`. The arm covers the spellings that
    //~ do go through a field read (`m["forEach"]`, a detached method value).
    //~
    //~ The mutators (`set`/`delete`/`clear`) are synthesized too, because a
    //~ Map does NOT always reach a mutation site at its typed `SmeltJsMap`:
    //~ a declared `Map<K, V> | SomeCacheInterface` erases the whole union to
    //~ `SmeltUnknown`, and es-toolkit's `memoize` mutates its cache through
    //~ exactly that spelling (`cache.set(key, result)`). With the mutators
    //~ missing, the read answered `undefined`, the call collapsed to a null
    //~ callback, and every write vanished silently — a memoizing function
    //~ never memoized. The marker array is the map's own shared storage
    //~ (`SmeltArray` is an `Rc<RefCell<..>>` handle), so an insert, a delete
    //~ and a clear all land in the same entries every later read walks. Key
    //~ identity is SameValueZero (`same_js_key`) and the return values are
    //~ JavaScript's: `set` answers the map, `delete` a boolean, `clear`
    //~ `undefined`.
    if let Some(SmeltUnknown::Array(pairs)) = map.get("__smelt_map") {
        //~ The live handle, kept alongside the read-only snapshot the accessors
        //~ below close over: a mutator must reach the shared storage, not a copy.
        let entry_store = pairs.clone();
        let pairs = pairs.into_vec();
        let entry_at = |pair: &SmeltUnknown| -> Option<(SmeltUnknown, SmeltUnknown)> { let SmeltUnknown::Array(entry) = pair else { return None }; let mut entry = entry.clone().into_vec().into_iter(); match (entry.next(), entry.next()) { (Some(key), Some(value)) => Some((key, value)), _ => None } };
        let entries = pairs.iter().filter_map(entry_at).collect::<Vec<_>>();
        match field {
            "keys" => { let keys = entries.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(keys.clone().into())))); }
            "values" => { let values = entries.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(values.clone().into())))); }
            "entries" => { let pairs = pairs.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(pairs.clone().into())))); }
            "get" => { let entries = entries.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); Ok(entries.iter().find(|(key, _)| key.same_js_key(&needle)).map_or(SmeltUnknown::Undefined, |(_, value)| value.clone())) })); }
            "has" => { let entries = entries.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); Ok(SmeltUnknown::Bool(entries.iter().any(|(key, _)| key.same_js_key(&needle)))) })); }
            "forEach" => { let entries = entries.clone(); let receiver = SmeltUnknown::Object(map.clone()); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { if let Some(SmeltUnknown::Function(callback)) = args.into_iter().next() { for (key, value) in entries.clone() { callback(vec![value, key, receiver.clone()])?; } } Ok(SmeltUnknown::Undefined) })); }
            "set" => { let store = entry_store.clone(); let receiver = SmeltUnknown::Object(map.clone()); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let mut args = args.into_iter(); let key = args.next().unwrap_or(SmeltUnknown::Undefined); let value = args.next().unwrap_or(SmeltUnknown::Undefined); let existing = store.iter().position(|pair| smelt_map_entry_key_is(&pair, &key)); let entry = SmeltUnknown::Array(vec![key, value].into()); match existing { Some(index) => store.set_index(index, entry), None => store.push(entry) } Ok(receiver.clone()) })); }
            "delete" => { let store = entry_store.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); let kept = store.iter().filter(|pair| !smelt_map_entry_key_is(pair, &needle)).collect::<Vec<_>>(); let removed = kept.len() != store.len(); store.replace_all(kept); Ok(SmeltUnknown::Bool(removed)) })); }
            "clear" => { let store = entry_store.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { store.replace_all(Vec::new()); Ok(SmeltUnknown::Undefined) })); }
            _ => {}
        }
    }
    //~ Erased `Set` prototype methods. A real Set exposes
    //~ `values`/`keys`/`entries`/`has`/`forEach` through `Set.prototype`, which
    //~ the `{ __smelt_set: [...] }` marker object does not store as own fields.
    //~ Synthesize them so generic `unknown`-typed code that iterates or probes
    //~ an erased Set (e.g. `es-toolkit` `isEqualWith`'s `Array.from(a.values())`
    //~ membership walk) sees the members instead of `undefined`. Each returns a
    //~ fresh closure over a clone of the member array. `values`/`keys` yield the
    //~ member array (Set keys equal values); `entries` yields `[value, value]`
    //~ pairs; `has` applies SameValueZero (`same_js_key`); `forEach` invokes the
    //~ callback with `(value, value, set)` per JS semantics.
    //~ The mutators (`add`/`delete`/`clear`) stay absent: unlike a Map, a Set
    //~ whose element type is concrete erases to a plain `SmeltUnknown::Array`
    //~ rather than to the `{ __smelt_set: [..] }` marker, so a write through
    //~ an erased Set is a different (unmodeled) shape, not this one.
    if let Some(SmeltUnknown::Array(members)) = map.get("__smelt_set") {
        let members = members.into_vec();
        match field {
            "values" | "keys" => { let members = members.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(members.clone().into())))); }
            "entries" => { let members = members.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(members.clone().into_iter().map(|value| SmeltUnknown::Array(vec![value.clone(), value].into())).collect::<Vec<_>>().into())))); }
            "has" => { let members = members.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); Ok(SmeltUnknown::Bool(members.iter().any(|member| member.same_js_key(&needle)))) })); }
            "forEach" => { let members = members.clone(); let receiver = SmeltUnknown::Object(map.clone()); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { if let Some(SmeltUnknown::Function(callback)) = args.into_iter().next() { for member in members.clone() { callback(vec![member.clone(), member.clone(), receiver.clone()])?; } } Ok(SmeltUnknown::Undefined) })); }
            _ => {}
        }
    }
    // A missing property reads as JS `undefined`, distinct from an
    // explicit `null` value (`obj.missing === undefined`, `!== null`).
    //~ Prototype-chain read. `Object.create(proto)` stores the prototype's
    //~ members behind a `__smelt_proto:` prefix so they stay out of own-key
    //~ enumeration; an own field shadows the inherited one, exactly as in JS.
    //~ A class instance answers a method read from its prototype slot, the
    //~ same way `Object.create(proto)` answers an inherited property from
    //~ `__smelt_proto:`. Both are consulted only after the own property
    //~ misses, exactly as JavaScript's prototype chain does.
    let smelt_field_value = match map.get(field) { Some(value) => Some(value), None => match map.get(&format!("__smelt_proto:{field}")) { Some(value) => Some(value), None => map.get(&format!("__smelt_method:{field}")) } };
    match smelt_field_value.unwrap_or(SmeltUnknown::Undefined) {
        SmeltUnknown::Object(getter) if getter.contains_key("__smelt_get") => match getter.get("__smelt_get") {
            Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| smelt_panic_throw(error)),
            _ => SmeltUnknown::Null,
        },
        value => value,
    }
}

// @smelt:end
// @smelt:item-end

// `Object.prototype`'s members, as a lookup FALLBACK rather than stored
// entries.
//
// In JavaScript every plain object inherits them: `'toString' in {}` is
// `true`, `({}).toString` is a function, and `({}).toString ===
// Object.prototype.toString` — one function, on the prototype, so any two
// reads are `===`. Smelt models an object as its own entries, so these
// members have to come from somewhere else, and they must NOT be entries:
// enumeration (`Object.keys`, `for...in`), structural equality and JSON
// would then see them, making `{}` a two-key object unequal to `{}`.
//
// Each member is built once per key and cached with one canonical identity
// (`smelt_method_identity`, the same registry class-method reads use),
// which is what makes the `===` above hold. The receiver is taken from the
// first argument, matching the explicit-`this` spelling
// (`Object.prototype.toString.call(v)`); the frontend lowers that spelling
// to a direct helper call, so this path serves value-position reads
// (`const f = Object.prototype.toString`) and presence checks.
//
// `constructor` is deliberately NOT in the table: `smelt_get_object_field`
// already answers it for a class instance and for a marker-bearing host
// record, and a plain object answering `undefined` there is what lets
// es-toolkit `isEqualWith` fall through to its `isPlainObject(a) &&
// isPlainObject(b)` arm.
// @smelt:item SMELT_OBJECT_PROTOTYPE_MEMBERS when unknown
thread_local! {
    /// One cached `Object.prototype` member value per key, so repeated reads are `===`.
    static SMELT_OBJECT_PROTOTYPE_MEMBERS: ::std::cell::RefCell<::std::collections::HashMap<&'static str, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// Apply one `Object.prototype` member to an explicit receiver.
fn smelt_object_prototype_apply(key: &str, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let mut args = args.into_iter();
    let receiver = args.next().unwrap_or(SmeltUnknown::Undefined);
    match key {
        "Object.prototype.toString" | "Object.prototype.toLocaleString" => SmeltUnknown::String(smelt_object_to_string_tag(&receiver).into()),
        "Object.prototype.valueOf" => smelt_unbox_primitive(receiver),
        "Object.prototype.hasOwnProperty" | "Object.prototype.propertyIsEnumerable" => { let key = smelt_property_key(args.next().unwrap_or(SmeltUnknown::Undefined)); SmeltUnknown::Bool(smelt_has_own_property(&receiver, &key)) }
        //~ `isPrototypeOf` asks whether the receiver appears on the argument's
        //~ prototype CHAIN. Smelt represents a prototype as an opaque sentinel
        //~ (`smelt_prototype_sentinel`) rather than as a linked object, so there is
        //~ no chain to walk: the honest answer for the only receivers that can
        //~ reach here — a plain object or the `Object.prototype` sentinel read as a
        //~ value — is `false`, which is also what JavaScript answers for every
        //~ plain object receiver.
        _ => SmeltUnknown::Bool(false),
    }
}

/// Read an `Object.prototype` member, or `None` when the name is not one.
fn smelt_object_prototype_member(field: &str) -> Option<SmeltUnknown> {
    let key: &'static str = match field {
        "toString" => "Object.prototype.toString",
        "toLocaleString" => "Object.prototype.toLocaleString",
        "valueOf" => "Object.prototype.valueOf",
        "hasOwnProperty" => "Object.prototype.hasOwnProperty",
        "isPrototypeOf" => "Object.prototype.isPrototypeOf",
        "propertyIsEnumerable" => "Object.prototype.propertyIsEnumerable",
        _ => return None,
    };
    Some(SMELT_OBJECT_PROTOTYPE_MEMBERS.with(|members| members.borrow_mut().entry(key).or_insert_with(|| { let function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_object_prototype_apply(key, args))); smelt_link_function_identity_key(&function, smelt_method_identity(key)); SmeltUnknown::Function(function) }).clone()))
}

// @smelt:item-end

// Property reads on an erased ARRAY. A JS array answers `length`, its
// element indices, the named properties written through its side table
// (own properties, so they shadow the prototype), and then
// `Array.prototype`'s methods; anything else is `undefined`.
// @smelt:item smelt_get_array_field when unknown
/// Read a property off an erased JavaScript array (`arr.k` / `arr[k]`).
fn smelt_get_array_field(values: &SmeltArray, field: &str) -> SmeltUnknown {
    if field == "length" { return SmeltUnknown::Number(values.len() as f64); }
    if let Ok(index) = field.parse::<usize>() { return values.get(index).unwrap_or(SmeltUnknown::Undefined); }
    values.named_property(field).or_else(|| smelt_array_prototype_member(values, field)).unwrap_or(SmeltUnknown::Undefined)
}

//~ One erased property read for every receiver shape, so `v.k` and `v[k]`
//~ cannot diverge and each rule (marker records, array named properties,
//~ the `Object.prototype` fallback) lands in one place.
//~
//~ Last stop of the chain is `Object.prototype`, whose members every object
//~ inherits — so a read that found no own entry (and no `__smelt_proto:` /
//~ `__smelt_method:` level, both resolved inside `smelt_get_object_field`)
//~ resolves there rather than answering `undefined` for `toString` or
//~ `hasOwnProperty`. It sits HERE, at the property-read seam, and not
//~ inside `smelt_get_object_field`, because helpers that ask that function
//~ for a user-defined OWN override (`smelt_value_of_method`,
//~ `smelt_function_method`) must not be handed the inherited member.
//~
//~ The `"__smelt_proto:object"` arm is the prototype SENTINEL: Smelt
//~ represents `Object.prototype` (and `Object.getPrototypeOf({})`) as that
//~ opaque string rather than as a record, precisely so its members do not
//~ become enumerable entries of every object created from it. A member read
//~ on the sentinel resolves through the same fallback table an ordinary
//~ object uses, which is what makes `Object.prototype.toString` a value.
//~
//~ A FUNCTION is an object too, and its own properties live in the
//~ identity-keyed bag (`function_object_prelude`) because an
//~ `Rc<dyn Fn(..)>` has nowhere to store them. Routing the read there is
//~ what makes `f.prototype` an object rather than `undefined`, and what
//~ lets a property source code wrote onto a function read back.
/// Read a property off any erased value (JS `value.field`).
fn smelt_get_unknown_field(value: &SmeltUnknown, field: &str) -> SmeltUnknown {
    match value {
        //~ A marker-bearing host record resolves its MODELED MEMBERS first.
        //~ `smelt_host_method` answers `None` for a record with an own key of
        //~ that name, so an own property still wins as JavaScript's own-property
        //~ lookup does; what changes is that `(headers as any).get` is the
        //~ header list's `get` rather than `undefined`. The sibling erased-read
        //~ path in `place.rs` already asked; this one did not, so the same read
        //~ answered differently depending on which spelling reached it.
        SmeltUnknown::Object(map) => match smelt_host_method(map, field).unwrap_or_else(|| smelt_get_object_field(map, field)) { SmeltUnknown::Undefined => smelt_object_prototype_member(field).unwrap_or(SmeltUnknown::Undefined), value => value },
        SmeltUnknown::Array(values) => smelt_get_array_field(values, field),
        SmeltUnknown::String(marker) if &**marker == "__smelt_proto:object" => smelt_object_prototype_member(field).unwrap_or(SmeltUnknown::Undefined),
        //~ An erased STRING answers `length`, its character indices and the
        //~ `String.prototype` methods (`erased_method_prelude`). The other
        //~ `__smelt_proto:` strings are prototype SENTINELS rather than source
        //~ text, so they keep answering `undefined` as before.
        SmeltUnknown::String(text) if !text.starts_with("__smelt_proto:") => smelt_get_string_field(text, field),
        SmeltUnknown::Function(function) => match smelt_function_value_property(function, field) { SmeltUnknown::Undefined => smelt_object_prototype_member(field).unwrap_or(SmeltUnknown::Undefined), value => value },
        //~ A PROMISE reached dynamically still has its continuation members: the
        //~ typed `p.catch(f)` lowers to an `AsyncOp`, and this is the same
        //~ operation for a promise the static types lost (see
        //~ `smelt_promise_member`).
        SmeltUnknown::Promise(promise) => smelt_promise_member(promise, field),
        _ => SmeltUnknown::Undefined,
    }
}

//~ The two JavaScript property-PRESENCE tests, kept next to the
//~ property-read chain above so the three cannot drift apart.
//~
//~ `Object.hasOwn(value, key)` (and `hasOwnProperty` /
//~ `propertyIsEnumerable`, which Smelt's object model cannot tell apart
//~ from it) answers only for the value's OWN properties, while
//~ `key in value` continues onto the prototype chain -- so
//~ `Object.hasOwn({}, 'toString')` is `false` and `'toString' in {}` is
//~ `true`. Which of the two a containment test wants is carried on
//~ `Rvalue::DictContainsKey` as `PropertyLookup`; emitting one fused
//~ disjunction for both spellings, as the erased containment path used to,
//~ is necessarily wrong for one of them.
//~
//~ "Own" includes the properties Smelt SYNTHESIZES rather than stores as
//~ record entries -- an array's `length` and in-range indices, a boxed
//~ string wrapper's `length` and character indices, a byte view's decoded
//~ elements. Those are own properties in JavaScript too (storing them as
//~ entries is what Smelt avoids, because it would make them enumerable and
//~ would double them in a clone), so they belong here and not on the
//~ prototype half.
/// Whether a boxed-`String` wrapper answers `key` as one of its OWN properties.
fn smelt_boxed_string_own_property(map: &SmeltObject, key: &str) -> bool { let Some(SmeltUnknown::String(text)) = map.get("__smelt_string").and(map.get("value")) else { return false; }; key == "length" || key.parse::<usize>().is_ok_and(|index| index < smelt_utf16_len(&text)) }
/// Whether `value` has `key` as an own property (JS `Object.hasOwn`).
fn smelt_has_own_property(value: &SmeltUnknown, key: &str) -> bool {
    match value {
        SmeltUnknown::Object(map) => map.contains_key(key) || smelt_host_buffer_element(map, key).is_some() || smelt_boxed_string_own_property(map, key),
        SmeltUnknown::Array(values) => key == "length" || values.named_keys().contains(&key.to_owned()) || key.parse::<usize>().is_ok_and(|index| index < values.len()),
        SmeltUnknown::String(text) => key == "length" || key.parse::<usize>().is_ok_and(|index| index < smelt_utf16_len(text)),
        _ => false,
    }
}

//~ The prototype half. An erased `Map`/`Set` marker record exposes `size`
//~ through its prototype (mirroring the virtual `.size` synthesized in
//~ `smelt_get_object_field`), `Symbol.iterator` is on `Array.prototype`
//~ and `String.prototype`, and every object -- array and string wrapper
//~ included -- inherits `Object.prototype`'s members.
/// Whether `value` has `key` as an own OR inherited property (JS `key in value`).
fn smelt_has_property(value: &SmeltUnknown, key: &str) -> bool {
    if smelt_has_own_property(value, key) { return true; }
    match value {
        //~ A method of an erased class instance lives on its prototype, which
        //~ the erased object carries as `__smelt_method:`/`__smelt_proto:`
        //~ entries (the same ones a property READ resolves through), so
        //~ `'getResponse' in err` is true for an erased `HTTPException`.
        SmeltUnknown::Object(map) => ((map.contains_key("__smelt_map") || map.contains_key("__smelt_set")) && key == "size") || map.contains_key(&format!("__smelt_method:{key}")) || map.contains_key(&format!("__smelt_proto:{key}")) || smelt_object_prototype_member(key).is_some(),
        SmeltUnknown::Array(_) | SmeltUnknown::String(_) => key == "__smelt_symbol_iterator" || smelt_object_prototype_member(key).is_some(),
        _ => false,
    }
}

fn smelt_unknown_is_nullish(value: &SmeltUnknown) -> bool { matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined) }
fn smelt_unknown_is_undefined(value: &SmeltUnknown) -> bool { matches!(value, SmeltUnknown::Undefined) }
fn smelt_missing_property_value() -> SmeltUnknown { SmeltUnknown::Undefined }
fn smelt_missing_index_value() -> SmeltUnknown { SmeltUnknown::Undefined }

// @smelt:item-end
