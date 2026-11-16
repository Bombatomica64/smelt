//! What a program demands of the runtime, computed once from its MIR.
//!
//! Every decision that a runtime family is emitted, and every rendering choice
//! that depends on one being there, reads this module's [`RuntimeDemand`]. It is
//! built in two steps:
//!
//! 1. **One complete scan** ([`RuntimeDemand::of`]) walks the type table, the
//!    declarations and every statement and terminator of every function and
//!    closure, recording the facts it sees as root [`Need`]s.
//! 2. **The `requires` graph** ([`REQUIRES`]) closes those roots: a family that
//!    is written against another family declares the edge once, in one table,
//!    and the closure is what the emitter reads.
//!
//! A runtime item that references a family it did not declare surfaces as a
//! compile error in a generated crate (E0425/E0433), which is the failure the
//! table exists to prevent. `undefined_runtime_symbols` in the crate root checks
//! the finished text for exactly that in debug builds.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the emitter shards"
)]

mod op_needs;

use smelt_hir::Type;
use smelt_mir::{
    BasicBlock, Callee, Constant, LocalDecl, LocalId, Mir, Operand, Place, Read, Rvalue, Statement,
    Terminator,
};
use smelt_stdlib::{BackendDependency, StdlibClass};

use crate::reachable_types::ReachableTypes;
use crate::stdlib::{module_hash_set_key_safe, stdlib_class_of_class_symbol};

