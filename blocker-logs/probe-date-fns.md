# Probe: date-fns (first scoping pass, 2026-11-04)

## Reproduction

- Source: `date-fns/date-fns` at `cd53d2538cfa318404eff7ade6449b49bf34562e`
  (tag `v4.4.0`, 2026-05-30). Since v4.2, date-fns has been a pnpm monorepo.
  The `date-fns` package itself is `pkgs/core`; `pkgs/tz` (`@date-fns/tz`)
  and `pkgs/utc` (`@date-fns/utc`) are sibling packages.
- Fixture: `.github/compat/date-fns/Smelt.toml`. It uses root
  `pkgs/core/src`, entry `pkgs/core/src/index.ts`, and no `test-prefix` and
  no excludes, as the first-pass rule asks.
- Smelt: this branch's `target/debug/smelt`, built from `ba730229` with the
  default features (`ty`, `ts7`).
- Commands:
  - `smelt --manifest-path <lib>/Smelt.toml probe --format md`
  - `smelt ts7-parity-report <lib>/Smelt.toml --output blocker-logs/ts7-parity-date-fns.md`
  - `smelt build`, then repeated with an exclude list in a scratch copy of
    the manifest, to enumerate every whole-crate abort (see "Scoped build"
    below).
  - `smelt rust-test-report --build-manifest <scoped manifest> --cargo-manifest
    <lib>/dist-smelt/Cargo.toml --full --diagnostics --suppress-warnings`
  - `smelt smelt-unknown-report <lib>/dist-smelt/src` and
    `smelt typedness-report <lib>/dist-smelt`

There is an older, unrelated note at `blocker-logs/date-fns-full-baseline.md`
(July 2026, pre-monorepo ref `09ece22e`). Its blocker, the module-level
`let defaultOptions: DefaultOptions = {}`, no longer reproduces:
`_lib/defaultOptions` lowers on current main.

## How far it transpiles

| stage | result |
| --- | --- |
| Per-file HIR (`probe` scan) | **1,493 files, 0 with blockers.** Every source and spec file lowers to HIR on its own. |
| Whole-crate build, fixture as committed | **Aborts in MIR** at `pkgs/core/src/parse/index.ts`: ``class method `set` is not resolvable``. This is the only library-side abort. |
| Library only, excluding `parse/index.ts` and its importer `isMatch/index.ts` | **Emits a crate** (270 `.rs` files). |
| Library plus specs (`test-prefix = ["**/test.ts"]`) | 194 of 253 spec files import `TZDate`/`UTCDate`, which have no TypeScript implementation (family 2). The other 59 specs, minus 5 that import the excluded `parse`/`isMatch`, **emit a crate**: 731 `.rs` files, 54 spec modules, 308 `#[test]` functions. |
| `cargo check` of that crate | **Fails: 131 errors** (130 × E0382, 1 × E0599) and 367 warnings, so no test runs. |
| TS7 type parity (library sources) | **77.2%** (8,372 of 10,845). For comparison, es-toolkit is at 86.6% and hono at 91.8%. |
| Typedness (scoped crate, 730 programs) | values 66.8% typed, reads 52.9%, calls 57.5%. Only **3** values are carried as real generics. |
| `SmeltUnknown` (scoped crate) | 43,329 occurrences, of which **5,614 are avoidable erasure**. The sidecar records 2,175 erasure decisions, 2,146 of them avoidable. |

### Scoped build

The excludes used to reach the emitted crate are listed below. They exist
only in a scratch manifest. Following the first-pass rule, the committed
fixture keeps no excludes, so the daily probe still records the real
first abort.

- `pkgs/core/src/parse/index.ts`: family 3, abstract generic method
  dispatch.
- `pkgs/core/src/isMatch/index.ts`, `pkgs/core/src/fp/{isMatch,isMatchWithOptions,parse,parseWithOptions}/index.ts`
  and the specs `isMatch`, `setDefaultOptions`, `locale/zh-CN`, `zh-HK` and
  `zh-TW`: these import the excluded `parse`/`isMatch`.
- `pkgs/tz/**`, `pkgs/utc/**`, and the 194 specs that import
  `@date-fns/tz` or `@date-fns/utc`: family 2.

## Blocker families

