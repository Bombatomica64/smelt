//! Synthetic standard-library class recognition shared by frontends and codegen.

/// Standard-library class modeled with dedicated runtime support instead of
/// user-defined struct emission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum StdlibClass {
    /// JavaScript `Date`, represented by timestamp and date helper operations.
    Date,
    /// WHATWG `Headers`, backed by the generated concrete `SmeltHeaders`
    /// runtime type (a case-insensitive, insertion-ordered header multi-map).
    /// Reads keep their exact types: `get` is `string | null`, `has` is a
    /// boolean, `getSetCookie` is a string list.
    Headers,
    /// JavaScript `Map`, represented by dictionary HIR values.
    Map,
    /// Synthetic `RegExp` match result (`RegExp.exec` / `String.matchAll`),
    /// backed by the generated concrete `SmeltMatch` runtime type. Consumer
    /// reads (`m[0]`, `m.index`, `m.input`, `m.groups.name`) resolve to typed
    /// accessors on `SmeltMatch` instead of the erased `SmeltUnknown` path.
    Match,
    /// Synthetic named-capture-group accessor for `matchResult.groups`. It is
    /// the same underlying `SmeltMatch` value; a `.name` read on it resolves to
    /// the typed named-group accessor.
    MatchGroups,
    /// Remeda parser helper result class synthesized during TypeScript lowering.
    MatchFnResult,
    /// JavaScript `RegExp`, backed by the generated regex runtime shim.
    RegExp,
    /// JavaScript `Set`, represented by set HIR values.
    Set,
    /// WHATWG `URLSearchParams`, backed by the generated concrete
    /// `SmeltUrlSearchParams` runtime type (an ordered, case-sensitive
    /// name/value pair list with `application/x-www-form-urlencoded`
    /// serialization).
    UrlSearchParams,
    /// WHATWG `Response`, backed by the generated concrete `SmeltResponse`
    /// runtime type (a status line, a `SmeltHeaders`, and a single-use
    /// `SmeltBody`).
    Response,
    /// WHATWG `Request`, backed by the generated concrete `SmeltRequest`
    /// runtime type (a serialized URL, a method, a `SmeltHeaders`, and the same
    /// single-use `SmeltBody` a response holds).
    Request,
    /// `node:events` `EventEmitter`, backed by the generated concrete
    /// `SmeltEventEmitter` runtime type (an insertion-ordered listener list).
    EventEmitter,
    /// `node:http` `Server`, backed by the generated concrete `SmeltHttpServer`
    /// runtime type (a request handler, a bound address, and a shutdown
    /// signal).
    HttpServer,
    /// `node:http` `IncomingMessage`: the request half of one exchange. Holds
    /// an emitter by COMPOSITION rather than by inheritance, so `req.on('data',
    /// ..)` is the same operation on the same listener list that a plain
    /// `EventEmitter` receiver gets.
    IncomingMessage,
    /// `node:http` `ServerResponse`: the response half of one exchange, and the
    /// only modeled class with settable members (`res.statusCode = 200`).
    ServerResponse,
    /// WHATWG `ReadableStream`, as it appears at `Response.body` /
    /// `Request.body`: the BODY HANDLE itself, backed by the generated
    /// `SmeltBody`.
    ///
    /// The stream's own surface (`getReader`, `pipeTo`, `tee`, async iteration)
    /// is deliberately absent — a member call on one is a named blocker — and
    /// what IS modeled is the whole of what the idiom needs: a body is present
    /// or absent (`if (res.body)`, `res.body === null`), it can be handed back
    /// to a constructor (`new Response(res.body, init)`, which SHARES the
    /// handle exactly as passing the response itself does), and `bodyUsed`
    /// reports whether it has been read.
    ///
    /// Typing it as the body's TEXT was considered and rejected: `if (res.body)`
    /// would then answer `false` for an empty-STRING body, where JavaScript
    /// answers `true` because a stream object exists either way. Presence is
    /// exactly what the handle can answer honestly.
    ReadableStream,
    /// WHATWG `TextEncoder`, backed by the generated concrete `SmeltTextEncoder`
    /// runtime type. The spec fixes its encoding at UTF-8, so the value carries
    /// only a JS reference identity and its `encoding` data property.
    TextEncoder,
    /// WHATWG `TextDecoder`, backed by the generated concrete `SmeltTextDecoder`
    /// runtime type (an encoding label plus the reference identity).
    TextDecoder,
    /// A concrete byte view: the value `TextEncoder.encode` answers, backed by
    /// the generated `SmeltUint8Array` runtime type (a shared `Vec<u8>` with a
    /// JS reference identity).
    ///
    /// Reached only through the reserved synthetic name
    /// [`BYTE_ARRAY_CLASS_NAME`], never through the source spelling
    /// `Uint8Array`. That separation is deliberate: the eleven typed-array
    /// VIEWS are still byte-backed host records (`host_object.rs`), because a
    /// view's identity carries an element type, a byte offset, and a shared
    /// `ArrayBuffer` that reflective construction reads back at runtime. A
    /// source `Uint8Array` annotation therefore keeps its erased meaning, and a
    /// concrete byte view crosses into it through the ordinary
    /// `IntoSmeltUnknown` boundary adapter. Converting the whole view family to
    /// concrete Rust is recorded as demand, not done here.
    ByteArray,
    /// WHATWG `Blob`, backed by the generated concrete `SmeltBlob` runtime type
    /// (immutable bytes, a MIME type, and optional `File` metadata).
    Blob,
    /// WHATWG `FormData`, backed by the generated concrete `SmeltFormData`
    /// runtime type: an insertion-ordered, case-SENSITIVE name/value pair list
    /// whose values are `string | File`.
    ///
    /// The same pair-list shape as `Headers` and `URLSearchParams`, and it
    /// belongs after `Blob` for the value type: an entry value is a real
    /// two-arm union of MODELED types (`String` and the blob runtime type), so
    /// nothing about it has to be erased. Before `Blob` was concrete this would
    /// have needed a `SmeltUnknown` for the file arm.
    FormData,
    /// WHATWG `File`. Backed by the SAME `SmeltBlob` runtime type: the spec's
    /// `File` is a `Blob` plus exactly two data properties (`name`,
    /// `lastModified`), so a file is a blob whose name is present. Rust has no
    /// inheritance, and modeling the subtype as an optional-name blob is what
    /// makes `file instanceof Blob` free and what the erased record already
    /// does — it stamps `__smelt_file` on top of `__smelt_blob` rather than
    /// carrying a second shape.
    File,
}

