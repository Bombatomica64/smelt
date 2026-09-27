// A module binding read from a function or closure must see the module's one
// evaluated value: a folded constant only when it is provably identical, and a
// module slot otherwise -- never a default fabricated from the declared type.

function zero(): number {
  return 0;
}

// A record literal the const folder cannot fold (its entry calls a function).
const p: Record<string, [number, string]> = { k: [zero(), 'a'] };

function readP(key: string): string {
  const entry = p[key];
  return `${entry[0]}:${entry[1]}`;
}

// An annotated array written by the module body and read from a closure.
const zs: number[] = [];
zs.push(1);
const cnt = (): number => zs.length;

// A never-reassigned `let` with a literal initializer is a constant.
let greeting = 'hello';
function greet(): string {
  return greeting;
}

// An arrow read from another module slot's initializer.
const double = (n: number): number => n * 2;
const table: Record<string, number> = { four: double(2) };
function lookup(key: string): number {
  return table[key];
}

// A slot's initializer runs at its declaration, not at the first read.
let seed = 1;
const snapshot: Record<string, number> = { seed: double(seed) };
seed = 10;
function snapshotSeed(): number {
  return snapshot['seed'] + seed;
}

console.log(readP('k'));
console.log(cnt());
console.log(greet());
console.log(lookup('four'));
console.log(snapshotSeed());