| # | family | size | example |
| ---: | --- | --- | --- |
| 1 | **`DateType extends Date` generics and the `DateArg<DateType>` union are erased to `SmeltUnknown`** | Parity: 386 `Unknown ← TypeParam`, 323 `Unknown ← Named(Date)`, 192 `Optional<Unknown> ← undefined \| Function` (the `ContextFn<ResultDate>` `in` option), 64 `Float ← Named(Date)`, 60 `Float ← TypeParam`. Erasure sidecar: 607 `erased-union` (union of 3 arms, which is `DateArg = DateType \| number \| string`), 378 `erased-generic` `ResultDate`, 342 `unresolved-class` `Date`, 286 `unfilled-generic`. Typedness: 3 generic values in the whole crate. | `addDays/index.ts:53` `function addDays<DateType extends Date, ResultDate extends Date = DateType>(date: DateArg<DateType>, amount: number, options?: AddDaysOptions<ResultDate>): ResultDate`, and `constructFrom/index.ts:50`, which dispatches on `typeof date === "function"`, `constructFromSymbol in date` and `date.constructor`. |
| 2 | **Date subclasses implemented in JavaScript behind hand-written `.d.ts`** (`@date-fns/tz` `TZDate`, `@date-fns/utc` `UTCDate`) | 194 of 253 spec files. In the sibling packages the aborts are `declare methods are not lowered yet` (`pkgs/tz/src/date/index.d.ts`) and `exported const declarations require an initializer` (`mini.d.ts`). With those packages excluded, specs abort with ``TZDate from `@date-fns/tz` is declared but not implemented``. | `addDays/test.ts:1` `import { TZDate, tz } from "@date-fns/tz";` resolves to `pkgs/tz/src/date/index.{d.ts,js}`. |
| 3 | **Abstract generic method dispatch over a base-class array** | 1 MIR abort. It gates the whole crate and the `parse`/`isMatch` family: 2 library entry points, 4 `fp` wrappers and 5 specs. Parity on `parse/_lib`: 85 `Class(ParseResult) ← null \| Object{2 props}`, 48 `Bool ← null \| Named(MatchFnResult)`, 41 `TypeParam(DateType) ← number`. | `parse/index.ts:520` `setter.set(date, flags, subFnOptions)`, where `Setter` declares `public abstract set<DateType extends Date>(...): DateType \| [DateType, ParseFlags]` and the subclasses `ValueSetter<Value>` / `DateTimezoneSetter` implement it. |
| 4 | **Date setters on an erased receiver move the receiver twice** (E0382) | 130 of 131 `cargo check` errors (`use of partially moved value: _date` 51, `date` 22, `date_` 19, `fourth_of_january` 6, ...). | Generated `addBusinessDays_index_1.rs:85`: the `setDate` template does `match _date { SmeltUnknown::Object(value) => ... }` by value and then reads `_date` again in `if let (.., SmeltUnknown::Object(receiver)) = (&smelt_date_result, _date)`. Source: `_date.setDate(_date.getDate() + amount)`. |
| 5 | **`RegExpMatchArray` shape** | Parity: 48 `Class(__SmeltMatch) ← RegExpMatchArray`, 29 for the `\| null` form, 6 + 6 `List<String> ← RegExpMatchArray`. 1 E0599: `Option<SmeltRegExp>` has no `Display`. | `locale/_lib/buildMatchFn/index.ts:76-79` `const matchPattern = (width && args.matchPatterns[width]) \|\| args.matchPatterns[args.defaultMatchWidth]; const matchResult = string.match(matchPattern);` |
| 6 | **Optional option-bag fields typed as `Unknown`** | Parity: 26 `Optional<Unknown> ← LocaleOptions`, 24 + 20 `Day` literal unions, 15 `FirstWeekContainsDate`, 11/6/3 `Pick<Locale, ...>`. Sidecar: 24 `unresolved-class` `DefaultOptions`. | `startOfWeek/index.ts:52` `options?.weekStartsOn ?? options?.locale?.options?.weekStartsOn ?? defaultOptions.weekStartsOn ?? ...` |
| 7 | **Number-literal unions widened, or mis-typed as `String`** | Parity: 20 `String ← number` and 20 `String ← Named(Date)` (in `add`/`eachDayOfInterval`), 15 `String ← RangeError`, and the `NearestMinutes` / `RoundingMethod` literal unions erased. | `add/index.ts:79` (`years = 0` destructuring default of a `Duration`), `_lib/protectedTokens/index.ts:21` `throw new RangeError(_message)` |

### Top 3 for date-fns