impl StdlibClass {
    /// Return whether values of this class carry a `node:events` listener list.
    ///
    /// This is the "has an emitter" test that replaced "is the emitter" once
    /// `IncomingMessage` gained one: Node's `IncomingMessage` extends
    /// `EventEmitter`, so `req.on('data', ..)` must reach the same operation as
    /// `emitter.on('data', ..)`. Answering it from the registry — rather than
    /// from a name comparison at each dispatch site — is what keeps the
    /// frontend's method dispatch, the result typing, and codegen's receiver
    /// rendering agreeing about which receivers are emitters.
    #[must_use]
    pub const fn has_event_emitter(self) -> bool {
        matches!(self, Self::EventEmitter | Self::IncomingMessage)
    }

    /// Return whether values of this class cross the dynamic boundary through
    /// their runtime type's OWN `IntoSmeltUnknown` adapter.
    ///
    /// True for every class backed by a generated runtime type that keeps its
    /// state somewhere other than declared fields — behind a shared cell, in a
    /// closure, in a byte vector. That is the whole set of them, and the reason
    /// they need their own adapter is that the generic struct erasure reads
    /// DECLARED FIELDS and stamps `__smelt_class`: for a prelude type it finds
    /// no fields, so it produces a record with no state and no host identity.
    ///
    /// This drives two things that must not disagree. The emitter routes an
    /// erasure of such a value to `.into_smelt_unknown()` instead of the
    /// declared-field builder, and `instance_of_text` answers `instanceof` on
    /// an erased value through the marker that same adapter stamps. Both used
    /// to consult their own hand-maintained lists of class names, which is how
    /// the two came apart for the six classes that had neither.
    #[must_use]
    pub const fn erases_through_adapter(self) -> bool {
        matches!(
            self,
            Self::Headers
                | Self::UrlSearchParams
                | Self::Response
                | Self::Request
                | Self::Blob
                | Self::File
                | Self::RegExp
                | Self::Match
                | Self::ByteArray
                | Self::TextEncoder
                | Self::TextDecoder
                | Self::EventEmitter
                | Self::HttpServer
                | Self::IncomingMessage
                | Self::ServerResponse
                | Self::FormData
        )
    }

