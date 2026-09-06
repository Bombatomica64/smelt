//! Rule identities and source-shape metadata for standard-library mappings.

use crate::BackendDependency;

/// Source language that produced a standard-library call shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SourceLanguage {
    /// TypeScript or JavaScript input.
    TypeScript,
    /// Python input.
    Python,
}

/// Broad API namespace for a standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApiNamespace {
    /// JSON parse/stringify APIs.
    Json,
    /// Regular-expression APIs.
    Regex,
    /// Random-number APIs.
    Random,
    /// HTTP client APIs.
    Http,
    /// Date and datetime APIs.
    DateTime,
    /// URL parsing and field APIs.
    Url,
}

/// Receiver shape for a source API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReceiverKind {
    /// No receiver, such as `fetch(url)`.
    FreeFunction,
    /// Static namespace receiver, such as `JSON.parse`.
    Namespace,
    /// Instance receiver, such as `new RegExp(pattern).test(text)`.
    Instance,
}

/// Source API call shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ApiShape {
    /// A function or method call.
    Call,
    /// A constructor call.
    Constructor,
    /// A property access.
    Property,
}

/// Argument shape required by a supported rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ArgShape {
    /// Exactly one value argument.
    OneValue,
    /// Exactly one string argument.
    OneString,
    /// Exactly two string arguments.
    TwoStrings,
    /// Exactly two integer arguments.
    TwoInts,
    /// Exactly one numeric argument.
    OneNumber,
    /// No arguments.
    None,
}

/// Return shape produced by a standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReturnShape {
    /// Boolean result.
    Bool,
    /// String result.
    String,
    /// Floating-point result.
    Float,
    /// Integer result.
    Int,
    /// Type is supplied by call-site context.
    Contextual,
}

/// Side-effect profile of a standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EffectKind {
    /// Pure deterministic mapping.
    Pure,
    /// Random-value generation.
    Random,
    /// HTTP or external IO.
    Io,
}

