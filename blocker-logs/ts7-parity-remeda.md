# TypeScript 7.1.0-beta type parity

Inputs: remeda/Smelt.toml

**Parity: 91.0%** (21313 of 23413 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 21313 |
| smelt-erased | 932 |
| wider | 78 |
| mismatch | 1090 |
| ts-dynamic | 4509 |
| ts-deferred | 88 |
| no-checker-node | 2358 |

## Smelt-erased families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 188 | `Unknown ← Function & Object{2 props}` | remeda/packages/remeda/src/internal/lazyDataLastImpl.ts:27:7 `((data: unknown) => unknown) & { lazy: (...args: any) => LazyEvaluator; lazyArgs` (erasure: unmodeled-type) |
| 95 | `Unknown ← Array<string>` | remeda/packages/remeda/src/constant.test.ts:61:50 `string[]` |
| 92 | `Unknown ← Array<number>` | remeda/packages/remeda/src/clone.test.ts:208:36 `number[]` |
| 87 | `Unknown ← void` | remeda/packages/remeda/src/funnel.lodash-debounce-with-cached-value.test.ts:205:5 `void` |
| 61 | `Unknown ← number` | remeda/packages/remeda/src/clone.test.ts:36:5 `number` |
| 43 | `Unknown ← string` | remeda/packages/remeda/src/stringToPath.ts:122:9 `string` |
| 33 | `Unknown ← undefined \| number` | remeda/packages/remeda/src/debounce.ts:122:7 `number \| undefined` |
| 31 | `Unknown ← Array<never>` | remeda/packages/remeda/src/chunk.test.ts:24:40 `never[]` |
| 28 | `Unknown ← TypeParam & object` | remeda/packages/remeda/src/clone.ts:62:43 `T & object` |
| 25 | `Unknown ← Function` | remeda/packages/remeda/src/internal/lazyDataLastImpl.ts:25:10 `(data: unknown) => unknown` |
| 15 | `Unknown ← Array<… \| …> \| [string-literal, string-literal] \| [string-literal, … \| …] \| [… \| …, string-literal] \| [string-literal, string-literal, … \| …]` | remeda/packages/remeda/src/stringToPath.test.ts:100:12 `(string \| number)[] \| ["foo", "bar"] \| ["foo", ...(string \| number)[]] \| [...(st` |
| 14 | `Unknown ← Array<… \| …> \| [string-literal, string-literal] \| [… \| …, string-literal] \| [string-literal, string-literal, … \| …]` | remeda/packages/remeda/src/stringToPath.test.ts:105:12 `(string \| number)[] \| ["foo", "bar"] \| [...(string \| number)[], "bar"] \| ["foo",` |
| 13 | `Unknown ← Array<Object{2 props}>` | remeda/packages/remeda/src/groupByProp.test.ts:45:12 `{ 1: string; 2: number; }[]` (erasure: dynamic-operator) |
| 11 | `Unknown ← Object{0 props}` | remeda/packages/remeda/src/prop.ts:466:14 `{}` |
| 8 | `Unknown ← string-literal` | remeda/packages/remeda/src/forEachObj.test.ts:32:12 `"123"` |
| 8 | `Unknown ← undefined` | remeda/packages/remeda/src/first.test.ts:45:12 `undefined` |
| 7 | `Unknown ← Array<Object{1 props}>` | remeda/packages/remeda/src/evolve.test.ts:83:15 `{ a: number; }[]` |
| 7 | `Unknown ← Array<string \| number>` | remeda/packages/remeda/src/stringToPath.test.ts:62:51 `(string \| number)[]` |
| 6 | `Unknown ← Array<Array<… \| …>>` | remeda/packages/remeda/src/zip.test.ts:9:59 `(string \| number)[][]` |
| 6 | `Unknown ← Array<number \| Array<…>>` | remeda/packages/remeda/src/flat.test.ts:33:52 `(number \| number[])[]` |
| 6 | `Unknown ← Named(Date)` | remeda/packages/remeda/src/clone.test.ts:36:5 `Date` |
| 6 | `Unknown ← Named(Object)` | remeda/packages/remeda/src/clone.ts:68:19 `Object` (erasure: by-name) |
| 5 | `Unknown ← Array<Array<string>>` | remeda/packages/remeda/src/chunk.test.ts:6:58 `string[][]` |
| 4 | `Unknown ← Array<Array<Array<…>>>` | remeda/packages/remeda/src/flat.test.ts:25:12 `number[][][][][][]` |
| 4 | `Unknown ← Array<any> \| [[…, …], […, …]] \| []` | remeda/packages/remeda/src/chunk.test.ts:30:12 `any[] \| NonEmptyArray<NonEmptyArray<string>> \| []` |
| 4 | `Unknown ← Array<… \| …> \| [string-literal, string-literal] \| [string-literal, … \| …] \| [… \| …, string-literal]` | remeda/packages/remeda/src/stringToPath.test.ts:25:12 `(string \| number)[] \| ["foo", "bar"] \| ["foo", ...(string \| number)[]] \| [...(st` |
| 4 | `Unknown ← Array<… \| …> \| [string-literal, … \| …] \| [string-literal, … \| …] \| [… \| …, … \| …] \| [string-literal, … \| …, … \| …]` | remeda/packages/remeda/src/stringToPath.test.ts:72:12 `(string \| number)[] \| ["foo", "123" \| 123] \| ["foo", ...(string \| number)[]] \| [` |
| 4 | `Unknown ← Array<… \| …> \| [string-literal]` | remeda/packages/remeda/src/stringToPath.test.ts:15:10 `(string \| number)[] \| ["foo"]` |
| 4 | `Unknown ← Object{1 props}` | remeda/packages/remeda/src/evolve.test.ts:46:12 `{ n: number; }` |
| 4 | `Unknown ← Object{4 props} \| Object{4 props}` | remeda/packages/remeda/src/omit.test.ts:8:10 `SimplifiedWritable<{ a: number; b: number; c: number; d: number; } \| OmitUnbound` |

## Wider-type families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 14 | `Optional<Dict> ← Object{1 props}` | remeda/packages/remeda/src/clone.test.ts:220:14 `{ a: number; }` |
| 9 | `Optional<TypeParam(T)> ← TypeParam` | remeda/packages/remeda/src/internal/heap.ts:49:10 `T` |
| 6 | `Optional<String> ← string` | remeda/packages/remeda/src/capitalize.ts:59:6 `string` |
| 6 | `String \| Class(RegExp) ← Named(RegExp)` | remeda/packages/remeda/src/truncate.ts:275:29 `RegExp` |
| 6 | `String \| Unknown ← string-literal` | remeda/packages/remeda/src/when.test.ts:15:14 `"was true"` |
| 4 | `Bool \| Float ← boolean` | remeda/packages/remeda/src/funnel.lodash-debounce-with-cached-value.test.ts:57:12 `boolean` |
| 4 | `Optional<Bool> \| Optional<Float> ← undefined \| number` | remeda/packages/remeda/src/funnel.lodash-debounce-with-cached-value.test.ts:46:5 `number \| undefined` |
| 4 | `Optional<Float> ← number` | remeda/packages/remeda/src/difference.ts:59:33 `number` |
| 4 | `String \| Class(NonNullable) ← string` | remeda/packages/remeda/src/defaultTo.test.ts:7:14 `string` |
| 3 | `Float \| Class(NonNullable) ← number` | remeda/packages/remeda/src/defaultTo.test.ts:22:14 `number` |
| 2 | `Bool \| Class(NonNullable) ← NonNullable<boolean \| undefined>` | remeda/packages/remeda/src/defaultTo.test.ts:30:14 `NonNullable<boolean \| undefined>` |
| 2 | `Class(LazyEmpty) \| Class(LazyMany) \| Class(LazyNext) ← Object{4 props}` | remeda/packages/remeda/src/pipe.ts:368:21 `LazyNext<any>` |
| 2 | `Function \| Function ← Function` | remeda/packages/remeda/src/when.ts:165:9 `(data: T, ...extraArgs: ExtraArgs) => WhenTrue` |
| 2 | `Optional<Bool> \| Optional<Float> ← number` | remeda/packages/remeda/src/funnel.lodash-debounce-with-cached-value.test.ts:80:58 `number` |
| 2 | `Optional<List<Unknown>> ← Array<T[number]>` | remeda/packages/remeda/src/groupByProp.ts:199:21 `T[number][]` |
| 2 | `String \| Unknown ← string` | remeda/packages/remeda/src/truncate.ts:277:20 `string` |
| 1 | `Bool \| Optional<Bool> ← undefined \| boolean-literal \| boolean-literal` | remeda/packages/remeda/src/pipe.ts:355:11 `boolean \| undefined` |
| 1 | `Float \| List<Unknown> ← number` | remeda/packages/remeda/src/flat.ts:116:39 `number` |
| 1 | `Optional<Class(RegExpExecArray)> ← Named(RegExpExecArray)` | remeda/packages/remeda/src/stringToPath.ts:120:58 `RegExpExecArray` |
| 1 | `Optional<List<String>> ← Array<string>` | remeda/packages/remeda/src/defaultTo.test.ts:63:22 `string[]` |
| 1 | `Optional<List<Unknown>> ← Array<never>` | remeda/packages/remeda/src/defaultTo.test.ts:38:24 `never[]` |
| 1 | `Optional<TypeParam(T)> ← TypeParam \| TypeParam & Object{0 props}` | remeda/packages/remeda/src/defaultTo.ts:68:33 `Fallback \| NonNullable<T>` |

## Mismatch families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 139 | `List<Unknown> ← Array<number>` | remeda/packages/remeda/src/clone.test.ts:208:30 `number[]` |
| 42 | `List<Function \| Class(LazyFunction)> ← Array<number>` | remeda/packages/remeda/src/dropLastWhile.test.ts:34:7 `number[]` |
| 39 | `List<Unknown> ← number` | remeda/packages/remeda/src/clamp.test.ts:5:10 `number` |
| 29 | `List<Class(PropertyKey)> ← undefined` | remeda/packages/remeda/src/prop.test.ts:7:10 `undefined` |
| 27 | `List<Function \| Class(LazyFunction)> ← undefined \| number` | remeda/packages/remeda/src/findLast.test.ts:26:7 `number \| undefined` |
| 27 | `Optional<Unknown> ← undefined \| number` | remeda/packages/remeda/src/debounce.ts:148:9 `number \| undefined` |
| 23 | `List<Unknown> ← Array<string>` | remeda/packages/remeda/src/debounce.test.ts:28:22 `string[]` |
| 20 | `List<Unknown> ← Array<string \| number>` | remeda/packages/remeda/src/concat.test.ts:17:34 `(string \| number)[]` |
| 19 | `List<Unknown> ← [Function, string-literal]` | remeda/packages/remeda/src/firstBy.test.ts:26:37 `[IdentityFunction, "desc"]` |
| 16 | `Class(PropertyKey) ← string \| number \| symbol` | remeda/packages/remeda/src/countBy.ts:71:29 `PropertyKey` |
| 16 | `TypeParam(T) ← Array<unknown> \| []` | remeda/packages/remeda/src/take.ts:47:33 `IterableContainer` |
| 15 | `List<Unknown> ← Function` | remeda/packages/remeda/src/addProp.test.ts:15:35 `(obj: { a: number; }) => Simplify<Omit<T, K> & { -readonly [P in keyof T as P ex` |
| 14 | `List<Unknown> ← number-literal` | remeda/packages/remeda/src/constant.test.ts:12:10 `1` |
| 14 | `List<Unknown> ← undefined \| number` | remeda/packages/remeda/src/findLast.test.ts:7:12 `number \| undefined` |
| 13 | `List<Function \| Class(LazyFunction)> ← number` | remeda/packages/remeda/src/length.test.ts:31:7 `number` |
| 13 | `List<Unknown> ← undefined` | remeda/packages/remeda/src/first.test.ts:12:10 `undefined` |
| 13 | `String ← Named(Error)` | remeda/packages/remeda/src/purry.ts:64:9 `Error` (erasure: untyped-builtin) |
| 13 | `String ← boolean` | remeda/packages/remeda/src/debounce.ts:280:9 `boolean` |
| 12 | `List<TypeParam(S)> ← Array<number>` | remeda/packages/remeda/src/partition.test.ts:30:7 `number[]` |
| 11 | `List<Unknown> ← Array<undefined \| null \| string \| number \| symbol \| bigint-literal \| boolean-literal \| boolean-literal \| Array<…> \| Named(Date) \| Named(Error) \| Named(Map) \| Named(Promise) \| Named(RegExp) \| Named(Set) \| Named(TestClass) \| Named(Uint8Array) \| Function \| Object{1 props} \| […, …, …]>` | remeda/packages/remeda/test/typesDataProvider.ts:35:40 `(string \| number \| symbol \| 1n \| boolean \| number[] \| Date \| Error \| Map<string,` (erasure: by-name) |
| 10 | `List<Unknown> ← Object{0 props}` | remeda/packages/remeda/src/indexBy.test.ts:8:5 `Record<PropertyKey, { dir: string; code: number; }>` |
| 9 | `List<TypeParam(T)> ← Array<number>` | remeda/packages/remeda/src/internal/binarySearchCutoffIndex.test.ts:10:36 `number[]` |
| 9 | `List<Unknown> ← Object{1 props}` | remeda/packages/remeda/src/constant.test.ts:18:27 `{ a?: boolean \| undefined; }` |
| 9 | `List<Unknown> ← [Function, Function]` | remeda/packages/remeda/src/conditional.test.ts:29:33 `[(data: unknown) => data is string, () => string]` |
| 9 | `List<Unknown> ← [number-literal \| number-literal \| number-literal \| number-literal, number-literal \| number-literal \| number-literal \| number-literal, number-literal \| number-literal \| number-literal \| number-literal, number-literal \| number-literal \| number-literal \| number-literal]` | remeda/packages/remeda/src/shuffle.test.ts:9:20 `[2 \| 4 \| 5 \| 7, 2 \| 4 \| 5 \| 7, 2 \| 4 \| 5 \| 7, 2 \| 4 \| 5 \| 7]` |
| 9 | `TypeParam(T) ← Array<unknown>` | remeda/packages/remeda/src/take.ts:47:20 `unknown[]` |
| 8 | `List<(Int, TypeParam(T))> ← Named(ArrayIterator)` | remeda/packages/remeda/src/chunk.ts:254:33 `ArrayIterator<[number, T]>` |
| 8 | `List<Class(PropertyKey)> ← Array<string \| number \| symbol>` | remeda/packages/remeda/src/prop.ts:452:40 `readonly PropertyKey[]` |
| 8 | `List<Function \| Class(LazyFunction)> ← undefined \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal` | remeda/packages/remeda/src/nthBy.test.ts:68:7 `"a" \| "aa" \| "aaa" \| "aaaa" \| "b" \| "bb" \| "bbb" \| "bbbb" \| undefined` |
| 8 | `List<TypeParam(T)> ← Array<Object{1 props}>` | remeda/packages/remeda/src/sortedIndexBy.test.ts:155:9 `{ age: number; }[]` |

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| remeda/packages/remeda/src/doNothing.ts | 50.0% | 1 | 0 | 0 | 1 | 0 |
| remeda/packages/remeda/src/isArray.ts | 50.0% | 1 | 0 | 0 | 1 | 0 |
| remeda/packages/remeda/src/pathOr.ts | 54.5% | 6 | 2 | 0 | 3 | 10 |
| remeda/packages/remeda/src/setPath.ts | 56.5% | 13 | 0 | 0 | 10 | 13 |
| remeda/packages/remeda/src/stringToPath.ts | 56.8% | 21 | 13 | 1 | 2 | 0 |
| remeda/packages/remeda/src/isDefined.test.ts | 57.1% | 4 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isNonNull.test.ts | 57.1% | 4 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isNonNullish.test.ts | 57.1% | 4 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/fromKeys.ts | 58.8% | 10 | 2 | 0 | 5 | 2 |
| remeda/packages/remeda/src/debounce.ts | 59.5% | 66 | 29 | 0 | 16 | 2 |
| remeda/packages/remeda/src/length.ts | 63.6% | 7 | 0 | 0 | 4 | 1 |
| remeda/packages/remeda/src/stringToPath.test.ts | 64.2% | 226 | 126 | 0 | 0 | 67 |
| remeda/packages/remeda/src/prop.ts | 65.0% | 13 | 2 | 0 | 5 | 6 |
| remeda/packages/remeda/src/when.ts | 65.6% | 21 | 3 | 2 | 6 | 2 |
| remeda/packages/remeda/src/only.test.ts | 65.9% | 27 | 0 | 0 | 14 | 10 |
| remeda/packages/remeda/src/internal/toSingle.ts | 66.7% | 2 | 1 | 0 | 0 | 0 |
| remeda/packages/remeda/src/isArray.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isBigInt.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isBoolean.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isDate.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isError.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isFunction.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isNumber.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isPromise.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/isString.test.ts | 66.7% | 6 | 1 | 0 | 2 | 2 |
| remeda/packages/remeda/src/sumBy.ts | 66.7% | 22 | 0 | 0 | 11 | 3 |
| remeda/packages/remeda/src/invert.ts | 69.2% | 9 | 0 | 0 | 4 | 1 |
| remeda/packages/remeda/src/funnel.lodash-throttle.test.ts | 70.6% | 240 | 98 | 0 | 2 | 124 |
| remeda/packages/remeda/src/takeLast.ts | 70.6% | 12 | 0 | 0 | 5 | 1 |
| remeda/packages/remeda/src/reverse.test.ts | 71.4% | 15 | 4 | 0 | 2 | 2 |

## TypeScript checker errors

| code | count | first message |
| --- | ---: | --- |
| TS18046 | 4 | 'x' is of type 'unknown'. |
| TS2307 | 235 | Cannot find module 'vitest' or its corresponding type declarations. |
| TS2322 | 2 | Type 'any[] \| [number, ...any[]]' is not assignable to type 'NonEmptyArray<number>'. |
| TS2339 | 5 | Property 'map' does not exist on type '(array: IterableContainer) => ReorderedArray<IterableContainer>'. |
| TS2345 | 27 | Argument of type '(data: number[]) => unknown[]' is not assignable to parameter of type '(input: number[]) => readonly number[]'. |
| TS2353 | 2 | Object literal may only specify known properties, and 'c' does not exist in type 'TestType'. |
| TS2536 | 1 | Type 'Prop' cannot be used to index type '{}'. |
| TS2554 | 163 | Expected 0 arguments, but got 1. |
| TS2556 | 3 | A spread argument must either have a tuple type or be passed to a rest parameter. |
| TS2578 | 13 | Unused '@ts-expect-error' directive. |
| TS2589 | 3 | Type instantiation is excessively deep and possibly infinite. |
| TS2591 | 3 | Cannot find name 'node:process'. Do you need to install type definitions for node? Try `npm i --save-dev @types/node` and then add 'node' to the types field in your tsconfig. |
| TS2769 | 12 | No overload matches this call. |
| TS5114 | 2 | Instantiations of type 'TimesArray' appear infinitely circular. |
| TS7006 | 37 | Parameter 'val' implicitly has an 'any' type. |
| TS7031 | 9 | Binding element 'input' implicitly has an 'any' type. |
