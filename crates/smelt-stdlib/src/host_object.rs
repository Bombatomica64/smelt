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
}

/// Concise constructor for a host-object registry entry.
const fn host(class_name: &'static str, marker: &'static str) -> HostObject {
    HostObject {
        class_name,
        marker,
        is_boxed_primitive: false,
        to_string_tag: class_name,
        byte_buffer: None,
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
) -> HostObject {
    HostObject {
        class_name,
        marker,
        is_boxed_primitive: false,
        to_string_tag,
        byte_buffer: Some(role),
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
    ),
    byte_buffer(
        "SharedArrayBuffer",
        "__smelt_sharedarraybuffer",
        "SharedArrayBuffer",
        ByteBufferRole::Storage,
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
    ),
    byte_buffer(
        "DataView",
        "__smelt_dataview",
        "DataView",
        ByteBufferRole::View,
    ),
    host("WeakMap", "__smelt_weakmap"),
    host("WeakSet", "__smelt_weakset"),
    host("File", "__smelt_file"),
    host("Blob", "__smelt_blob"),
    // Fetch API `Request` host object. Source code (es-toolkit's `isPlainObject`
    // spec) constructs it only to probe host identity
    // (`isPlainObject(new Request('...')) === false`); none of its structural
    // surface is read, so it is a marker-only host object like `WeakMap` /
    // `DataView`. `instanceof Request` resolves through this marker.
    host("Request", "__smelt_request"),
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
        assert_eq!(views, ["Buffer", "DataView"]);
        assert_eq!(byte_buffer_role("ArrayBuffer"), Some(ByteBufferRole::Storage));
        assert_eq!(byte_buffer_role("Buffer"), Some(ByteBufferRole::View));
        assert_eq!(byte_buffer_role("WeakMap"), None);
        assert_eq!(byte_buffer_role("NotAHostObject"), None);
        assert_eq!(byte_buffer_host_objects().count(), 4);
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
