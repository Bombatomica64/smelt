# TypeScript 7.1.0-beta type parity

Inputs: date-fns/Smelt.toml

**Parity: 77.2%** (8372 of 10845 statically typed expressions agree)

| verdict | count |
| --- | ---: |
| agree | 8372 |
| smelt-erased | 1419 |
| wider | 128 |
| mismatch | 926 |
| ts-dynamic | 30 |
| ts-deferred | 30 |
| no-checker-node | 778 |

## Smelt-erased families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 386 | `Unknown ← TypeParam` | date-fns/pkgs/core/src/constructFrom/index.ts:54:12 `ResultDate` (erasure: dynamic-operator) |
| 323 | `Unknown ← Named(Date)` | date-fns/pkgs/core/src/getISOWeekYear/index.ts:35:17 `Date` (erasure: unfilled-generic) |
| 192 | `Unknown ← number` | date-fns/pkgs/core/src/addDays/index.ts:67:3 `number` |
| 123 | `Unknown ← string` | date-fns/pkgs/core/src/locale/en-US/_lib/formatDistance/index.ts:103:22 `string` |
| 107 | `Unknown ← null \| Named(MatchFnResult)` | date-fns/pkgs/core/src/parse/_lib/parsers/EraParser.ts:16:11 `MatchFnResult<Era> \| null` |
| 32 | `Unknown ← undefined \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal` | date-fns/pkgs/core/src/startOfWeek/index.ts:52:5 `Day \| undefined` |
| 30 | `Unknown ← Object{1 props}` | date-fns/pkgs/core/src/formatDistance/index.ts:106:18 `Pick<Locale, "formatDistance">` |
| 24 | `Unknown ← undefined \| number` | date-fns/pkgs/core/src/formatDuration/index.ts:105:21 `number \| undefined` (erasure: dynamic-operator) |
| 20 | `Unknown ← number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal` | date-fns/pkgs/core/src/startOfWeek/index.ts:52:5 `Day` |
| 20 | `Unknown ← undefined \| number-literal \| number-literal` | date-fns/pkgs/core/src/getWeekYear/index.ts:66:5 `FirstWeekContainsDate \| undefined` |
| 19 | `Unknown ← undefined \| Object{7 props}` | date-fns/pkgs/core/src/startOfWeek/index.ts:55:5 `Pick<Locale, keyof Locale> \| undefined` |
| 18 | `Unknown ← string \| number \| Function \| Named(Date)` | date-fns/pkgs/core/src/getWeekYear/index.ts:72:45 `string \| number \| ContextFn<Date> \| Date` |
| 18 | `Unknown ← string \| number \| TypeParam \| Function` | date-fns/pkgs/core/src/toDate/index.ts:50:24 `ContextFn<ResultDate> \| DateArg<DateType>` |
| 14 | `Unknown ← string \| number \| Named(Date)` | date-fns/pkgs/core/src/areIntervalsOverlapping/index.ts:65:13 `DateArg<Date>` |
| 12 | `Unknown ← number-literal \| number-literal` | date-fns/pkgs/core/src/locale/en-US/_lib/formatDistance/index.ts:102:31 `-1 \| 1` (erasure: by-name) |
| 7 | `Unknown ← string-literal \| string-literal` | date-fns/pkgs/core/src/locale/_lib/buildLocalizeFn/index.ts:110:47 `"formatting" \| "standalone"` |
| 6 | `Unknown ← Named(Locale)` | date-fns/pkgs/core/src/format/index.ts:352:62 `Locale` |
| 6 | `Unknown ← Object{3 props}` | date-fns/pkgs/core/src/format/index.ts:352:18 `Pick<Locale, "formatLong" \| "localize" \| "options">` |
| 6 | `Unknown ← string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/formatISO/index.ts:57:26 `ISOStringRepresentation` |
| 5 | `Unknown ← Function` | date-fns/pkgs/core/src/constructFrom/index.ts:54:12 `<DateType extends Date = Date>(value: string \| number \| Date) => DateType` |
| 5 | `Unknown ← number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal` | date-fns/pkgs/core/src/roundToNearestHours/index.ts:70:21 `NearestHours` |
| 5 | `Unknown ← number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal` | date-fns/pkgs/core/src/roundToNearestMinutes/index.ts:65:21 `NearestMinutes` |
| 5 | `Unknown ← string-literal \| string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/formatDistanceStrict/index.ts:147:44 `RoundingMethod` |
| 3 | `Unknown ← Named(Localize)` | date-fns/pkgs/core/src/format/index.ts:413:7 `Localize` |
| 3 | `Unknown ← string-literal \| string-literal \| string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/locale/_lib/buildFormatLongFn/index.ts:18:17 `FormatLongWidth` |
| 3 | `Unknown ← undefined \| Object{1 props}` | date-fns/pkgs/core/src/formatDistance/index.ts:106:18 `Pick<Locale, "formatDistance"> \| undefined` |
| 3 | `Unknown ← undefined \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| Object{7 props}` | date-fns/pkgs/core/src/setDefaultOptions/index.ts:60:9 `0 \| 1 \| 2 \| 3 \| 4 \| 5 \| 6 \| Pick<Locale, keyof Locale> \| undefined` |
| 2 | `Unknown ← Function \| Function \| Function \| Function \| Function \| Function \| Function \| Function \| Function` | date-fns/pkgs/core/src/lightFormat/index.ts:114:25 `((date: Date, token: string) => string) \| ((date: Date, token: string) => string` |
| 2 | `Unknown ← Object{4 props}` | date-fns/pkgs/core/src/formatRelative/index.ts:65:18 `Pick<Locale, "formatLong" \| "formatRelative" \| "localize" \| "options">` |
| 2 | `Unknown ← number \| TypeParam` | date-fns/pkgs/core/src/max/index.ts:53:33 `number \| ResultDate` |

## Wider-type families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 56 | `Optional<String> ← string` | date-fns/pkgs/core/src/locale/_lib/buildMatchFn/index.ts:84:27 `string` |
| 38 | `Optional<Float> ← number` | date-fns/pkgs/core/src/areIntervalsOverlapping/index.ts:64:10 `number` |
| 17 | `Optional<Class(Date)> ← Named(Date)` | date-fns/pkgs/core/src/closestTo/index.ts:67:10 `Date` |
| 2 | `Optional<Class(Parser)> ← Named(Parser)` | date-fns/pkgs/core/src/parse/index.ts:433:20 `Parser<any>` |
| 2 | `Optional<Class(RegExp)> ← Named(RegExp)` | date-fns/pkgs/core/src/locale/_lib/buildMatchFn/index.ts:77:7 `RegExp` |
| 2 | `Optional<Dict> ← Object{5 props}` | date-fns/pkgs/core/src/locale/_lib/buildLocalizeFn/index.ts:118:22 `LocalizePeriodValuesMap<Value>` |
| 2 | `Optional<TypeParam(ResultDate)> ← TypeParam` | date-fns/pkgs/core/src/max/index.ts:50:20 `ResultDate` |
| 2 | `TypeParam(DateType) \| String \| Float ← TypeParam` | date-fns/pkgs/core/src/max/index.ts:47:42 `DateType` |
| 1 | `Optional<String> ← string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/intlFormatDistance/index.ts:199:12 `IntlFormatDistanceUnit` |
| 1 | `Optional<TypeParam(TInput)> ← TypeParam` | date-fns/pkgs/core/src/parse/_lib/utils.ts:19:18 `TInput` |
| 1 | `Optional<TypeParam(Value)> ← TypeParam` | date-fns/pkgs/core/src/parse/_lib/Parser.ts:23:9 `Value` |
| 1 | `String \| Class(ValueSetter) ← Named(ValueSetter)` | date-fns/pkgs/core/src/parse/index.ts:466:20 `ValueSetter<any>` |
| 1 | `String \| Class(ValueSetter) ← string` | date-fns/pkgs/core/src/parse/index.ts:468:17 `string` |
| 1 | `TypeParam(DateType) \| (TypeParam(DateType), Class(ParseFlags)) ← TypeParam` | date-fns/pkgs/core/src/parse/index.ts:527:14 `ResultDate` |
| 1 | `TypeParam(DateType) \| None ← TypeParam` | date-fns/pkgs/core/src/constructFrom/index.ts:53:66 `DateType` |

## Mismatch families (HIR ← TypeScript)

| count | shape | first example |
| ---: | --- | --- |
| 192 | `Optional<Unknown> ← undefined \| Function` | date-fns/pkgs/core/src/addDays/index.ts:61:30 `ContextFn<ResultDate> \| undefined` |
| 85 | `Class(ParseResult) ← null \| Object{2 props}` | date-fns/pkgs/core/src/parse/_lib/Parser.ts:16:20 `ParseResult<Value>` |
| 64 | `Float ← Named(Date)` | date-fns/pkgs/core/src/isSaturday/index.ts:31:10 `Date` (erasure: unfilled-generic) |
| 60 | `Float ← TypeParam` | date-fns/pkgs/core/src/addDays/index.ts:67:17 `ResultDate` |
| 48 | `Bool ← null \| Named(MatchFnResult)` | date-fns/pkgs/core/src/parse/_lib/parsers/EraParser.ts:16:11 `MatchFnResult<Era> \| null` (erasure: by-name) |
| 48 | `Class(__SmeltMatch) ← Named(RegExpMatchArray)` | date-fns/pkgs/core/src/locale/_lib/buildMatchFn/index.ts:84:27 `RegExpMatchArray` |
| 41 | `TypeParam(DateType) ← number` | date-fns/pkgs/core/src/parse/_lib/parsers/EraParser.ts:39:5 `number` |
| 29 | `Optional<Class(__SmeltMatch)> ← null \| Named(RegExpMatchArray)` | date-fns/pkgs/core/src/locale/_lib/buildMatchFn/index.ts:79:25 `RegExpMatchArray \| null` |
| 26 | `Optional<Unknown> ← undefined \| Named(LocaleOptions)` | date-fns/pkgs/core/src/startOfWeek/index.ts:53:5 `LocaleOptions \| undefined` |
| 24 | `Optional<Unknown> ← undefined \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal \| number-literal` | date-fns/pkgs/core/src/startOfWeek/index.ts:52:5 `Day \| undefined` (erasure: by-name) |
| 20 | `String ← Named(Date)` | date-fns/pkgs/core/src/eachDayOfInterval/index.ts:67:11 `Date` |
| 20 | `String ← number` | date-fns/pkgs/core/src/add/index.ts:79:5 `number` |
| 15 | `Optional<Unknown> ← undefined \| number-literal \| number-literal` | date-fns/pkgs/core/src/getWeekYear/index.ts:66:5 `FirstWeekContainsDate \| undefined` (erasure: by-name) |
| 15 | `String ← Named(RangeError)` | date-fns/pkgs/core/src/_lib/protectedTokens/index.ts:21:42 `RangeError` (erasure: untyped-builtin) |
| 11 | `Optional<Unknown> ← undefined \| Object{1 props}` | date-fns/pkgs/core/src/startOfWeek/index.ts:53:5 `Pick<Locale, "options"> \| undefined` (erasure: by-name) |
| 10 | `Class(Intl.RelativeTimeFormatUnit) ← string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/intlFormatDistance/index.ts:162:7 `RelativeTimeFormatUnit` |
| 9 | `Class(Date) ← number` | date-fns/pkgs/core/src/_lib/getTimezoneOffsetInMilliseconds/index.ts:30:3 `number` |
| 8 | `Optional<Unknown> ← undefined \| string-literal \| string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/differenceInHours/index.ts:45:28 `RoundingMethod \| undefined` |
| 7 | `Bool ← undefined \| number` | date-fns/pkgs/core/src/milliseconds/index.ts:44:7 `number \| undefined` |
| 7 | `Optional<Unknown> ← undefined \| number` | date-fns/pkgs/core/src/eachDayOfInterval/index.ts:74:14 `number \| undefined` (erasure: by-name) |
| 7 | `String ← undefined \| number` | date-fns/pkgs/core/src/milliseconds/index.ts:34:3 `number \| undefined` |
| 6 | `List<String> ← Named(RegExpMatchArray)` | date-fns/pkgs/core/src/format/index.ts:374:29 `RegExpMatchArray` |
| 6 | `Optional<List<String>> ← null \| Named(RegExpMatchArray)` | date-fns/pkgs/core/src/format/index.ts:374:29 `RegExpMatchArray \| null` |
| 6 | `Optional<TypeParam(Options)> ← undefined \| Named(EachWeekOfIntervalOptions)` | date-fns/pkgs/core/src/eachWeekOfInterval/index.ts:78:44 `EachWeekOfIntervalOptions<Date> \| undefined` |
| 6 | `Optional<Unknown> ← undefined \| Object{3 props}` | date-fns/pkgs/core/src/format/index.ts:352:18 `Pick<Locale, "formatLong" \| "localize" \| "options"> \| undefined` (erasure: by-name) |
| 6 | `Optional<Unknown> ← undefined \| boolean-literal \| boolean-literal` | date-fns/pkgs/core/src/locale/en-US/_lib/formatDistance/index.ts:101:7 `boolean \| undefined` (erasure: by-name) |
| 5 | `Optional<Unknown> ← Named(MatchFnOptions) & Object{0 props}` | date-fns/pkgs/core/src/locale/_lib/buildMatchFn/index.ts:74:19 `MatchFnOptions<Value> & Record<string, unknown>` |
| 5 | `Optional<Unknown> ← undefined \| string-literal \| string-literal \| string-literal \| string-literal \| string-literal` | date-fns/pkgs/core/src/locale/_lib/buildLocalizeFn/index.ts:116:9 `LocaleWidth \| undefined` (erasure: by-name) |
| 5 | `String ← Named(RegExp)` | date-fns/pkgs/core/src/format/index.ts:451:29 `RegExp` |
| 4 | `Bool ← undefined \| boolean-literal \| boolean-literal` | date-fns/pkgs/core/src/interval/index.ts:70:7 `boolean \| undefined` |

## Files (lowest parity first)

| file | parity | agree | erased | wider | mismatch | dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| date-fns/pkgs/core/src/setDayOfYear/index.ts | 36.4% | 4 | 6 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/endOfSecond/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/setDate/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/setHours/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/setMilliseconds/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/setMinutes/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/setSeconds/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/startOfSecond/index.ts | 37.5% | 3 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/toDate/index.ts | 40.0% | 2 | 2 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/eachWeekOfInterval/index.ts | 40.8% | 31 | 35 | 0 | 10 | 0 |
| date-fns/pkgs/core/src/set/index.ts | 43.8% | 28 | 34 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/endOfMinute/index.ts | 44.4% | 4 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/startOfMinute/index.ts | 44.4% | 4 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/clamp/index.ts | 45.0% | 9 | 4 | 0 | 7 | 0 |
| date-fns/pkgs/core/src/startOfWeekYear/index.ts | 45.2% | 19 | 16 | 0 | 7 | 0 |
| date-fns/pkgs/core/src/setWeekYear/index.ts | 46.3% | 25 | 20 | 0 | 9 | 0 |
| date-fns/pkgs/core/src/setYear/index.ts | 47.1% | 8 | 7 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/setISOWeekYear/index.ts | 47.2% | 17 | 16 | 0 | 3 | 0 |
| date-fns/pkgs/core/src/eachQuarterOfInterval/index.ts | 49.2% | 29 | 24 | 0 | 6 | 0 |
| date-fns/pkgs/core/src/endOfHour/index.ts | 50.0% | 5 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/endOfYesterday/index.ts | 50.0% | 12 | 7 | 0 | 5 | 0 |
| date-fns/pkgs/core/src/isThisHour/index.ts | 50.0% | 5 | 3 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/isThisISOWeek/index.ts | 50.0% | 6 | 4 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/isThisMonth/index.ts | 50.0% | 6 | 4 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/isThisQuarter/index.ts | 50.0% | 6 | 4 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/isThisYear/index.ts | 50.0% | 6 | 4 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/isToday/index.ts | 50.0% | 6 | 4 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/isYesterday/index.ts | 50.0% | 7 | 5 | 0 | 2 | 0 |
| date-fns/pkgs/core/src/startOfHour/index.ts | 50.0% | 5 | 4 | 0 | 1 | 0 |
| date-fns/pkgs/core/src/startOfMonth/index.ts | 50.0% | 7 | 6 | 0 | 1 | 0 |

## TypeScript checker errors

None: every input type-checks under TypeScript 7.