/// Stable identity for a recognized standard-library lowering rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum RuleId {
    /// TypeScript `JSON.stringify(value)`.
    TsJsonStringify,
    /// TypeScript `JSON.parse<T>(text)`.
    TsJsonParse,
    /// TypeScript `new RegExp(pattern).test(text)`.
    TsRegExpTest,
    /// TypeScript `Math.random()`.
    TsMathRandom,
    /// TypeScript `fetch(url)`.
    TsFetch,
    /// TypeScript `Date.now()`.
    TsDateNow,
    /// TypeScript `new Date(timestamp).toISOString()`.
    TsDateToIsoString,
    /// TypeScript `new URL(text).field`.
    TsUrlField,
    /// TypeScript `structuredClone(value)`.
    TsStructuredClone,
    /// TypeScript `Object(value)` called as a function (boxes a primitive).
    TsObjectBox,
    /// TypeScript `Promise.resolve`, `Promise.all`, `Promise.race`, or `Promise.allSettled`.
    TsPromiseStatic,
    /// TypeScript global primitive conversion or numeric parse call.
    TsPrimitiveCast,
    /// TypeScript `Symbol(...)` or `Symbol.for(...)`.
    TsSymbol,
    /// TypeScript deterministic numeric `Math.*` call.
    TsMathNumeric,
    /// TypeScript numeric `Number.*` predicate.
    TsNumberPredicate,
    /// TypeScript `Number.parseFloat(...)`.
    TsNumberParseFloat,
    /// TypeScript `Number.parseInt(...)`.
    TsNumberParseInt,
    /// TypeScript supported static `Object.*` call.
    TsObjectStatic,
    /// TypeScript supported static `Array.*` call.
    TsArrayStatic,
    /// TypeScript supported static `Buffer.*` call
    /// (`Buffer.from`/`Buffer.alloc`/`Buffer.concat`/`Buffer.isBuffer`).
    TsBufferStatic,
    /// TypeScript `Map.prototype.has`.
    TsMapHas,
    /// TypeScript `Map.prototype.get`.
    TsMapGet,
    /// TypeScript `Map` mutating method.
    TsMapMutation,
    /// TypeScript `Map` projection method.
    TsMapProjection,
    /// TypeScript `Set.prototype.has`.
    TsSetHas,
    /// TypeScript `Set` mutating method.
    TsSetMutation,
    /// TypeScript `Set` projection method.
    TsSetProjection,
    /// TypeScript `Headers.prototype.get`.
    TsHeadersGet,
    /// TypeScript `Headers.prototype.has`.
    TsHeadersHas,
    /// TypeScript `Headers` mutating method (`set`, `append`, `delete`).
    TsHeadersMutation,
    /// TypeScript `Headers` projection method (`keys`, `values`, `entries`,
    /// `getSetCookie`).
    TsHeadersProjection,
    /// TypeScript `URLSearchParams` read (`get`, `getAll`, `has`).
    TsUrlSearchParamsRead,
    /// TypeScript `URLSearchParams` mutating method (`set`, `append`,
    /// `delete`, `sort`).
    TsUrlSearchParamsMutation,
    /// TypeScript `URLSearchParams` projection (`keys`, `values`, `entries`).
    TsUrlSearchParamsProjection,
    /// TypeScript `URLSearchParams.prototype.toString`.
    TsUrlSearchParamsToString,
    /// TypeScript `Response` data-property read (`status`, `ok`, `statusText`,
    /// `headers`, `bodyUsed`).
    TsResponseRead,
    /// TypeScript `Response` body reader (`text`).
    ///
    /// Separate from [`Self::TsResponseRead`] because a body reader is `async`
    /// *and* single-use: it answers a `Promise` and consumes the body, so a
    /// second call is the spec's `TypeError`. A data-property read does neither.
    TsResponseBodyRead,
    /// TypeScript `Response.prototype.clone`.
    ///
    /// Its own rule because the spec gives the clone an independent unread
    /// body, which is not what any other `Response` member does.
    TsResponseClone,
    /// TypeScript `Request` data-property read (`url`, `method`, `headers`,
    /// `bodyUsed`).
    TsRequestRead,
    /// TypeScript `Request` body reader (`text`).
    TsRequestBodyRead,
    /// TypeScript `Request.prototype.clone`.
    TsRequestClone,
    /// TypeScript `EventEmitter` listener registration (`on`, `addListener`,
    /// `once`).
    TsEventEmitterRegister,
    /// TypeScript `EventEmitter` listener removal (`off`, `removeListener`,
    /// `removeAllListeners`).
    TsEventEmitterRemove,
    /// TypeScript `EventEmitter.prototype.emit`.
    TsEventEmitterEmit,
    /// TypeScript `EventEmitter` read (`listenerCount`).
    TsEventEmitterRead,
    /// Python `json.dumps(value)`.
    PyJsonDumps,
    /// Python `json.loads(text)`.
    PyJsonLoads,
    /// Python `re.search(pattern, text)`.
    PyReSearch,
    /// Python `re.match(pattern, text)`.
    PyReMatch,
    /// Python `re.fullmatch(pattern, text)`.
    PyReFullMatch,
    /// Python `random.random()`.
    PyRandomRandom,
    /// Python `random.randint(start, end)`.
    PyRandomRandInt,
    /// Python `random.choice(values)`.
    PyRandomChoice,
    /// Python `requests.get(url)`.
    PyRequestsGet,
    /// Python `datetime.datetime.now()` or `utcnow()`.
    PyDateTimeNow,
    /// Python `datetime.datetime.fromtimestamp(seconds)`.
    PyDateTimeFromTimestamp,
    /// Python `urllib.parse.urlparse(text).field`.
    PyUrlparseField,
}

