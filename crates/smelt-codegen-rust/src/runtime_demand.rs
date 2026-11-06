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

use smelt_hir::Type;
use smelt_mir::{
    BasicBlock, Callee, LocalDecl, LocalId, Mir, Operand, Place, Rvalue, Statement,
    Terminator,
};
use smelt_stdlib::{BackendDependency, StdlibClass};

use crate::stdlib::{module_hash_set_key_safe, stdlib_class_of_class_symbol};

/// A runtime family, or a backend crate, a program can demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Need {
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
    /// The iterator-result enum a list's `next()` answers with.
    IteratorResult,
    /// Generator state machines.
    Generator,
    /// The timer queue and promise task queue.
    Timers,
    /// `Date.now()` override state.
    DateNow,
    /// `getTimezoneOffset()` override state.
    DateTimezoneOffset,
    /// The `Date` host class.
    Date,
    /// JavaScript `Math.round`.
    MathRound,
    /// Mutable closure captures shared through the capture registry.
    SharedCaptures,
    /// `encodeURI` and friends.
    UriEncode,
    /// A throwing URI decoder.
    UriDecode,
    /// `JSON.stringify` with an indent.
    JsonIndent,
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
    /// `structuredClone`.
    StructuredClone,
    /// A `typeof` the backend lowers.
    Typeof,
    /// `Array.prototype.concat` with a spreadable argument.
    ConcatSpread,
    /// `Object.defineProperty`/`defineProperties`.
    DefineProperties,
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
    /// The `tokio` crate.
    DepTokio,
    /// The `genawaiter` crate.
    DepGenawaiter,
    /// The `reqwest` crate.
    DepReqwest,
    /// The `serde_json` crate.
    DepSerdeJson,
    /// The regex crates. Also what the `SmeltRegExp` runtime is gated on.
    DepRegex,
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
}

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
            Need::PanicRoute,
            Need::DepRegex,
            Need::DepChrono,
            Need::DepChronoTz,
            Need::DepTokio,
        ],
    ),
    (Need::SmeltList, &[Need::ObjectIds]),
    (Need::PrimSet, &[Need::ObjectIds]),
    (Need::DepRegex, &[Need::ObjectIds]),
    (Need::Date, &[Need::ObjectIds, Need::HostOrigins]),
    // These are emitted inside the carrier's block, or carry its values.
    (Need::IteratorResult, &[Need::Unknown]),
    (Need::Generator, &[Need::Unknown, Need::IteratorResult, Need::DepGenawaiter]),
    (Need::Timers, &[Need::Unknown]),
    (Need::StructuredClone, &[Need::Unknown]),
    (Need::DefineProperties, &[Need::Unknown]),
    (Need::ThisChannel, &[Need::Unknown]),
    (Need::ThisRead, &[Need::ThisChannel]),
    (Need::Construct, &[Need::Unknown, Need::ThisChannel]),
    (Need::InstanceOfValue, &[Need::Unknown]),
    (Need::ClassValue, &[Need::Unknown]),
    (Need::VitestMock, &[Need::Unknown]),
    (Need::HostOverride, &[Need::Unknown]),
    (Need::ErasedFunction, &[Need::Unknown]),
    // A throw of a branded error record crosses the erased channel even where
    // the value the operation answers is concrete.
    (Need::UriDecode, &[Need::Unknown, Need::UriEncode]),
    (Need::Base64, &[Need::Unknown]),
    (Need::RegExpCompile, &[Need::Unknown]),
    (Need::UrlThrowing, &[Need::Unknown, Need::Url]),
    (Need::DataViewAccess, &[Need::Unknown, Need::ByteArray, Need::DataView]),
    (Need::BigInt, &[Need::Unknown]),
    // The host families.
    (Need::Response, &[Need::Headers, Need::Body, Need::ObjectIds]),
    (Need::Request, &[Need::Headers, Need::Body, Need::ObjectIds, Need::DepUrl]),
    (Need::Headers, &[Need::ObjectIds, Need::HostOrigins]),
    (Need::Body, &[Need::ObjectIds]),
    (Need::BodyHostMethod, &[Need::JsonParse]),
    (Need::JsonParse, &[Need::DepSerdeJson]),
    (Need::ErasedAnswer, &[Need::Unknown]),
    (
        Need::HttpServer,
        &[Need::Headers, Need::EventEmitter, Need::Timers, Need::ObjectIds, Need::HostOrigins],
    ),
    (Need::FetchServer, &[Need::HttpServer, Need::Response, Need::Request]),
    (Need::EventEmitter, &[Need::ObjectIds, Need::HostOrigins]),
    (Need::Url, &[Need::UrlSearchParams, Need::HostOrigins]),
    (Need::UrlSearchParams, &[Need::ObjectIds, Need::HostOrigins, Need::DepUrl]),
    (Need::FormData, &[Need::Blob, Need::ObjectIds, Need::HostOrigins, Need::DepUrl]),
    (Need::Blob, &[Need::ByteArray, Need::ObjectIds]),
    (Need::TextEncoder, &[Need::ByteArray, Need::ObjectIds, Need::HostOrigins]),
    (Need::TextDecoder, &[Need::ByteArray, Need::ObjectIds, Need::HostOrigins]),
    (Need::ByteArray, &[Need::ObjectIds, Need::HostOrigins]),
    (Need::CryptoRandomValues, &[Need::ByteArray]),
    (Need::CryptoDigest, &[Need::ByteArray, Need::DepSha]),
    (Need::CryptoKey, &[Need::Unknown, Need::ByteArray, Need::DepSha, Need::DepHmac]),
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
    (Need::Sqlite, &[Need::Unknown, Need::ByteArray, Need::HostOrigins]),
];