/// Declares [`Need`] and [`Need::ALL`] from one list, so the list of every
/// variant cannot miss one: a variant added to the enum is in `ALL` by
/// construction, which is what the per-need closure test and the bitset bound
/// rely on.
macro_rules! declare_needs {
    ($($(#[$meta:meta])* $variant:ident,)*) => {
        /// A runtime family, or a backend crate, a program can demand.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Need {
            $($(#[$meta])* $variant,)*
        }

        impl Need {
            /// Every need, in declaration order.
            pub(crate) const ALL: &[Self] = &[$(Self::$variant),*];
        }
    };
}

declare_needs! {
    /// The erased `SmeltUnknown` carrier and everything written against it.
    Unknown,
    /// An operation whose answer is erased though nothing has to spell `unknown`:
    /// a dynamic `JSON.parse`, a member read through a bound. The carrier counts
    /// as present in the program (see `FunctionEmitter::find_type_id`).
    ErasedAnswer,
    /// The identity-bearing `SmeltList<T>`.
    SmeltList,
    /// The `smelt_next_object_id` reference-identity counter.
    ObjectIds,
    /// The insertion-ordered primitive-keyed `SmeltPrimSet<T>`.
    PrimSet,
    /// The panic route: a throw that cannot propagate a `Result`, and its recovery.
    PanicRoute,
    /// JavaScript `bigint`.
    BigInt,
    /// `IntoSmeltUnknown`/`SmeltFromUnknown` for tuples of three or more
    /// elements, which only a program with such a tuple type needs.
    WideTuple,
    /// JavaScript `symbol` values and property-key records (`SmeltSymbol`,
    /// `SmeltPropKey`).
    Symbol,
    /// The iterator-result enum a list's `next()` answers with.
    IteratorResult,
    /// Generator state machines.
    Generator,
    /// The timer queue and promise task queue.
    Timers,
    /// The `{ status, value | reason }` record of `Promise.allSettled`.
    AllSettled,
    /// `Date.now()` override state.
    DateNow,
    /// `getTimezoneOffset()` override state.
    DateTimezoneOffset,
    /// The `Date` host class.
    Date,
    /// JavaScript `Math.round`.
    MathRound,
    /// The `parseInt`/`parseFloat` prefix parsers.
    ParseNumber,
    /// `smelt_string_to_number`, ECMA-262 `StringToNumber` over a Rust `&str`:
    /// the JavaScript string runtime converts through it, and a Python program
    /// calls it on its own `String`s.
    StringToNumber,
    /// The `JsString` UTF-16 string type and its borrowed `JsStr` view.
    JsString,
    /// Python's `repr`, for a container `print` shows.
    PyRepr,
    /// `smelt_erased_pattern_call`: `.match`/`.search` on an erased receiver.
    ErasedPatternCall,
    /// Mutable closure captures shared through the capture registry.
    SharedCaptures,
    /// `encodeURI` and friends.
    UriEncode,
    /// A throwing URI decoder.
    UriDecode,
    /// `JSON.stringify` with an indent.
    JsonIndent,
    /// `smelt_json_stringify_value`: a `JSON.stringify` typed `string |
    /// undefined` (its value may have no JSON text).
    JsonStringifyValue,
    /// The fallible `JSON.parse` adapter.
    JsonParse,
    /// `String.prototype.localeCompare`.
    LocaleCompare,
    /// `Headers`.
    Headers,
    /// `Response`.
    Response,
    /// `Request`.
    Request,
    /// The shared `Request`/`Response` body.
    Body,
    /// A read of a member of a body-holding host value.
    BodyHostMethod,
    /// `FormData`.
    FormData,
    /// `Blob` and `File`.
    Blob,
    /// `URLSearchParams`.
    UrlSearchParams,
    /// `URL`.
    Url,
    /// A throwing `new URL(..)` or `href` write.
    UrlThrowing,
    /// `TextEncoder`.
    TextEncoder,
    /// `TextDecoder`.
    TextDecoder,
    /// The byte-array family: typed arrays, `ArrayBuffer`, `DataView`.
    ByteArray,
    /// The `DataView` class.
    DataView,
    /// A fallible `DataView` accessor.
    DataViewAccess,
    /// `crypto.getRandomValues`.
    CryptoRandomValues,
    /// `crypto.subtle.digest`.
    CryptoDigest,
    /// `CryptoKey` and HMAC.
    CryptoKey,
    /// `node:path`.
    NodePath,
    /// `node:crypto` hashing.
    NodeHash,
    /// `node:events`.
    EventEmitter,
    /// `node:http`.
    HttpServer,
    /// A fetch-style server built on `node:http`.
    FetchServer,
    /// `node:sqlite`.
    Sqlite,
    /// `atob`/`btoa`.
    Base64,
    /// Python's `/`, `//` and `%` helpers.
    PythonDivision,
    /// Python's raising `d[k]` read.
    PythonDictRead,
    /// Python's raising `xs[i]` read.
    PythonListRead,
    /// A raised Python exception, `SmeltPyException`.
    PyException,
    /// Python's arbitrary-precision `int`, `SmeltPyInt`.
    PyInt,
    /// Python `str` methods with CPython's own rules (`py_str.rs`).
    PyStrMethods,
    /// Python's format-spec mini-language (`py_format.rs`).
    PyFormat,
    /// `SmeltBinding`, the storage of a source binding that can be read
    /// before its first assignment.
    Binding,
    /// The `ReferenceError` of a JavaScript binding read in its temporal dead zone.
    BindingTdz,
    /// The `UnboundLocalError`/`NameError` of a Python name read unbound.
    BindingPy,
    /// The shared, ordered `SmeltRecord` dict and its property store.
    Record,
    /// A Python program: dicts keep plain insertion order.
    PyDictOrder,
    /// A class read as a value.
    ClassValue,
    /// `globalThis` as a value.
    GlobalObject,
    /// A record write through an erased target.
    UnknownAssign,
    /// A run-time `new RegExp(p)`.
    RegExpCompile,
    /// Vitest mocks and asymmetric matchers.
    VitestMock,
    /// The `fast-check` model.
    FastCheck,
    /// `structuredClone`.
    StructuredClone,
    /// A `typeof` the backend lowers.
    Typeof,
    /// `Array.prototype.concat` with a spreadable argument.
    ConcatSpread,
    /// `Object.defineProperty`/`defineProperties`.
    DefineProperties,
    /// `Object.setPrototypeOf`.
    SetPrototypeOf,
    /// A host constructor held as a value (`SmeltHostConstructor`).
    HostConstructor,
    /// A class prototype reached reflectively (`Object.getPrototypeOf`,
    /// `Object.create`, `Object.setPrototypeOf`, `C.prototype`): class
    /// prototypes carry their methods (`class_proto::class_prototype_methods_fn`).
    ClassPrototypeMethods,
    /// The dynamically scoped `this` channel.
    ThisChannel,
    /// A read of that channel.
    ThisRead,
    /// JavaScript `[[Construct]]` through a function value.
    Construct,
    /// `instanceof` through a function value.
    InstanceOfValue,
    /// A reassigned host global.
    HostOverride,
    /// Host values retained in the erased record's origin slot.
    HostOrigins,
    /// Live erased views of typed `Map`s and `Set`s (`smelt_live_container`).
    LiveContainers,
    /// Live erased views of typed arrays (`SmeltLiveList`).
    LiveArrays,
    /// The erased `SmeltErasedFunction` adapter.
    ErasedFunction,
    /// The erased generic list aliases.
    ///
    /// A list of a type parameter is a cheap necessary condition, not an exact
    /// one: a parameter in scope everywhere it appears renders as a Rust generic
    /// and never reaches a helper.
    ErasedListAlias,
    /// `Option<SmeltUnknown>` property reads.
    OptionalUnknownRead,
    /// A record field stored as an accessor.
    Accessors,
    /// Python's number formatting.
    PythonNumberFormat,
    /// `Number.prototype`'s formatting methods (`toFixed`, `toPrecision`,
    /// `toExponential`, `toString(radix)`).
    NumberMethods,
    /// The `tokio` crate.
    DepTokio,
    /// The `genawaiter` crate.
    DepGenawaiter,
    /// The `reqwest` crate.
    DepReqwest,
    /// The `serde_json` crate.
    DepSerdeJson,
    /// The Python `re` crates (`regex`, `fancy-regex`).
    DepRegex,
    /// The `regress` crate: what the JavaScript `SmeltRegExp` runtime is gated on.
    DepRegress,
    /// A regex op both languages lower to. Its dialect is its body's source
    /// language: [`RuntimeDemand::scan_body_in`] resolves it to [`Need::DepRegex`]
    /// in a Python body and [`Need::DepRegress`] in a TypeScript one, and it is
    /// never left set.
    RegexOps,
    /// The `rand` crate.
    DepRand,
    /// The `chrono` crate.
    DepChrono,
    /// The `chrono-tz` crate.
    DepChronoTz,
    /// The `url` crate.
    DepUrl,
    /// The `unicode-normalization` crate.
    DepUnicodeNormalization,
    /// The `uuid` crate.
    DepUuid,
    /// The SHA crates.
    DepSha,
    /// The `hmac` crate.
    DepHmac,
    /// The `md-5` crate.
    DepMd5,
    /// The `indexmap` crate, which the insertion-ordered dictionary renders as.
    DepIndexMap,
}

/// [`RuntimeDemand`] holds one bit per [`Need`] in a `u128`: a 129th need would
/// alias bit 0 in a release build, so the bound is checked where the enum is.
const _: () = assert!(
    Need::ALL.len() <= 128,
    "Need outgrew the u128 demand bitset"
);

/// The declared dependency graph: a family on the left is written against every
/// family on the right, so demanding it demands them.
///
/// Two families that only ever appear *inside* another block's emission and are
/// decided per use site, `Typeof` and `ConcatSpread`, have no edge to `Unknown`
/// on purpose: a `typeof` over a concrete value or a `concat` of concrete lists
/// is rendered inline, needs no helper, and must not pull the erased carrier into
/// a wholly typed program. Their helpers exist for an erased operand, which the
/// type table already demands.
const REQUIRES: &[(Need, &[Need])] = &[
    // The carrier's own surface.
    (
        Need::Unknown,
        &[
            Need::SmeltList,
            Need::Record,
            Need::PanicRoute,
            Need::DepRegress,
            Need::DepChrono,
            Need::DepChronoTz,
            Need::DepTokio,
            // The erased string payload is a `JsString`.
            Need::JsString,
            // An erased number answers its `Number.prototype` members.
            Need::NumberMethods,
        ],
    ),
    (Need::SmeltList, &[Need::ObjectIds]),
    // `smelt_js_string_to_number` trims and checks a `JsStr`, then converts
    // through the `&str` rule.
    (Need::JsString, &[Need::StringToNumber]),
    (Need::PyRepr, &[Need::PythonNumberFormat]),
    (Need::ErasedPatternCall, &[Need::Unknown, Need::DepRegress]),
    (Need::PyException, &[Need::PyRepr]),
    (Need::Record, &[Need::ObjectIds]),
    (Need::PythonDivision, &[Need::PyException]),
    // An `int` raises Python's arithmetic errors and prints through `repr`.
    (Need::PyInt, &[Need::PyException, Need::PyRepr]),
    (Need::PyStrMethods, &[Need::PyException]),
    // `format` of an `int` reads a `SmeltPyInt`, of a `float` its `repr`.
    (
        Need::PyFormat,
        &[Need::PyException, Need::PyInt, Need::PythonNumberFormat],
    ),
    (Need::PythonDictRead, &[Need::PyException]),
    // A checked binding read raises through the error its language throws.
    (Need::BindingTdz, &[Need::Binding, Need::Unknown]),
    (Need::BindingPy, &[Need::Binding, Need::PyException]),
    // The checked read indexes the identity-bearing list it is handed.
    (Need::PythonListRead, &[Need::PyException, Need::SmeltList]),
    (Need::PrimSet, &[Need::ObjectIds]),
    (Need::DepRegress, &[Need::ObjectIds, Need::JsString]),
    (Need::Date, &[Need::ObjectIds, Need::HostOrigins]),
    // These are emitted inside the carrier's block, or carry its values.
    (Need::IteratorResult, &[Need::Unknown]),
    (
        Need::Generator,
        &[Need::Unknown, Need::IteratorResult, Need::DepGenawaiter],
    ),
    (Need::Timers, &[Need::Unknown]),
    (Need::AllSettled, &[Need::Unknown]),
    (Need::StructuredClone, &[Need::Unknown]),
    (Need::DefineProperties, &[Need::Unknown]),
    (Need::SetPrototypeOf, &[Need::Unknown]),
    // It erases to the interned constructor object.
    (Need::HostConstructor, &[Need::Unknown]),
    // The prototype methods read their receiver from the `this` channel.
    (
        Need::ClassPrototypeMethods,
        &[Need::Unknown, Need::ThisChannel],
    ),
    (Need::ThisChannel, &[Need::Unknown]),
    (Need::ThisRead, &[Need::ThisChannel]),
    (Need::Construct, &[Need::Unknown, Need::ThisChannel]),
    (Need::InstanceOfValue, &[Need::Unknown]),
    (Need::ClassValue, &[Need::Unknown]),
    (Need::VitestMock, &[Need::Unknown]),
    (Need::FastCheck, &[Need::Unknown, Need::VitestMock]),
    (Need::HostOverride, &[Need::Unknown]),
    (Need::ErasedFunction, &[Need::Unknown]),
    // A throw of a branded error record crosses the erased channel even where
    // the value the operation answers is concrete.
    (Need::UriDecode, &[Need::Unknown, Need::UriEncode]),
    (Need::Base64, &[Need::Unknown]),
    (Need::RegExpCompile, &[Need::Unknown]),
    (Need::UrlThrowing, &[Need::Unknown, Need::Url]),
    (
        Need::DataViewAccess,
        &[Need::Unknown, Need::ByteArray, Need::DataView],
    ),
    (Need::BigInt, &[Need::Unknown]),
    // A symbol erases to the carrier's `Symbol` tag and its key form is the
    // reserved spelling the carrier's objects store it under.
    (Need::Symbol, &[Need::Unknown]),
    // The host families.
    (
        Need::Response,
        &[Need::Headers, Need::Body, Need::ObjectIds],
    ),
    (
        Need::Request,
        &[Need::Headers, Need::Body, Need::ObjectIds, Need::DepUrl],
    ),
    (Need::Headers, &[Need::ObjectIds, Need::HostOrigins]),
    (Need::Body, &[Need::ObjectIds]),
    (Need::BodyHostMethod, &[Need::JsonParse]),
    (Need::JsonParse, &[Need::DepSerdeJson]),
    (Need::ErasedAnswer, &[Need::Unknown]),
    (
        Need::HttpServer,
        &[
            Need::Headers,
            Need::EventEmitter,
            Need::Timers,
            Need::ObjectIds,
            Need::HostOrigins,
        ],
    ),
    (
        Need::FetchServer,
        &[Need::HttpServer, Need::Response, Need::Request],
    ),
    // A listener is stored and compared as an erased callable, and the emitter
    // record converts through the carrier's `SmeltFromUnknown`.
    (
        Need::EventEmitter,
        &[Need::Unknown, Need::ObjectIds, Need::HostOrigins],
    ),
    (Need::Url, &[Need::UrlSearchParams, Need::HostOrigins]),
    (
        Need::UrlSearchParams,
        &[Need::ObjectIds, Need::HostOrigins, Need::DepUrl],
    ),
    (
        Need::FormData,
        &[Need::Blob, Need::ObjectIds, Need::HostOrigins, Need::DepUrl],
    ),
    (Need::Blob, &[Need::ByteArray, Need::ObjectIds]),
    (
        Need::TextEncoder,
        &[Need::ByteArray, Need::ObjectIds, Need::HostOrigins],
    ),
    (
        Need::TextDecoder,
        &[Need::ByteArray, Need::ObjectIds, Need::HostOrigins],
    ),
    (Need::ByteArray, &[Need::ObjectIds, Need::HostOrigins]),
    (Need::CryptoRandomValues, &[Need::ByteArray]),
    (Need::CryptoDigest, &[Need::ByteArray, Need::DepSha]),
    (
        Need::CryptoKey,
        &[Need::Unknown, Need::ByteArray, Need::DepSha, Need::DepHmac],
    ),
    (
        Need::NodeHash,
        &[
            Need::Unknown,
            Need::ByteArray,
            Need::ObjectIds,
            Need::HostOrigins,
            Need::DepSha,
            Need::DepMd5,
        ],
    ),
    (
        Need::Sqlite,
        &[Need::Unknown, Need::ByteArray, Need::HostOrigins],
    ),
];

/// Proof that the closed demand holds a need: only [`RuntimeDemand::require`]
/// constructs one, so a runtime use that takes it cannot be written for a
/// family the program does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Demanded {
    /// The need demanded; private, so only [`RuntimeDemand::require`] can
    /// construct the proof.
    _need: Need,
}

/// Proof that the program carries the erased runtime carrier: the token every
/// carrier template takes (see `FunctionEmitter::carrier_format`), obtained
/// from [`RuntimeDemand::carrier`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct CarrierUse {
    /// Private, so only [`RuntimeDemand::carrier`] can construct one.
    _proof: (),
}

/// The closed set of everything one program demands of the runtime.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeDemand {
    /// One bit per [`Need`], closed over [`REQUIRES`].
    needs: u128,
}

