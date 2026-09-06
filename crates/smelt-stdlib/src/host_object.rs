//! Canonical registry for host-object identities.
//!
//! Several JavaScript host builtins and boxed primitive wrappers — `ArrayBuffer`,
//! `DataView`, `WeakMap`, `WeakSet`, `SharedArrayBuffer`, `File`, `Blob`,
//! `DOMException`, and the boxed `Number`/`Boolean`/`String`/`Symbol` wrappers —
//! have no useful structural shape that source code reads. They are constructed
//! and then only tested with `value instanceof X` (the `isWeakMap`/`isArrayBuffer`
//! family and the deep-clone dispatch). Their identity is known *statically* at
//! the construction site.
//!
//! Rather than let each host type invent its own `__smelt_<marker>` string in the
//! frontend construction path, the `instanceof` codegen path, and the runtime
//! for-in / structural-equality helpers independently, this module is the single
//! source of truth for that identity. All three consumers read from
//! [`HOST_OBJECTS`] so the construct side, the `instanceof` side, and the runtime
//! host-marker registry can never drift apart (a drift that previously left the
//! boxed-`Boolean` marker out of the runtime for-in filter).
//!
//! This is deliberately *not* a general dynamic boundary: each entry is a concrete
//! host identity with a known constructor. Genuine `unknown`/interop values still
//! flow through the tagged dynamic ABI; this registry only names the host objects
//! whose identity Smelt can resolve ahead of time.

/// How a host object relates to raw byte storage.
///
/// JavaScript splits the binary-data host objects into *storage* — an
/// `ArrayBuffer`/`SharedArrayBuffer` that owns bytes — and *views* over storage
/// (`DataView`, the typed arrays, and Node's `Buffer`). The distinction is
/// observable: `ArrayBuffer.isView(x)` answers `true` only for a view, and
/// es-toolkit's `isTypedArray` is exactly `ArrayBuffer.isView(x) && !(x
/// instanceof DataView)`.
///
/// Both roles are *byte-backed*: their modeled records carry a `bytes` list, so
/// `slice`/`subarray`, `byteLength`, and indexed element reads/writes all resolve
/// against real storage rather than answering `undefined`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ByteBufferRole {
    /// Owns the bytes (`ArrayBuffer`, `SharedArrayBuffer`).
    ///
    /// `ArrayBuffer.isView` is `false` for these.
    Storage,
    /// A view over bytes (`DataView`, Node `Buffer`).
    ///
    /// `ArrayBuffer.isView` is `true` for these.
    View,
}

/// The element type a typed-array view reads out of its byte storage.
///
/// A JavaScript typed array is *bytes plus an element type*: the same eight bytes
/// are two `Float32Array` elements, one `Float64Array` element, or eight
/// `Uint8Array` elements, and the same byte `0xff` reads as `255` through a
/// `Uint8Array` and `-1` through an `Int8Array`. Modeling the views without the
/// element type is what made every one of them report
/// `Object.prototype.toString` tag `[object Array]` with the *byte* count as its
/// `length`.
///
/// This enum is the real element typing: [`Self::byte_width`] gives the stride
/// that turns a byte count into an element count, and the runtime byte-buffer
/// prelude derives its little-endian decode/encode pair from the variant. It is
/// deliberately a closed set — the eleven views JavaScript defines — so a view's
/// identity is resolved statically at its construction site rather than guessed
/// from a value at runtime.
// Deliberately exhaustive (no `#[non_exhaustive]`): JavaScript defines exactly
// these eleven element types, and the codegen crate matches on the variant to
// derive its decode/encode pair — a wildcard arm there would silently emit a
// byte-wide codec for a newly added width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedArrayElement {
    /// Signed 8-bit integer (`Int8Array`).
    Int8,
    /// Unsigned 8-bit integer (`Uint8Array`, Node `Buffer`).
    Uint8,
    /// Unsigned 8-bit integer with saturating (not wrapping) writes
    /// (`Uint8ClampedArray`).
    Uint8Clamped,
    /// Signed 16-bit integer (`Int16Array`).
    Int16,
    /// Unsigned 16-bit integer (`Uint16Array`).
    Uint16,
    /// Signed 32-bit integer (`Int32Array`).
    Int32,
    /// Unsigned 32-bit integer (`Uint32Array`).
    Uint32,
    /// IEEE-754 single-precision float (`Float32Array`).
    Float32,
    /// IEEE-754 double-precision float (`Float64Array`).
    Float64,
    /// Signed 64-bit integer (`BigInt64Array`).
    BigInt64,
    /// Unsigned 64-bit integer (`BigUint64Array`).
    BigUint64,
}