    /// Return whether a REFLECTED `new <this class>()` may build a marker
    /// record instead of running the class's own constructor.
    ///
    /// Reflected construction — `new (Object.getPrototypeOf(x).constructor)()`,
    /// which es-toolkit's `clone` uses — has only a class NAME to work from, so
    /// for a host class it builds a record carrying that class's identity
    /// marker. That is right for a class whose values ARE records, and wrong
    /// for one backed by a generated runtime type: the record would answer
    /// `instanceof` correctly and then silently fail every method called on it.
    ///
    /// `Blob` and `File` are the exception among the runtime-typed classes, and
    /// deliberately: `blob_prelude` emits a reflected constructor that shares
    /// `smelt_blob_record` with the erasure, so the record it builds is exactly
    /// the one a real blob erases to and the two cannot drift.
    ///
    /// Asking the registry replaces two hand-maintained exclusion lists in
    /// `reflection_prelude`, which is how five of the six classes registered in
    /// round 11 would otherwise have silently become reflectively constructible
    /// as records the moment they gained a marker.
    #[must_use]
    pub const fn reflects_to_marker_record(self) -> bool {
        !self.erases_through_adapter() || matches!(self, Self::Blob | Self::File)
    }

    /// Return whether an ERASED value can be converted BACK into this class's
    /// concrete Rust representation.
    ///
    /// The other half of [`Self::erases_through_adapter`], and deliberately a
    /// separate question: erasing needs only `IntoSmeltUnknown`, while
    /// recovering needs a `SmeltFromUnknown` that can honestly produce a value.
    /// Conflating them is what made the round-10 narrowing rule exclude classes
    /// whose `instanceof` was perfectly answerable.
    ///
    /// It is what lets `x instanceof Headers` NARROW an erased `x`: the
    /// narrowing is emitted exactly where it can be materialized, so the rule
    /// stays general — "there is a sound checked cast for this class" — rather
    /// than becoming a list of spellings. A class without a recovery keeps its
    /// erased type across the guard, because inventing a conversion for it
    /// would be guessing at a representation.
    ///
    /// `HttpServer` is the one class that erases but does NOT recover. A server
    /// is a live listening socket with a handler closure and a tokio shutdown
    /// sender; a record cannot describe one, and the origin registry only helps
    /// for a record that came from an erasure in this same thread. Rather than
    /// fabricate a server that is not listening, narrowing one stays a named
    /// blocker — the honest answer, and the same reason its `instanceof` is
    /// still worth answering: identity is knowable where reconstruction is not.
    ///
    /// `Blob` and `File` both answer true and share one adapter, since they
    /// share one runtime type (see [`Self::File`]).
    #[must_use]
    pub const fn narrows_from_erased(self) -> bool {
        self.erases_through_adapter() && !matches!(self, Self::HttpServer)
    }

    /// Return whether values of this class are the generated `SmeltBlob` type.
    ///
    /// `Blob` and `File` share one Rust representation (see [`Self::File`]), and
    /// several sites — the emitted Rust type, the erasure adapter, the
    /// pay-for-use gate — need "is this the blob runtime type" rather than
    /// "which of the two spellings is it". Asking the registry keeps those
    /// sites from each re-deriving the pairing.
    #[must_use]
    pub const fn is_blob_runtime_type(self) -> bool {
        matches!(self, Self::Blob | Self::File)
    }