impl RuleId {
    /// Return the backend dependency required by this rule, when any.
    #[must_use]
    pub const fn backend_dependency(self) -> Option<BackendDependency> {
        match self {
            Self::TsJsonStringify | Self::TsJsonParse | Self::PyJsonDumps | Self::PyJsonLoads => {
                Some(BackendDependency::SerdeJson)
            }
            Self::TsRegExpTest | Self::PyReSearch | Self::PyReMatch | Self::PyReFullMatch => {
                Some(BackendDependency::Regex)
            }
            Self::TsMathRandom
            | Self::PyRandomRandom
            | Self::PyRandomRandInt
            | Self::PyRandomChoice => Some(BackendDependency::Rand),
            Self::TsFetch | Self::PyRequestsGet => Some(BackendDependency::Reqwest),
            Self::TsDateNow
            | Self::TsDateToIsoString
            | Self::PyDateTimeNow
            | Self::PyDateTimeFromTimestamp => Some(BackendDependency::Chrono),
            // `URLSearchParams` serializes and parses through
            // `url::form_urlencoded`, which the `url` crate already carries.
            Self::TsUrlField
            | Self::PyUrlparseField
            | Self::TsUrlSearchParamsRead
            | Self::TsUrlSearchParamsMutation
            | Self::TsUrlSearchParamsProjection
            | Self::TsUrlSearchParamsToString
            // `new Request(input)` answers the WHATWG-SERIALIZED url
            // (`https://a.test` reads back as `https://a.test/`), which is
            // `url::Url`'s own serialization rather than the input string.
            | Self::TsRequestRead => Some(BackendDependency::Url),
            Self::TsStructuredClone
            | Self::TsObjectBox
            | Self::TsPromiseStatic
            | Self::TsPrimitiveCast
            | Self::TsSymbol
            | Self::TsMathNumeric
            | Self::TsNumberPredicate
            | Self::TsNumberParseFloat
            | Self::TsNumberParseInt
            | Self::TsObjectStatic
            | Self::TsArrayStatic
            | Self::TsBufferStatic
            | Self::TsMapHas
            | Self::TsMapGet
            | Self::TsMapMutation
            | Self::TsMapProjection
            | Self::TsSetHas
            | Self::TsSetMutation
            | Self::TsSetProjection
            // `SmeltHeaders` is a generated runtime type with no external crate
            // behind it: WHATWG header semantics (case folding, comma-joined
            // reads, the `Set-Cookie` carve-out) are the implementation, so
            // there is nothing to depend on.
            | Self::TsHeadersGet
            | Self::TsHeadersHas
            | Self::TsHeadersMutation
            | Self::TsHeadersProjection
            // `Response` is a generated concrete type: a status line, a
            // `SmeltHeaders`, and a buffered `SmeltBody`. Nothing in that needs
            // a crate, so it adds no backend dependency (`fetch` returning one
            // is what pulls in reqwest, under its own rule).
            | Self::TsResponseRead
            | Self::TsResponseBodyRead
            | Self::TsResponseClone
            | Self::TsRequestBodyRead
            | Self::TsRequestClone
            // The emitter is a generated list of erased callbacks; nothing in
            // it needs a crate.
            | Self::TsEventEmitterRegister
            | Self::TsEventEmitterRemove
            | Self::TsEventEmitterEmit
            | Self::TsEventEmitterRead => None,
        }
    }

