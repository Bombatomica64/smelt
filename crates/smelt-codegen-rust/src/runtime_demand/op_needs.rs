//! What each MIR operation demands of the runtime.
//!
//! The demand is a codegen fact: [`Need`] names runtime families and backend
//! crates, which neither HIR nor MIR knows about. The match is exhaustive, so a
//! new op does not compile until it says what it needs.

use super::{Need, RuntimeDemand};
use smelt_mir::Rvalue;

impl RuntimeDemand {
    /// Records what one rvalue demands, whether it is assigned or is the callee
    /// of a call terminator.
    pub(super) fn scan_rvalue(&mut self, rvalue: &Rvalue) {
        match rvalue {
            Rvalue::ThisRead => {
                self.add(Need::ThisRead);
            }
            Rvalue::BindThis { .. } | Rvalue::ErasedFunctionMethodCall { .. } => {
                self.add(Need::ThisChannel);
            }
            Rvalue::Construct { .. } => {
                self.add(Need::Construct);
            }
            Rvalue::ConstructLike { .. } => {
                self.add(Need::Construct);
            }
            Rvalue::NumericRound { op, .. } => {
                if matches!(op, smelt_hir::NumericRoundOp::Round) {
                    self.add(Need::MathRound);
                }
            }
            Rvalue::NumericRandom => {
                self.add(Need::DepRand);
            }
            Rvalue::NumericRandomInt { .. } => {
                self.add(Need::DepRand);
            }
            Rvalue::ParseIntRadix { .. } => {
                self.add(Need::ParseNumber);
            }
            Rvalue::PrimitiveCast { op, .. } => {
                if matches!(op, smelt_hir::PrimitiveCastOp::ParseFloat) {
                    self.add(Need::ParseNumber);
                }
            }
            Rvalue::StringNormalize { .. } => {
                self.add(Need::DepUnicodeNormalization);
            }
            Rvalue::StructuredClone { .. } => {
                self.add(Need::StructuredClone);
            }
            Rvalue::StringLocaleCompare { .. } => {
                self.add(Need::LocaleCompare);
            }
            Rvalue::StringSearch { op, .. } => {
                if matches!(op, smelt_hir::StringSearchOp::Regex) {
                    self.add(Need::DepRegress);
                }
            }
            Rvalue::RegexIsMatch { .. } => {
                self.add(Need::RegexOps);
            }
            Rvalue::RegexReplace { .. } => {
                self.add(Need::RegexOps);
            }
            Rvalue::RegexReplaceCallback { .. } => {
                self.add(Need::RegexOps);
            }
            Rvalue::RegexSplit { .. } => {
                self.add(Need::RegexOps);
            }
            Rvalue::RegexFind { .. } => {
                self.add(Need::DepRegress);
            }
            Rvalue::RegexExec { .. } => {
                self.add(Need::DepRegress);
            }
            Rvalue::RegexTest { .. } => {
                self.add(Need::DepRegress);
            }
            Rvalue::RegexMatchAll { .. } => {
                self.add(Need::DepRegress);
            }
            Rvalue::HeadersNew { .. } => {
                self.add(Need::Headers);
            }
            Rvalue::UrlSearchParamsNew { .. } => {
                self.add(Need::UrlSearchParams);
                self.add(Need::DepUrl);
            }
            Rvalue::TextEncoderNew => {
                self.add(Need::TextEncoder);
            }
            Rvalue::TextDecoderNew { .. } => {
                self.add(Need::TextDecoder);
            }
            Rvalue::TextEncoderOp { .. } => {
                self.add(Need::TextEncoder);
            }
            Rvalue::TextDecoderOp { .. } => {
                self.add(Need::TextDecoder);
            }
            Rvalue::GlobalObject => {
                self.add(Need::GlobalObject);
            }
            Rvalue::TypedArrayNew { .. } => {
                self.add(Need::ByteArray);
            }
            Rvalue::ByteArrayOp { .. } => {
                self.add(Need::ByteArray);
            }
            Rvalue::UrlSearchParamsOp { .. } => {
                self.add(Need::UrlSearchParams);
                self.add(Need::DepUrl);
            }
            Rvalue::AbortSignalOp { op, .. } => {
                if matches!(op, smelt_hir::AbortSignalOp::Timeout) {
                    self.add(Need::Timers);
                }
            }
            Rvalue::CryptoOp { op, .. } => {
                if matches!(op, smelt_hir::CryptoOp::GetRandomValues) {
                    self.add(Need::CryptoRandomValues);
                }
                if matches!(op, smelt_hir::CryptoOp::Digest) {
                    self.add(Need::CryptoDigest);
                }
                if matches!(op, smelt_hir::CryptoOp::ImportKey | smelt_hir::CryptoOp::Sign | smelt_hir::CryptoOp::Verify) {
                    self.add(Need::CryptoKey);
                }
                if matches!(op, smelt_hir::CryptoOp::RandomUuid) {
                    self.add(Need::DepUuid);
                }
            }
            Rvalue::FormDataNew => {
                self.add(Need::FormData);
            }
            Rvalue::FormDataOp { .. } => {
                self.add(Need::FormData);
            }
            Rvalue::ResponseNew { .. } => {
                self.add(Need::Response);
            }
            Rvalue::EventEmitterNew => {
                self.add(Need::EventEmitter);
            }
            Rvalue::EventEmitterOp { .. } => {
                self.add(Need::EventEmitter);
            }
            Rvalue::HttpCreateServer { .. } => {
                self.add(Need::HttpServer);
            }
            Rvalue::SqliteOpen { .. } => {
                self.add(Need::Sqlite);
            }
            Rvalue::SqliteOp { .. } => {
                self.add(Need::Sqlite);
            }
            Rvalue::HttpCreateFetchServer { .. } => {
                self.add(Need::FetchServer);
            }
            Rvalue::HttpServerOp { .. } => {
                self.add(Need::HttpServer);
            }
            Rvalue::IncomingMessageOp { .. } => {
                self.add(Need::HttpServer);
            }
            Rvalue::ServerResponseOp { .. } => {
                self.add(Need::HttpServer);
            }
            Rvalue::RequestNew { .. } => {
                self.add(Need::Request);
                self.add(Need::DepUrl);
            }
            Rvalue::RequestOp { op, .. } => {
                self.add(Need::Request);
                if matches!(op, smelt_hir::RequestOp::FormData) {
                    self.add(Need::FormData);
                }
                if matches!(op, smelt_hir::RequestOp::Json) {
                    self.add(Need::JsonParse);
                }
            }
            Rvalue::ResponseOp { op, .. } => {
                self.add(Need::Response);
                if matches!(op, smelt_hir::ResponseOp::FormData) {
                    self.add(Need::FormData);
                }
                if matches!(op, smelt_hir::ResponseOp::Json) {
                    self.add(Need::JsonParse);
                }
            }
            Rvalue::HeadersOp { .. } => {
                self.add(Need::Headers);
            }
            Rvalue::ConcatSpread { .. } => {
                self.add(Need::ConcatSpread);
            }
            Rvalue::ListRandomChoice { .. } => {
                self.add(Need::DepRand);
            }
            Rvalue::DictPop { default, .. } => {
                if default.is_none() {
                    self.add(Need::PythonDictRead);
                }
            }
            Rvalue::DictPopItem { .. } => {
                self.add(Need::PythonDictRead);
            }
            Rvalue::StringSplit { .. } => {
                self.add(Need::RegexOps);
            }
            Rvalue::JsonStringify { .. } => {
                self.add(Need::Unknown);
                self.add(Need::DepSerdeJson);
            }
            Rvalue::JsonIndent { .. } => {
                self.add(Need::JsonIndent);
                self.add(Need::DepChrono);
            }
            Rvalue::HttpGetText { .. } => {
                self.add(Need::DepReqwest);
            }
            Rvalue::DateNow => {
                self.add(Need::DateNow);
                self.add(Need::DepChrono);
            }
            Rvalue::DateSetNow { .. } => {
                self.add(Need::DateNow);
                self.add(Need::DepChrono);
            }
            Rvalue::DateResetNow => {
                self.add(Need::DateNow);
                self.add(Need::DepChrono);
            }
            Rvalue::DateTimezoneOffset => {
                self.add(Need::DateTimezoneOffset);
            }
            Rvalue::DateSetTimezoneOffset { .. } => {
                self.add(Need::DateTimezoneOffset);
            }
            Rvalue::DateResetTimezoneOffset => {
                self.add(Need::DateTimezoneOffset);
            }
            Rvalue::VitestMockFn { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::VitestMockCalledTimes { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::VitestMockCalledWith { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::VitestRestoreAllMocks => {
                self.add(Need::VitestMock);
            }
            Rvalue::VitestSpyOn { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::VitestAsymmetricEqual { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::VitestMockLastResolvedWith { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::DateTimezoneContext { .. } => {
                self.add(Need::DepChronoTz);
                self.add(Need::DepChrono);
            }
            Rvalue::DateToIsoString { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::DateToString { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::DateToUtcString { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::DateFromParts { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::DateFromValue { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::DateGetPart { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::DateSetPart { .. } => {
                self.add(Need::DepChrono);
            }
            Rvalue::UrlField { .. } => {
                self.add(Need::DepUrl);
            }
            Rvalue::BlobFromParts { .. } => {
                self.add(Need::Blob);
            }
            Rvalue::BlobOp { .. } => {
                self.add(Need::Blob);
            }
            Rvalue::UnmodeledImport => {
                self.add(Need::Unknown);
            }
            Rvalue::TestRuntimeCheck { .. } => {
                self.add(Need::VitestMock);
            }
            Rvalue::FastCheckNamespace | Rvalue::FastCheckRun { .. } => {
                self.add(Need::FastCheck);
            }
            Rvalue::HostGlobalRead { .. } => {
                self.add(Need::HostOverride);
            }
            Rvalue::HostGlobalWrite { .. } => {
                self.add(Need::HostOverride);
            }
            Rvalue::HostGlobalPresent { .. } => {
                self.add(Need::HostOverride);
            }
            Rvalue::ClassValue { .. } => {
                self.add(Need::ClassValue);
            }
            Rvalue::InstanceOfValue { .. } => {
                self.add(Need::InstanceOfValue);
            }
            // IsLooselyEqual runs over erased values (`smelt_js_loose_eq`).
            Rvalue::Binary { op: smelt_hir::BinOp::JsLooseEq | smelt_hir::BinOp::JsLooseNotEq, .. } => {
                self.add(Need::Unknown);
            }
            Rvalue::UnknownIs { .. } => {
                self.add(Need::Unknown);
            }
            Rvalue::TypeofValue { .. } => {
                self.add(Need::Typeof);
            }
            Rvalue::DefineProperties { .. } => {
                self.add(Need::DefineProperties);
            }
            Rvalue::UnknownCast { .. } => {
                self.add(Need::Unknown);
            }
            Rvalue::AsyncOp { op, .. } => {
                self.add(Need::DepTokio);
                if matches!(op, smelt_hir::AsyncOp::HttpFetch) {
                    self.add(Need::Response);
                }
                if matches!(op, smelt_hir::AsyncOp::HttpFetch | smelt_hir::AsyncOp::HttpGetText) {
                    self.add(Need::DepReqwest);
                }
                if op.arms_a_timer() {
                    self.add(Need::Timers);
                }
            }
            Rvalue::UriTranscode { .. } => {
                self.add(Need::UriEncode);
                self.add(Need::UriDecode);
            }
            Rvalue::HostModuleCall { op, .. } => {
                if matches!(op, smelt_hir::HostModuleOp::Path(_)) {
                    self.add(Need::NodePath);
                }
                if matches!(op, smelt_hir::HostModuleOp::Hash(_)) {
                    self.add(Need::NodeHash);
                }
            }
            Rvalue::UrlOp { op, .. } => {
                if !matches!(op, smelt_hir::UrlOp::Set(smelt_hir::UrlField::Href)) {
                    self.add(Need::Url);
                }
                if matches!(op, smelt_hir::UrlOp::Set(smelt_hir::UrlField::Href)) {
                    self.add(Need::UrlThrowing);
                }
            }
            Rvalue::PythonDivision { .. } => {
                self.add(Need::PythonDivision);
            }
            Rvalue::PythonDictRead { .. } => {
                self.add(Need::PythonDictRead);
            }
            Rvalue::PythonListRead { .. } => {
                self.add(Need::PythonListRead);
            }
            Rvalue::PyExceptionNew { .. } => {
                self.add(Need::PyException);
            }
            Rvalue::PyStr { .. } => {
                self.add(Need::PyRepr);
            }
            Rvalue::PyRepr { .. } => {
                self.add(Need::PyRepr);
            }
            Rvalue::PyExceptionArgs { .. } => {
                self.add(Need::PyException);
            }
            Rvalue::PyExceptionCaughtBy { .. } => {
                self.add(Need::PyException);
            }
            Rvalue::PythonSequenceRead { .. } => {
                self.add(Need::PyException);
            }
            Rvalue::PythonStepSlice { .. }
            | Rvalue::PythonListExtrema { .. }
            | Rvalue::ListIndex { .. }
            | Rvalue::ListRemove { .. } => {
                self.add(Need::PyException);
            }
            Rvalue::Base64Transcode { .. } => {
                self.add(Need::Base64);
            }
            Rvalue::RegExpCompile { .. } => {
                self.add(Need::RegExpCompile);
            }
            Rvalue::DataViewAccess { .. } => {
                self.add(Need::DataViewAccess);
            }
            Rvalue::BigIntCall { op, .. } => {
                if matches!(op, smelt_hir::BigIntOp::JsonStringify) {
                    self.add(Need::DepSerdeJson);
                }
            }
            Rvalue::UrlNew { .. } => {
                self.add(Need::UrlThrowing);
            }
            Rvalue::JsonParse { .. } => {
                self.add(Need::JsonParse);
                self.add(Need::ErasedAnswer);
            }
            // Wherever the parameter is not a real Rust generic, a read through
            // its bound converts from the erased carrier.
            Rvalue::BoundMethod { .. } | Rvalue::BoundField { .. } => {
                self.add(Need::ErasedAnswer);
            }
            Rvalue::GeneratorNext { .. }
            | Rvalue::GeneratorDone { .. }
            | Rvalue::GeneratorValue { .. }
            | Rvalue::GeneratorDelegate { .. }
            | Rvalue::ClosureCallSpread { .. }
            | Rvalue::ClosureCallRest { .. }
            | Rvalue::OptionalField { .. }
            | Rvalue::OptionalIndex { .. }
            | Rvalue::Len { .. }
            | Rvalue::NumericAbs { .. }
            | Rvalue::NumericExtrema { .. }
            | Rvalue::NumericHypot { .. }
            | Rvalue::NumericPredicate { .. }
            | Rvalue::NumericUnaryFunc { .. }
            | Rvalue::NumericPow { .. }
            | Rvalue::NumericAtan2 { .. }
            | Rvalue::NumericToStringRadix { .. }
            | Rvalue::NumericToFixed { .. }
            | Rvalue::StringCase { .. }
            | Rvalue::StringIsWellFormed { .. }
            | Rvalue::StringToWellFormed { .. }
            | Rvalue::ObjectToStringTag { .. }
            // The operand's `Symbol` type already demands `Need::Symbol`.
            | Rvalue::SymbolDescription { .. }
            | Rvalue::SymbolUnique { .. }
            | Rvalue::StringTrim { .. }
            | Rvalue::StringAffix { .. }
            | Rvalue::StringReplace { .. }
            | Rvalue::StringRemoveAffix { .. }
            | Rvalue::StringRepeat { .. }
            | Rvalue::StringPad { .. }
            | Rvalue::StringPredicate { .. }
            | Rvalue::StringCharAt { .. }
            | Rvalue::StringCharCodeAt { .. }
            | Rvalue::StringCodePointAt { .. }
            | Rvalue::StringContains { .. }
            | Rvalue::StringSlice { .. }
            | Rvalue::ListContains { .. }
            | Rvalue::SetContains { .. }
            | Rvalue::SetDisjoint { .. }
            | Rvalue::SetRelation { .. }
            | Rvalue::SetAdd { .. }
            | Rvalue::SetRemove { .. }
            | Rvalue::SetClear { .. }
            | Rvalue::SetUpdate { .. }
            | Rvalue::SetCopy { .. }
            | Rvalue::SetBinary { .. }
            | Rvalue::SetProjection { .. }
            | Rvalue::ProcessEnvRead { .. }
            | Rvalue::ProcessArgv
            | Rvalue::ListConcat { .. }
            | Rvalue::ListSearch { .. }
            | Rvalue::ListCallback { .. }
            | Rvalue::ListFromLength { .. }
            | Rvalue::ListFromLengthMap { .. }
            | Rvalue::ListReduce { .. }
            | Rvalue::ListSlice { .. }
            | Rvalue::ListSplice { .. }
            | Rvalue::ListFill { .. }
            | Rvalue::ListCopyWithin { .. }
            | Rvalue::ListWith { .. }
            | Rvalue::ListFlat { .. }
            | Rvalue::ListProjection { .. }
            | Rvalue::ListCopy { .. }
            | Rvalue::ListCount { .. }
            | Rvalue::ListSum { .. }
            | Rvalue::ListBoolFold { .. }
            | Rvalue::ListSorted { .. }
            | Rvalue::ListReversed { .. }
            | Rvalue::ListEnumerate { .. }
            | Rvalue::ListZip { .. }
            | Rvalue::ListRange { .. }
            | Rvalue::ListNext { .. }
            | Rvalue::TupleContains { .. }
            | Rvalue::DictContainsKey { .. }
            | Rvalue::DictSet { .. }
            | Rvalue::DictRemoveKey { .. }
            | Rvalue::DictGet { .. }
            | Rvalue::DictSetDefault { .. }
            | Rvalue::DictClear { .. }
            | Rvalue::DictUpdate { .. }
            | Rvalue::DictAssign { .. }
            | Rvalue::CallableObjectAssign { .. }
            | Rvalue::DictCopy { .. }
            | Rvalue::DictProjection { .. }
            | Rvalue::StringChars { .. }
            | Rvalue::StringJoin { .. }
            | Rvalue::FileReadText { .. }
            | Rvalue::FileWriteText { .. }
            | Rvalue::HostConstruct { .. }
            | Rvalue::BuiltinNamespace { .. }
            | Rvalue::ArgumentsObject { .. }
            | Rvalue::FunctionTableLookup { .. }
            | Rvalue::InstanceOf { .. }
            | Rvalue::PrototypeSentinel { .. }
            | Rvalue::BoxPrimitive { .. }
            | Rvalue::ObjectFromPrototype { .. }
            | Rvalue::ListToSet { .. }
            | Rvalue::ListPairsToDict { .. }
            | Rvalue::TupleToList { .. }
            | Rvalue::ListToTuple { .. }
            | Rvalue::TupleToSet { .. }
            | Rvalue::TupleIndex { .. }
            | Rvalue::TupleSlice { .. }
            | Rvalue::ClosureCall { .. }
            | Rvalue::OptionalMethod { .. }
            | Rvalue::OptionalCoalesce { .. }
            | Rvalue::ListPush { .. }
            | Rvalue::ListExtend { .. }
            | Rvalue::ListInsert { .. }
            | Rvalue::ListUnshift { .. }
            | Rvalue::ListReverse { .. }
            | Rvalue::ListClear { .. }
            | Rvalue::ListSort { .. }
            | Rvalue::ListPop { .. }
            | Rvalue::ListShift { .. }
            | Rvalue::Use(..)
            | Rvalue::DictEntryPush { .. }
            | Rvalue::GeneratorYield { .. }
            | Rvalue::List(..)
            | Rvalue::Set(..)
            | Rvalue::Dict(..)
            | Rvalue::Tuple(..)
            | Rvalue::Binary { .. }
            | Rvalue::Unary { .. }
            | Rvalue::UnionMethod { .. }
            | Rvalue::Struct { .. }
            | Rvalue::ExternalClassInstance { .. }
            | Rvalue::Closure { .. }
            | Rvalue::GlobalGet { .. }
            | Rvalue::GlobalSet { .. }
            | Rvalue::Await(..) => {}
        }
    }
}
