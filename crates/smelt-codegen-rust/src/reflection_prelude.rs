//! Runtime prelude for host-identity *reflection*.
//!
//! JavaScript clone idioms reach a host constructor two ways. Directly:
//!
//! ```js
//! new DataView(value.buffer.slice(0), value.byteOffset, value.byteLength)
//! ```
//!
//! and reflectively, off the value's own prototype:
//!
//! ```js
//! const Constructor = Object.getPrototypeOf(obj).constructor;
//! return new Constructor(obj.buffer.slice(0));
//! ```
//!
//! es-toolkit uses the first in `cloneDeepWith` and the second in `clone`, and
//! its specs compare the two results against each other. So the reflected
//! constructor cannot be an approximation of the direct one — they have to be the
//! same code. This module emits that one constructor
//! ([`runtime_symbols::host::REFLECTED_CONSTRUCT`]), the marker→kind
//! discriminator that selects it, the cached per-kind prototype object it hangs
//! off, and the interned per-name constructor value that `x.constructor` and a
//! bare `Blob` reference both resolve to.
//!
//! Three tables drive everything, all derived here rather than restated at call
//! sites:
//!
//! * [`REFLECTED_MARKERS`] — the markers whose *prototype* exposes a reflected
//!   constructor. Deliberately narrow: giving a marker a reflected prototype
//!   turns `Object.getPrototypeOf(x)` from an opaque string sentinel into a real
//!   object, which changes what `Object.create(proto)` copies, so a marker joins
//!   this list only when source code actually constructs through its prototype.
//! * [`CONSTRUCTOR_CLASS_MARKERS`] — every marker whose record answers a
//!   `.constructor` read. This one is broad: reading `x.constructor` is a pure
//!   query and cannot perturb the prototype chain.
//! * The shared host-object registry, for the byte-buffer and Blob/File kinds the
//!   constructor knows how to build.

use smelt_stdlib::runtime_symbols::{byte_buffer, host};

use crate::rust::CodeWriter;

/// Markers, in match order, whose prototype exposes a reflected constructor.
///
/// Each entry is `(marker, kind, class_name)`. Order matters: a `File` record
/// also carries `__smelt_blob`, so the more specific marker must win.
///
/// This is intentionally *not* the whole host registry. Handing a marker a
/// reflected prototype object replaces its `"__smelt_proto:object"` string
/// sentinel, and `Object.create(thatObject)` then copies the prototype's members
/// into the fresh object behind `__smelt_proto:` keys — visible to the structural
/// equality that library specs compare clones with. Only the identities whose
/// constructors source code actually reaches reflectively belong here.
const REFLECTED_MARKERS: &[(&str, &str, &str)] = &[
    ("__smelt_date", "date", "Date"),
    ("__smelt_map", "map", "Map"),
    ("__smelt_set", "set", "Set"),
    ("__smelt_regexp", "regexp", "RegExp"),
    ("__smelt_dataview", "dataview", "DataView"),
    // Node `Buffer`: its clone path is the typed-array one,
    // `new (Object.getPrototypeOf(buf).constructor)(buf.length)` followed by an
    // indexed element copy, so the prototype has to expose a constructor that
    // allocates a `Buffer` of that byte length.
    ("__smelt_buffer", "buffer", "Buffer"),
    ("__smelt_error", "error", "Error"),
    ("__smelt_file", "file", "File"),
    ("__smelt_number", "number", "Number"),
    ("__smelt_boolean", "boolean", "Boolean"),
];

/// [`REFLECTED_MARKERS`] plus one entry per typed-array view.
///
/// The eleven views join the reflected-prototype set for the same reason Node's
/// `Buffer` is in the const list above: the lodash-compatible clone of a typed
/// array is `new (Object.getPrototypeOf(view).constructor)(view.buffer.slice(0),
/// view.byteOffset, view.length)`, so the view's prototype has to expose a
/// constructor that rebuilds a view of that same identity. They are derived from
/// the registry rather than restated so a new view cannot be added in one place
/// and forgotten in the other.
fn reflected_markers() -> Vec<(&'static str, &'static str, &'static str)> {
    REFLECTED_MARKERS
        .iter()
        .copied()
        .chain(
            smelt_stdlib::HOST_OBJECTS
                .iter()
                .filter(|entry| smelt_stdlib::is_typed_array_class_name(entry.class_name))
                .filter_map(|entry| {
                    entry
                        .marker
                        .strip_prefix("__smelt_")
                        .map(|kind| (entry.marker, kind, entry.class_name))
                }),
        )
        .collect()
}

