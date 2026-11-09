// Records, objects and interfaces: key order, reads, writes, erasure, literals.

function keyCount(value: unknown): number {
  return Object.keys(value as object).length;
}

function keyOrder(): void {
  const plain = { foo: 1, bar: 2, baz: 3 };
  const erased: unknown = plain;
  console.log(Object.keys(erased as object).join());
  const mixed: Record<string, unknown> = { b: 1, 2: "x", a: 3, 1: "y" };
  const erasedMixed: unknown = mixed;
  console.log(Object.keys(erasedMixed as object).join());
  const bag = erasedMixed as Record<string, unknown>;
  console.log(Object.entries(bag).map(([k, v]) => k + "=" + String(v)).join());
}

function hidesMetadata(): void {
  console.log(keyCount(new Date(0)), keyCount(/abc/u), keyCount("ab"), keyCount([1, 2, 3]));
  const fromArray = ["p", "q"] as unknown as Record<string, unknown>;
  console.log(fromArray["0"], fromArray["1"], fromArray["2"]);
}

function propertyKeys(): void {
  const record: Record<string, number> = { short: 5 };
  const key: any = ["short"];
  console.log(record[key]);
}

function cycle(): void {
  const symbolKey = Symbol("key");
  const complex: any = { loop: null, [symbolKey]: "symbol" };
  complex.loop = complex;
  console.log(complex.loop === complex, complex[symbolKey]);
}

function restAndDefaults(): void {
  const source = { call: () => 1, cancel: () => 2, flush: () => 3 };
  const { call, ...rest } = source;
  console.log(call(), Object.keys(rest).join(), rest.cancel() + rest.flush());
  const options: Record<string, unknown> = { trailing: false };
  const { leading = false, trailing = true, maxWait } = options;
  console.log(leading, trailing, maxWait);
}

interface Handle {
  readonly call: () => number;
  readonly cancel: () => number;
  readonly flush: () => number;
}
function makeHandle(): Handle {
  return { call: () => 1, cancel: () => 2, flush: () => 3 };
}
function wrapHandle(): Record<string, unknown> {
  const { call, ...rest } = makeHandle();
  call();
  return rest;
}

interface Options {
  width?: string;
}
function format(options: Options): string {
  return options.width ?? "full";
}

interface Shape {
  plain?: number;
  camelCase?: string;
  snake_case?: string;
}
function buildShape(): Shape {
  return { plain: 1, camelCase: "a", snake_case: "b" };
}

interface Inner {
  camelCase?: string;
  count?: number;
}
interface Outer {
  innerShape?: Inner;
}
function buildOuter(): Outer {
  return { innerShape: { camelCase: "deep", count: 2 } };
}

interface FormatLong {
  date: (options: Options) => string;
}

interface Duration {
  years?: number;
  months?: number;
}

function throttle(
  callback: () => void,
  wait = 0,
  { leading = true, trailing = true }: { leading?: boolean; trailing?: boolean } = {},
): boolean {
  callback();
  return leading && trailing && wait >= 0;
}

function callbackTable(): Record<string, (value: number) => string> {
  const suffix = "!";
  return {
    a: (value: number) => value.toString(),
    b: (value: number) => value.toString() + suffix,
  };
}

interface Comparison {
  comparison?: number;
}
function readOptional(options?: Comparison): number {
  return options?.comparison ?? 0;
}

interface Flags {
  era?: number;
}
function setEra(flags: Flags, value: number): void {
  flags.era = value;
}
function readEra(): number {
  const flags: Flags = {};
  setEra(flags, 1);
  return flags.era!;
}

function erasedOptional(flags: Flags): string {
  const erased: unknown = flags;
  return Object.keys(erased as object).join();
}

function removeKey(value: any, key: string): boolean {
  return delete value[key];
}

function assignName(value: any): any {
  value.name = "Grace";
  return value;
}

function assignByKey(out: Record<string, unknown>, key: unknown, value: unknown): Record<string, unknown> {
  out[key as string] = value;
  return out;
}

function group(values: string[]): Record<string, string[]> {
  const output: Record<string, string[]> = {};
  for (const value of values) {
    const key = value[0];
    const items = output[key];
    if (items === undefined) {
      output[key] = [value];
    } else {
      items.push(value);
    }
  }
  return output;
}

function symbolKeys(): void {
  const SymbolKey = Symbol("kind");
  const item: any = { [SymbolKey]: "cat", [Symbol("inline")]: "dog", 2: 123 };
  console.log(item[SymbolKey], item[2], Object.keys(item).join());
}

function hasLength(value: unknown): boolean {
  return Object.hasOwn(value as object, "length");
}

function eraseRead(value: any, key: string): unknown {
  return value[key];
}

function fillAny(): any {
  const result: any = [];
  for (let i = 0; i < 3; i++) {
    result[i] = i;
  }
  return result;
}

function buildDynamic(key: any, value: unknown): any {
  const result: any = {};
  result[key] = value;
  return result;
}