impl TypedArrayElement {
    /// Bytes one element occupies — the view's `BYTES_PER_ELEMENT`.
    ///
    /// This is the stride that separates a typed array's `byteLength` from its
    /// `length`: a `Float64Array` over eight bytes has one element, a
    /// `Uint8Array` over the same eight bytes has eight.
    #[must_use]
    pub const fn byte_width(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 | Self::Uint8Clamped => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Float64 | Self::BigInt64 | Self::BigUint64 => 8,
        }
    }

    /// A stable lowercase tag naming this element type.
    ///
    /// The generated runtime dispatches its decode/encode on this string, so the
    /// emitter and the emitted code share one spelling rather than each
    /// re-deriving one from the variant.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Int8 => "int8",
            Self::Uint8 => "uint8",
            Self::Uint8Clamped => "uint8clamped",
            Self::Int16 => "int16",
            Self::Uint16 => "uint16",
            Self::Int32 => "int32",
            Self::Uint32 => "uint32",
            Self::Float32 => "float32",
            Self::Float64 => "float64",
            Self::BigInt64 => "bigint64",
            Self::BigUint64 => "biguint64",
        }
    }
}

/// A single host-object identity: the JavaScript constructor name, the dedicated
/// identity marker key stamped onto the constructed record, and whether the
/// identity denotes a boxed primitive wrapper.
///
/// The `marker` is the `__smelt_<name>` key that gives the constructed record its
/// distinct identity. `instanceof` resolves through this key, and the runtime
/// for-in / `Object.keys` filters hide records carrying it so a host object never
/// leaks its internal marker keys as enumerable properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostObject {
    /// The JavaScript constructor / class name (`"WeakMap"`, `"ArrayBuffer"`).
    pub class_name: &'static str,
    /// The dedicated identity marker key (`"__smelt_weakmap"`).
    pub marker: &'static str,
    /// Whether this identity is a boxed primitive wrapper (`new Number(1)`).
    ///
    /// Boxed wrappers are distinct from the same-named coercion calls
    /// (`Number(x)`), which lower to primitive values. The wrapper object has
    /// `typeof === "object"`, so the runtime `typeof` narrowing must miss it while
    /// `instanceof` still resolves through the marker.
    pub is_boxed_primitive: bool,
    /// The `Object.prototype.toString.call(x)` body for this identity.
    ///
    /// Defaults to `class_name`, which is right for every host object whose
    /// constructor name *is* its spec tag. Node's `Buffer` is the exception: it
    /// subclasses `Uint8Array`, so the platform reports `[object Uint8Array]`,
    /// and es-toolkit's equality/clone tag dispatch depends on that (its
    /// `isEqualWith` comments "Buffers are also treated as `[object Uint8Array]`s"
    /// and routes them through the typed-array arm).
    pub to_string_tag: &'static str,
    /// Byte storage role, when this host object is backed by bytes.
    ///
    /// `None` for the identity-only host objects (`WeakMap`, `Intl.*`, the boxed
    /// wrappers, ...) that have no byte surface.
    pub byte_buffer: Option<ByteBufferRole>,
    /// The element type this view reads out of its bytes, for the typed arrays.
    ///
    /// `Some` for the eleven typed-array views and for Node's `Buffer` (a
    /// `Uint8Array` subclass). `None` for `ArrayBuffer`/`SharedArrayBuffer`, which
    /// own bytes without interpreting them, for `DataView`, whose element type is
    /// chosen per `getFloat32`/`setInt16`/... call rather than by the view, and for
    /// every host object with no byte surface at all.
    ///
    /// A `Some` entry is what makes `length` the element count, an indexed read
    /// decode at the right width and signedness, and
    /// `new Float32Array(arrayBuffer)` see two elements where
    /// `new Uint8Array(arrayBuffer)` sees eight.
    pub element: Option<TypedArrayElement>,
}