    /// The string parameters a listener for `event` receives, when this class
    /// publishes a known schema for it.
    ///
    /// A plain `EventEmitter`'s events are open — any name, any listener
    /// signature — which is the dynamic boundary its listener store is built
    /// on. A MODELED class is different: `IncomingMessage` emits exactly the
    /// events `node:http` documents, and `data` always carries one chunk while
    /// `end` always carries nothing. That is static knowledge, so the source's
    /// own listener keeps a real parameter type (`(chunk: string) => void`)
    /// instead of taking an erased value and coercing it by hand. Only the
    /// registration adapter still erases, and it is the boundary either way.
    ///
    /// `None` means "no schema" and the listener stays erased, which is the
    /// answer for every event of a plain emitter and for the events of a
    /// modeled class whose payload is not a string (`error` carries an error
    /// object; nothing reads it yet, so nothing claims to know its shape).
    #[must_use]
    pub fn event_listener_string_params(self, event: &str) -> Option<usize> {
        match (self, event) {
            (Self::IncomingMessage, "data") => Some(1),
            (Self::IncomingMessage, "end" | "close") => Some(0),
            _ => None,
        }
    }
}

/// Reserved synthetic class name for a `RegExp` match result value.
///
/// The name is not writable in user TypeScript (double-underscore prefix), so
/// it never collides with a source class; it exists only to carry the concrete
/// match shape through the internal type system.
pub const MATCH_CLASS_NAME: &str = "__SmeltMatch";

/// Reserved synthetic class name for `matchResult.groups` named-group access.
pub const MATCH_GROUPS_CLASS_NAME: &str = "__SmeltMatchGroups";

/// Reserved synthetic class name for a concrete byte view.
///
/// `TextEncoder.encode` answers a value of this class. The name is not writable
/// in user TypeScript (double-underscore prefix), so it never collides with a
/// source class, and — unlike the spelling `Uint8Array` — it never collides with
/// the byte-backed host record the typed-array views still use. See
/// [`StdlibClass::ByteArray`].
pub const BYTE_ARRAY_CLASS_NAME: &str = "__SmeltUint8Array";

/// Return the stdlib class modeled by a TypeScript class type name.
///
/// Codegen consults this instead of comparing class symbol names inline so
/// stdlib class identities stay in one registry.
#[must_use]
pub fn typescript_stdlib_class(name: &str) -> Option<StdlibClass> {
    match name {
        "Date" => Some(StdlibClass::Date),
        "Headers" => Some(StdlibClass::Headers),
        "Map" => Some(StdlibClass::Map),
        MATCH_CLASS_NAME => Some(StdlibClass::Match),
        MATCH_GROUPS_CLASS_NAME => Some(StdlibClass::MatchGroups),
        "MatchFnResult" => Some(StdlibClass::MatchFnResult),
        "RegExp" => Some(StdlibClass::RegExp),
        "Set" => Some(StdlibClass::Set),
        "URLSearchParams" => Some(StdlibClass::UrlSearchParams),
        "Response" => Some(StdlibClass::Response),
        "Request" => Some(StdlibClass::Request),
        "EventEmitter" => Some(StdlibClass::EventEmitter),
        // The `node:http` names as the module exports them, so a source that
        // annotates its handler (`(req: IncomingMessage, res: ServerResponse)`)
        // resolves to the same modeled classes the untyped handler is given.
        // A user class of the same name shadows these, as it does for every
        // other entry here.
        "Server" => Some(StdlibClass::HttpServer),
        "IncomingMessage" => Some(StdlibClass::IncomingMessage),
        "ServerResponse" => Some(StdlibClass::ServerResponse),
        "FormData" => Some(StdlibClass::FormData),
        "ReadableStream" => Some(StdlibClass::ReadableStream),
        "TextEncoder" => Some(StdlibClass::TextEncoder),
        "TextDecoder" => Some(StdlibClass::TextDecoder),
        BYTE_ARRAY_CLASS_NAME => Some(StdlibClass::ByteArray),
        "Blob" => Some(StdlibClass::Blob),
        "File" => Some(StdlibClass::File),
        _ => None,
    }
}