impl RuntimeDemand {
    /// Scans `mir` once and closes what it found over the `requires` graph.
    #[must_use]
    pub(crate) fn of(
        mir: &Mir,
        reference_classes: &std::collections::HashSet<smelt_hir::Symbol>,
    ) -> Self {
        let mut demand = Self::default();
        let reachable = ReachableTypes::of(mir);
        demand.scan_declarations(mir);
        demand.scan_types(mir, &reachable);
        for function in &mir.functions {
            demand.scan_body_in(
                function.language,
                mir,
                &function.locals,
                &function.blocks,
                function.return_ty,
            );
            // An async body returns its awaited type in MIR and is emitted as
            // a `SmeltFuture` of it, which lives in the carrier's block.
            if function.is_async {
                demand.add(Need::Unknown);
            }
            if function.language == smelt_hir::Language::Python {
                demand.add(Need::PyDictOrder);
                // A Python string is a Rust `String`, numeric text converts
                // through the `&str` rule directly.
                demand.add(Need::StringToNumber);
            }
        }
        // A class's `__str__`/`__repr__` is called through its `SmeltPyRepr`
        // impl, which cannot answer an error; a raising one re-raises through
        // the panic route.
        for class in &mir.classes {
            for protocol in &class.protocols {
                if let smelt_mir::MirClassProtocol::Str { method }
                | smelt_mir::MirClassProtocol::Repr { method } = protocol
                {
                    demand.add(Need::PyRepr);
                    if usize::try_from(method.0)
                        .ok()
                        .and_then(|index| mir.functions.get(index))
                        .is_some_and(|function| function.can_throw)
                    {
                        demand.add(Need::PanicRoute);
                    }
                }
            }
        }
        for closure in &mir.closures {
            demand.scan_body_in(
                closure.language,
                mir,
                &closure.locals,
                &closure.blocks,
                closure.return_ty,
            );
        }
        // A record value at a `Record<K, V>` slot crosses through its erasure
        // (`FunctionEmitter::rendered_at_type`: the dictionary is read off
        // the record's erased object, or is a view of it), so the conversion
        // needs the erased runtime even in a program with no erased type.
        crate::classify::for_each_record_flow(mir, |_, slot| {
            if matches!(mir.types.get(slot), Some(Type::Dict(_, _))) {
                demand.add(Need::Unknown);
            }
        });
        // A deep `==` over reference records reads them through their views
        // (`classify::deep_compares_a_reference_shape`), which only a program
        // with the erased runtime has.
        if crate::classify::deep_compares_a_reference_shape(mir, reference_classes) {
            demand.add(Need::Unknown);
        }
        demand.close();
        // With the carrier, every concrete union in the type table gets its
        // enum (`emitter::union::emit_union_definitions`: the emitter derives
        // unions no position holds), and each enum is written against what its
        // arms are. So the types inside every union count too.
        if demand.has(Need::Unknown) {
            let with_unions = ReachableTypes::of_with_unions(mir);
            demand.scan_types(mir, &with_unions);
            demand.close();
        }
        // Which container a dict renders as depends on whether the carrier is
        // present, so this reads the closed demand and adds a leaf need.
        if reachable.ids().any(|id| {
            matches!(mir.types.get(id), Some(Type::Dict(key, _)) if crate::emitter::dict_renders_as_index_map(mir, &demand, *key))
        }) {
            demand.add(Need::DepIndexMap);
        }
        demand
    }