/// Concise constructor for a host-object registry entry.
const fn host(class_name: &'static str, marker: &'static str) -> HostObject {
    HostObject {
        class_name,
        marker,
        is_boxed_primitive: false,
        to_string_tag: class_name,
        byte_buffer: None,
        element: None,
    }
}

/// Concise constructor for a byte-backed host-object registry entry.
///
/// `to_string_tag` is spelled out because the byte-buffer family is exactly where
/// constructor name and spec tag come apart (Node `Buffer` reports
/// `[object Uint8Array]`).
const fn byte_buffer(
    class_name: &'static str,
    marker: &'static str,
    to_string_tag: &'static str,
    role: ByteBufferRole,
    element: Option<TypedArrayElement>,
) -> HostObject {
    HostObject {
        class_name,
        marker,
        is_boxed_primitive: false,
        to_string_tag,
        byte_buffer: Some(role),
        element,
    }
}

/// Concise constructor for one of the eleven typed-array view registry entries.
///
/// Every typed array is a [`ByteBufferRole::View`] whose spec tag *is* its
/// constructor name (`[object Float32Array]`), so the only thing that varies
/// between them is the element type — which is exactly the distinction the old
/// shared-numeric-list model erased.
const fn typed_array(
    class_name: &'static str,
    marker: &'static str,
    element: TypedArrayElement,
) -> HostObject {
    HostObject {
        class_name,
        marker,
        is_boxed_primitive: false,
        to_string_tag: class_name,
        byte_buffer: Some(ByteBufferRole::View),
        element: Some(element),
    }
}

/// Concise constructor for a boxed-primitive-wrapper registry entry.
const fn boxed(class_name: &'static str, marker: &'static str) -> HostObject {
    HostObject {
        class_name,
        marker,
        is_boxed_primitive: true,
        to_string_tag: class_name,
        byte_buffer: None,
        element: None,
    }
}