/// The JavaScript typed-array view constructor names, in a stable order.
///
/// These are the eleven `TypedArray` element-view classes (`Uint8Array`,
/// `Int8Array`, ..., `BigInt64Array`, `BigUint64Array`). Each has a full
/// [`crate::host_object`] registry entry — its own identity marker, spec tag, and
/// element type — and this shared name list is the single source of truth every
/// frontend/codegen site consults to decide whether a constructor / bare
/// identifier / `instanceof` target names a typed array. Keeping the set here —
/// rather than re-spelling the `matches!(...)` arm in each site — stops the
/// construction side, the value-resolution side, and the `instanceof` side from
/// drifting apart.
pub const TYPED_ARRAY_CLASS_NAMES: [&str; 11] = [
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
];

/// Return whether a class name is one of the JavaScript typed-array views.
///
/// Consulted by the TypeScript frontend (constructor lowering, bare-value
/// resolution, `instanceof` targeting) and codegen so the eleven typed-array
/// names are recognized from one place. `BigInt64Array` / `BigUint64Array` are
/// included even though their elements are `BigInt` values: they are eight-byte
/// integer views like any other, and Smelt models a JavaScript `BigInt` as a
/// number, so they share both the recognizer and the element codec with the
/// numeric views.
#[must_use]
pub fn is_typed_array_class_name(name: &str) -> bool {
    TYPED_ARRAY_CLASS_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact stdlib class names resolve to their registry identity.
    #[test]
    fn recognizes_stdlib_class_names() {
        assert_eq!(typescript_stdlib_class("Date"), Some(StdlibClass::Date));
        assert_eq!(
            typescript_stdlib_class("Headers"),
            Some(StdlibClass::Headers)
        );
        assert_eq!(typescript_stdlib_class("Map"), Some(StdlibClass::Map));
        assert_eq!(
            typescript_stdlib_class(MATCH_CLASS_NAME),
            Some(StdlibClass::Match)
        );
        assert_eq!(
            typescript_stdlib_class(MATCH_GROUPS_CLASS_NAME),
            Some(StdlibClass::MatchGroups)
        );
        assert_eq!(
            typescript_stdlib_class("MatchFnResult"),
            Some(StdlibClass::MatchFnResult)
        );
        assert_eq!(typescript_stdlib_class("RegExp"), Some(StdlibClass::RegExp));
        assert_eq!(typescript_stdlib_class("Set"), Some(StdlibClass::Set));
        assert_eq!(
            typescript_stdlib_class("URLSearchParams"),
            Some(StdlibClass::UrlSearchParams)
        );
    }

    /// User class names never resolve to a stdlib identity.
    #[test]
    fn rejects_user_class_names() {
        for name in ["Regexp", "RegExpLike", "Dates", "HashMap", "MyClass"] {
            assert_eq!(typescript_stdlib_class(name), None);
        }
    }

    /// Every typed-array view constructor is recognized, including the `BigInt`
    /// views, while plain `Array` and lookalikes are not.
    #[test]
    fn recognizes_typed_array_class_names() {
        for name in TYPED_ARRAY_CLASS_NAMES {
            assert!(
                is_typed_array_class_name(name),
                "expected `{name}` to be recognized as a typed array"
            );
        }
        assert!(is_typed_array_class_name("BigUint64Array"));
        for name in ["Array", "Uint8", "TypedArray", "Float16Array", "MyUint8Array"] {
            assert!(
                !is_typed_array_class_name(name),
                "did not expect `{name}` to be recognized as a typed array"
            );
        }
    }
}