    /// The demand of a program that needs exactly `needs`, closed over
    /// [`REQUIRES`]: what the per-need closure check renders.
    #[cfg(test)]
    pub(crate) fn closed_over(needs: &[Need]) -> Self {
        let mut demand = Self::default();
        for need in needs {
            demand.add(*need);
        }
        demand.close();
        demand
    }

    /// Records `need` alone, unclosed, for a test of the bit layout.
    #[cfg(test)]
    pub(crate) const fn add_for_test(&mut self, need: Need) {
        self.add(need);
    }

    /// Whether the program demands `need`.
    #[must_use]
    pub(crate) const fn has(&self, need: Need) -> bool {
        self.needs & bit(need) != 0
    }

    /// The proof that the program demands `need`, which the emitter needs to
    /// write that family's runtime text.
    ///
    /// # Errors
    ///
    /// When the scan did not predict `need`: the use would name runtime items
    /// the crate does not carry, so it fails here, at the emit site that asked,
    /// rather than as an undefined symbol in the finished crate.
    pub(crate) fn require(&self, need: Need) -> Result<Demanded, crate::EmitError> {
        if self.has(need) {
            Ok(Demanded { _need: need })
        } else {
            Err(crate::EmitError::new(format!(
                "internal: the emitter wrote a use of the `{need:?}` runtime family, which the runtime demand scan did not predict"
            )))
        }
    }

    /// The proof that the program carries the erased runtime carrier, which
    /// [`crate::emitter`]'s carrier templates take. See [`Self::require`].
    ///
    /// # Errors
    ///
    /// When the scan did not predict an erased value.
    pub(crate) fn carrier(&self) -> Result<CarrierUse, crate::EmitError> {
        self.require(Need::Unknown)
            .map(|_| CarrierUse { _proof: () })
    }

    /// Records `need` as demanded.
    const fn add(&mut self, need: Need) {
        self.needs |= bit(need);
    }

    /// Withdraws `need`.
    const fn remove(&mut self, need: Need) {
        self.needs &= !bit(need);
    }

    /// Records what one body of `language` demands, resolving its shared
    /// regex ops ([`Need::RegexOps`]) to that language's regex dialect.
    fn scan_body_in(
        &mut self,
        language: smelt_hir::Language,
        mir: &Mir,
        locals: &[LocalDecl],
        blocks: &[BasicBlock],
        return_ty: smelt_hir::TypeId,
    ) {
        self.scan_body(mir, locals, blocks, return_ty);
        self.scan_operands(language, mir, locals, blocks);
        if self.has(Need::RegexOps) {
            self.remove(Need::RegexOps);
            self.add(if language == smelt_hir::Language::Python {
                Need::DepRegex
            } else {
                Need::DepRegress
            });
        }
    }

    /// Records what the operands a body reads demand beyond their types.
    ///
    /// A constant operand has no type id, so [`ReachableTypes`] cannot see it:
    /// a symbol literal is a `SmeltSymbol`, a bigint literal a `SmeltBigInt`, a
    /// Python `int` literal the arbitrary-precision `int` runtime,
    /// and a JavaScript string literal a `JsString` (a Python one is a plain
    /// `String`). A class method read as a value (`const f = obj.method`) is a
    /// closure over the receiver that the erased call ABI invokes with its
    /// arguments as erased values ([`class_method_read_as_value`]), and a class
    /// value read with a run-time key (`raw[key]`) is looked up on its erased
    /// view ([`class_read_with_a_runtime_key`]).
    fn scan_operands(
        &mut self,
        language: smelt_hir::Language,
        mir: &Mir,
        locals: &[LocalDecl],
        blocks: &[BasicBlock],
    ) {
        let mut constant = |operand: &Operand| self.scan_operand(language, mir, locals, operand);
        for block in blocks {
            for statement in &block.statements {
                statement.reads(|read| {
                    if let Read::Operand(operand) = read {
                        constant(operand);
                    }
                });
            }
            if let Some(terminator) = &block.terminator {
                terminator.reads(&mut constant);
            }
        }
    }