/// The canonical set of host objects whose identity Smelt models with a dedicated
/// marker record.
///
/// Ordering is irrelevant; lookups are by `class_name` or `marker`. Adding a new
/// host identity here automatically wires it into the frontend construction
/// helper, the `instanceof` lowering, and the runtime host-marker registry.
pub const HOST_OBJECTS: &[HostObject] = &[
    byte_buffer(
        "ArrayBuffer",
        "__smelt_arraybuffer",
        "ArrayBuffer",
        ByteBufferRole::Storage,
        None,
    ),
    byte_buffer(
        "SharedArrayBuffer",
        "__smelt_sharedarraybuffer",
        "SharedArrayBuffer",
        ByteBufferRole::Storage,
        None,
    ),
    // The eleven `TypedArray` views. Each is a real *view identity*: its own
    // marker (so `Object.prototype.toString` reports `[object Float32Array]` and
    // two views over the same buffer are distinguishable), `ByteBufferRole::View`
    // (so `ArrayBuffer.isView`/`isTypedArray` answer `true`), and its own element
    // type (so `length` is the element count and an indexed read decodes at the
    // right width and signedness). Before these entries existed all eleven shared
    // one `Vec<f64>`, which reported tag `[object Array]` and the *byte* count as
    // `length` — indistinguishable from each other and from a plain `number[]`.
    typed_array("Int8Array", "__smelt_int8array", TypedArrayElement::Int8),
    typed_array("Uint8Array", "__smelt_uint8array", TypedArrayElement::Uint8),
    typed_array(
        "Uint8ClampedArray",
        "__smelt_uint8clampedarray",
        TypedArrayElement::Uint8Clamped,
    ),
    typed_array("Int16Array", "__smelt_int16array", TypedArrayElement::Int16),
    typed_array(
        "Uint16Array",
        "__smelt_uint16array",
        TypedArrayElement::Uint16,
    ),
    typed_array("Int32Array", "__smelt_int32array", TypedArrayElement::Int32),
    typed_array(
        "Uint32Array",
        "__smelt_uint32array",
        TypedArrayElement::Uint32,
    ),
    typed_array(
        "Float32Array",
        "__smelt_float32array",
        TypedArrayElement::Float32,
    ),
    typed_array(
        "Float64Array",
        "__smelt_float64array",
        TypedArrayElement::Float64,
    ),
    typed_array(
        "BigInt64Array",
        "__smelt_bigint64array",
        TypedArrayElement::BigInt64,
    ),
    typed_array(
        "BigUint64Array",
        "__smelt_biguint64array",
        TypedArrayElement::BigUint64,
    ),
    // Node's `Buffer` byte-buffer host object. es-toolkit constructs it
    // (`Buffer.from`/`Buffer.alloc`/`Buffer.concat`) and inspects it via
    // `Buffer.isBuffer(x)` / `value instanceof Buffer`, both of which resolve
    // through this marker (see `buffer_constructor_expression` and
    // `instance_of_text`). Modeled as a concrete byte-buffer record rather than
    // a shapeless dynamic value. `Buffer` subclasses `Uint8Array`, so its spec
    // tag is `[object Uint8Array]`, not `[object Buffer]`.
    byte_buffer(
        "Buffer",
        "__smelt_buffer",
        "Uint8Array",
        ByteBufferRole::View,
        Some(TypedArrayElement::Uint8),
    ),
    // `DataView` is byte-addressed on purpose: its element type is chosen per
    // `getFloat32`/`setInt16`/... call, not by the view, so it has no single
    // element type and its `length` is its byte length.
    byte_buffer(
        "DataView",
        "__smelt_dataview",
        "DataView",
        ByteBufferRole::View,
        None,
    ),
    host("WeakMap", "__smelt_weakmap"),
    host("WeakSet", "__smelt_weakset"),
    host("File", "__smelt_file"),
    host("Blob", "__smelt_blob"),
    // Fetch API `Request` host object. Source code (es-toolkit's `isPlainObject`
    // The concrete fetch types. Unlike the marker-only entries above, these have
    // real generated runtime types (`SmeltHeaders`, `SmeltUrlSearchParams`) and
    // their structural surface IS read — through typed methods, not through the
    // record. They are registered here for what happens at the erased boundary:
    // the marker is what makes the internal `entries` slot non-enumerable in
    // `for...in` (a real header list enumerates nothing) and what `instanceof`
    // resolves through. Their construction never takes the marker-only path.
    host("Headers", "__smelt_headers"),
    host("URLSearchParams", "__smelt_urlsearchparams"),
    // `Request` and `Response` moved up from the marker-only group when they
    // gained real runtime types (`SmeltRequest`/`SmeltResponse`). The marker
    // still does the same two jobs — `instanceof` resolves through it and
    // `isPlainObject(new Request('...'))` is `false` because of it — but it is
    // now stamped by the type's own erasure adapter rather than by a
    // marker-record constructor, so the concrete value is what the program
    // holds and the record exists only at the boundary.
    host("Request", "__smelt_request"),
    host("Response", "__smelt_response"),
    host("DOMException", "__smelt_domexception"),
    // ECMA-402 `Intl` namespace constructors. Source code constructs these only
    // to probe host identity (`isPlainObject(new Intl.Locale('en')) === false`);
    // none of their structural surface is read, so each is a marker-only host
    // object keyed by its full qualified path (the construction site is always
    // `new Intl.<Constructor>(...)`). `Intl.DateTimeFormat` and
    // `Intl.RelativeTimeFormat` are deliberately absent: the opaque-formatter
    // model claims them first and never stamps a marker (see
    // `intl_date_time_format_constructor_expression`).
    host("Intl.Collator", "__smelt_intl_collator"),
    host("Intl.DisplayNames", "__smelt_intl_displaynames"),
    host("Intl.DurationFormat", "__smelt_intl_durationformat"),
    host("Intl.ListFormat", "__smelt_intl_listformat"),
    host("Intl.Locale", "__smelt_intl_locale"),
    host("Intl.NumberFormat", "__smelt_intl_numberformat"),
    host("Intl.PluralRules", "__smelt_intl_pluralrules"),
    host("Intl.Segmenter", "__smelt_intl_segmenter"),
    boxed("Number", "__smelt_number"),
    boxed("Boolean", "__smelt_boolean"),
    boxed("String", "__smelt_string"),
    boxed("Symbol", "__smelt_symbol"),
];

