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

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]



/// The reflected-construct *kind* string for a host constructor class name.
///
/// The kind is the class's registry marker with the shared `__smelt_` prefix
/// stripped (`"dataview"` for `__smelt_dataview`), which is the key the emitted
/// `smelt_reflected_construct` dispatches on. `None` for names that are not
/// modeled host objects, and for the ones that must not be reflectively
/// constructed as marker records.
pub(crate) fn reflected_construct_kind(class_name: &str) -> Option<&'static str> {
    // A class backed by a generated runtime type must not be reflectively
    // constructed as a marker record: the record would answer `instanceof`
    // correctly and then silently fail every method the program calls on it.
    // `StdlibClass::reflects_to_marker_record` is that question, asked of the
    // registry rather than of a list here — the list is what let `EventEmitter`
    // be excluded while the five classes registered alongside it were not.
    if smelt_stdlib::typescript_stdlib_class(class_name)
        .is_some_and(|class| !class.reflects_to_marker_record())
    {
        return None;
    }
    // `ReadableStream` carries a marker for identity but has no modeled surface
    // at all — not even a runtime type for the registry to recognize — so it
    // needs naming here. The same reasoning applies: a marker record must not
    // stand in for a surface that does not exist.
    if class_name == "ReadableStream" {
        return None;
    }
    // The host-object registry's own answer to the same question, for the
    // identities whose METHODS a subsystem owns rather than the record:
    // reflectively building an `AbortSignal` from its name alone would produce
    // a record with no listener list for `addEventListener` to append to. Asked
    // of the registry rather than named here, so a new such identity cannot
    // silently become reflectively constructible.
    if !smelt_stdlib::reflectively_constructible(class_name) {
        return None;
    }
    smelt_stdlib::host_object_marker(class_name).and_then(|marker| marker.strip_prefix("__smelt_"))
}
