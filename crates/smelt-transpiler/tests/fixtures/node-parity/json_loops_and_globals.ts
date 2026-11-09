// Loop shapes, JSON, host values and global numeric helpers print what Node prints.

function collect(values: number[], flags: boolean[]): number {
  let total = 0;
  let index = 0;
  while (index < flags.length) {
    if (flags[index]) {
      index = index + 1;
      continue;
    }
    for (const value of values) {
      if (value > 2) {
        break;
      }
      total = total + value;
    }
    index = index + 1;
  }
  return total;
}
console.log(collect([1, 2, 3], [false, true, false]));

function valid(value: boolean): boolean {
  return value;
}
function count(a: boolean, b: boolean): number {
  if (!valid(a) || !valid(b)) return NaN;
  let result = 0;
  while (a) {
    result += 1;
    break;
  }
  return result;
}
console.log(count(true, true), count(true, false));

function read(values: number[]): number[] {
  const result: number[] = [];
  let index = 0;
  while (index < values.length) {
    const value = values[index++];
    result.push(value);
  }
  return result;
}
console.log(read([4, 5]));

function firstOutside(value: number): number {
  let previous = value;
  let current = value;
  do {
    previous = current;
    current = current - 1;
  } while (current > 0);
  return previous;
}
console.log(firstOutside(3));

// JSON
const numberList: number[] = [1, 2];
console.log(JSON.stringify(numberList));
function jsonKey(args: unknown[]): string {
  return JSON.stringify({ args });
}
console.log(jsonKey([1, "a"]));
const parsedList = JSON.parse("[1,2]") as number[];
console.log(parsedList.length);
const enabled = JSON.parse('{"enabled":true}');
console.log(enabled.enabled);
interface ServerConfig {
  host: string;
  port: number;
}
const config = JSON.parse('{"host":"localhost","port":1337}') as ServerConfig;
console.log(config.host, config.port);
const bag = JSON.parse('{"a":1}') as Record<string, unknown>;
console.log(bag["a"]);
function parseBroken(): string {
  try {
    JSON.parse("{");
    return "parsed";
  } catch {
    return "threw";
  }
}
console.log(parseBroken());

// Regular expressions, includes, sets and maps
const pattern = "\\d+";
console.log(new RegExp(pattern).test("abc123"), RegExp(pattern).test("abc"), /\d+/.test("a1"));
const word = "Smelt";
console.log(word.includes("mel"), word.includes("x"));
function containsFrom(haystack: string, needle: string, from: number): boolean {
  return haystack.includes(needle, from);
}
console.log(containsFrom("abc", "b", 0), containsFrom("abc", "b", 2), containsFrom("abc", "", 99), containsFrom("abc", "a", -5));
const withNan: number[] = [1, NaN];
console.log(withNan.includes(NaN), withNan.includes(2));

const numberSet: Set<number> = new Set([1, 2, 3]);
const emptyStrings: Set<string> = new Set();
const dedupSource: number[] = [3, 1, 3];
const fromSource = new Set(dedupSource);
console.log(numberSet.has(2), numberSet.has(9), emptyStrings.size, fromSource.size, [...fromSource]);
const selected: Set<number> = new Set([1, 2]);
console.log([1, 2, 3].filter((value) => selected.has(value)));
let setTotal = 0;
for (const item of selected) {
  setTotal = setTotal + item;
}
console.log(setTotal);
selected.clear();
console.log(selected.size, selected.has(1));
const objectKeyA = { id: 1 };
const objectKeyB = { id: 1 };
const objectKeys = new Set([objectKeyA]);
objectKeyA.id = 2;
console.log(objectKeys.has(objectKeyA), objectKeys.has(objectKeyB));

const mapping: Map<string, number> = new Map([["a", 1], ["b", 2]]);
let last: [string, number] = ["", 0];
for (const entry of mapping) {
  last = entry;
}
console.log(last[0], last[1]);