/// Look up the host-object identity for a JavaScript constructor name.
///
/// Returns `None` for names that are not modeled host objects so callers can fall
/// through to their existing user-class / stdlib dispatch.
#[must_use]
pub fn host_object_by_class(class_name: &str) -> Option<&'static HostObject> {
    HOST_OBJECTS
        .iter()
        .find(|entry| entry.class_name == class_name)
}

/// Return the identity marker key for a modeled host constructor, or `None`.
///
/// Thin convenience over [`host_object_by_class`] for callers that only need the
/// marker string.
#[must_use]
pub fn host_object_marker(class_name: &str) -> Option<&'static str> {
    host_object_by_class(class_name).map(|entry| entry.marker)
}

/// Every host-object identity marker key, for the runtime host-marker registry.
///
/// The generated runtime uses this to hide host records from `for-in` /
/// `Object.keys` enumeration. It intentionally excludes markers owned by other
/// subsystems (dates, errors, regexps, abort controllers, namespaces) which the
/// runtime tracks through their own dedicated helpers.
pub fn host_object_markers() -> impl Iterator<Item = &'static str> {
    HOST_OBJECTS.iter().map(|entry| entry.marker)
}

/// Every byte-backed host object, with its byte-storage role.
///
/// Consumers: the runtime byte-buffer helpers (`slice`/`subarray`, indexed
/// element access, `ArrayBuffer.isView`) and the frontend construction sites that
/// stamp a `bytes` list onto the record. Driving all of them from this one
/// iterator is what keeps "which markers have bytes" from being restated per
/// call site.
pub fn byte_buffer_host_objects() -> impl Iterator<Item = (&'static str, ByteBufferRole)> {
    HOST_OBJECTS
        .iter()
        .filter_map(|entry| entry.byte_buffer.map(|role| (entry.marker, role)))
}

/// Return the byte-storage role of a host constructor, or `None` when the host
/// object has no byte surface.
#[must_use]
pub fn byte_buffer_role(class_name: &str) -> Option<ByteBufferRole> {
    host_object_by_class(class_name).and_then(|entry| entry.byte_buffer)
}

/// Every byte-backed host object that reads its bytes through a fixed element
/// type, as `(marker, element)`.
///
/// This is the table the runtime element codec is generated from: one
/// little-endian decode/encode pair per element type, selected by the record's
/// own marker. Driving it from the registry is what keeps "how wide is a
/// `Float64Array` element" from being restated in the `length` computation, the
/// indexed read, the indexed write, and the constructor.
pub fn typed_array_host_objects() -> impl Iterator<Item = (&'static str, TypedArrayElement)> {
    HOST_OBJECTS
        .iter()
        .filter_map(|entry| entry.element.map(|element| (entry.marker, element)))
}