    /// Records what one operand demands (see [`Self::scan_operands`]),
    /// including the index operand of an indexed place it reads
    /// (`iterable[Symbol.iterator]` reads a symbol constant there).
    fn scan_operand(
        &mut self,
        language: smelt_hir::Language,
        mir: &Mir,
        locals: &[LocalDecl],
        operand: &Operand,
    ) {
        let value = match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                if class_method_read_as_value(mir, locals, place)
                    || class_read_with_a_runtime_key(mir, locals, place)
                {
                    self.add(Need::Unknown);
                }
                match place {
                    Place::Index { base, index, .. } => {
                        if indexes_a_bigint_view(mir, locals, *base) {
                            self.add(Need::BigInt);
                        }
                        self.scan_operand(language, mir, locals, index);
                    }
                    Place::Global {
                        projection: smelt_mir::GlobalProjection::Index { index, .. },
                        ..
                    } => self.scan_operand(language, mir, locals, index),
                    Place::Local(_) | Place::Field { .. } | Place::Global { .. } => {}
                }
                return;
            }
            Operand::Const(value) => value,
        };
        match value {
            Constant::Symbol(_) => self.add(Need::Symbol),
            Constant::BigInt(_) => self.add(Need::BigInt),
            // A Python `int` literal is the arbitrary-precision `int` runtime.
            Constant::PyInt(_) => self.add(Need::PyInt),
            Constant::String(_) | Constant::StringUnits(_)
                if language != smelt_hir::Language::Python =>
            {
                self.add(Need::JsString);
            }
            Constant::String(_)
            | Constant::StringUnits(_)
            | Constant::Bool(_)
            | Constant::Int(_)
            | Constant::Float(_)
            | Constant::Undefined
            | Constant::None => {}
        }
    }

    /// Follows [`REQUIRES`] to a fixed point.
    fn close(&mut self) {
        loop {
            let before = self.needs;
            for (need, requires) in REQUIRES {
                if self.has(*need) {
                    for required in *requires {
                        self.add(*required);
                    }
                }
            }
            if self.needs == before {
                return;
            }
        }
    }

    /// Records what the shape of the program's declarations demands.
    fn scan_declarations(&mut self, mir: &Mir) {
        if !mir.accessor_fields.is_empty() {
            self.add(Need::Accessors);
        }
        // A per-evaluation class's statics frame erases to a class value
        // (`emitter::statics_frame`), whose static hooks, enumerable view and
        // `instanceof` need the class-value runtime.
        if mir.classes.iter().any(|class| class.statics_of.is_some()) {
            self.add(Need::ClassValue);
            self.add(Need::InstanceOfValue);
        }
        // A generic class, interface or function emits `IntoSmeltUnknown +
        // SmeltFromUnknown` bounds on its type parameters, so the erasure traits
        // must exist even with no erased value in the program.
        if mir
            .classes
            .iter()
            .any(|class| !class.type_params.is_empty())
            || mir
                .interfaces
                .iter()
                .any(|interface| !interface.type_params.is_empty())
        {
            self.add(Need::Unknown);
        }
        for function in &mir.functions {
            if !function.type_params.is_empty() {
                self.add(Need::Unknown);
            }
            if function.language == smelt_hir::Language::Python {
                self.add(Need::PythonNumberFormat);
                // A container `print` may show is drawn by Python's `repr`.
                if function.locals.iter().any(|local| {
                    matches!(
                        mir.types.get(local.ty),
                        Some(Type::List(_) | Type::Tuple(_) | Type::Dict(_, _) | Type::Set(_))
                    )
                }) {
                    self.add(Need::PyRepr);
                }
            }
            if function.is_generator {
                self.add(Need::Generator);
            }
            if function.can_throw {
                self.add(Need::PanicRoute);
            }
            if function.is_async {
                let is_main = mir.symbols.get(function.name) == Some("main");
                if function.is_test || is_main {
                    self.add(Need::DepTokio);
                }
                // An async module body ends by running the event loop to idle,
                // which is the timer helpers' own run-until-idle entry.
                if is_main && !function.is_test {
                    self.add(Need::Timers);
                }
            }
        }
        for closure in &mir.closures {
            if !closure.captures.is_empty() {
                self.add(Need::SharedCaptures);
            }
            if closure.is_generator {
                self.add(Need::Generator);
            }
            if closure.can_throw {
                self.add(Need::PanicRoute);
            }
        }
    }

    /// Records what the types the program can spell demand
    /// ([`ReachableTypes`]): a type interned only for a comparison or an
    /// erasure record renders nowhere, so it demands nothing.
    fn scan_types(&mut self, mir: &Mir, reachable: &ReachableTypes) {
        for ty in reachable.ids().filter_map(|id| mir.types.get(id)) {
            match ty {
                // Unions render as the carrier, and a dict literal can erase its
                // entries without interning `Unknown`.
                Type::Unknown | Type::Never | Type::Union(_) => self.add(Need::Unknown),
                Type::GeneratorResult { .. } => self.add(Need::IteratorResult),
                Type::Class { name, .. } => {
                    if crate::classes::class_name_renders_erased(mir, *name) {
                        self.add(Need::Unknown);
                    }
                    if let Some(class) = stdlib_class_of_class_symbol(mir, *name) {
                        self.add_stdlib_class(class);
                        // `ArrayBuffer` storage is built through the
                        // carrier's constructors (`emitter::typed_array`).
                        if class == StdlibClass::ArrayBuffer {
                            self.add(Need::Unknown);
                        }
                    }
                }
                Type::Set(item) => {
                    // An erased Set is a live view of the typed one.
                    self.add(Need::LiveContainers);
                    // A `Set` of non-primitives is `SmeltJsSet`, whose membership
                    // projects elements through `IntoSmeltUnknown`.
                    let need = if module_hash_set_key_safe(mir, *item) {
                        Need::PrimSet
                    } else {
                        Need::Unknown
                    };
                    self.add(need);
                }
                // `SmeltJsMap` and `SmeltFuture` live in the carrier's block;
                // an erased Map is a live view of the typed one.
                Type::JsMap(_, _) => {
                    self.add(Need::Unknown);
                    self.add(Need::LiveContainers);
                }
                Type::Future(_) => self.add(Need::Unknown),
                Type::List(item) => {
                    self.add(Need::SmeltList);
                    // An erased array is a live view of the typed one.
                    self.add(Need::LiveArrays);
                    if matches!(mir.types.get(*item), Some(Type::TypeParam { .. })) {
                        self.add(Need::ErasedListAlias);
                    }
                }
                Type::BigInt => self.add(Need::BigInt),
                Type::PyInt => self.add(Need::PyInt),
                // A JavaScript string is a `JsString` wherever it is spelled.
                Type::String => self.add(Need::JsString),
                Type::Tuple(items) if items.len() >= 3 => self.add(Need::WideTuple),
                Type::Symbol | Type::PropKey => self.add(Need::Symbol),
                Type::Dict(key, _) if crate::emitter::dict_key_uses_smelt_record(mir, *key) => {
                    self.add(Need::Record);
                }
                Type::Generator { .. } => self.add(Need::Generator),
                Type::Optional(inner) => {
                    if matches!(mir.types.get(*inner), Some(Type::Unknown)) {
                        self.add(Need::OptionalUnknownRead);
                    }
                }
                Type::Function(function) => {
                    if function.may_throw {
                        self.add(Need::PanicRoute);
                    }
                    if function_is_erased_shape(mir, function) {
                        self.add(Need::ErasedFunction);
                    }
                }
                // Nothing beyond the always-emitted core: a scalar, a type
                // parameter, the unit, a dict that is no record, a tuple
                // narrower than three.
                Type::Bool
                | Type::Int
                | Type::Float
                | Type::FixedInt(_)
                | Type::PyStr
                | Type::TypeParam { .. }
                | Type::None
                | Type::Dict(..)
                | Type::Tuple(_) => {}
            }
        }
    }

    /// Records the host family a stdlib class belongs to.
    fn add_stdlib_class(&mut self, class: StdlibClass) {
        if class.is_blob_runtime_type() {
            self.add(Need::Blob);
        }
        match class {
            StdlibClass::Headers => self.add(Need::Headers),
            StdlibClass::PyException | StdlibClass::PyExceptionArgs => self.add(Need::PyException),
            StdlibClass::Response => self.add(Need::Response),
            StdlibClass::Request => self.add(Need::Request),
            StdlibClass::EventEmitter => self.add(Need::EventEmitter),
            StdlibClass::SqliteDatabase | StdlibClass::SqliteStatement => self.add(Need::Sqlite),
            StdlibClass::HttpServer
            | StdlibClass::IncomingMessage
            | StdlibClass::ServerResponse => {
                self.add(Need::HttpServer);
            }
            StdlibClass::UrlSearchParams => self.add(Need::UrlSearchParams),
            StdlibClass::Date => self.add(Need::Date),
            StdlibClass::Url => self.add(Need::Url),
            StdlibClass::CryptoKey => self.add(Need::CryptoKey),
            StdlibClass::HostConstructor => self.add(Need::HostConstructor),
            StdlibClass::FormData => self.add(Need::FormData),
            StdlibClass::TextEncoder => self.add(Need::TextEncoder),
            StdlibClass::TextDecoder => self.add(Need::TextDecoder),
            StdlibClass::TypedArray | StdlibClass::ArrayBuffer => self.add(Need::ByteArray),
            StdlibClass::DataView => {
                self.add(Need::ByteArray);
                self.add(Need::DataView);
            }
            StdlibClass::NodeHash => self.add(Need::NodeHash),
            StdlibClass::RegExp => self.add(Need::DepRegress),
            // No block of their own: what they need comes with the types
            // that hold them (the carrier, the containers).
            StdlibClass::Map
            | StdlibClass::Match
            | StdlibClass::MatchGroups
            | StdlibClass::MatchFnResult
            | StdlibClass::Set
            | StdlibClass::ReadableStream
            | StdlibClass::Blob
            | StdlibClass::File => {}
            // `StdlibClass` is `#[non_exhaustive]`: a class added to
            // `smelt-stdlib` lands here until it is listed above.
            _ => {}
        }
    }

    /// Records what one function or closure body demands.
    fn scan_body(
        &mut self,
        mir: &Mir,
        locals: &[LocalDecl],
        blocks: &[BasicBlock],
        return_ty: smelt_hir::TypeId,
    ) {
        if locals
            .iter()
            .any(|local| local.init == smelt_mir::LocalInit::MaybeUnbound)
        {
            self.add(Need::Binding);
        }
        for block in blocks {
            for statement in &block.statements {
                self.scan_statement(mir, locals, statement);
            }
            if let Some(terminator) = &block.terminator {
                self.scan_terminator(mir, locals, terminator);
                if host_flow_coerces_to_collection(mir, locals, terminator, return_ty)
                    || logs_a_value_holding_a_function(mir, locals, terminator)
                {
                    self.add(Need::Unknown);
                }
                // Python's `print` writes `str(value)` through `SmeltPyRepr`
                // (`emitter::console_inspect`'s `console_value`).
                if let Terminator::Call {
                    callee:
                        Callee::Builtin(smelt_mir::BuiltinFn::ConsoleLog {
                            absent: smelt_mir::AbsentSpelling::None,
                        }),
                    ..
                } = terminator
                {
                    self.add(Need::PyRepr);
                }
                // A class value returned at `string` is its string conversion,
                // which goes through the erased form.
                if let Terminator::Return(operand) = terminator
                    && matches!(mir.types.get(return_ty), Some(Type::String | Type::PyStr))
                    && operand_is_class_local(mir, locals, operand)
                {
                    self.add(Need::Unknown);
                }
            }
        }
    }

    /// Records what one statement demands, including facts that need the body's
    /// local declarations to answer.
    fn scan_statement(&mut self, mir: &Mir, locals: &[LocalDecl], statement: &Statement) {
        let Some(value) = statement.value() else {
            return;
        };
        self.scan_rvalue(value);
        if let Statement::Assign {
            dest,
            value: Rvalue::Use(operand),
        } = statement
            && let Some(dest_ty) = local_type(locals, *dest)
            && host_value_coerces_to_collection(mir, locals, operand, dest_ty)
        {
            self.add(Need::Unknown);
        }
        // A program that reaches a class prototype reflectively can call the
        // methods the prototype carries (docs/design/class-prototypes.md,
        // section 7). Only such a program pays for them: a class read only
        // through its static type never needs its methods by runtime name.
        if !mir.classes.is_empty() && !self.has(Need::ClassPrototypeMethods) {
            let reflects = matches!(
                value,
                Rvalue::PrototypeSentinel { .. }
                    | Rvalue::ObjectFromPrototype { .. }
                    | Rvalue::SetPrototypeOf { .. }
            );
            let mut reads_prototype = false;
            value.reads(|operand| {
                reads_prototype |= operand_reads_erased_prototype(mir, locals, operand);
            });
            if reflects || reads_prototype {
                self.add(Need::ClassPrototypeMethods);
            }
        }
        // A `JSON.stringify` typed `string | undefined` answers through the
        // helper that maps a value with no JSON text to `undefined`.
        if let Statement::Assign {
            dest,
            value: Rvalue::JsonStringify { .. },
        } = statement
            && usize::try_from(dest.0)
                .ok()
                .and_then(|index| locals.get(index))
                .is_some_and(|decl| matches!(mir.types.get(decl.ty), Some(Type::Optional(_))))
        {
            self.add(Need::JsonStringifyValue);
        }
        if let Statement::Assign {
            value: Rvalue::DictAssign { target, .. },
            ..
        } = statement
            && operand_is_erased_local(mir, locals, target)
        {
            self.add(Need::UnknownAssign);
        }
        // A narrowing read of `T | undefined` at a type parameter that renders
        // erased goes through the same helper as one at `unknown`.
        if let Statement::Assign {
            dest,
            value:
                Rvalue::Use(Operand::Copy(Place::Local(source)) | Operand::Move(Place::Local(source))),
        } = statement
            && reads_optional_at_type_param(mir, locals, *dest, *source)
        {
            self.add(Need::OptionalUnknownRead);
        }
        // A class METHOD read as a value (`holder[KEY]` before a call) is the
        // erased bound-method value the emitter renders it as, and its read is
        // typed through the erased answer, which a program with no other
        // erased value would otherwise not have.
        if !self.has(Need::ErasedAnswer) {
            value.reads(|operand| {
                if let Operand::Copy(Place::Field { base, field })
                | Operand::Move(Place::Field { base, field }) = operand
                    && local_class_declares_method(mir, locals, *base, *field)
                {
                    self.add(Need::ErasedAnswer);
                }
            });
        }
        // A read that can produce a prototype METHOD off an erased value (a
        // modeled method name, `call`/`apply`/`bind`, or a dynamic key) yields
        // a function that takes its receiver from the call, so the program
        // carries the receiver channel and every erased call installs one.
        if !self.has(Need::ThisChannel) {
            value.reads(|operand| {
                if operand_may_read_prototype_method(mir, locals, operand) {
                    self.add(Need::ThisChannel);
                }
            });
        }
        // `json()` and friends read through the member's erased host method.
        if !self.has(Need::BodyHostMethod) {
            value.reads(|operand| {
                if let Operand::Copy(Place::Index { base, .. })
                | Operand::Move(Place::Index { base, .. }) = operand
                    && local_holds_a_body(mir, locals, *base)
                {
                    self.add(Need::BodyHostMethod);
                }
            });
        }
    }

    /// Records what one terminator demands.
    fn scan_terminator(&mut self, mir: &Mir, locals: &[LocalDecl], terminator: &Terminator) {
        // A member read can also be a terminator's own operand (`return
        // base.m`, `throw`, an awaited or switched value).
        if let Terminator::Return(operand)
        | Terminator::Throw(operand)
        | Terminator::Await {
            future: operand, ..
        }
        | Terminator::Switch { cond: operand, .. }
        | Terminator::Match {
            scrutinee: operand, ..
        } = terminator
            && operand_may_read_prototype_method(mir, locals, operand)
        {
            self.add(Need::ThisChannel);
        }
        let Terminator::Call {
            callee,
            args,
            unwind,
            ..
        } = terminator
        else {
            return;
        };
        if let Callee::Indirect(operand) = callee
            && operand_may_read_prototype_method(mir, locals, operand)
        {
            self.add(Need::ThisChannel);
        }
        if args
            .iter()
            .any(|operand| operand_may_read_prototype_method(mir, locals, operand))
        {
            self.add(Need::ThisChannel);
        }
        // A `catch` recovers through the panic route whether or not its callee
        // can throw: a non-throwing callee can be handed a throwing closure.
        if unwind.is_some() {
            self.add(Need::PanicRoute);
        }
        if let Callee::Op(op, _) = callee {
            self.scan_rvalue(op);
        }
    }
}

