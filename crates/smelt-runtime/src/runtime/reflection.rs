// Class constructors and prototypes reached through reflection.
//
// Runtime prelude for host-identity *reflection*.
//
// JavaScript clone idioms reach a host constructor two ways. Directly:
//
// ```js
// new DataView(value.buffer.slice(0), value.byteOffset, value.byteLength)
// ```
//
// and reflectively, off the value's own prototype:
//
// ```js
// const Constructor = Object.getPrototypeOf(obj).constructor;
// return new Constructor(obj.buffer.slice(0));
// ```
//
// es-toolkit uses the first in `cloneDeepWith` and the second in `clone`, and
// its specs compare the two results against each other. So the reflected
// constructor cannot be an approximation of the direct one — they have to be the
// same code. This module emits that one constructor
// ([`runtime_symbols::host::REFLECTED_CONSTRUCT`]), the marker→kind
// discriminator that selects it, the cached per-kind prototype object it hangs
// off, and the interned per-name constructor value that `x.constructor` and a
// bare `Blob` reference both resolve to.
//
// Three tables drive everything, all derived here rather than restated at call
// sites:
//
// * [`REFLECTED_MARKERS`] — the markers whose *prototype* exposes a reflected
//   constructor. Deliberately narrow: giving a marker a reflected prototype
//   turns `Object.getPrototypeOf(x)` from an opaque string sentinel into a real
//   object, which changes what `Object.create(proto)` copies, so a marker joins
//   this list only when source code actually constructs through its prototype.
// * [`CONSTRUCTOR_CLASS_MARKERS`] — every marker whose record answers a
//   `.constructor` read. This one is broad: reading `x.constructor` is a pure
//   query and cannot perturb the prototype chain.
// * The shared host-object registry, for the byte-buffer and Blob/File kinds the
//   constructor knows how to build.

// @smelt:item smelt_regexp_literal when unknown
/// Stringify a marker-bearing erased RegExp as JavaScript does: `/source/flags`.
fn smelt_regexp_literal(map: &SmeltObject) -> String { let source = match map.get("source") { Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }; let flags = match map.get("flags") { Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }; format!("/{source}/{flags}") }

//~ JavaScript exposes `instance.constructor`, and code branches on it:
//~ es-toolkit `isEqualWith` gates instance comparison on
//~ `areObjectsEqual(a.constructor, b.constructor) || (isPlainObject(a) && isPlainObject(b))`.
//~ The class marker records the class name, so one constructor value is
//~ interned per name and every instance of a class reads back the SAME one.
//~
//~ It is a `Function` on purpose. `areObjectsEqual` tags it `[object Function]`
//~ and that branch decides by identity (`a === b`), so two classes compare
//~ unequal and two instances of one class compare equal — without recursing
//~ into the constructor's own `.constructor`, which an object value would do.
//~ Calling it yields a fresh instance carrying the same class marker, which is
//~ what `new obj.constructor()` means.
thread_local! { static SMELT_CLASS_CONSTRUCTORS: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }

/// One interned constructor value per class name, for `instance.constructor`.
fn smelt_class_constructor(class_name: String) -> SmeltUnknown { SMELT_CLASS_CONSTRUCTORS.with(|constructors| constructors.borrow_mut().entry(class_name.clone()).or_insert_with(|| { let marker = class_name.clone(); SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let fields = Vec::from([("__smelt_class".to_owned(), SmeltUnknown::String(marker.as_str().into()))]); Ok(SmeltUnknown::Object(SmeltObject::new(fields))) })) }).clone()) }

