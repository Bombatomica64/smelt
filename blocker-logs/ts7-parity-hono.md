# TypeScript 7.1.0-beta type parity

Inputs: hono/Smelt.toml

**Parity: 93.9%** (15502 of 16514 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 15502 |
| smelt-erased | 291 |
| wider | 250 |
| mismatch | 471 |
| ts-dynamic | 1448 |
| ts-deferred | 5 |
| no-checker-node | 624 |

## Smelt-erased families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 55 | `Unknown ← void` | hono/src/router/reg-exp-router/router.ts:82:11 `void` |
| 46 | `Unknown ← Named(Hono)` | hono/src/middleware/cors/index.test.ts:7:3 `Hono<{}, BlankSchema, "/", "/api/*">` (erasure: type-level) |
| 26 | `Unknown ← string` | hono/src/utils/url.ts:293:41 `string` |
| 22 | `Unknown ← number` | hono/src/utils/url.ts:230:9 `number` |
| 14 | `Unknown ← Array<string>` | hono/src/middleware/cors/index.ts:72:28 `string[]` |
| 12 | `Unknown ← Object{0 props}` | hono/src/request.ts:444:19 `Record<string, string>` |
| 11 | `Unknown ← string \| Named(File)` | hono/src/utils/body.ts:171:7 `string \| File` |
| 9 | `Unknown ← string \| Array<… \| …> \| Named(File)` | hono/src/utils/body.ts:203:7 `string \| (string \| File)[] \| File` |
| 8 | `Unknown ← Function` | hono/src/helper/ssg/utils.ts:69:10 `{ (callbackfn: (previousValue: RouterRoute, currentValue: RouterRoute, currentIn` |
| 6 | `Unknown ← Array<[…, …]> \| Array<[…, …]>` | hono/src/hono-base.ts:324:22 `[[H, RouterRoute], ParamIndexMap][] \| [[H, RouterRoute], Params][]` |
| 6 | `Unknown ← TypeParam` | hono/src/utils/body.ts:131:12 `T` (erasure: unfilled-generic) |
| 6 | `Unknown ← undefined \| string` | hono/src/request.ts:187:14 `string \| undefined` |
| 5 | `Unknown ← Named(Error)` | hono/src/compose.ts:5:3 `Error` |
| 5 | `Unknown ← Named(Promise)` | hono/src/request.ts:237:16 `Promise<any>` (erasure: dynamic-operator) |
| 5 | `Unknown ← string \| Object{0 props} \| Named(File)` | hono/src/utils/body.test.ts:100:12 `string \| BodyDataValueDot \| File` |
| 4 | `Unknown ← Function \| Function` | hono/src/hono-base.ts:332:5 `((compareFn?: ((a: [[H, RouterRoute], ParamIndexMap], b: [[H, RouterRoute], Para` |
| 4 | `Unknown ← string \| Array<string> \| Function` | hono/src/middleware/cors/index.ts:76:16 `string \| string[] \| ((origin: string, c: Context<any, any, {}>) => string \| Prom` |
| 4 | `Unknown ← string \| Array<… \| …> \| Object{0 props} \| Named(File)` | hono/src/utils/body.test.ts:426:20 `string \| (string \| File)[] \| BodyDataValueDotAll \| File` |
| 3 | `Unknown ← Array<string \| Named(File)>` | hono/src/utils/body.ts:205:9 `(string \| File)[]` |
| 3 | `Unknown ← Array<string> \| Function` | hono/src/middleware/cors/index.ts:82:23 `string[] \| ((origin: string, c: Context<any, any, {}>) => string \| Promise<strin` |
| 3 | `Unknown ← Function \| Function \| Function \| Function \| Function` | hono/src/request.ts:237:16 `(() => Promise<ArrayBuffer>) \| (() => Promise<Blob>) \| (() => Promise<FormData>)` |
| 3 | `Unknown ← Named(Promise) \| Named(Response)` | hono/src/context.ts:496:65 `Promise<Response> \| Response` |
| 2 | `Unknown ← Array<never>` | hono/src/middleware/cors/index.test.ts:414:55 `never[]` |
| 2 | `Unknown ← Named(File)` | hono/src/utils/body.ts:248:23 `File` |
| 2 | `Unknown ← Named(Response)` | hono/src/compose.ts:69:17 `Response` |
| 2 | `Unknown ← Object{1 props}` | hono/src/preset/tiny.test.ts:38:20 `{ foo: string; }` |
| 2 | `Unknown ← [Array<…>] \| [Array<…>, Array<…>]` | hono/src/hono-base.ts:378:40 `Result<[H, RouterRoute]>` |
| 1 | `Unknown ← Array<Named(FilterStaticGenerateRouteData)>` | hono/src/helper/ssg/utils.ts:69:10 `FilterStaticGenerateRouteData[]` (erasure: by-name) |
| 1 | `Unknown ← Array<Named(RouterRoute)>` | hono/src/helper/ssg/utils.ts:69:10 `RouterRoute[]` (erasure: by-name) |
| 1 | `Unknown ← Array<Object{2 props}>` | hono/src/utils/body.test.ts:350:13 `{ name: string; type: string; }[]` |

## Wider-type families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 30 | `Optional<String> ← string` | hono/src/utils/url.ts:64:11 `string` |
| 21 | `Optional<List<(…, …)> \| List<…> \| List<…>> ← Array<[…, …]> \| Array<[…, …]>` | hono/src/router/reg-exp-router/router.test.ts:33:14 `[string, ParamIndexMap][] \| [string, Params][]` |
| 17 | `Optional<Class(Node_1)> ← Named(Node)` | hono/src/router/reg-exp-router/node.ts:100:9 `Node` |
| 16 | `Optional<Float> ← number` | hono/src/router/reg-exp-router/node.ts:124:32 `number` |
| 16 | `Optional<List<String>> ← Array<string>` | hono/src/request.ts:124:35 `ParamStash` |
| 13 | `Optional<Class(Node)> ← Named(Node)` | hono/src/router/trie-router/node.ts:42:22 `Node<T>` |
| 9 | `Optional<(String, Dict) \| String \| (…, …)> ← [string, Object{0 props}] \| [string, Object{0 props}]` | hono/src/router/reg-exp-router/router.test.ts:195:16 `[string, ParamIndexMap] \| [string, Params]` |
| 9 | `String \| Future<String> ← string` | hono/src/utils/html.ts:61:12 `string` |
| 6 | `Optional<Class(Response)> ← Named(Response)` | hono/src/http-exception.ts:68:40 `Response` |
| 6 | `Optional<Class(Trie)> ← Named(Trie)` | hono/src/router/reg-exp-router/router.ts:62:7 `Trie` |
| 6 | `Optional<String \| Dict \| String \| … \| …> ← string` | hono/src/router/reg-exp-router/router.test.ts:195:16 `string` |
| 5 | `Bool \| Class(RegExp) ← Named(RegExp)` | hono/src/router/trie-router/node.ts:165:23 `RegExp` |
| 5 | `Optional<Dict> ← Object{0 props}` | hono/src/router/reg-exp-router/router.ts:157:26 `Record<string, [number, ParamAssocArray]>` |
| 4 | `Optional<(Float, List<(…, …)>)> ← [number, Array<[…, …]>]` | hono/src/router/reg-exp-router/trie.ts:56:5 `[number, ParamAssocArray]` |
| 4 | `Optional<Optional<List<String>>> ← null \| Array<string>` | hono/src/router/reg-exp-router/node.ts:66:9 `string[] \| null` |
| 3 | `Future<String> \| Class(HtmlEscapedString) ← Named(Promise)` | hono/src/utils/html.ts:149:36 `Promise<string>` |
| 3 | `Optional<(…, …) \| Dict \| Dict \| (…, …)> ← Object{0 props} \| Object{0 props}` | hono/src/request.ts:104:22 `ParamIndexMap \| Params` |
| 3 | `Optional<Class(HandlerSet)> ← Object{3 props}` | hono/src/router/trie-router/node.ts:71:27 `HandlerSet<T>` |
| 3 | `Optional<Function> ← Function` | hono/src/router/reg-exp-router/node.ts:157:26 `() => string` |
| 3 | `Optional<JsMap> ← Named(Map)` | hono/src/context.ts:614:7 `Map<unknown, unknown>` |
| 3 | `Optional<List<… \| …> \| Dict \| List<…>> ← Array<string \| number>` | hono/src/router/reg-exp-router/prepared-router.ts:77:17 `(string \| number)[]` |
| 3 | `Optional<String \| Dict \| String \| … \| …> ← Object{0 props} \| Object{0 props}` | hono/src/router/reg-exp-router/router.test.ts:196:39 `ParamIndexMap \| Params` |
| 2 | `Optional<Class(Date)> ← Named(Date)` | hono/src/utils/cookie.ts:225:9 `Date` |
| 2 | `Optional<Class(HandlerParamsSet)> ← Object{3 props} & Object{1 props}` | hono/src/router/trie-router/node.ts:71:26 `HandlerParamsSet<T>` |
| 2 | `Optional<Class(Headers)> ← Named(Headers)` | hono/src/context.ts:677:11 `Headers` |
| 2 | `Optional<List<(String, String, TypeParam(T))>> ← Array<[string, string, TypeParam]>` | hono/src/router/smart-router/router.ts:27:20 `[string, string, T][]` |
| 2 | `Optional<List<(…, …) \| (…)>> ← Array<[…] \| […, …]>` | hono/src/router/reg-exp-router/prepared-router.ts:73:18 `([(string \| number)[]] \| [(string \| number)[], ParamIndexMap])[]` |
| 2 | `Optional<List<Dict> \| Function> ← Array<Object{2 props}>` | hono/src/router/pattern-router/router.test.ts:7:11 `{ reason: string; tests: string[]; }[]` (erasure: dynamic-operator) |
| 2 | `Optional<List<Dict> \| Function> ← Function` | hono/src/router/common.case.test.ts:16:3 `<T>() => Router<T>` |
| 2 | `Optional<List<Float>> ← Array<number>` | hono/src/router/trie-router/node.ts:159:17 `number[]` |

## Mismatch families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 64 | `Class(HonoBase) ← Named(Hono)` | hono/src/middleware/cors/index.test.ts:95:3 `Hono<BlankEnv, { "/api/abc": { $get: { output: { success: true; }; outputFormat:` (erasure: type-level) |
| 40 | `String ← Named(RegExp)` | hono/src/utils/buffer.ts:80:43 `RegExp` |
| 26 | `String ← Named(Error)` | hono/src/compose.ts:5:32 `Error` (erasure: untyped-builtin) |
| 17 | `Bool ← Object{1 props}` | hono/src/router/pattern-router/router.test.ts:21:23 `typeof UnsupportedPathError` (erasure: test-harness) |
| 14 | `Class(__SmeltMatch) ← Named(RegExpMatchArray)` | hono/src/utils/url.ts:64:11 `RegExpMatchArray` |
| 14 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpMatchArray)` | hono/src/utils/url.ts:60:17 `RegExpMatchArray \| null` |
| 11 | `Class(Response) ← undefined \| Named(Response)` | hono/src/compose.ts:77:9 `Response \| undefined` |
| 9 | `Bool ← undefined \| string-literal \| boolean-literal \| boolean-literal` | hono/src/utils/url.ts:66:11 `"" \| boolean \| undefined` |
| 8 | `Optional<List<(…, …)> \| List<…> \| List<…>> ← undefined \| Array<string>` | hono/src/router/reg-exp-router/router.test.ts:33:19 `ParamStash \| undefined` |
| 8 | `String \| Class(Int8Array) \| Class(Uint8Array) \| Class(Uint8ClampedArray) \| Class(Int16Array) \| Class(Uint16Array) \| Class(Int32Array) \| Class(Uint32Array) \| Class(Float32Array) \| Class(Float64Array) \| Class(BigInt64Array) \| Class(BigUint64Array) \| Class(ArrayBuffer) \| None \| Class(Blob) \| Class(FormData) \| Class(ReadableStream) \| Class(URLSearchParams) ← string \| Named(ArrayBuffer) \| Named(ArrayBufferView) \| Named(Blob) \| Named(FormData) \| Named(ReadableStream) \| Named(URLSearchParams)` | hono/src/request.ts:228:11 `BodyInit` |
| 7 | `List<String> ← Named(RegExpMatchArray) \| []` | hono/src/router/reg-exp-router/trie.ts:44:20 `RegExpMatchArray \| []` |
| 7 | `List<String> ← undefined \| Array<string>` | hono/src/middleware/cors/index.test.ts:159:12 `string[] \| undefined` |
| 7 | `Optional<String \| (String, String, … \| …)> ← undefined \| string \| [string, string, … \| …]` | hono/src/router/trie-router/node.ts:43:23 `string \| readonly [string, string, true \| RegExp] \| undefined` |
| 6 | `List<String \| Future<String>> ← Array<string>` | hono/src/utils/html.ts:59:26 `string[]` |
| 5 | `Bool ← undefined \| boolean-literal \| boolean-literal` | hono/src/context.ts:419:9 `boolean \| undefined` |
| 5 | `List<((…, …), Unknown) \| ((…))> ← Array<[[…, …], Object{0 props}]>` | hono/src/compose.test.ts:703:19 `MiddlewareTuple[]` |
| 5 | `List<(String, Dict)> \| List<String> \| List<(…, …)> ← number` | hono/src/router/reg-exp-router/router.test.ts:34:14 `number` |
| 5 | `String \| Bool \| Float \| Class(Int8Array) \| Class(Uint8Array) \| Class(Uint8ClampedArray) \| Class(Int16Array) \| Class(Uint16Array) \| Class(Int32Array) \| Class(Uint32Array) \| Class(Float32Array) \| Class(Float64Array) \| Class(BigInt64Array) \| Class(BigUint64Array) \| Class(DataView) \| Class(ArrayBuffer) \| None \| List<String \| Bool \| Float \| None \| List<…> \| Dict> \| Dict ← null \| string \| number \| boolean-literal \| boolean-literal \| Named(ArrayBuffer) \| Named(ArrayBufferView) \| Array<… \| … \| … \| … \| … \| … \| …> \| Object{0 props}` | hono/src/utils/crypto.ts:15:30 `Data` |
| 4 | `Class(__SmeltMatch) ← Named(RegExpExecArray)` | hono/src/router/trie-router/node.ts:167:30 `RegExpExecArray` |
| 4 | `List<Unknown> ← Array<string>` | hono/src/middleware/cors/index.ts:93:26 `string[]` |
| 4 | `Optional<Class(Response) \| Float \| Class(ResponseInit)> ← undefined \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| Named(Response) \| Named(ResponseInit)` | hono/src/context.ts:767:58 `ContentfulStatusCode \| ResponseOrInit<StatusCode> \| undefined` |
| 4 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpExecArray)` | hono/src/router/trie-router/node.ts:165:23 `RegExpExecArray \| null` |
| 4 | `Optional<Unknown> ← Object{0 props}` | hono/src/router/reg-exp-router/prepared-router.ts:158:17 `MatcherMap<unknown>` |
| 4 | `String \| Class(Uint8Array) \| Class(DataView) \| Class(ArrayBuffer) ← string \| Named(ArrayBuffer) \| Named(ArrayBufferView)` | hono/src/utils/cookie.ts:40:28 `string \| BufferSource` |
| 4 | `String ← undefined \| string` | hono/src/utils/body.ts:109:21 `string \| undefined` |
| 3 | `(String, Dict) ← Array<string \| Object{0 props}>` | hono/src/router/trie-router/node.test.ts:45:50 `(string \| {})[]` |
| 3 | `(String, String, Bool \| Class(RegExp)) ← [string, string, Named(RegExp)]` | hono/src/utils/url.ts:66:11 `[string, string, RegExp]` |
| 3 | `Class(Int8Array) \| Class(Uint8Array) \| Class(Uint8ClampedArray) \| Class(Int16Array) \| Class(Uint16Array) \| Class(Int32Array) \| Class(Uint32Array) \| Class(Float32Array) \| Class(Float64Array) \| Class(BigInt64Array) \| Class(BigUint64Array) \| Class(DataView) \| Class(ArrayBuffer) ← Named(ArrayBuffer) \| Named(ArrayBufferView)` | hono/src/utils/crypto.ts:37:5 `ArrayBuffer \| ArrayBufferView<ArrayBufferLike>` |
| 3 | `Class(RegExpRouterWithMatcherExport) ← Named(class)` | hono/src/router/reg-exp-router/prepared-router.ts:103:18 `RegExpRouterWithMatcherExport<string>` |
| 3 | `Class(Uint8Array) \| Class(DataView) \| Class(ArrayBuffer) ← Named(ArrayBuffer) \| Named(ArrayBufferView)` | hono/src/utils/cookie.ts:40:21 `BufferSource` |

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| hono/src/router/common.case.test.ts | 71.4% | 5 | 0 | 1 | 1 | 6 |
| hono/src/router/common.case.test.ts | 71.4% | 5 | 0 | 1 | 1 | 6 |
| hono/src/utils/crypto.ts | 72.9% | 43 | 1 | 0 | 15 | 0 |
| hono/src/router/reg-exp-router/matcher.ts | 74.5% | 35 | 6 | 1 | 5 | 2 |
| hono/src/hono.ts | 75.0% | 9 | 0 | 1 | 2 | 0 |
| hono/src/middleware/pretty-json/index.ts | 76.8% | 43 | 5 | 0 | 8 | 3 |
| hono/src/router/reg-exp-router/router.test.ts | 79.6% | 519 | 46 | 54 | 33 | 43 |
| hono/src/middleware/cors/index.ts | 80.0% | 164 | 30 | 0 | 11 | 0 |
| hono/src/preset/tiny.ts | 80.0% | 4 | 0 | 0 | 1 | 0 |
| hono/src/preset/tiny.test.ts | 80.6% | 25 | 4 | 0 | 2 | 3 |
| hono/src/helper/route/index.ts | 81.7% | 107 | 10 | 8 | 6 | 4 |
| hono/src/router/pattern-router/router.test.ts | 83.3% | 15 | 1 | 1 | 1 | 1 |
| hono/src/utils/body.ts | 85.0% | 192 | 21 | 1 | 12 | 10 |
| hono/src/utils/html.ts | 85.0% | 182 | 1 | 16 | 15 | 10 |
| hono/src/compose.ts | 85.4% | 88 | 5 | 0 | 10 | 13 |
| hono/src/utils/color.ts | 85.7% | 12 | 1 | 0 | 1 | 10 |
| hono/src/request.ts | 86.0% | 240 | 13 | 9 | 17 | 21 |
| hono/src/router/reg-exp-router/node.ts | 86.9% | 311 | 0 | 39 | 8 | 8 |
| hono/src/router/pattern-router/router.ts | 87.1% | 121 | 1 | 3 | 14 | 3 |
| hono/src/router/reg-exp-router/prepared-router.ts | 87.1% | 311 | 10 | 14 | 22 | 3 |
| hono/src/context.ts | 88.0% | 351 | 2 | 13 | 33 | 7 |
| hono/src/helper/dev/index.ts | 88.1% | 118 | 12 | 0 | 4 | 0 |
| hono/src/router/trie-router/node.ts | 88.9% | 385 | 2 | 29 | 17 | 11 |
| hono/src/utils/encode.ts | 89.1% | 57 | 3 | 0 | 4 | 0 |
| hono/src/utils/html.test.ts | 89.5% | 17 | 0 | 0 | 2 | 2 |
| hono/src/router/smart-router/router.ts | 89.9% | 89 | 1 | 4 | 5 | 3 |
| hono/src/http-exception.ts | 90.3% | 28 | 0 | 2 | 1 | 1 |
| hono/src/router/reg-exp-router/router.ts | 91.1% | 276 | 3 | 16 | 8 | 17 |
| hono/src/middleware/powered-by/index.test.ts | 91.2% | 73 | 4 | 0 | 3 | 9 |
| hono/src/utils/cookie.ts | 91.6% | 380 | 2 | 9 | 24 | 2 |

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