/// The closed set of everything one program demands of the runtime.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeDemand {
    /// One bit per [`Need`], closed over [`REQUIRES`].
    needs: u128,
}

impl RuntimeDemand {
    /// Scans `mir` once and closes what it found over the `requires` graph.
    #[must_use]
    pub(crate) fn of(mir: &Mir) -> Self {
        let mut demand = Self::default();
        demand.scan_declarations(mir);
        demand.scan_types(mir);
        for function in &mir.functions {
            demand.scan_body(mir, &function.locals, &function.blocks, function.return_ty);
        }
        for closure in &mir.closures {
            demand.scan_body(mir, &closure.locals, &closure.blocks, closure.return_ty);
        }
        demand.close();
        demand
    }

    /// Whether the program demands `need`.
    #[must_use]
    pub(crate) const fn has(&self, need: Need) -> bool {
        self.needs & bit(need) != 0
    }

    /// Records `need` as demanded.
    const fn add(&mut self, need: Need) {
        self.needs |= bit(need);
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
        // A generic class, interface or function emits `IntoSmeltUnknown +
        // SmeltFromUnknown` bounds on its type parameters, so the erasure traits
        // must exist even with no erased value in the program.
        if mir.classes.iter().any(|class| !class.type_params.is_empty())
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

    /// Records what the type table demands.
    fn scan_types(&mut self, mir: &Mir) {
        for ty in mir.types.all() {
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
                    }
                }
                Type::Set(item) => {
                    // A `Set` of non-primitives is `SmeltJsSet`, whose membership
                    // projects elements through `IntoSmeltUnknown`.
                    let need = if module_hash_set_key_safe(mir, *item) {
                        Need::PrimSet
                    } else {
                        Need::Unknown
                    };
                    self.add(need);
                }
                // `SmeltJsMap` and `SmeltFuture` live in the carrier's block.
                Type::JsMap(_, _) | Type::Future(_) => self.add(Need::Unknown),
                Type::List(item) => {
                    self.add(Need::SmeltList);
                    if matches!(mir.types.get(*item), Some(Type::TypeParam { .. })) {
                        self.add(Need::ErasedListAlias);
                    }
                }
                Type::BigInt => self.add(Need::BigInt),
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
                _ => {}
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
            StdlibClass::Response => self.add(Need::Response),
            StdlibClass::Request => self.add(Need::Request),
            StdlibClass::EventEmitter => self.add(Need::EventEmitter),
            StdlibClass::SqliteDatabase | StdlibClass::SqliteStatement => self.add(Need::Sqlite),
            StdlibClass::HttpServer | StdlibClass::IncomingMessage | StdlibClass::ServerResponse => {
                self.add(Need::HttpServer);
            }
            StdlibClass::UrlSearchParams => self.add(Need::UrlSearchParams),
            StdlibClass::Date => self.add(Need::Date),
            StdlibClass::Url => self.add(Need::Url),
            StdlibClass::CryptoKey => self.add(Need::CryptoKey),
            StdlibClass::FormData => self.add(Need::FormData),
            StdlibClass::TextEncoder => self.add(Need::TextEncoder),
            StdlibClass::TextDecoder => self.add(Need::TextDecoder),
            StdlibClass::TypedArray | StdlibClass::ArrayBuffer => self.add(Need::ByteArray),
            StdlibClass::DataView => {
                self.add(Need::ByteArray);
                self.add(Need::DataView);
            }
            StdlibClass::NodeHash => self.add(Need::NodeHash),
            StdlibClass::RegExp => self.add(Need::DepRegex),
            _ => {}
        }
    }

    /// Records what one function or closure body demands.
    fn scan_body(&mut self, mir: &Mir, locals: &[LocalDecl], blocks: &[BasicBlock], return_ty: smelt_hir::TypeId) {
        for block in blocks {
            for statement in &block.statements {
                self.scan_statement(mir, locals, statement);
            }
            if let Some(terminator) = &block.terminator {
                self.scan_terminator(terminator);
                // A class value returned at `string` is its string conversion,
                // which goes through the erased form.
                if let Terminator::Return(operand) = terminator
                    && matches!(mir.types.get(return_ty), Some(Type::String))
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
        let value = statement.value();
        self.scan_rvalue(value);
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
            value: Rvalue::Use(Operand::Copy(Place::Local(source)) | Operand::Move(Place::Local(source))),
        } = statement
            && reads_optional_at_type_param(mir, locals, *dest, *source)
        {
            self.add(Need::OptionalUnknownRead);
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
    fn scan_terminator(&mut self, terminator: &Terminator) {
        let Terminator::Call { callee, unwind, .. } = terminator else {
            return;
        };
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

/// Adds the runtime families one op states in its `needs` fact.
macro_rules! add_op_needs {
    ($demand:ident,) => {};
    ($demand:ident, needs ($($need:ident $(if $condition:expr)?),* $(,)?) $($rest:tt)*) => {
        $(
            if true $(&& $condition)? {
                $demand.add(Need::$need);
            }
        )*
    };
    ($demand:ident, $other:ident $value:tt $($rest:tt)*) => {
        add_op_needs!($demand, $($rest)*);
    };
}

/// Generates [`RuntimeDemand::scan_rvalue`] from the op table's `needs` facts.
macro_rules! define_scan_rvalue {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl RuntimeDemand {
            /// Records what one rvalue demands, whether it is assigned or is the
            /// callee of a call terminator.
            #[allow(
                unused_variables,
                reason = "each arm binds every field; a fact reads the ones it needs"
            )]
            fn scan_rvalue(&mut self, rvalue: &Rvalue) {
                match rvalue {
                    $(
                        Rvalue::$name $({ $($field,)* })? => {
                            add_op_needs!(self, $($fact $fact_value)*);
                        }
                    )*
                    // Wherever the parameter is not a real Rust generic, a read
                    // through its bound converts from the erased carrier.
                    Rvalue::BoundMethod { .. } | Rvalue::BoundField { .. } => {
                        self.add(Need::ErasedAnswer);
                    }
                    _ => {}
                }
            }
        }
    };
}

smelt_hir::for_each_op!(define_scan_rvalue);

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
        .is_some_and(|decl| matches!(mir.types.get(decl.ty), Some(Type::Unknown)))
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

/// Whether `dest` is a type parameter and `source` that parameter's optional.
fn reads_optional_at_type_param(mir: &Mir, locals: &[LocalDecl], dest: LocalId, source: LocalId) -> bool {
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
        (Need::DepRand, BackendDependency::Rand),
        (Need::DepChrono, BackendDependency::Chrono),
        (Need::DepChronoTz, BackendDependency::ChronoTz),
        (Need::DepUrl, BackendDependency::Url),
        (Need::DepUnicodeNormalization, BackendDependency::UnicodeNormalization),
        (Need::HttpServer, BackendDependency::Hyper),
        (Need::Sqlite, BackendDependency::Rusqlite),
        (Need::DepUuid, BackendDependency::Uuid),
        (Need::CryptoRandomValues, BackendDependency::GetRandom),
        (Need::DepSha, BackendDependency::Sha),
        (Need::DepHmac, BackendDependency::Hmac),
        (Need::Base64, BackendDependency::Base64),
        (Need::BigInt, BackendDependency::BigInt),
        (Need::DepMd5, BackendDependency::Md5),
    ]
    .into_iter()
    .filter_map(|(need, dependency)| demand.has(need).then_some(dependency))
    .collect()
}
