//! Rule identities for standard-library mappings.

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
    /// TypeScript `Promise.resolve`, `Promise.reject`, `Promise.all`, `Promise.race`, or `Promise.allSettled`.
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
    /// TypeScript `BigInt.asIntN(bits, value)` or `BigInt.asUintN(bits, value)`.
    TsBigIntStatic,
    /// TypeScript `String.fromCharCode(...)` / `String.fromCodePoint(...)`.
    TsStringFromCodes,
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
    /// TypeScript `FormData` read (`get`, `getAll`, `has`).
    TsFormDataRead,
    /// TypeScript `FormData` mutating method (`set`, `append`, `delete`).
    TsFormDataMutation,
    /// TypeScript `FormData` projection (`keys`, `values`, `entries`,
    /// `forEach`).
    TsFormDataProjection,
    /// TypeScript `AbortSignal.abort(reason?)`.
    TsAbortSignalAbort,
    /// TypeScript `AbortSignal.timeout(ms)`.
    ///
    /// Its own rule rather than a shared `AbortSignal` static: `timeout` is the
    /// one that needs the async runtime — it schedules the abort on the timer
    /// queue — where `abort` answers an already-settled signal synchronously.
    TsAbortSignalTimeout,
    /// TypeScript `crypto.randomUUID()`.
    TsCryptoRandomUuid,
    /// TypeScript `crypto.getRandomValues(view)`.
    TsCryptoGetRandomValues,
    /// TypeScript `crypto.subtle.digest(algorithm, data)`.
    ///
    /// Its own rule rather than a member of a shared `subtle` rule: `digest` is
    /// the one `SubtleCrypto` method with no key, so it is the only one whose
    /// whole input is the algorithm name and the bytes.
    TsCryptoDigest,
    /// TypeScript `crypto.subtle.importKey(format, keyData, algorithm,
    /// extractable, usages)`: a `CryptoKey` from raw key material (HMAC).
    TsCryptoImportKey,
    /// TypeScript `crypto.subtle.sign(algorithm, key, data)`: an HMAC tag.
    TsCryptoSign,
    /// TypeScript `crypto.subtle.verify(algorithm, key, signature, data)`.
    TsCryptoVerify,
    /// TypeScript `TextEncoder.prototype.encode`.
    TsTextEncoderEncode,
    /// TypeScript `TextDecoder.prototype.decode`.
    TsTextDecoderDecode,
    /// TypeScript `Blob`/`File` data-property read (`size`, `type`, `name`,
    /// `lastModified`).
    TsBlobRead,
    /// TypeScript `Blob`/`File` body reader (`text`, `arrayBuffer`, `bytes`).
    ///
    /// Separate from [`Self::TsBlobRead`] because a body reader is `async`: its
    /// result is a `Promise`, so the call site awaits rather than reads.
    TsBlobBodyRead,
    /// TypeScript `Blob.prototype.slice`.
    TsBlobSlice,
    /// TypeScript `btoa` / `atob`: the two directions of the base64 codec.
    ///
    /// One rule for both, because they are one codec and one dependency, and
    /// both are FALLIBLE in the same way — each throws the branded
    /// `InvalidCharacterError` `DOMException` for input its direction cannot
    /// represent (a code point above U+00FF for `btoa`, a malformed base64
    /// string for `atob`).
    TsBase64,
    /// TypeScript typed-array data-property read (`length`, `byteLength`,
    /// `byteOffset`, `buffer`).
    TsTypedArrayRead,
    /// TypeScript typed-array method (`subarray`, `slice`, `set`, `fill`).
    ///
    /// Separate from [`Self::TsTypedArrayRead`] because these are CALLS with a
    /// range or a source operand, where the reads are members with none; the
    /// dispatch sites are the call-handler chain and the member-read chain
    /// respectively, and one rule spanning both would make each site accept
    /// spellings the other owns.
    TsTypedArrayMethod,
    /// TypeScript `DataView` element accessor (`getInt16`, `setFloat64`, ...).
    ///
    /// Separate from [`Self::TsTypedArrayMethod`] because the element kind is
    /// an argument of the CALL rather than a property of the receiver: the
    /// accessor's own name carries the width, the signedness and the
    /// direction, and the byte order is a parameter. One rule for all twenty
    /// spellings, resolved through
    /// [`crate::data_view_accessor`](crate::data_view_accessor) rather than an
    /// entry per name.
    TsDataViewAccess,
    /// TypeScript `Response` data-property read (`status`, `ok`, `statusText`,
    /// `headers`, `bodyUsed`).
    TsResponseRead,
    /// TypeScript `Response` body reader (`text`, `json`, `formData`, `arrayBuffer`, `bytes`).
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
    /// TypeScript `Request` body reader (`text`, `json`, `formData`, `arrayBuffer`, `bytes`).
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
    /// TypeScript `node:http` `createServer(handler)`.
    TsHttpCreateServer,
    /// TypeScript `node:http` `Server.prototype.listen`.
    TsHttpServerListen,
    /// TypeScript `node:http` `Server.prototype.close`.
    TsHttpServerClose,
    /// TypeScript `node:http` `Server.prototype.address`.
    TsHttpServerAddress,
    /// TypeScript `node:http` `ServerResponse` header access (`setHeader`,
    /// `getHeader`).
    TsServerResponseHeader,
    /// TypeScript `node:http` `ServerResponse.prototype.writeHead`.
    TsServerResponseWriteHead,
    /// TypeScript `node:http` `ServerResponse.prototype.write`.
    TsServerResponseWrite,
    /// TypeScript `node:http` `ServerResponse.prototype.end`.
    TsServerResponseEnd,
    /// TypeScript `node:sqlite` `DatabaseSync` (`new DatabaseSync(path)`,
    /// `exec`, `prepare`, `close`).
    TsSqliteDatabase,
    /// TypeScript `node:sqlite` `StatementSync` (`all`, `get`, `run`).
    TsSqliteStatement,
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