/// Whether `operand` reads a member off an ERASED local that can be a
/// prototype method: by a modeled method name (`Array`/`String`/`Number`),
/// as `call`/`apply`/`bind`, or by a key only known at run time.
fn operand_may_read_prototype_method(mir: &Mir, locals: &[LocalDecl], operand: &Operand) -> bool {
    let (Operand::Copy(place) | Operand::Move(place)) = operand else {
        return false;
    };
    let erased = |base: &LocalId| {
        usize::try_from(base.0)
            .ok()
            .and_then(|index| locals.get(index))
            .is_some_and(|local| matches!(mir.types.get(local.ty), Some(Type::Unknown)))
    };
    match place {
        Place::Field { base, field } => {
            erased(base)
                && mir
                    .names
                    .get(*field)
                    .or_else(|| mir.symbols.get(*field))
                    .is_some_and(|name| {
                        matches!(name, "call" | "apply" | "bind")
                            || smelt_stdlib::erased_array_prototype_method(name).is_some()
                            || smelt_stdlib::erased_string_prototype_method(name).is_some()
                            || smelt_stdlib::erased_number_prototype_method(name).is_some()
                    })
        }
        Place::Index { base, index, .. } => erased(base) && !matches!(**index, Operand::Const(_)),
        Place::Local(_) | Place::Global { .. } => false,
    }
}