/// Non-registry markers whose records answer a `.constructor` read.
///
/// The registry markers are appended to these; see
/// [`constructor_class_markers`].
const CONSTRUCTOR_CLASS_MARKERS: &[(&str, &str)] = &[
    ("__smelt_date", "Date"),
    ("__smelt_map", "Map"),
    ("__smelt_set", "Set"),
    ("__smelt_regexp", "RegExp"),
    ("__smelt_error", "Error"),
];

/// Every `(marker, class_name)` pair whose record answers `.constructor`.
///
/// Registry markers come last but are matched in registry order, which puts
/// `__smelt_file` ahead of `__smelt_blob` so a `File` reports `File`.
fn constructor_class_markers() -> Vec<(&'static str, &'static str)> {
    CONSTRUCTOR_CLASS_MARKERS
        .iter()
        .copied()
        .chain(
            smelt_stdlib::HOST_OBJECTS
                .iter()
                .map(|entry| (entry.marker, entry.class_name)),
        )
        .collect()
}

/// The reflected-construct *kind* string for a host constructor class name.
///
/// The kind is the class's registry marker with the shared `__smelt_` prefix
/// stripped, which is exactly what `smelt_reflected_marker_kind` hands back for a
/// record of that identity — so a class name and a live record agree on which
/// constructor to run. `None` for names that are not modeled host objects.
pub fn reflected_construct_kind(class_name: &str) -> Option<&'static str> {
    smelt_stdlib::host_object_marker(class_name).and_then(|marker| marker.strip_prefix("__smelt_"))
}

