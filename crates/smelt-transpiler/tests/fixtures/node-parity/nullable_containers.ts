// A `T | null` keeps its `null` inside every container: printed by
// `console.log`, erased into `unknown` (a live view of the typed list, `Map`
// or `Set`, a copied record or tuple), and read back. `T | null` and
// `T | undefined` share one Rust `Option<T>`; the type says which absent
// value it holds, and the emitter hands that to the runtime's container code
// (`emitter::absent_value`). A `T | undefined` keeps `undefined`.
class Point {
  x: number | null;
  tags: (string | null)[];
  constructor(x: number | null, tags: (string | null)[]) {
    this.x = x;
    this.tags = tags;
  }
}
interface Row {
  a: number | null;
  b?: number;
}
type Cell = string | number | null;

function show(value: unknown): void {
  console.log(value);
}
function lastIsNull(values: unknown): boolean {
  const list = values as unknown[];
  return list[list.length - 1] === null;
}

const numbers: (number | null)[] = [1, null, 3];
console.log(numbers);
show(numbers);
console.log(lastIsNull(numbers));

const point = new Point(null, ["a", null]);
console.log(point);
const row: Row = { a: null };
console.log(row);
show(row);
const withB: Row = { a: null, b: 3 };
console.log(withB);

const map = new Map<string, number | null>([["k", null], ["j", 2]]);
console.log(map);
show(map);
console.log((map as unknown as Map<string, unknown>).get("k") === null);

const set = new Set<number | null>([1, null]);
console.log(set);
show(set);

const pair: [number | null, string] = [null, "z"];
console.log(pair);
show(pair);

const nested: (number | null)[][] = [[null, 1]];
console.log(nested);
show(nested);

const record: Record<string, number | null> = { q: null };
console.log(record);
show(record);

const holder: { v: (number | null)[] } = { v: [null] };
console.log(holder);
show(holder);

// Several present arms and `null`: the optional of the present union.
const cells: Cell[] = ["a", 2, null];
console.log(cells);
show(cells);
console.log(lastIsNull(cells), JSON.stringify(cells));
function pick(i: number): Cell {
  return i === 0 ? null : i === 1 ? "one" : i;
}
for (let i = 0; i < 3; i++) {
  const cell = pick(i);
  console.log(cell, cell === null, typeof cell);
  show(cell);
}
const erased: unknown = cells;
(erased as Cell[]).push(null);
console.log(cells.length, cells[3] === null, cells);
const rows: { v: Cell }[] = [{ v: null }, { v: "x" }];
console.log(rows);
show(rows);
console.log(JSON.stringify(rows));
const cellMap = new Map<string, Cell>([["a", null], ["b", 1]]);
show(cellMap);

// `undefined` stays `undefined`.
const sparse: (number | undefined)[] = [1, undefined];
console.log(sparse);
show(sparse);
console.log(lastIsNull(sparse));