/// Return the opaque `Object.getPrototypeOf` sentinel for an erased value.
//~ Class instances carry a hidden `__smelt_class` marker and map to a distinct
//~ `"__smelt_proto:class"` sentinel so they are not treated as plain objects; arrays,
//~ `null`, and plain objects keep their existing sentinels.
//~
//~ The sentinel strings are themselves valid inputs: walking the prototype
//~ chain (`while (Object.getPrototypeOf(proto) !== null)`) re-invokes this
//~ helper on a returned sentinel, so each sentinel must advance one link
//~ toward `null` (Array/Promise/class prototypes inherit from
//~ `Object.prototype`, whose prototype is `null`). Without this the walk
//~ would return `"__smelt_proto:object"` forever and never terminate.
//~ Emit the marker→constructor-class discriminator for prototype reflection.
//~
//~ The answer is the name of the class whose prototype the record reflects, so
//~ the prototype's `constructor` slot can be the interned global of that very
//~ name. A marker whose stored VALUE is itself a modeled constructor name is how
//~ Smelt records a *subclass* identity (`__smelt_error: "AggregateError"`), and
//~ that name wins over the marker table's base class — otherwise every error
//~ subclass would reflect the base `Error` constructor and a reflected rebuild
//~ would lose the subclass's own construction shape.
/// Discriminate the class whose prototype a host-marker record reflects.
//~ `None` for plain objects, arrays and class instances, which keep their
//~ opaque `"__smelt_proto:*"` string sentinels.
fn smelt_reflected_marker_class(map: &SmeltObject) -> Option<String> {
    let (marker, class) = [("__smelt_date", "Date"), ("__smelt_map", "Map"), ("__smelt_set", "Set"), ("__smelt_regexp", "RegExp"), ("__smelt_dataview", "DataView"), ("__smelt_buffer", "Buffer"), ("__smelt_error", "Error"), ("__smelt_file", "File"), ("__smelt_number", "Number"), ("__smelt_boolean", "Boolean"), ("__smelt_int8array", "Int8Array"), ("__smelt_uint8array", "Uint8Array"), ("__smelt_uint8clampedarray", "Uint8ClampedArray"), ("__smelt_int16array", "Int16Array"), ("__smelt_uint16array", "Uint16Array"), ("__smelt_int32array", "Int32Array"), ("__smelt_uint32array", "Uint32Array"), ("__smelt_float32array", "Float32Array"), ("__smelt_float64array", "Float64Array"), ("__smelt_bigint64array", "BigInt64Array"), ("__smelt_biguint64array", "BigUint64Array")].into_iter().find(|(marker, _)| map.contains_key(marker))?;
    if let Some(SmeltUnknown::String(name)) = map.get(marker) { if smelt_builtin_construct_kind(&name.to_string_lossy()).is_some() { return Some(name.to_string_lossy()); } }
    Some(class.to_owned())
}
//~ Emit the marker→constructor-class lookup behind `x.constructor`.
/// The class name a marker-bearing record's `.constructor` read resolves to.
//~
//~ `blob.constructor === Blob` holds in JavaScript, and es-toolkit's clone
//~ specs assert exactly that on a cloned host object. Unmarked records answer
//~ `None`, so a plain object's `.constructor` stays `undefined` — which is what
//~ makes two plain objects compare as equal instances.
fn smelt_marker_constructor_class(map: &SmeltObject) -> Option<&'static str> {
    //~ A built-in error subclass names itself in its marker (`TypeError`), so its
    //~ `.constructor` is that class, not the base `Error`.
    if let Some(SmeltUnknown::String(name)) = map.get("__smelt_error") {
        let name = name.to_string_lossy();
        if let Some(class) = ["TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError", "AggregateError"].into_iter().find(|class| *class == name) { return Some(class); }
    }
    [("__smelt_date", "Date"), ("__smelt_map", "Map"), ("__smelt_set", "Set"), ("__smelt_regexp", "RegExp"), ("__smelt_error", "Error"), ("__smelt_abortcontroller", "AbortController"), ("__smelt_abortsignal", "AbortSignal"), ("__smelt_arraybuffer", "ArrayBuffer"), ("__smelt_sharedarraybuffer", "SharedArrayBuffer"), ("__smelt_int8array", "Int8Array"), ("__smelt_uint8array", "Uint8Array"), ("__smelt_uint8clampedarray", "Uint8ClampedArray"), ("__smelt_int16array", "Int16Array"), ("__smelt_uint16array", "Uint16Array"), ("__smelt_int32array", "Int32Array"), ("__smelt_uint32array", "Uint32Array"), ("__smelt_float32array", "Float32Array"), ("__smelt_float64array", "Float64Array"), ("__smelt_bigint64array", "BigInt64Array"), ("__smelt_biguint64array", "BigUint64Array"), ("__smelt_buffer", "Buffer"), ("__smelt_dataview", "DataView"), ("__smelt_weakmap", "WeakMap"), ("__smelt_weakset", "WeakSet"), ("__smelt_file", "File"), ("__smelt_blob", "Blob"), ("__smelt_headers", "Headers"), ("__smelt_urlsearchparams", "URLSearchParams"), ("__smelt_url", "URL"), ("__smelt_crypto_key", "CryptoKey"), ("__smelt_request", "Request"), ("__smelt_response", "Response"), ("__smelt_textencoder", "TextEncoder"), ("__smelt_textdecoder", "TextDecoder"), ("__smelt_eventemitter", "EventEmitter"), ("__smelt_httpserver", "Server"), ("__smelt_incomingmessage", "IncomingMessage"), ("__smelt_serverresponse", "ServerResponse"), ("__smelt_formdata", "FormData"), ("__smelt_databasesync", "DatabaseSync"), ("__smelt_statementsync", "StatementSync"), ("__smelt_readablestream", "ReadableStream"), ("__smelt_domexception", "DOMException"), ("__smelt_intl_collator", "Intl.Collator"), ("__smelt_intl_displaynames", "Intl.DisplayNames"), ("__smelt_intl_durationformat", "Intl.DurationFormat"), ("__smelt_intl_listformat", "Intl.ListFormat"), ("__smelt_intl_locale", "Intl.Locale"), ("__smelt_intl_numberformat", "Intl.NumberFormat"), ("__smelt_intl_pluralrules", "Intl.PluralRules"), ("__smelt_intl_segmenter", "Intl.Segmenter"), ("__smelt_number", "Number"), ("__smelt_boolean", "Boolean"), ("__smelt_string", "String"), ("__smelt_symbol", "Symbol")].into_iter().find(|(marker, _)| map.contains_key(marker)).map(|(_, class)| class)
}
//~ Emit the single host constructor shared by direct and reflected construction.
/// Build a modeled host instance of `kind` from constructor arguments.
//~
//~ This is the one constructor for a host identity: the direct `new X(...)`
//~ lowering and the reflected `new getPrototypeOf(x).constructor(...)` path
//~ both call it, so their records are indistinguishable — which the clone
//~ specs rely on when they compare a reflectively-built clone against a
//~ directly-built expectation.
//~
//~ Kinds it builds structurally: the byte buffers (from a length, another byte
//~ buffer, or an element array), `DataView` (a window over a storage buffer),
//~ `Blob`/`File` (from `BlobPart`s), and `Error` (from `(message, { cause })`).
//~ Every other kind copies its argument with a fresh identity, which is what
//~ `new Map(m)` / `new Date(d)` / `new Number(n)` observably do.
fn smelt_reflected_construct(kind: &'static str, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    match kind {
        "Error" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError" | "TypeError" | "URIError" | "AggregateError" => { let mut fields = Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String(kind.into()))]); let mut it = args.into_iter(); let errors = if kind == "AggregateError" { it.next() } else { None }; if let Some(message) = it.next() { fields.push(("message".to_owned(), message)); } fields.push(("stack".to_owned(), SmeltUnknown::Undefined)); let cause = match it.next() { Some(SmeltUnknown::Object(options)) => options.get("cause").unwrap_or(SmeltUnknown::Undefined), _ => SmeltUnknown::Undefined }; fields.push(("cause".to_owned(), cause)); if let Some(errors) = errors { fields.push(("errors".to_owned(), errors)); } SmeltUnknown::Object(SmeltObject::new(fields)) }
        //~ Byte buffers: length | byte-backed source | element array.
        "arraybuffer" | "sharedarraybuffer" | "int8array" | "uint8array" | "uint8clampedarray" | "int16array" | "uint16array" | "int32array" | "uint32array" | "float32array" | "float64array" | "bigint64array" | "biguint64array" | "buffer" | "dataview" => { let marker = [("arraybuffer", "__smelt_arraybuffer"), ("sharedarraybuffer", "__smelt_sharedarraybuffer"), ("int8array", "__smelt_int8array"), ("uint8array", "__smelt_uint8array"), ("uint8clampedarray", "__smelt_uint8clampedarray"), ("int16array", "__smelt_int16array"), ("uint16array", "__smelt_uint16array"), ("int32array", "__smelt_int32array"), ("uint32array", "__smelt_uint32array"), ("float32array", "__smelt_float32array"), ("float64array", "__smelt_float64array"), ("bigint64array", "__smelt_bigint64array"), ("biguint64array", "__smelt_biguint64array"), ("buffer", "__smelt_buffer"), ("dataview", "__smelt_dataview")].into_iter().find(|(entry, _)| *entry == kind).map_or("__smelt_arraybuffer", |(_, marker)| marker); smelt_host_buffer_construct(marker, args) }
        //~ Blob / File: `new Blob(parts, { type })`, `new File(parts, name, { type, lastModified })`.
        "blob" | "file" => { let mut it = args.into_iter(); let parts = it.next().unwrap_or(SmeltUnknown::Undefined); let (name, options) = if kind == "file" { let name = it.next().map(|value| value.to_string()); (name, it.next()) } else { (None, it.next()) }; let option_field = |field: &str| match &options { Some(SmeltUnknown::Object(map)) => map.get(field), _ => None }; let blob_type = match option_field("type") { Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() }; let last_modified = match option_field("lastModified") { Some(SmeltUnknown::Number(value)) => Some(value), _ => None }; smelt_blob_record_from_parts(parts, blob_type, name, last_modified) }
        _ => smelt_fresh_identity(args.into_iter().next().unwrap_or(SmeltUnknown::Undefined)),
    }
}
//~ Emit the cached per-class prototype object.
//~ One cached prototype object per reflected class, so
//~ `Object.getPrototypeOf(a) === Object.getPrototypeOf(b)` holds for two values
//~ of the same class (`SmeltObject` `===` compares the stable `id`). Its
//~ `constructor` slot is the interned constructor value, so it is both callable
//~ and `===` the bare global reference.
thread_local! { static SMELT_MARKER_PROTOS: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }
fn smelt_reflected_prototype(class: String) -> SmeltUnknown { SMELT_MARKER_PROTOS.with(|cache| cache.borrow_mut().entry(class.clone()).or_insert_with(|| { let ctor = smelt_builtin_namespace(&class); SmeltUnknown::Object(SmeltObject::new(Vec::from([("constructor".to_owned(), ctor)]))) }).clone()) }
//~ Emit the interned per-name global constructor / namespace value.
/// The kind a global constructor name constructs, when it names a modeled host
/// identity.
//~ `None` for pure namespaces (`Math`, `JSON`), which are not callable.
fn smelt_builtin_construct_kind(name: &str) -> Option<&'static str> { [("ArrayBuffer", "arraybuffer"), ("SharedArrayBuffer", "sharedarraybuffer"), ("Int8Array", "int8array"), ("Uint8Array", "uint8array"), ("Uint8ClampedArray", "uint8clampedarray"), ("Int16Array", "int16array"), ("Uint16Array", "uint16array"), ("Int32Array", "int32array"), ("Uint32Array", "uint32array"), ("Float32Array", "float32array"), ("Float64Array", "float64array"), ("BigInt64Array", "bigint64array"), ("BigUint64Array", "biguint64array"), ("Buffer", "buffer"), ("DataView", "dataview"), ("WeakMap", "weakmap"), ("WeakSet", "weakset"), ("File", "file"), ("Blob", "blob"), ("DOMException", "domexception"), ("Intl.Collator", "intl_collator"), ("Intl.DisplayNames", "intl_displaynames"), ("Intl.DurationFormat", "intl_durationformat"), ("Intl.ListFormat", "intl_listformat"), ("Intl.Locale", "intl_locale"), ("Intl.NumberFormat", "intl_numberformat"), ("Intl.PluralRules", "intl_pluralrules"), ("Intl.Segmenter", "intl_segmenter"), ("Number", "number"), ("Boolean", "boolean"), ("String", "string"), ("Symbol", "symbol"), ("Date", "date"), ("Map", "map"), ("Set", "set"), ("RegExp", "regexp"), ("Error", "Error"), ("EvalError", "EvalError"), ("RangeError", "RangeError"), ("ReferenceError", "ReferenceError"), ("SyntaxError", "SyntaxError"), ("TypeError", "TypeError"), ("URIError", "URIError"), ("AggregateError", "AggregateError")].into_iter().find(|(class, _)| *class == name).map(|(_, kind)| kind) }
/// The interned value for a global builtin *name* used as a value.
//~
//~ JavaScript exposes one object per global name, so `Blob === Blob` and
//~ `blob.constructor === Blob` both hold. A record mints its identity on
//~ construction, so building one per reference would make both `false`;
//~ caching per name is what makes the two spellings meet. Names that also name
//~ a modeled host constructor get a `__smelt_call` slot, so `new Ctor(...)`
//~ through a captured reference constructs instead of answering `null`.
thread_local! { static SMELT_BUILTIN_NAMESPACES: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }
fn smelt_builtin_namespace(name: &str) -> SmeltUnknown { SMELT_BUILTIN_NAMESPACES.with(|cache| cache.borrow_mut().entry(name.to_owned()).or_insert_with(|| { let record = SmeltObject::new(Vec::from([("__smelt_builtin_namespace".to_owned(), SmeltUnknown::Bool(true)), ("name".to_owned(), SmeltUnknown::String(name.into()))])); if let Some(kind) = smelt_builtin_construct_kind(name) { record.insert("__smelt_call".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_reflected_construct(kind, args))))); } SmeltUnknown::Object(record) }).clone()) }
// @smelt:item-end