1. **Erased `DateType extends Date` generics and the `DateArg` union.** This
   is the defining shape of the library. It drives most of the 22.8% parity
   gap and most of the 5,614 avoidable `SmeltUnknown` occurrences, and it
   indirectly causes family 4.
2. **`TZDate`/`UTCDate` exist only as JavaScript plus `.d.ts`.** This blocks
   194 of 253 spec files.
3. **Abstract generic method dispatch (`Setter.set`).** This is the one MIR
   abort. It gates the committed fixture's whole-crate build and all of
   `parse`.

## General rule or one-off gap?

- **Family 1 needs a general rule, and it is the central one.** A hand-written
  Rust port would define a `DateLike` trait, a supertrait over the `Date`
  methods date-fns uses plus a constructor (`constructFrom`), and write
  `fn add_days<D: DateLike, R: DateLike>(date: DateArg<D>, ...) -> R`, with
  `enum DateArg<D> { Date(D), Number(f64), String(String) }`. Smelt already
  has the parts: generic type parameters, generated unions, and trait bounds
  in other corpora. The missing rule is **a class-bounded type parameter
  (`T extends SomeClass`) lowers to `T: SomeClassTrait` instead of erasing**,
  plus `constructFrom`'s `date.constructor` / `[constructFromSymbol]`
  dispatch as a trait method. This is general, and it is not date-fns
  specific: any `T extends Base` library hits it. The 342 `unresolved-class
  Date` backend decisions suggest the emitter does not even render plain
  `Date` concretely once it flows through these generics. Check that
  before designing the trait.
- **Family 2 is a scope question rather than a lowering rule.** The real
  `TZDate`/`UTCDate` code is JavaScript (`pkgs/tz/src/date/index.js`), and
  the TypeScript probe scanner deliberately excludes JavaScript inputs (see
  `.github/compat/README.md` on immer and RxJS). There are three options,
  and this is the user's call:
  1. Accept JavaScript implementation files behind a `.d.ts`, typed by the
     declaration.
  2. Model `TZDate`/`UTCDate` as host classes, as was done for `Blob`/`File`
     in es-toolkit.
  3. Leave the 194 specs out and measure the 59 that remain.

  Two parts are general declaration-lowering gaps worth fixing regardless:
  `declare` methods on a `.d.ts` class, and `export const x: T;` without an
  initializer.
- **Family 3 needs a general rule.** Calling an abstract method through the
  base type is ordinary `dyn Trait` (or enum) dispatch. Here the method is
  also generic (`set<DateType extends Date>`), so it is not object-safe as
  written. The Rust-team answer is to make `DateType` a trait-level
  parameter, or to lower it through family 1's `DateLike` trait. It is
  worth checking whether non-generic abstract methods already resolve, to
  separate "abstract dispatch" from "generic abstract dispatch".
- **Family 4 is an emitter bug that is cheap to fix, but it is downstream of
  family 1.** The `Date` setter template for an erased receiver should
  borrow (`match &_date`) or clone once. It is general (every setter on
  every erased `Date`), but once family 1 lands, these receivers become
  concrete `Date` values and the template path changes, so fix it with or
  after family 1.
- **Family 5 is general.** `RegExpMatchArray` has two different
  representations (`__SmeltMatch` vs `List<String>`), and
  `String.prototype.match(RegExp | string)` receives an `Option<RegExp>`
  operand from `a && b || c`. This is a small runtime/emitter rule, not a
  date-fns special case.
- **Families 6 and 7 are general narrowing and literal-union gaps** that
  es-toolkit and hono have also chipped at: `??` chains over optional
  option-bag fields, and number-literal unions such as `0 | 1 | ... | 6`
  lowering to a number rather than `Unknown`. They are small individually
  but frequent.

Nothing found so far requires a date-fns-specific rule.

## Not covered yet

- No generated test has run, because `cargo check` fails first. Runtime
  behaviour (time zone, DST and locale formatting against `chrono`) is
  entirely unmeasured. Expect a new family there: date-fns specs construct
  local-time `new Date(y, m, d)` and assert local-time results.
- The parity report covers library sources only. Spec files are not
  measured, because the committed fixture has no `test-prefix`.
- The ~540 locale files are reachable from the entry, but only the
  locales that the 54 scoped specs touch went through `cargo check`.
- No CI gate or baseline was added. The fixture is only in
  `.github/compat/libraries.json`, so the daily `Library Probes` run will
  track it.