/// Return the fixed element type of a host constructor's view, or `None`.
///
/// `None` covers both the non-byte-backed host objects and the byte-addressed
/// ones (`ArrayBuffer`, `DataView`), whose bytes carry no single element type.
#[must_use]
pub fn typed_array_element(class_name: &str) -> Option<TypedArrayElement> {
    host_object_by_class(class_name).and_then(|entry| entry.element)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registry marker is a distinct `__smelt_`-prefixed key. Distinctness
    /// is what makes `instanceof X` unambiguous, so a duplicate would silently
    /// collide two host identities.
    #[test]
    fn markers_are_unique_and_prefixed() {
        let mut seen = std::collections::HashSet::new();
        for entry in HOST_OBJECTS {
            assert!(
                entry.marker.starts_with("__smelt_"),
                "marker `{}` for `{}` must be `__smelt_`-prefixed",
                entry.marker,
                entry.class_name,
            );
            assert!(
                seen.insert(entry.marker),
                "duplicate host-object marker `{}`",
                entry.marker,
            );
        }
    }

    /// Class-name lookup and marker lookup agree for every registry entry, so the
    /// construction side (`by_class`) and the runtime registry (`markers`) stay in
    /// lock-step.
    #[test]
    fn lookups_round_trip() {
        for entry in HOST_OBJECTS {
            assert_eq!(host_object_by_class(entry.class_name), Some(entry));
            assert_eq!(host_object_marker(entry.class_name), Some(entry.marker));
        }
        assert_eq!(host_object_by_class("NotAHostObject"), None);
        assert_eq!(host_object_marker("NotAHostObject"), None);
    }

    /// The boxed primitive wrappers are exactly `Number`/`Boolean`/`String`/
    /// `Symbol`. Their objects have `typeof === "object"` so `instanceof` must
    /// resolve through the marker while `typeof` narrowing misses them.
    #[test]
    fn boxed_primitive_wrappers_are_classified() {
        let boxed = HOST_OBJECTS
            .iter()
            .filter(|entry| entry.is_boxed_primitive)
            .map(|entry| entry.class_name)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            boxed,
            ["Number", "Boolean", "String", "Symbol"]
                .into_iter()
                .collect(),
        );
    }

    /// The byte-backed host objects are exactly the binary-data family, split into
    /// the two roles `ArrayBuffer.isView` distinguishes. A regression here would
    /// silently change `isTypedArray`, since es-toolkit defines it as
    /// `ArrayBuffer.isView(x) && !(x instanceof DataView)`.
    #[test]
    fn byte_buffer_roles_are_classified() {
        let mut storage = Vec::new();
        let mut views = Vec::new();
        for entry in HOST_OBJECTS {
            match entry.byte_buffer {
                Some(ByteBufferRole::Storage) => storage.push(entry.class_name),
                Some(ByteBufferRole::View) => views.push(entry.class_name),
                None => {}
            }
        }
        assert_eq!(storage, ["ArrayBuffer", "SharedArrayBuffer"]);
        assert_eq!(
            views,
            [
                "Int8Array",
                "Uint8Array",
                "Uint8ClampedArray",
                "Int16Array",
                "Uint16Array",
                "Int32Array",
                "Uint32Array",
                "Float32Array",
                "Float64Array",
                "BigInt64Array",
                "BigUint64Array",
                "Buffer",
                "DataView",
            ],
        );
        assert_eq!(byte_buffer_role("ArrayBuffer"), Some(ByteBufferRole::Storage));
        assert_eq!(byte_buffer_role("Buffer"), Some(ByteBufferRole::View));
        assert_eq!(byte_buffer_role("Float32Array"), Some(ByteBufferRole::View));
        assert_eq!(byte_buffer_role("WeakMap"), None);
        assert_eq!(byte_buffer_role("NotAHostObject"), None);
        assert_eq!(byte_buffer_host_objects().count(), 15);
    }

    /// Every typed-array view carries its own element type and its own marker, so
    /// two views over the same bytes are distinguishable and each reports the
    /// element count — not the byte count — as its `length`. `Buffer` shares
    /// `Uint8`'s element type because it subclasses `Uint8Array`, while
    /// `ArrayBuffer` and `DataView` stay byte-addressed.
    #[test]
    fn typed_array_views_carry_their_element_type() {
        let elements = typed_array_host_objects().collect::<Vec<_>>();
        assert_eq!(elements.len(), 12, "eleven views plus Node `Buffer`");
        assert_eq!(
            typed_array_element("Float32Array"),
            Some(TypedArrayElement::Float32),
        );
        assert_eq!(
            typed_array_element("Uint8ClampedArray"),
            Some(TypedArrayElement::Uint8Clamped),
        );
        assert_eq!(typed_array_element("Buffer"), Some(TypedArrayElement::Uint8));
        assert_eq!(typed_array_element("ArrayBuffer"), None);
        assert_eq!(typed_array_element("DataView"), None);
        assert_eq!(typed_array_element("WeakMap"), None);
        // Byte widths are the platform's `BYTES_PER_ELEMENT`; these are what turn
        // a byte count into an element count.
        assert_eq!(TypedArrayElement::Int8.byte_width(), 1);
        assert_eq!(TypedArrayElement::Uint16.byte_width(), 2);
        assert_eq!(TypedArrayElement::Float32.byte_width(), 4);
        assert_eq!(TypedArrayElement::Float64.byte_width(), 8);
        assert_eq!(TypedArrayElement::BigUint64.byte_width(), 8);
        // Element tags are distinct, since the generated codec dispatches on them.
        let tags = elements
            .iter()
            .map(|(_marker, element)| element.tag())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(tags.len(), 11, "one tag per element type, `Buffer` reusing `uint8`");
    }

    /// Every typed-array class name in the shared frontend list has a registry
    /// entry, so the construction side, the `instanceof` side, and the runtime
    /// element codec cannot recognize different sets of views.
    #[test]
    fn typed_array_class_names_all_have_registry_entries() {
        for name in crate::TYPED_ARRAY_CLASS_NAMES {
            let entry = host_object_by_class(name)
                .unwrap_or_else(|| panic!("`{name}` must have a host-object registry entry"));
            assert_eq!(entry.byte_buffer, Some(ByteBufferRole::View));
            assert!(
                entry.element.is_some(),
                "`{name}` must carry an element type",
            );
            assert_eq!(entry.to_string_tag, name);
        }
    }

    /// Only Node's `Buffer` reports a spec tag that differs from its constructor
    /// name; every other host identity tags as itself. es-toolkit's `isEqualWith`
    /// and `cloneDeepWith` dispatch on the tag, so `Buffer` reporting
    /// `[object Buffer]` would fall off the end of their `switch` statements.
    #[test]
    fn only_buffer_overrides_its_to_string_tag() {
        for entry in HOST_OBJECTS {
            if entry.class_name == "Buffer" {
                assert_eq!(entry.to_string_tag, "Uint8Array");
            } else {
                assert_eq!(
                    entry.to_string_tag, entry.class_name,
                    "`{}` should tag as itself",
                    entry.class_name,
                );
            }
        }
    }

    /// `host_object_markers` yields the same set the entries carry, so the runtime
    /// for-in filter hides every host record's internal marker key — including the
    /// boxed-primitive markers that previously leaked as enumerable properties.
    #[test]
    fn markers_iterator_covers_boxed_primitives() {
        let markers = host_object_markers().collect::<std::collections::HashSet<_>>();
        for expected in ["__smelt_boolean", "__smelt_string", "__smelt_number"] {
            assert!(
                markers.contains(expected),
                "runtime host-marker set must include `{expected}`",
            );
        }
    }
}
