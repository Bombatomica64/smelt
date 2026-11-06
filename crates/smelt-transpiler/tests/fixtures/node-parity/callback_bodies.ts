// Callback bodies are ordinary bodies: every statement and expression form means
// what it means anywhere else.
type Formatter = (value: string) => string;
const lower: Formatter = (value) => value.toLowerCase();
const table: Record<string, Formatter> = { A: lower };
function apply(values: string[]): string[] {
  return values.map((value) => {
    const key = value[0];
    const formatter = table[key];
    if (formatter) {
      return formatter(value);
    }
    return value;
  });
}
console.log(apply(['Abc', 'Bcd']));

function purry<T>(
  callback: (...args: unknown[]) => unknown,
): (data: readonly T[], compare: (left: T, right: T) => number) => unknown {
  const n = 2;
  return (data, compare) => callback(data, compare, n);
}
function wrap<T>(
  func: (data: readonly T[], compare: (left: T, right: T) => number, n: number) => unknown,
): (data: readonly T[], compare: (left: T, right: T) => number) => unknown {
  const n = 2;
  return purry<T>((...args) => func(...(args as [readonly T[], (left: T, right: T) => number]), n));
}
console.log(JSON.stringify(wrap<number>((data, compare, n) => [...data].sort(compare).slice(0, n))([3, 1, 2], (a, b) => a - b)));

function maxWidth(rows: unknown[][]): number {
  return Math.max(...rows.map((r) => r.length));
}
console.log(maxWidth([[1], [1, 2, 3], []]));

function cases(values: string[], upper: boolean): string[] {
  return values.map((value) => '' + (upper ? value.toUpperCase() : value.toLowerCase()));
}
console.log(cases(['aB'], true), cases(['aB'], false));

const seen = new Set<number>([0, 2]);
console.log([9, 8, 7].filter((_, index) => seen.has(index)));

const names = ['ab', 'cd'];
console.log(names.find((name) => name.startsWith('c')));

function escapeWith(text: string, replacements: Record<string, unknown>): string {
  return text.replace(/[ab]/g, (match) => String(replacements[match]));
}
console.log(escapeWith('abc', { a: 1, b: null }));
function replaceAll(text: string, lookup: unknown): string {
  return text.replace(/[abc]/g, (match) => (lookup as Record<string, any>)[match]);
}
console.log(replaceAll('abd', { a: 1, b: 'x', d: 2 }));

const point: [number, number] = [2, 3];
console.log([point].map(([x, y]) => x + y));

const bump = (value = 1) => value + 1;
console.log(bump(), bump(4));

function zipLike<T, R>(...combine: Array<((...g: T[]) => R) | ArrayLike<T>>): R[] {
  const iteratee = combine.pop();
  const groups = combine as Array<ArrayLike<T>>;
  if (iteratee == null) {
    return [];
  }
  return groups.map((group) => (iteratee as (...g: T[]) => R)(...(group as T[])));
}
console.log(JSON.stringify(zipLike<number, number>([1, 2], [3, 4], (a, b) => a + b)));

function run(target: unknown, source: unknown, custom: (a: unknown, b: unknown) => boolean | undefined): boolean {
  return custom(target, source) ?? false;
}
let calls = 0;
console.log(run(1, 2, () => { calls += 1; return undefined; }), calls);