// Array construction
function bare(n: number): unknown[] {
  return Array(n);
}
function constructed(n: number): unknown[] {
  return new Array(n);
}
console.log(bare(2).length, constructed(3).length, bare(1)[0]);
function fillIndexes(total: number): number[] {
  const result: number[] = Array(total);
  for (let i = 0; i < total; i++) {
    result[i] = i;
  }
  return result;
}
console.log(fillIndexes(3));
function singleElement(): string[] {
  return Array("a");
}
function severalElements(): number[] {
  return Array(1, 2, 3);
}
console.log(singleElement(), severalElements());
function holes(): unknown[][] {
  const grid: unknown[][] = [Array(1), Array(1)];
  grid[0].push(1);
  return grid;
}
console.log(holes()[0].length, holes()[1].length);

// Numbers
const big = 1e21;
const small = 1e-7;
console.log((42).toString(), big.toString(), small.toString());
function dateText(result: Date): string {
  return result.toString();
}
console.log(dateText(new Date(NaN)));
console.log(Number.parseFloat("42.5"), parseFloat("3.5x"), parseFloat("x"));
function parseValue(value: any): number {
  return parseFloat(value);
}
console.log(parseValue("1.5"), parseValue(2.5), parseValue(true));
console.log(Number.parseInt("42"), parseInt("7f", 16), parseInt("z"));
function leadingNumber(regexp: RegExp, text: string): number {
  const match = text.match(regexp);
  if (!match) return -1;
  return parseInt(match[0], 10);
}
console.log(leadingNumber(/\d+/, "abc12"), leadingNumber(/\d+/, "abc"));
const finiteCheck = 4;
console.log(Number.isFinite(finiteCheck), Number.isNaN(finiteCheck));
console.log(Infinity, -Infinity, NaN);
function invalid<ResultDate extends Date>(value: ResultDate): boolean {
  return isNaN(value.getTime());
}
console.log(invalid(new Date(NaN)), invalid(new Date(0)));
function numeric(value: number | undefined): boolean {
  return value != null && isNaN(value);
}
function optionalResult(): number | undefined {
  return undefined;
}
function numericResult(): boolean {
  const value = optionalResult();
  return value != null && isNaN(value);
}
function dateValue(value: Date | undefined): boolean {
  return value instanceof Date && isNaN(value.getTime());
}
console.log(numeric(NaN), numeric(1), numeric(undefined), numericResult(), dateValue(new Date(NaN)), dateValue(undefined));
function select<ResultDate extends Date>(result: ResultDate | undefined): unknown {
  return result || NaN;
}
console.log(select(undefined), select(new Date(0)) instanceof Date);
function absent<ResultDate extends Date>(result: ResultDate | undefined): boolean {
  return !result;
}
console.log(absent(undefined), absent(new Date(0)));

// Parameters and dispatch
const isMissing = (_: number, arg2?: number): boolean => arg2 === undefined;
console.log(isMissing(1), isMissing(1, 2));
const dispatchImpl = (
  data: number,
  predicate: (value: number) => boolean,
  ...rest: readonly number[]
): number => (predicate(data) ? data : rest[0]);
function dispatch(...args: readonly unknown[]): unknown {
  // @ts-expect-error -- exercises dynamic dispatch through erased arguments
  return dispatchImpl(...args);
}
console.log(dispatch(1, (value: number) => value > 0, 9), dispatch(1, (value: number) => value > 5, 9));

// instanceof and host identity
class Box {
  constructor() {}
}
class Other {
  constructor() {}
}
const boxValue = new Box();
console.log(boxValue instanceof Box, boxValue instanceof Other);
function isDate(value: unknown): boolean {
  return value instanceof Date;
}
console.log(isDate(new Date(NaN)), isDate(1));
function isBlob(value: unknown): boolean {
  if (typeof Blob === "undefined") {
    return false;
  }
  return value instanceof Blob;
}
console.log(isBlob(new Blob(["content"], { type: "text/plain" })), isBlob(1));
const file = new File(["content"], "file.txt", { type: "text/plain" });
console.log(file instanceof File, file instanceof Blob, file.name, file.size);
function isBoxedNumber(value: unknown): boolean {
  return value instanceof Number;
}
console.log(isBoxedNumber(new Number(42)), isBoxedNumber(42));
const direct: unknown = new Headers({ a: "b" });
if (direct instanceof Headers) {
  console.log(direct.get("a") ?? "null");
}
const enc: unknown = new TextEncoder();
console.log(enc instanceof TextEncoder);