    /// Return a concise source API name for diagnostics.
    #[must_use]
    pub const fn source_api(self) -> &'static str {
        match self {
            Self::TsJsonStringify => "JSON.stringify",
            Self::TsJsonParse => "JSON.parse",
            Self::TsRegExpTest => "RegExp.test",
            Self::TsMathRandom => "Math.random",
            Self::TsFetch => "fetch",
            Self::TsDateNow => "Date.now",
            Self::TsDateToIsoString => "Date.toISOString",
            Self::TsUrlField => "URL field access",
            Self::TsStructuredClone => "structuredClone",
            Self::TsObjectBox => "Object",
            Self::TsPromiseStatic => "Promise static method",
            Self::TsPrimitiveCast => "primitive conversion",
            Self::TsSymbol => "Symbol",
            Self::TsMathNumeric => "Math numeric method",
            Self::TsNumberPredicate => "Number predicate",
            Self::TsNumberParseFloat => "Number.parseFloat",
            Self::TsNumberParseInt => "Number.parseInt",
            Self::TsObjectStatic => "Object static method",
            Self::TsArrayStatic => "Array static method",
            Self::TsBufferStatic => "Buffer static method",
            Self::TsMapHas => "Map.has",
            Self::TsMapGet => "Map.get",
            Self::TsMapMutation => "Map mutation method",
            Self::TsMapProjection => "Map projection method",
            Self::TsSetHas => "Set.has",
            Self::TsSetMutation => "Set mutation method",
            Self::TsSetProjection => "Set projection method",
            Self::TsHeadersGet => "Headers.get",
            Self::TsHeadersHas => "Headers.has",
            Self::TsHeadersMutation => "Headers mutation method",
            Self::TsHeadersProjection => "Headers projection method",
            Self::TsUrlSearchParamsRead => "URLSearchParams read method",
            Self::TsUrlSearchParamsMutation => "URLSearchParams mutation method",
            Self::TsUrlSearchParamsProjection => "URLSearchParams projection method",
            Self::TsUrlSearchParamsToString => "URLSearchParams.toString",
            Self::TsResponseRead => "Response property read",
            Self::TsResponseBodyRead => "Response body reader",
            Self::TsResponseClone => "Response.clone",
            Self::TsRequestRead => "Request property read",
            Self::TsRequestBodyRead => "Request body reader",
            Self::TsRequestClone => "Request.clone",
            Self::TsEventEmitterRegister => "EventEmitter listener registration",
            Self::TsEventEmitterRemove => "EventEmitter listener removal",
            Self::TsEventEmitterEmit => "EventEmitter.emit",
            Self::TsEventEmitterRead => "EventEmitter read",
            Self::PyJsonDumps => "json.dumps",
            Self::PyJsonLoads => "json.loads",
            Self::PyReSearch => "re.search",
            Self::PyReMatch => "re.match",
            Self::PyReFullMatch => "re.fullmatch",
            Self::PyRandomRandom => "random.random",
            Self::PyRandomRandInt => "random.randint",
            Self::PyRandomChoice => "random.choice",
            Self::PyRequestsGet => "requests.get",
            Self::PyDateTimeNow => "datetime.datetime.now",
            Self::PyDateTimeFromTimestamp => "datetime.datetime.fromtimestamp",
            Self::PyUrlparseField => "urllib.parse.urlparse field access",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert that newly registry-backed Chrono and URL rules expose stable
    /// dependency and source API metadata for frontend diagnostics/codegen.
    #[test]
    fn dependency_backed_date_and_url_rules_report_metadata() {
        let cases = [
            (RuleId::TsDateNow, BackendDependency::Chrono, "Date.now"),
            (
                RuleId::TsDateToIsoString,
                BackendDependency::Chrono,
                "Date.toISOString",
            ),
            (
                RuleId::PyDateTimeNow,
                BackendDependency::Chrono,
                "datetime.datetime.now",
            ),
            (
                RuleId::PyDateTimeFromTimestamp,
                BackendDependency::Chrono,
                "datetime.datetime.fromtimestamp",
            ),
            (
                RuleId::TsUrlField,
                BackendDependency::Url,
                "URL field access",
            ),
            (
                RuleId::PyUrlparseField,
                BackendDependency::Url,
                "urllib.parse.urlparse field access",
            ),
        ];

        for (rule, dependency, source_api) in cases {
            assert_eq!(rule.backend_dependency(), Some(dependency));
            assert_eq!(rule.source_api(), source_api);
        }
    }
}