/// Whether `operand` reads `prototype` off an erased value (`C.prototype`
/// once `C` is a class value), which reaches a class prototype by name.
fn operand_reads_erased_prototype(mir: &Mir, locals: &[LocalDecl], operand: &Operand) -> bool {
    let (Operand::Copy(Place::Field { base, field }) | Operand::Move(Place::Field { base, field })) =
        operand
    else {
        return false;
    };
    usize::try_from(base.0)
        .ok()
        .and_then(|index| locals.get(index))
        .is_some_and(|local| matches!(mir.types.get(local.ty), Some(Type::Unknown)))
        && mir.names.get(*field).or_else(|| mir.symbols.get(*field)) == Some("prototype")
}

/// The bit of a [`Need`].
#[expect(
    clippy::as_conversions,
    reason = "a fieldless enum's discriminant has no other const conversion"
)]
const fn bit(need: Need) -> u128 {
    1_u128 << (need as u32)
}

/// Whether a function type is the one erased-callable shape: a sync function
/// whose sole parameter is a rest list of erased elements.
fn function_is_erased_shape(mir: &Mir, function: &smelt_hir::FunctionType) -> bool {
    if function.is_async || matches!(mir.types.get(function.return_ty), Some(Type::Future(_))) {
        return false;
    }
    let Some(0) = function.rest else {
        return false;
    };
    let [param] = function.params.as_slice() else {
        return false;
    };
    matches!(
        mir.types.get(*param),
        Some(Type::List(item))
            if matches!(
                mir.types.get(*item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
            )
    )
}

/// Whether `target` names a local declared as the erased carrier.
fn operand_is_erased_local(mir: &Mir, locals: &[LocalDecl], target: &Operand) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = target else {
        return false;
    };
    usize::try_from(local.0)
        .ok()
        .and_then(|index| locals.get(index))
        .is_some_and(|decl| {
            // A type parameter merges in place only where it renders erased
            // (`map::dict_assign_expr`); asking for the helper wherever one
            // could is harmless, since the item is emitted on demand.
            matches!(
                mir.types.get(decl.ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        })
}

/// Whether `operand` reads a local of a class type.
fn operand_is_class_local(mir: &Mir, locals: &[LocalDecl], operand: &Operand) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    usize::try_from(local.0)
        .ok()
        .and_then(|index| locals.get(index))
        .is_some_and(|decl| matches!(mir.types.get(decl.ty), Some(Type::Class { .. })))
}

/// Whether `base` is a local of a class type that declares a method named
/// `field` (and no field of that name).
fn local_class_declares_method(
    mir: &Mir,
    locals: &[LocalDecl],
    base: LocalId,
    field: smelt_hir::Symbol,
) -> bool {
    let Some(decl) = usize::try_from(base.0)
        .ok()
        .and_then(|index| locals.get(index))
    else {
        return false;
    };
    let Some(Type::Class { name, .. }) = mir.types.get(decl.ty) else {
        return false;
    };
    let Some(class) = crate::classes::find_class(mir, *name) else {
        return false;
    };
    !class
        .fields
        .iter()
        .any(|class_field| class_field.name == field)
        && class
            .methods
            .iter()
            .filter_map(|id| {
                usize::try_from(id.0)
                    .ok()
                    .and_then(|index| mir.functions.get(index))
            })
            .any(|function| function.name == field)
}

/// Whether `dest` is a type parameter and `source` that parameter's optional.
fn reads_optional_at_type_param(
    mir: &Mir,
    locals: &[LocalDecl],
    dest: LocalId,
    source: LocalId,
) -> bool {
    let local_ty = |local: LocalId| {
        usize::try_from(local.0)
            .ok()
            .and_then(|index| locals.get(index))
            .map(|decl| decl.ty)
    };
    let (Some(dest_ty), Some(source_ty)) = (local_ty(dest), local_ty(source)) else {
        return false;
    };
    matches!(mir.types.get(dest_ty), Some(Type::TypeParam { .. }))
        && matches!(mir.types.get(source_ty), Some(Type::Optional(inner)) if *inner == dest_ty)
}

/// Whether the local `base` is declared as a `Request` or `Response`.
fn local_holds_a_body(mir: &Mir, locals: &[LocalDecl], base: LocalId) -> bool {
    let Some(Type::Class { name, .. }) = usize::try_from(base.0)
        .ok()
        .and_then(|index| locals.get(index))
        .and_then(|local| mir.types.get(local.ty))
    else {
        return false;
    };
    matches!(
        stdlib_class_of_class_symbol(mir, *name),
        Some(StdlibClass::Request | StdlibClass::Response)
    )
}

/// The backend crates a demand links, for the generated manifest.
#[must_use]
pub(crate) fn backend_dependencies(demand: &RuntimeDemand) -> Vec<BackendDependency> {
    [
        (Need::DepReqwest, BackendDependency::Reqwest),
        (Need::DepSerdeJson, BackendDependency::SerdeJson),
        (Need::DepRegex, BackendDependency::Regex),
        (Need::DepRegress, BackendDependency::Regress),
        (Need::DepRand, BackendDependency::Rand),
        (Need::DepChrono, BackendDependency::Chrono),
        (Need::DepChronoTz, BackendDependency::ChronoTz),
        (Need::DepUrl, BackendDependency::Url),
        (
            Need::DepUnicodeNormalization,
            BackendDependency::UnicodeNormalization,
        ),
        (Need::HttpServer, BackendDependency::Hyper),
        (Need::Sqlite, BackendDependency::Rusqlite),
        (Need::DepUuid, BackendDependency::Uuid),
        (Need::CryptoRandomValues, BackendDependency::GetRandom),
        (Need::DepSha, BackendDependency::Sha),
        (Need::DepHmac, BackendDependency::Hmac),
        (Need::Base64, BackendDependency::Base64),
        (Need::BigInt, BackendDependency::BigInt),
        (Need::PyInt, BackendDependency::BigInt),
        (Need::DepMd5, BackendDependency::Md5),
        (Need::DepIndexMap, BackendDependency::IndexMap),
    ]
    .into_iter()
    .filter_map(|(need, dependency)| demand.has(need).then_some(dependency))
    .fold(Vec::new(), |mut dependencies, dependency| {
        // Two needs may link one crate (a JavaScript `bigint` and a Python
        // `int` both link `num-bigint`); the manifest names it once.
        if !dependencies.contains(&dependency) {
            dependencies.push(dependency);
        }
        dependencies
    })
}

/// Whether `place` reads a class method as a value: a field projection whose
/// name is a method of the receiver's class (or of an ancestor) and not one of
/// its stored fields. Codegen renders it as a closure over the receiver that
/// the erased call ABI invokes (`emitter::field_access`'s
/// `class_method_reference_expr`), whose arguments arrive as `SmeltUnknown`.
fn class_method_read_as_value(mir: &Mir, locals: &[LocalDecl], place: &Place) -> bool {
    let Place::Field { base, field } = place else {
        return false;
    };
    let Some(local) = usize::try_from(base.0).ok().and_then(|at| locals.get(at)) else {
        return false;
    };
    let Some(Type::Class { name, .. }) = mir.types.get(local.ty) else {
        return false;
    };
    let Some(receiver_class) = crate::classes::find_class(mir, *name) else {
        return false;
    };
    if crate::classes::effective_class_fields(mir, receiver_class)
        .iter()
        .any(|stored| stored.name == *field)
    {
        return false;
    }
    let mut current = Some(receiver_class);
    let mut seen = std::collections::HashSet::new();
    while let Some(ancestor) = current {
        if !seen.insert(ancestor.name) {
            break;
        }
        if ancestor.methods.iter().any(|method_id| {
            usize::try_from(method_id.0)
                .ok()
                .and_then(|at| mir.functions.get(at))
                .is_some_and(|function| {
                    matches!(
                        function.origin,
                        smelt_mir::HirOrigin::ClassMethod { method: declared, .. } if declared == *field
                    )
                })
        }) {
            return true;
        }
        current = ancestor
            .base
            .and_then(|base_name| crate::classes::find_class(mir, base_name));
    }
    false
}

/// The declared type of `local` in `locals`.
fn local_type(locals: &[LocalDecl], local: LocalId) -> Option<smelt_hir::TypeId> {
    usize::try_from(local.0)
        .ok()
        .and_then(|at| locals.get(at))
        .map(|decl| decl.ty)
}

/// Whether `operand`, read where a value of type `target` is expected, is a
/// host value (a stdlib class with an erasure adapter, `Uint8Array`, `Date`,
/// `Response`, ..) taken as a record or collection. Codegen converts it through
/// the host value's erasure adapter (`emitter::coercion`'s `rendered_at_type`),
/// which is written against the carrier.
fn host_value_coerces_to_collection(
    mir: &Mir,
    locals: &[LocalDecl],
    operand: &Operand,
    target: smelt_hir::TypeId,
) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    let Some(source) = local_type(locals, *local) else {
        return false;
    };
    matches!(
        mir.types.get(target),
        Some(Type::Dict(_, _) | Type::List(_) | Type::Set(_))
    ) && matches!(mir.types.get(source), Some(Type::Class { name, .. })
        if stdlib_class_of_class_symbol(mir, *name).is_some_and(StdlibClass::erases_through_adapter))
}