/// Render `[(marker, value), ...]` as a Rust array literal of string pairs.
fn pair_array(pairs: &[(&str, &str)]) -> String {
    let entries = pairs
        .iter()
        .map(|(left, right)| format!("({left:?}, {right:?})"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

/// Emit the host-reflection runtime helpers into the generated prelude.
pub fn emit(writer: &mut CodeWriter) {
    emit_marker_kind(writer);
    emit_constructor_class(writer);
    emit_construct(writer);
    emit_prototype(writer);
    emit_builtin_namespace(writer);
}

/// Emit the marker→constructor-class discriminator for prototype reflection.
///
/// The answer is the name of the class whose prototype the record reflects, so
/// the prototype's `constructor` slot can be the interned global of that very
/// name. A marker whose stored VALUE is itself a modeled constructor name is how
/// Smelt records a *subclass* identity (`__smelt_error: "AggregateError"`), and
/// that name wins over the marker table's base class — otherwise every error
/// subclass would reflect the base `Error` constructor and a reflected rebuild
/// would lose the subclass's own construction shape.
fn emit_marker_kind(writer: &mut CodeWriter) {
    let table = pair_array(
        &reflected_markers()
            .iter()
            .map(|(marker, _kind, class)| (*marker, *class))
            .collect::<Vec<_>>(),
    );
    writer.line("/// Discriminate the class whose prototype a host-marker record reflects.");
    writer.line("/// `None` for plain objects, arrays and class instances, which keep their");
    writer.line("/// opaque `\"__smelt_proto:*\"` string sentinels.");
    writer.line("fn smelt_reflected_marker_class(map: &SmeltObject) -> Option<String> {");
    writer.line(format!(
        "    let (marker, class) = {table}.into_iter().find(|(marker, _)| map.contains_key(marker))?;"
    ));
    writer.line("    if let Some(SmeltUnknown::String(name)) = map.get(marker) { if smelt_builtin_construct_kind(&name).is_some() { return Some(name.to_string()); } }");
    writer.line("    Some(class.to_owned())");
    writer.line("}");
}

/// Emit the marker→constructor-class lookup behind `x.constructor`.
fn emit_constructor_class(writer: &mut CodeWriter) {
    let table = pair_array(&constructor_class_markers());
    writer.line("/// The class name a marker-bearing record's `.constructor` read resolves to.");
    writer.line("///");
    writer.line("/// `blob.constructor === Blob` holds in JavaScript, and es-toolkit's clone");
    writer.line("/// specs assert exactly that on a cloned host object. Unmarked records answer");
    writer.line("/// `None`, so a plain object's `.constructor` stays `undefined` — which is what");
    writer.line("/// makes two plain objects compare as equal instances.");
    writer.line(format!(
        "fn {class_of}(map: &SmeltObject) -> Option<&'static str> {{ {table}.into_iter().find(|(marker, _)| map.contains_key(marker)).map(|(_, class)| class) }}",
        class_of = host::MARKER_CONSTRUCTOR_CLASS,
    ));
}

/// Emit the single host constructor shared by direct and reflected construction.
fn emit_construct(writer: &mut CodeWriter) {
    // Every byte-backed identity — the storage buffers, the eleven typed-array
    // views, Node `Buffer`, and `DataView` — is built by the one byte-buffer
    // constructor, which resolves the element type from the marker. `DataView`
    // used to need its own arm only because that constructor could not yet
    // window a separate storage buffer or record a `buffer`/`byteOffset`.
    let byte_kinds = smelt_stdlib::byte_buffer_host_objects()
        .filter_map(|(marker, _role)| marker.strip_prefix("__smelt_").map(|kind| (kind, marker)))
        .collect::<Vec<_>>();
    let plain_byte_kinds = byte_kinds
        .iter()
        .map(|(kind, _marker)| format!("{kind:?}"))
        .collect::<Vec<_>>()
        .join(" | ");
    let kind_marker_table = pair_array(&byte_kinds);

    writer.line("/// Build a modeled host instance of `kind` from constructor arguments.");
    writer.line("///");
    writer.line("/// This is the one constructor for a host identity: the direct `new X(...)`");
    writer.line("/// lowering and the reflected `new getPrototypeOf(x).constructor(...)` path");
    writer.line("/// both call it, so their records are indistinguishable — which the clone");
    writer.line("/// specs rely on when they compare a reflectively-built clone against a");
    writer.line("/// directly-built expectation.");
    writer.line("///");
    writer.line("/// Kinds it builds structurally: the byte buffers (from a length, another byte");
    writer.line("/// buffer, or an element array), `DataView` (a window over a storage buffer),");
    writer.line("/// `Blob`/`File` (from `BlobPart`s), and `Error` (from `(message, { cause })`).");
    writer.line("/// Every other kind copies its argument with a fresh identity, which is what");
    writer.line("/// `new Map(m)` / `new Date(d)` / `new Number(n)` observably do.");
    writer.line(format!(
        "fn {construct}(kind: &'static str, args: Vec<SmeltUnknown>) -> SmeltUnknown {{",
        construct = host::REFLECTED_CONSTRUCT,
    ));
    writer.line("    match kind {");
    // Error: `new Constructor(message, { cause })`.
    let error_kinds = smelt_stdlib::ERROR_CLASS_NAMES
        .iter()
        .map(|class| format!("{class:?}"))
        .collect::<Vec<_>>()
        .join(" | ");
    writer.line(format!(
        "        {error_kinds} => {{ let mut fields = Vec::from([(\"__smelt_error\".to_owned(), SmeltUnknown::String(kind.into()))]); let mut it = args.into_iter(); let errors = if kind == \"AggregateError\" {{ it.next() }} else {{ None }}; if let Some(message) = it.next() {{ fields.push((\"message\".to_owned(), message)); }} fields.push((\"stack\".to_owned(), SmeltUnknown::Undefined)); let cause = match it.next() {{ Some(SmeltUnknown::Object(options)) => options.get(\"cause\").unwrap_or(SmeltUnknown::Undefined), _ => SmeltUnknown::Undefined }}; fields.push((\"cause\".to_owned(), cause)); if let Some(errors) = errors {{ fields.push((\"errors\".to_owned(), errors)); }} SmeltUnknown::Object(SmeltObject::new(fields)) }}"
    ));
    // Byte buffers: length | byte-backed source | element array.
    writer.line(format!(
        "        {plain_byte_kinds} => {{ let marker = {kind_marker_table}.into_iter().find(|(entry, _)| *entry == kind).map_or(\"__smelt_arraybuffer\", |(_, marker)| marker); {construct}(marker, args) }}",
        construct = byte_buffer::CONSTRUCT,
    ));
    // Blob / File: `new Blob(parts, { type })`, `new File(parts, name, { type, lastModified })`.
    writer.line(format!(
        "        \"blob\" | \"file\" => {{ let mut it = args.into_iter(); let parts = it.next().unwrap_or(SmeltUnknown::Undefined); let (name, options) = if kind == \"file\" {{ let name = it.next().map(|value| value.to_string()); (name, it.next()) }} else {{ (None, it.next()) }}; let option_field = |field: &str| match &options {{ Some(SmeltUnknown::Object(map)) => map.get(field), _ => None }}; let blob_type = match option_field(\"type\") {{ Some(SmeltUnknown::String(text)) => text.to_string(), _ => String::new() }}; let last_modified = match option_field(\"lastModified\") {{ Some(SmeltUnknown::Number(value)) => Some(value), _ => None }}; {blob}(parts, blob_type, name, last_modified) }}",
        blob = host::BLOB_RECORD_FROM_PARTS,
    ));
    writer.line("        _ => smelt_fresh_identity(args.into_iter().next().unwrap_or(SmeltUnknown::Undefined)),");
    writer.line("    }");
    writer.line("}");

}

/// Emit the cached per-class prototype object.
fn emit_prototype(writer: &mut CodeWriter) {
    writer.line("/// One cached prototype object per reflected class, so");
    writer.line("/// `Object.getPrototypeOf(a) === Object.getPrototypeOf(b)` holds for two values");
    writer.line("/// of the same class (`SmeltObject` `===` compares the stable `id`). Its");
    writer.line("/// `constructor` slot is the interned constructor value, so it is both callable");
    writer.line("/// and `===` the bare global reference.");
    writer.line("thread_local! { static SMELT_MARKER_PROTOS: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
    writer.line(format!(
        "fn smelt_reflected_prototype(class: String) -> SmeltUnknown {{ SMELT_MARKER_PROTOS.with(|cache| cache.borrow_mut().entry(class.clone()).or_insert_with(|| {{ let ctor = {namespace}(&class); SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"constructor\".to_owned(), ctor)]))) }}).clone()) }}",
        namespace = host::BUILTIN_NAMESPACE,
    ));
}

/// Emit the interned per-name global constructor / namespace value.
fn emit_builtin_namespace(writer: &mut CodeWriter) {
    let constructible = smelt_stdlib::HOST_OBJECTS
        .iter()
        .filter_map(|entry| {
            reflected_construct_kind(entry.class_name)
                .map(|kind| format!("({class:?}, {kind:?})", class = entry.class_name))
        })
        .chain(
            reflected_markers()
                .iter()
                .filter(|(_marker, _kind, class)| {
                    smelt_stdlib::host_object_by_class(class).is_none()
                        && !smelt_stdlib::is_error_class_name(class)
                })
                .map(|(_marker, kind, class)| format!("({class:?}, {kind:?})")),
        )
        // Every modeled `Error` constructor is its own kind, so a record that
        // carries a subclass marker (`__smelt_error: "AggregateError"`) rebuilds
        // through THAT constructor's signature rather than the base `Error` one.
        .chain(
            smelt_stdlib::ERROR_CLASS_NAMES
                .iter()
                .map(|class| format!("({class:?}, {class:?})")),
        )
        .collect::<Vec<_>>()
        .join(", ");

    writer.line("/// The kind a global constructor name constructs, when it names a modeled host");
    writer.line("/// identity. `None` for pure namespaces (`Math`, `JSON`), which are not callable.");
    writer.line(format!(
        "fn smelt_builtin_construct_kind(name: &str) -> Option<&'static str> {{ [{constructible}].into_iter().find(|(class, _)| *class == name).map(|(_, kind)| kind) }}"
    ));
    writer.line("/// The interned value for a global builtin *name* used as a value.");
    writer.line("///");
    writer.line("/// JavaScript exposes one object per global name, so `Blob === Blob` and");
    writer.line("/// `blob.constructor === Blob` both hold. A record mints its identity on");
    writer.line("/// construction, so building one per reference would make both `false`;");
    writer.line("/// caching per name is what makes the two spellings meet. Names that also name");
    writer.line("/// a modeled host constructor get a `__smelt_call` slot, so `new Ctor(...)`");
    writer.line("/// through a captured reference constructs instead of answering `null`.");
    writer.line("thread_local! { static SMELT_BUILTIN_NAMESPACES: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
    writer.line(format!(
        "fn {namespace}(name: &str) -> SmeltUnknown {{ SMELT_BUILTIN_NAMESPACES.with(|cache| cache.borrow_mut().entry(name.to_owned()).or_insert_with(|| {{ let record = SmeltObject::new(Vec::from([(\"__smelt_builtin_namespace\".to_owned(), SmeltUnknown::Bool(true)), (\"name\".to_owned(), SmeltUnknown::String(name.into()))])); if let Some(kind) = smelt_builtin_construct_kind(name) {{ record.insert(\"__smelt_call\".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok({construct}(kind, args))))); }} SmeltUnknown::Object(record) }}).clone()) }}",
        namespace = host::BUILTIN_NAMESPACE,
        construct = host::REFLECTED_CONSTRUCT,
    ));
}