interface Locale {
  code: string;
}
interface FormatOptions {
  locale?: Locale;
}
function formatLocale(options?: FormatOptions): string {
  return options?.locale?.code ?? "";
}

interface InFormatLong {
  date: (value: string) => string;
}
interface InLocale {
  formatLong: InFormatLong;
}
interface OutFormatLong {
  date: (value: string) => string;
}
interface OutLocale {
  formatLong: OutFormatLong;
}
interface OutOptions {
  locale?: OutLocale;
  weekStartsOn?: number;
}
function outFormat(options?: OutOptions): string {
  return (options?.locale?.formatLong.date("P") ?? "") + String(options?.weekStartsOn);
}
function forward(locale: InLocale): string {
  return outFormat({ locale, weekStartsOn: 0 });
}

interface CallbackOptions {
  locale?: RecursiveLocale;
}
interface RecursiveLocale {
  formatRelative: (token: string, options?: CallbackOptions) => string;
}
function forwardRecursive(value: unknown): string {
  const locale = value as RecursiveLocale;
  return locale.formatRelative("today", { locale });
}

interface Localize {
  month: (value: number, options?: string) => string;
}
interface LocalizeLocale {
  localize: Localize;
}
interface LocalizeOptions {
  locale?: LocalizeLocale;
}
function useLocale(options?: LocalizeOptions): string {
  return options?.locale?.localize.month(0) ?? "";
}

interface MatchResult {
  value: number;
  rest: string;
}
interface Match {
  era: (text: string, options?: unknown) => MatchResult | null;
}
interface MatchLocale {
  match: Match;
}
interface ParseOptions {
  locale?: MatchLocale;
}
function useOptions(options?: ParseOptions): MatchResult | null {
  return options?.locale?.match.era("BC") ?? null;
}

type Partialable = { origin: string; maxAge?: number };
function merge(base: Partialable, options: Partial<Partialable>): Partialable {
  return { ...base, ...options };
}

function main(): void {
  keyOrder();
  hidesMetadata();
  propertyKeys();
  cycle();
  restAndDefaults();
  console.log(Object.keys(wrapHandle()).join());
  console.log(format({ width: "short" }), format({}));
  const shape = buildShape();
  console.log(shape.plain, shape.camelCase, shape.snake_case);
  const outer = buildOuter();
  console.log(outer.innerShape?.camelCase, outer.innerShape?.count);
  const formatLong: FormatLong = { date: (options) => options.width ?? "none" };
  console.log(formatLong.date({ width: "short" }), formatLong.date({}));
  const duration: Duration = {};
  console.log(duration.years, duration.months, Object.keys(duration).length);
  console.log(throttle(() => {}, 1, { leading: false }), throttle(() => {}));
  const table = callbackTable();
  console.log(table["a"](1), table["b"](2));
  console.log(readOptional(), readOptional({}), readOptional({ comparison: 3 }));
  console.log(readEra());
  console.log("[" + erasedOptional({}) + "]", "[" + erasedOptional({ era: 2 }) + "]");
  const victim: any = { a: 1, b: 2 };
  console.log(removeKey(victim, "a"), Object.keys(victim).join(), removeKey(victim, "zz"));
  console.log(assignName({ age: 1 }).name);
  console.log(assignByKey({}, "k", 9)["k"]);
  const grouped = group(["ax", "by", "az"]);
  console.log(grouped["a"].join(), grouped["b"].join());
  symbolKeys();
  console.log(hasLength([1]), hasLength({}), hasLength("s"));
  console.log(eraseRead([7, 8], "length"), eraseRead([7, 8], "1"), eraseRead("hey", "length"), eraseRead("hey", "0"), eraseRead([1], "zz"));
  const filled = fillAny();
  console.log(Array.isArray(filled), filled.length, filled[2]);
  console.log(buildDynamic("x", 3).x, buildDynamic(1, "n")["1"]);
  const user: Record<string, string> = { name: "Ada" };
  user.name = "Grace";
  console.log(user.name);
  const key = "name";
  user[key] = "Linus";
  console.log(user[key]);
  const customLocale = { code: "fr" };
  console.log(formatLocale({ locale: customLocale }), formatLocale(), "[" + formatLocale({}) + "]");
  console.log(forward({ formatLong: { date: (v) => v + "!" } }));
  const recursive: RecursiveLocale = { formatRelative: (token, options) => token + (options?.locale ? "+" : "-") };
  console.log(forwardRecursive(recursive));
  const shortCallback = { localize: { month: () => "works" } };
  console.log(useLocale({ locale: shortCallback }));
  const matchLocale = { match: { era: () => ({ value: 0, rest: " works" }) } };
  const matched = useOptions({ locale: matchLocale });
  console.log(matched?.value, matched?.rest);
  console.log(merge({ origin: "o", maxAge: 1 }, { maxAge: 2 }).maxAge, merge({ origin: "o" }, {}).origin);
}

main();