/// Whether `terminator` hands a host value to a record or collection slot: a
/// `return` at such a return type, or an argument of a direct call at such a
/// parameter type (see [`host_value_coerces_to_collection`]).
fn host_flow_coerces_to_collection(
    mir: &Mir,
    locals: &[LocalDecl],
    terminator: &Terminator,
    return_ty: smelt_hir::TypeId,
) -> bool {
    if let Terminator::Return(operand) = terminator {
        return host_value_coerces_to_collection(mir, locals, operand, return_ty);
    }
    let Terminator::Call {
        callee: Callee::Static(callee),
        args,
        ..
    } = terminator
    else {
        return false;
    };
    let Some(function) = usize::try_from(callee.0)
        .ok()
        .and_then(|at| mir.functions.get(at))
    else {
        return false;
    };
    function.params.iter().zip(args).any(|(param, arg)| {
        local_type(&function.locals, *param)
            .is_some_and(|param_ty| host_value_coerces_to_collection(mir, locals, arg, param_ty))
    })
}

/// Whether `terminator` is a `console.log` of a value whose storage holds a
/// function. `util.inspect` has no typed impl for a callable, so such a value
/// is printed through its erasure (`emitter::console_inspect`'s `console_arg`
/// falls back to the carrier, which draws `[Function: name]`).
fn logs_a_value_holding_a_function(
    mir: &Mir,
    locals: &[LocalDecl],
    terminator: &Terminator,
) -> bool {
    let Terminator::Call {
        callee: Callee::Builtin(smelt_mir::BuiltinFn::ConsoleLog { .. }),
        args,
        ..
    } = terminator
    else {
        return false;
    };
    args.iter().any(|arg| {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = arg else {
            return false;
        };
        local_type(locals, *local).is_some_and(|ty| {
            crate::classes::type_holds_function(mir, ty, crate::classes::NominalStorage::Followed)
        })
    })
}

/// Whether `place` reads a class value with a run-time string key
/// (`raw[key]`). The key can name any declared member, so codegen looks it up
/// on the value's erased view (`emitter::place`'s keyed class read); without
/// the carrier it could only answer `undefined`.
fn class_read_with_a_runtime_key(mir: &Mir, locals: &[LocalDecl], place: &Place) -> bool {
    let Place::Index { base, index, .. } = place else {
        return false;
    };
    let base_is_class = local_type(locals, *base)
        .is_some_and(|ty| matches!(mir.types.get(ty), Some(Type::Class { .. })));
    let key_is_text = match index.as_ref() {
        Operand::Const(Constant::String(_) | Constant::StringUnits(_)) => true,
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
            local_type(locals, *local)
                .is_some_and(|ty| mir.types.get(ty).is_some_and(Type::is_text))
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Const(_) => false,
    };
    base_is_class && key_is_text
}

/// Whether `base` is a bigint typed-array view (`BigInt64Array`,
/// `BigUint64Array`), whose indexed elements are `BigInt` values read and
/// written through the bigint runtime (`SmeltTypedArray::get_bigint`).
fn indexes_a_bigint_view(mir: &Mir, locals: &[LocalDecl], base: LocalId) -> bool {
    local_type(locals, base).is_some_and(|ty| {
        matches!(mir.types.get(ty), Some(Type::Class { name, .. })
            if mir.symbols.get(*name).is_some_and(|text| matches!(text, "BigInt64Array" | "BigUint64Array")))
    })
}
