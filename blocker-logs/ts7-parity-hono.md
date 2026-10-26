# TypeScript 7.1.0-beta type parity

Inputs: hono/Smelt.toml

**Parity: 91.8%** (15281 of 16655 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 15281 |
| smelt-erased | 554 |
| wider | 256 |
| mismatch | 564 |
| ts-dynamic | 1443 |
| ts-deferred | 5 |
| no-checker-node | 635 |

## Smelt-erased families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 109 | `Unknown ← Named(Hono)` | hono/src/middleware/cors/index.test.ts:7:3 `Hono<{}, BlankSchema, "/", "/api/*">` (erasure: type-level) |
| 66 | `Unknown ← string` | hono/src/utils/url.ts:293:41 `string` |
| 62 | `Unknown ← void` | hono/src/router/reg-exp-router/router.ts:82:11 `void` |
| 54 | `Unknown ← Named(Context)` | hono/src/hono-base.ts:378:26 `Context<any, any, {}>` |
| 48 | `Unknown ← Named(Response) & Object{3 props}` | hono/src/context.ts:746:35 `Response & TypedResponse<null, StatusCode, "body">` |
| 25 | `Unknown ← number` | hono/src/utils/url.ts:230:9 `number` |
| 16 | `Unknown ← Function` | hono/src/hono-base.ts:209:30 `() => string` |
| 13 | `Unknown ← Array<string>` | hono/src/middleware/cors/index.ts:72:28 `string[]` |
| 13 | `Unknown ← Object{0 props}` | hono/src/request.ts:444:19 `Record<string, string>` |
| 11 | `Unknown ← string \| Named(File)` | hono/src/utils/body.ts:171:7 `string \| File` |
| 9 | `Unknown ← Named(Promise)` | hono/src/hono-base.ts:352:12 `Promise<Context<any, any, {}>>` |
| 9 | `Unknown ← string \| Array<… \| …> \| Named(File)` | hono/src/utils/body.ts:203:7 `string \| (string \| File)[] \| File` |
| 8 | `Unknown ← Named(Response)` | hono/src/hono-base.ts:40:17 `Response` (erasure: by-name) |
| 7 | `Unknown ← Named(URLSearchParams)` | hono/src/middleware/mount/index.test.ts:22:30 `URLSearchParams` (erasure: by-name) |
| 6 | `Unknown ← Array<[…, …]> \| Array<[…, …]>` | hono/src/hono-base.ts:324:22 `[[H, RouterRoute], ParamIndexMap][] \| [[H, RouterRoute], Params][]` |
| 6 | `Unknown ← Named(Error)` | hono/src/hono-base.ts:366:15 `Error` (erasure: by-name) |
| 6 | `Unknown ← TypeParam` | hono/src/hono-base.ts:231:7 `this` |
| 6 | `Unknown ← null \| string` | hono/src/middleware/trailing-slash/index.test.ts:46:14 `string \| null` |
| 6 | `Unknown ← undefined \| string` | hono/src/request.ts:187:14 `string \| undefined` |
| 5 | `Unknown ← Function \| Function` | hono/src/hono-base.ts:278:66 `H<any, any, BlankInput, any>` |
| 5 | `Unknown ← Named(Promise) \| Named(Response)` | hono/src/hono-base.ts:330:14 `Promise<Response> \| Response` |
| 5 | `Unknown ← string \| Object{0 props} \| Named(File)` | hono/src/utils/body.test.ts:100:12 `string \| BodyDataValueDot \| File` |
| 4 | `Unknown ← Named(SubtleCrypto)` | hono/src/utils/crypto.ts:45:17 `SubtleCrypto` (erasure: by-name) |
| 4 | `Unknown ← string \| Array<string> \| Function` | hono/src/middleware/cors/index.ts:76:16 `string \| string[] \| ((origin: string, c: Context<any, any, {}>) => string \| Prom` |
| 4 | `Unknown ← string \| Array<… \| …> \| Object{0 props} \| Named(File)` | hono/src/utils/body.test.ts:426:20 `string \| (string \| File)[] \| BodyDataValueDotAll \| File` |
| 3 | `Unknown ← Array<[TypeParam, string]>` | hono/src/router/reg-exp-router/router.ts:156:26 `HandlerWithMetadata<T>[]` |
| 3 | `Unknown ← Array<string \| Named(File)>` | hono/src/utils/body.ts:205:9 `(string \| File)[]` |
| 3 | `Unknown ← Array<string> \| Function` | hono/src/middleware/cors/index.ts:82:23 `string[] \| ((origin: string, c: Context<any, any, {}>) => string \| Promise<strin` |
| 3 | `Unknown ← Function \| Function \| Function \| Function \| Function` | hono/src/request.ts:237:16 `(() => Promise<ArrayBuffer>) \| (() => Promise<Blob>) \| (() => Promise<FormData>)` |
| 3 | `Unknown ← string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal` | hono/src/utils/mime.ts:21:20 `BaseMime` |

## Wider-type families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 29 | `Optional<String> ← string` | hono/src/utils/url.ts:64:11 `string` |
| 21 | `Optional<List<(…, …)> \| List<…> \| List<…>> ← Array<[…, …]> \| Array<[…, …]>` | hono/src/router/reg-exp-router/router.test.ts:33:14 `[string, ParamIndexMap][] \| [string, Params][]` |
| 17 | `Optional<Class(Node_1)> ← Named(Node)` | hono/src/router/reg-exp-router/node.ts:100:9 `Node` |
| 16 | `Optional<Float> ← number` | hono/src/router/reg-exp-router/node.ts:124:32 `number` |
| 15 | `Optional<List<String>> ← Array<string>` | hono/src/router/reg-exp-router/node.ts:82:22 `string[]` |
| 13 | `Optional<Class(Node)> ← Named(Node)` | hono/src/router/trie-router/node.ts:42:22 `Node<T>` |
| 13 | `String \| Future<String> ← string` | hono/src/utils/html.ts:61:12 `string` |
| 9 | `Optional<(String, Dict) \| String \| (…, …)> ← [string, Object{0 props}] \| [string, Object{0 props}]` | hono/src/router/reg-exp-router/router.test.ts:195:16 `[string, ParamIndexMap] \| [string, Params]` |
| 6 | `Optional<Class(Response)> ← Named(Response)` | hono/src/http-exception.ts:68:40 `Response` |
| 6 | `Optional<Class(Trie)> ← Named(Trie)` | hono/src/router/reg-exp-router/router.ts:62:7 `Trie` |
| 6 | `Optional<String \| Dict \| String \| … \| …> ← string` | hono/src/router/reg-exp-router/router.test.ts:195:16 `string` |
| 5 | `Class(RegExp) \| Bool ← Named(RegExp)` | hono/src/router/trie-router/node.ts:165:23 `RegExp` |
| 5 | `Optional<Dict> ← Object{0 props}` | hono/src/router/reg-exp-router/router.ts:157:26 `Record<string, [number, ParamAssocArray]>` |
| 4 | `Optional<(Float, List<(…, …)>)> ← [number, Array<[…, …]>]` | hono/src/router/reg-exp-router/trie.ts:56:5 `[number, ParamAssocArray]` |
| 4 | `Optional<Optional<List<String>>> ← null \| Array<string>` | hono/src/router/reg-exp-router/node.ts:66:9 `string[] \| null` |
| 4 | `String \| Class(Request) \| Class(URL) ← string` | hono/src/hono-base.ts:474:9 `string` |
| 3 | `Future<String> \| Class(HtmlEscapedString) ← Named(Promise)` | hono/src/utils/html.ts:149:36 `Promise<string>` |
| 3 | `Optional<Class(HandlerSet)> ← Object{3 props}` | hono/src/router/trie-router/node.ts:71:27 `HandlerSet<T>` |
| 3 | `Optional<Dict \| (…, …) \| Dict \| (…, …)> ← Object{0 props} \| Object{0 props}` | hono/src/request.ts:104:22 `ParamIndexMap \| Params` |
| 3 | `Optional<Function> ← Function` | hono/src/router/reg-exp-router/node.ts:157:26 `() => string` |
| 3 | `Optional<JsMap> ← Named(Map)` | hono/src/context.ts:614:7 `Map<unknown, unknown>` |
| 3 | `Optional<List<… \| …> \| Dict \| List<…>> ← Array<string \| number>` | hono/src/router/reg-exp-router/prepared-router.ts:77:17 `(string \| number)[]` |
| 3 | `Optional<String \| Dict \| String \| … \| …> ← Object{0 props} \| Object{0 props}` | hono/src/router/reg-exp-router/router.test.ts:196:39 `ParamIndexMap \| Params` |
| 2 | `Float \| String ← string` | hono/src/router/reg-exp-router/prepared-router.ts:41:24 `string` |
| 2 | `Optional<Class(Date)> ← Named(Date)` | hono/src/utils/cookie.ts:225:9 `Date` |
| 2 | `Optional<Class(HandlerParamsSet)> ← Object{3 props} & Object{1 props}` | hono/src/router/trie-router/node.ts:71:26 `HandlerParamsSet<T>` |
| 2 | `Optional<Class(Headers)> ← Named(Headers)` | hono/src/context.ts:677:11 `Headers` |
| 2 | `Optional<Function> \| Optional<Bool \| Function> ← undefined \| Function` | hono/src/middleware/mount/index.ts:80:23 `MountOptionHandler \| undefined` |
| 2 | `Optional<List<(String, String, TypeParam(T))>> ← Array<[string, string, TypeParam]>` | hono/src/router/smart-router/router.ts:27:20 `[string, string, T][]` |
| 2 | `Optional<List<(…, …) \| (…)>> ← Array<[…] \| […, …]>` | hono/src/router/reg-exp-router/prepared-router.ts:73:18 `([(string \| number)[]] \| [(string \| number)[], ParamIndexMap])[]` |

## Mismatch families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 62 | `String ← Named(URL)` | hono/src/middleware/mount/index.ts:24:15 `URL` |
| 40 | `String ← Named(RegExp)` | hono/src/utils/url.ts:26:23 `RegExp` |
| 28 | `Function ← Named(TextRespond)` | hono/src/middleware/cors/index.test.ts:293:37 `TextRespond` |
| 26 | `String ← Named(Error)` | hono/src/hono-base.ts:58:11 `Error` (erasure: untyped-builtin) |
| 17 | `Bool ← Object{1 props}` | hono/src/router/pattern-router/router.test.ts:21:23 `typeof UnsupportedPathError` (erasure: test-harness) |
| 14 | `Class(__SmeltMatch) ← Named(RegExpMatchArray)` | hono/src/utils/url.ts:64:11 `RegExpMatchArray` |
| 14 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpMatchArray)` | hono/src/utils/url.ts:60:17 `RegExpMatchArray \| null` |
| 13 | `Function ← Named(JSONRespond)` | hono/src/middleware/cors/index.test.ts:96:12 `JSONRespond` |
| 11 | `Class(Response) ← undefined \| Named(Response)` | hono/src/hono-base.ts:337:7 `Response \| undefined` |
| 11 | `List<Unknown> ← Array<string>` | hono/src/hono-base.ts:206:23 `string[]` (erasure: by-name) |
| 10 | `List<Unknown> ← Array<[[…, …], Object{0 props}]>` | hono/src/compose.test.ts:472:19 `MiddlewareTuple[]` |
| 10 | `Optional<Unknown> ← string` | hono/src/context.ts:675:19 `string` |
| 9 | `Bool ← undefined \| string-literal \| boolean-literal \| boolean-literal` | hono/src/utils/url.ts:66:11 `"" \| boolean \| undefined` |
| 8 | `List<String> ← undefined \| Array<string>` | hono/src/request.ts:124:12 `ParamStash \| undefined` |
| 8 | `Optional<List<(…, …)> \| List<…> \| List<…>> ← undefined \| Array<string>` | hono/src/router/reg-exp-router/router.test.ts:33:19 `ParamStash \| undefined` |
| 8 | `String \| None \| Class(Blob) \| Class(Int8Array) \| Class(Uint8Array) \| Class(Uint8ClampedArray) \| Class(Int16Array) \| Class(Uint16Array) \| Class(Int32Array) \| Class(Uint32Array) \| Class(Float32Array) \| Class(Float64Array) \| Class(BigInt64Array) \| Class(BigUint64Array) \| Class(ArrayBuffer) \| Class(FormData) \| Class(ReadableStream) \| Class(URLSearchParams) ← string \| Named(ArrayBuffer) \| Named(ArrayBufferView) \| Named(Blob) \| Named(FormData) \| Named(ReadableStream) \| Named(URLSearchParams)` | hono/src/request.ts:228:11 `BodyInit` |
| 7 | `Future<Unknown> ← Named(Promise)` | hono/src/hono-base.ts:378:32 `Promise<Context<any, any, {}>>` (erasure: by-name) |
| 7 | `List<String> ← Named(RegExpMatchArray) \| []` | hono/src/router/reg-exp-router/trie.ts:44:20 `RegExpMatchArray \| []` |
| 7 | `Optional<String \| (String, String, … \| …)> ← undefined \| string \| [string, string, … \| …]` | hono/src/router/trie-router/node.ts:43:23 `string \| readonly [string, string, true \| RegExp] \| undefined` |
| 6 | `List<String \| Future<String>> ← Array<string>` | hono/src/utils/html.ts:59:26 `string[]` |
| 5 | `Bool ← undefined \| boolean-literal \| boolean-literal` | hono/src/context.ts:419:9 `boolean \| undefined` |
| 5 | `Float \| String \| Bool \| None \| Class(Int8Array) \| Class(Uint8Array) \| Class(Uint8ClampedArray) \| Class(Int16Array) \| Class(Uint16Array) \| Class(Int32Array) \| Class(Uint32Array) \| Class(Float32Array) \| Class(Float64Array) \| Class(BigInt64Array) \| Class(BigUint64Array) \| Class(DataView) \| Class(ArrayBuffer) \| List<Float \| String \| Bool \| None \| List<…> \| Dict> \| Dict ← null \| string \| number \| boolean-literal \| boolean-literal \| Named(ArrayBuffer) \| Named(ArrayBufferView) \| Array<… \| … \| … \| … \| … \| … \| …> \| Object{0 props}` | hono/src/utils/crypto.ts:15:30 `Data` |
| 5 | `List<((…, …), Unknown) \| ((…))> ← Array<[[…, …], Object{0 props}]>` | hono/src/compose.test.ts:703:19 `MiddlewareTuple[]` |
| 5 | `List<(String, Dict)> \| List<String> \| List<(…, …)> ← number` | hono/src/router/reg-exp-router/router.test.ts:34:14 `number` |
| 4 | `Class(__SmeltMatch) ← Named(RegExpExecArray)` | hono/src/router/trie-router/node.ts:167:30 `RegExpExecArray` |
| 4 | `Function ← Named(SetHeaders)` | hono/src/middleware/cors/index.test.ts:283:7 `SetHeaders` |
| 4 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpExecArray)` | hono/src/router/trie-router/node.ts:165:23 `RegExpExecArray \| null` |
| 4 | `Optional<Float \| Class(Response) \| Class(ResponseInit)> ← undefined \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| Named(Response) \| Named(ResponseInit)` | hono/src/context.ts:767:58 `ContentfulStatusCode \| ResponseOrInit<StatusCode> \| undefined` |
| 4 | `Optional<Unknown> ← Object{0 props}` | hono/src/router/reg-exp-router/prepared-router.ts:158:17 `MatcherMap<unknown>` |
| 4 | `Optional<Unknown> ← undefined \| string` | hono/src/hono-base.ts:306:43 `string \| undefined` (erasure: by-name) |

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| hono/src/preset/tiny.test.ts | 54.8% | 17 | 12 | 0 | 2 | 3 |
| hono/src/router/reg-exp-router/matcher.ts | 66.0% | 31 | 10 | 0 | 6 | 2 |
| hono/src/router/common.case.test.ts | 71.4% | 5 | 0 | 1 | 1 | 6 |
| hono/src/hono.ts | 75.0% | 9 | 0 | 1 | 2 | 0 |
| hono/src/middleware/pretty-json/index.ts | 76.8% | 43 | 5 | 0 | 8 | 3 |
| hono/src/utils/crypto.ts | 78.0% | 46 | 1 | 0 | 12 | 0 |
| hono/src/router/reg-exp-router/router.test.ts | 79.8% | 520 | 46 | 53 | 33 | 43 |
| hono/src/preset/tiny.ts | 80.0% | 4 | 0 | 0 | 1 | 0 |
| hono/src/middleware/cors/index.ts | 80.4% | 160 | 28 | 0 | 11 | 0 |
| hono/src/helper/route/index.ts | 81.7% | 107 | 10 | 8 | 6 | 5 |
| hono/src/middleware/trailing-slash/index.test.ts | 81.8% | 611 | 78 | 0 | 58 | 92 |
| hono/src/middleware/powered-by/index.test.ts | 82.6% | 76 | 13 | 0 | 3 | 9 |
| hono/src/utils/html.ts | 83.6% | 179 | 0 | 20 | 15 | 10 |
| hono/src/middleware/pretty-json/index.test.ts | 83.9% | 381 | 58 | 0 | 15 | 31 |
| hono/src/utils/body.ts | 85.0% | 192 | 21 | 1 | 12 | 10 |
| hono/src/context.ts | 85.0% | 341 | 8 | 13 | 39 | 7 |
| hono/src/middleware/timeout/index.test.ts | 85.2% | 115 | 16 | 0 | 4 | 16 |
| hono/src/hono-base.ts | 85.2% | 445 | 45 | 8 | 24 | 16 |
| hono/src/compose.ts | 85.4% | 88 | 5 | 0 | 10 | 13 |
| hono/src/utils/color.ts | 85.7% | 12 | 1 | 0 | 1 | 10 |
| hono/src/middleware/serve-static/path.ts | 86.0% | 37 | 0 | 0 | 6 | 0 |
| hono/src/middleware/trailing-slash/index.ts | 86.1% | 186 | 10 | 0 | 20 | 0 |
| hono/src/request.ts | 86.4% | 241 | 13 | 7 | 18 | 21 |
| hono/src/router/reg-exp-router/prepared-router.ts | 86.6% | 309 | 10 | 14 | 24 | 3 |
| hono/src/middleware/mount/index.ts | 86.8% | 66 | 1 | 2 | 7 | 1 |
| hono/src/router/reg-exp-router/node.ts | 86.9% | 311 | 0 | 39 | 8 | 8 |
| hono/src/router/pattern-router/router.ts | 87.1% | 121 | 1 | 3 | 14 | 3 |
| hono/src/router/reg-exp-router/router.ts | 87.5% | 267 | 11 | 16 | 11 | 17 |
| hono/src/helper/dev/index.ts | 88.1% | 118 | 12 | 0 | 4 | 0 |
| hono/src/router/pattern-router/router.test.ts | 88.9% | 16 | 1 | 0 | 1 | 1 |

## TypeScript checker errors

| code | count | first message |
| --- | ---: | --- |
| TS2304 | 1347 | Cannot find name 'expect'. |
| TS2307 | 4 | Cannot find module 'vitest' or its corresponding type declarations. |
| TS2552 | 1 | Cannot find name 'expect'. Did you mean 'expected'? |
| TS2591 | 2 | Cannot find name 'node:path/posix'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig. |
| TS2593 | 719 | Cannot find name 'describe'. Do you need to install type definitions for a test runner? Try `npm i --save-dev @types/jest` or `npm i --save-dev @types/mocha` and then add 'jest' or 'mocha' to the types field in your tsconfig. |
| TS7006 | 14 | Parameter 'value' implicitly has an 'any' type. |
| TS7031 | 2 | Binding element 'task' implicitly has an 'any' type. |