// Tuples, keyed reads and indexes
const pair: [string, number] = ["Ada", 1];
console.log(pair[0], pair[1]);
class Parser {
  run(): number {
    return 1;
  }
}
function readParser(parsers: Record<string, Parser>, key: string): number {
  const parser = parsers[key];
  if (parser) {
    return parser.run();
  }
  return 0;
}
console.log(readParser({ a: new Parser() }, "a"), readParser({}, "b"));
const daysInMonths = [31, null, 31];
function readMonth(month: number): number | null | undefined {
  return daysInMonths[month];
}
console.log(readMonth(0) ?? "none", readMonth(1) ?? "none", readMonth(-1) ?? "none", readMonth(9) ?? "none");
const onChain: string[] = ["x", "y"];
function chained(list: string[] | undefined, at: number): string {
  return list?.[at] ?? "none";
}
console.log(chained(onChain, 1), chained(onChain, -1), chained(undefined, 0));
function lastOf<T>(values: readonly T[]): T | undefined {
  return values.at(-1);
}
console.log(lastOf([1, 2]), lastOf<number>([]));
type Formatter = (value: string) => string;
const lower: Formatter = (value) => value.toLowerCase();
const table: Record<string, Formatter> = { a: lower };
function applyTable(values: string[]): string[] {
  return values.map((value) => {
    const key = value[0];
    const formatter = table[key];
    if (formatter) {
      return formatter(value);
    }
    return value;
  });
}
console.log(applyTable(["aB", "Cd"]));
function chunkNumbers(data: number[], size: number): number[][] {
  const chunks = Math.ceil(data.length / size);
  const result: number[][] = [];
  for (let index = 0; index < chunks; index++) {
    const start = index * size;
    result.push(data.slice(start, start + size));
  }
  return result;
}
console.log(chunkNumbers([1, 2, 3], 2));

// Encoding, tags, clones, reflection
console.log(encodeURI("https://ex.com/a b?x=1&y=2#frag"));
const asValue = encodeURI;
console.log(asValue("a b"));
function tag(value: unknown): string {
  return Object.prototype.toString.call(value);
}
console.log(tag([]), tag(new Date(0)), tag(1), tag("a"), tag(null), tag(new Map()), tag(new Set()));
function copyErased(value: unknown): unknown {
  return structuredClone(value);
}
const original = { a: { b: 1 } };
const cloned = copyErased(original) as { a: { b: number } };
cloned.a.b = 2;
console.log(original.a.b, cloned.a.b);
interface Point {
  x: number;
  y: number;
}
function copyPoint(value: Point): Point {
  return structuredClone(value);
}
console.log(copyPoint({ x: 1, y: 2 }).y);
function rebuild(value: unknown): unknown {
  const Constructor = Object.getPrototypeOf(value).constructor;
  return new Constructor(value);
}
const rebuiltDate = rebuild(new Date(5));
console.log(rebuiltDate instanceof Date, (rebuiltDate as Date).getTime());
function eraseMap(): unknown {
  const m = new Map<string, number>([["a", 1]]);
  return m;
}
const erasedMap = eraseMap();
console.log(erasedMap instanceof Map, tag(erasedMap), Object.keys(erasedMap as object).length);

// Errors, module-level host values
const error = new Error("boom", { cause: "root" });
const aggregate = new AggregateError([new Error("a")], "many");
console.log(error.message, error.cause, aggregate.message, aggregate.errors.length);
const encoder = new TextEncoder();
const headers = new Headers({ "content-type": "text/plain" });
function encoded(text: string): number {
  return encoder.encode(text).length;
}
function contentType(): string {
  return headers.get("content-type") ?? "none";
}
console.log(encoded("abc"), contentType());
