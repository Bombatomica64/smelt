# TypeScript 7.1.0-beta type parity

Inputs: es-toolkit/Smelt.toml

**Parity: 86.6%** (24555 of 28346 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 24555 |
| smelt-erased | 1227 |
| wider | 160 |
| mismatch | 2404 |
| ts-dynamic | 5232 |
| ts-deferred | 209 |
| no-checker-node | 2055 |

## Smelt-erased families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 106 | `Unknown ← number` | es-toolkit/src/array/combinations.ts:70:5 `number` |
| 84 | `Unknown ← Object{1 props}` | es-toolkit/src/compat/_internal/isIterateeCall.ts:12:84 `{ length: number; }` |
| 74 | `Unknown ← string` | es-toolkit/src/array/flatMap.spec.ts:41:5 `string` (erasure: by-name) |
| 57 | `Unknown ← Array<number>` | es-toolkit/src/array/flatMapAsync.spec.ts:9:49 `number[]` |
| 43 | `Unknown ← Named(Error)` | es-toolkit/src/object/cloneDeepWith.ts:210:20 `Error` |
| 38 | `Unknown ← Object{0 props}` | es-toolkit/src/array/reduceAsync.spec.ts:183:20 `Record<string, number>` |
| 38 | `Unknown ← string \| symbol` | es-toolkit/src/compat/_internal/toKey.ts:10:12 `string \| symbol` |
| 33 | `Unknown ← Object{1 props} & Object{1 props}` | es-toolkit/src/object/merge.spec.ts:39:18 `{ characters: { name: string; }[]; } & { characters: { age: number; }[]; }` |
| 30 | `Unknown ← Array<number \| Array<…>>` | es-toolkit/src/array/filterAsync.spec.ts:15:45 `(number \| number[])[]` |
| 30 | `Unknown ← Named(File)` | es-toolkit/src/object/cloneDeepWith.ts:192:29 `File` |
| 30 | `Unknown ← Object{2 props} & Object{2 props}` | es-toolkit/src/object/merge.spec.ts:8:20 `{ a: number; b: number; } & { b: number; c: number; }` |
| 29 | `Unknown ← Named(Blob)` | es-toolkit/src/object/cloneDeepWith.ts:202:29 `Blob` |
| 29 | `Unknown ← void` | es-toolkit/src/promise/delay.ts:47:7 `void` |
| 28 | `Unknown ← Named(AbortController)` | es-toolkit/src/function/debounce.spec.ts:127:20 `AbortController` |
| 28 | `Unknown ← Object{3 props}` | es-toolkit/src/object/cloneDeepWith.spec.ts:31:20 `{ a: number; b: { c: number; }; d: { e: { f: number; }; }; }` |
| 25 | `Unknown ← Function` | es-toolkit/src/compat/array/pullAt.ts:91:7 `{ (start: number, deleteCount?: number \| undefined): any[]; (start: number, dele` |
| 20 | `Unknown ← Object{2 props}` | es-toolkit/src/function/flow.spec.ts:63:28 `{ multiplier: number; multiply: (x: number, y: number) => number; }` (erasure: by-name) |
| 20 | `Unknown ← undefined \| string` | es-toolkit/src/object/cloneDeepWith.ts:215:20 `string \| undefined` |
| 19 | `Unknown ← Array<string>` | es-toolkit/src/array/reduceAsync.spec.ts:78:20 `string[]` |
| 18 | `Unknown ← undefined` | es-toolkit/src/array/orderBy.spec.ts:69:27 `undefined` |
| 16 | `Unknown ← Array<any>` | es-toolkit/src/compat/object/has.ts:98:7 `any[]` |
| 15 | `Unknown ← Named(AbortSignal)` | es-toolkit/src/function/debounce.spec.ts:127:20 `AbortSignal` |
| 15 | `Unknown ← Named(AggregateError)` | es-toolkit/src/object/clone.spec.ts:231:34 `AggregateError` |
| 15 | `Unknown ← boolean` | es-toolkit/src/object/cloneDeepWith.ts:225:32 `boolean` |
| 14 | `Unknown ← Named(IArguments)` | es-toolkit/src/compat/object/mergeWith.ts:279:26 `IArguments` |
| 14 | `Unknown ← Named(Object)` | es-toolkit/src/compat/object/get.ts:497:21 `Object` |
| 12 | `Unknown ← Array<never>` | es-toolkit/src/array/flatMapAsync.spec.ts:22:70 `never[]` |
| 12 | `Unknown ← Named(EvalError)` | es-toolkit/src/object/cloneDeep.spec.ts:267:25 `EvalError` |
| 12 | `Unknown ← Named(RangeError)` | es-toolkit/src/object/cloneDeep.spec.ts:277:25 `RangeError` |
| 12 | `Unknown ← Named(ReferenceError)` | es-toolkit/src/object/cloneDeep.spec.ts:287:25 `ReferenceError` |

## Wider-type families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 15 | `Optional<String> ← string` | es-toolkit/src/compat/_internal/decimalAdjust.ts:12:12 `string` |
| 15 | `Optional<TypeParam(T)> ← TypeParam` | es-toolkit/src/compat/array/indexOf.ts:34:24 `T` |
| 10 | `Optional<Class(TemplateOptions)> ← Named(TemplateOptions)` | es-toolkit/src/compat/string/template.ts:133:7 `TemplateOptions` |
| 10 | `Optional<Optional<String>> ← string` | es-toolkit/src/compat/string/template.ts:147:12 `string` |
| 8 | `Float \| String ← number` | es-toolkit/src/array/reduceAsync.spec.ts:179:23 `number` |
| 7 | `Optional<Float> ← number` | es-toolkit/src/array/filterAsync.ts:42:39 `number` |
| 6 | `List<Unknown> \| Dict ← Object{0 props}` | es-toolkit/src/compat/array/map.ts:139:46 `Record<any, any>` |
| 6 | `Optional<None \| Unknown \| TypeParam(T) \| None> ← null` | es-toolkit/src/util/attemptAsync.spec.ts:13:19 `null` |
| 4 | `Class(PropertyKey) \| (String, Unknown) ← Object{0 props}` | es-toolkit/src/compat/array/dropRightWhile.ts:92:51 `Partial<T>` |
| 4 | `Float \| Unknown ← number` | es-toolkit/src/compat/array/forEach.ts:156:69 `number` |
| 4 | `String \| Class(RegExp) ← string` | es-toolkit/src/compat/string/truncate.ts:93:18 `string` |
| 3 | `Dict \| List<TypeParam(T)> ← Object{0 props}` | es-toolkit/src/compat/array/every.ts:136:30 `Record<any, any>` |
| 3 | `String \| Dict \| List<TypeParam(T)> ← Object{0 props}` | es-toolkit/src/compat/array/forEach.ts:156:46 `Record<any, any>` |
| 3 | `String \| Function ← string` | es-toolkit/src/compat/array/invokeMap.ts:70:31 `string` |
| 3 | `String \| List<Unknown> \| Dict ← string` | es-toolkit/src/compat/array/includes.ts:63:21 `string` |
| 2 | `(None, TypeParam(T)) \| (TypeParam(E), None) ← [TypeParam, null]` | es-toolkit/src/util/attempt.ts:44:12 `[E, null]` |
| 2 | `Bool \| Optional<Bool> ← boolean` | es-toolkit/src/compat/function/throttle.ts:128:5 `boolean` |
| 2 | `Dict \| Class(PropertyKey) \| (Class(PropertyKey), Unknown) ← Object{0 props}` | es-toolkit/src/compat/array/dropWhile.ts:53:46 `PartialShallow<T>` |
| 2 | `Float \| Class(Blob) ← Named(Blob)` | es-toolkit/src/predicate/isBlob.spec.ts:37:12 `Blob` |
| 2 | `Float \| Class(FileSmeltClassExprF675S450) ← Named(File)` | es-toolkit/src/predicate/isFile.spec.ts:44:12 `File` |
| 2 | `List<TypeParam(T)> \| Dict ← Object{0 props}` | es-toolkit/src/compat/array/groupBy.ts:89:74 `Record<any, T>` |
| 2 | `Optional<Class(MemoizeCache)> \| Optional<Function> ← undefined \| Function` | es-toolkit/src/function/memoize.ts:80:54 `((args: Parameters<F>[0]) => unknown) \| undefined` |
| 2 | `Optional<Class(TruncateOptions)> ← Object{3 props}` | es-toolkit/src/compat/string/truncate.ts:55:30 `TruncateOptions` |
| 2 | `Optional<Function> ← Function` | es-toolkit/src/compat/predicate/isEqualWith.ts:57:18 `IsEqualCustomizer` |
| 2 | `Optional<List<List<TypeParam(T)>>> ← Array<Array<TypeParam>>` | es-toolkit/src/compat/array/unzip.ts:22:28 `T[][]` |
| 2 | `Optional<None \| Unknown \| None \| Future<…>> ← null \| Named(Promise)` | es-toolkit/src/util/attempt.spec.ts:25:19 `Promise<never> \| null` |
| 2 | `Optional<Optional<Float>> ← undefined \| number` | es-toolkit/src/compat/function/debounce.ts:170:45 `number \| undefined` |
| 2 | `Optional<TypeParam(T) \| List<TypeParam(T)>> ← Array<TypeParam>` | es-toolkit/src/compat/object/transform.ts:134:47 `readonly T[]` |
| 2 | `Optional<TypeParam(T)> ← TypeParam & null \| TypeParam & Object{0 props}` | es-toolkit/src/compat/math/max.ts:22:47 `T & ({} \| null)` |
| 2 | `Optional<TypeParam(T)> ← TypeParam & object` | es-toolkit/src/compat/object/findKey.ts:26:25 `T & object` |

## Mismatch families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 290 | `List<TypeParam(T)> ← Array<number>` | es-toolkit/src/array/chunk.spec.ts:11:24 `number[]` |
| 83 | `List<Unknown> ← Array<TypeParam>` | es-toolkit/src/array/remove.ts:41:10 `T[]` |
| 73 | `List<Unknown> ← Array<number \| Array<…>>` | es-toolkit/src/array/flatten.spec.ts:5:21 `(number \| (number \| (number \| number[])[])[])[]` (erasure: dynamic-operator) |
| 58 | `List<Unknown> ← Array<number>` | es-toolkit/src/array/flatMapAsync.spec.ts:12:28 `number[]` |
| 53 | `String ← Named(Error)` | es-toolkit/src/array/chunk.ts:26:11 `Error` (erasure: untyped-builtin) |
| 52 | `Optional<List<TypeParam(T)>> ← Named(ArrayLike)` | es-toolkit/src/compat/array/chunk.ts:34:31 `ArrayLike<T>` |
| 51 | `List<TypeParam(T)> ← Array<string>` | es-toolkit/src/array/at.spec.ts:6:12 `string[]` |
| 48 | `List<TypeParam(T)> ← Named(ArrayLike)` | es-toolkit/src/compat/_internal/toArray.ts:2:24 `ArrayLike<T>` |
| 47 | `List<List<TypeParam(T)>> ← Array<Array<number>>` | es-toolkit/src/array/chunk.spec.ts:11:18 `number[][]` |
| 46 | `Optional<List<TypeParam(T)>> ← undefined \| null \| Named(ArrayLike)` | es-toolkit/src/compat/array/chunk.ts:30:34 `ArrayLike<T> \| null \| undefined` |
| 45 | `List<Class(PropertyKey)> ← Array<string \| number \| symbol>` | es-toolkit/src/compat/object/get.ts:519:7 `readonly PropertyKey[]` |
| 39 | `Class(PropertyKey) ← string \| number \| symbol` | es-toolkit/src/_internal/isUnsafeProperty.ts:12:10 `PropertyKey` |
| 38 | `Class(DebouncedFuncLeading) ← Named(ThrottledFunction)` | es-toolkit/src/function/throttle.spec.ts:8:27 `ThrottledFunction<any>` |
| 31 | `Class(PropertyKey) \| List<Class(PropertyKey)> ← string \| number \| symbol \| Array<… \| … \| …>` | es-toolkit/src/compat/object/get.ts:460:18 `readonly PropertyKey[] \| PropertyKey` |
| 27 | `Dict \| List<TypeParam(T)> ← Named(ArrayLike) \| Object{0 props}` | es-toolkit/src/compat/array/every.ts:103:31 `ArrayLike<T> \| Record<any, any>` |
| 26 | `List<Unknown> ← void` | es-toolkit/src/function/throttle.spec.ts:10:5 `void` |
| 25 | `List<TypeParam(T)> ← Array<Object{1 props}>` | es-toolkit/src/array/intersectionBy.spec.ts:7:12 `{ foo: number; }[]` |
| 23 | `List<Unknown> ← Named(IArguments)` | es-toolkit/src/function/ary.spec.ts:8:23 `IArguments` |
| 23 | `TypeParam(T) ← Object{0 props}` | es-toolkit/src/compat/object/omit.ts:87:18 `Partial<T>` |
| 21 | `String ← string \| number \| symbol` | es-toolkit/src/compat/array/dropRightWhile.ts:90:59 `string \| number \| symbol` |
| 19 | `List<TypeParam(T) \| String> ← Array<string \| number>` | es-toolkit/src/array/fill.spec.ts:15:12 `(string \| number)[]` |
| 18 | `Future<Unknown> ← Named(Promise)` | es-toolkit/src/array/reduceAsync.spec.ts:14:26 `Promise<number>` (erasure: unfilled-generic) |
| 18 | `List<Unknown> ← Array<string \| symbol>` | es-toolkit/src/compat/util/toPath.ts:24:12 `(string \| symbol)[]` |
| 17 | `Float ← undefined \| number` | es-toolkit/src/compat/array/findLast.ts:137:5 `number \| undefined` |
| 17 | `List<Unknown> ← Array<string>` | es-toolkit/src/array/unzip.spec.ts:13:7 `string[]` |
| 16 | `List<Unknown> ← Named(ArrayLike)` | es-toolkit/src/compat/predicate/isArrayLike.ts:42:68 `ArrayLike<unknown>` |
| 16 | `String ← Named(AbortController)` | es-toolkit/src/function/debounce.spec.ts:126:24 `AbortController` (erasure: untyped-builtin) |
| 15 | `List<Class(FlatArray)> ← Array<number>` | es-toolkit/src/array/flatMap.spec.ts:11:12 `number[]` |
| 15 | `List<TypeParam(U)> ← Array<number>` | es-toolkit/src/array/differenceBy.spec.ts:6:42 `number[]` |
| 14 | `Class(PropertyKey) \| List<Class(PropertyKey)> ← Array<string \| number \| symbol>` | es-toolkit/src/compat/object/get.ts:493:25 `readonly PropertyKey[]` |

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| es-toolkit/src/compat/toolkit.ts | 18.2% | 2 | 4 | 0 | 5 | 2 |
| es-toolkit/src/compat/array/sample.ts | 27.3% | 3 | 2 | 1 | 5 | 0 |
| es-toolkit/src/compat/_internal/isPrototype.ts | 33.3% | 2 | 1 | 0 | 3 | 5 |
| es-toolkit/src/compat/array/unzip.ts | 36.4% | 8 | 0 | 1 | 13 | 0 |
| es-toolkit/src/compat/array/differenceBy.ts | 37.5% | 6 | 0 | 0 | 10 | 4 |
| es-toolkit/src/compat/array/uniq.ts | 37.5% | 3 | 0 | 0 | 5 | 0 |
| es-toolkit/src/compat/array/differenceWith.ts | 40.0% | 8 | 0 | 0 | 12 | 0 |
| es-toolkit/src/compat/array/groupBy.ts | 41.2% | 7 | 0 | 1 | 9 | 0 |
| es-toolkit/src/compat/predicate/isNative.ts | 42.4% | 14 | 14 | 0 | 5 | 3 |
| es-toolkit/src/compat/array/head.ts | 42.9% | 3 | 0 | 0 | 4 | 0 |
| es-toolkit/src/compat/array/initial.ts | 42.9% | 3 | 0 | 0 | 4 | 0 |
| es-toolkit/src/compat/array/last.ts | 42.9% | 3 | 0 | 0 | 4 | 0 |
| es-toolkit/src/compat/array/tail.ts | 42.9% | 3 | 0 | 0 | 4 | 0 |
| es-toolkit/src/compat/array/zipWith.ts | 44.0% | 11 | 0 | 1 | 13 | 0 |
| es-toolkit/src/compat/array/uniqWith.ts | 46.2% | 6 | 0 | 0 | 7 | 0 |
| es-toolkit/src/compat/_internal/toArgs.ts | 50.0% | 2 | 1 | 0 | 1 | 0 |
| es-toolkit/src/compat/array/flatMapDeep.ts | 50.0% | 2 | 0 | 0 | 2 | 1 |
| es-toolkit/src/compat/array/without.ts | 50.0% | 4 | 0 | 0 | 4 | 0 |
| es-toolkit/src/compat/math/sum.ts | 50.0% | 1 | 0 | 0 | 1 | 0 |
| es-toolkit/src/function/memoize.ts | 50.0% | 10 | 2 | 2 | 6 | 9 |
| es-toolkit/src/string/words.ts | 50.0% | 3 | 0 | 0 | 3 | 0 |
| es-toolkit/src/util/attempt.ts | 50.0% | 2 | 0 | 2 | 0 | 1 |
| es-toolkit/src/util/attemptAsync.ts | 50.0% | 2 | 0 | 2 | 0 | 1 |
| es-toolkit/src/array/flatten.spec.ts | 50.7% | 77 | 0 | 0 | 75 | 23 |
| es-toolkit/src/compat/array/shuffle.ts | 52.6% | 10 | 0 | 3 | 6 | 0 |
| es-toolkit/src/compat/array/uniqBy.ts | 53.8% | 7 | 0 | 0 | 6 | 0 |
| es-toolkit/src/compat/array/castArray.ts | 54.5% | 6 | 1 | 0 | 4 | 0 |
| es-toolkit/src/compat/array/map.ts | 54.8% | 23 | 1 | 2 | 16 | 5 |
| es-toolkit/src/compat/array/keyBy.ts | 55.0% | 11 | 5 | 0 | 4 | 5 |
| es-toolkit/src/compat/array/takeRightWhile.ts | 55.6% | 10 | 0 | 0 | 8 | 0 |

## TypeScript checker errors

| code | count | first message |
| --- | ---: | --- |
| TS2304 | 5 | Cannot find name 'global'. |
| TS2307 | 186 | Cannot find module 'vitest' or its corresponding type declarations. |
| TS2552 | 7 | Cannot find name 'Buffer'. Did you mean 'buffer'? |
| TS2554 | 1 | Expected 0 arguments, but got 2. |
| TS2591 | 6 | Cannot find name 'node:perf_hooks'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig. |
| TS7006 | 5 | Parameter 'array' implicitly has an 'any' type. |
| TS7031 | 1 | Binding element 'event' implicitly has an 'any' type. |
