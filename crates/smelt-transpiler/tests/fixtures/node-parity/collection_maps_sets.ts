// Map and Set: methods, projections, optional receivers, erasure and identity.

function setMethods(): void {
  const values: Set<number> = new Set([1, 2]);
  const same = values.add(3);
  console.log(same === values, values.delete(2), values.delete(42), values.size, [...values].join());
  console.log([...values.keys()].join(), [...values.values()].join(), [...values.entries()].map(([a, b]) => a + ":" + b).join("|"));
  values.clear();
  console.log(values.size);
}

function mapMethods(): void {
  const mapping: Map<string, number> = new Map();
  const same = mapping.set("a", 1);
  mapping.set("b", 2);
  console.log(same === mapping, mapping.has("a"), mapping.has("z"), mapping.get("a"), mapping.get("z"), mapping.size);
  console.log([...mapping.keys()].join(), [...mapping.values()].join(), [...mapping.entries()].map(([k, v]) => k + ":" + v).join("|"));
  console.log(mapping.delete("a"), mapping.delete("a"), mapping.size);
  mapping.clear();
  console.log(mapping.size);
  const literal = new Map([["a", 1], ["b", 2]]);
  console.log(literal.get("b"), literal.size);
}

function erasedSlots(): void {
  const mapping: Map<string, unknown> = new Map();
  mapping.set("a", 1);
  mapping.set("b", "two");
  console.log(mapping.size, mapping.get("b"));
  const bag: Set<unknown> = new Set();
  bag.add(1);
  bag.add("two");
  bag.add(1);
  console.log(bag.size);
}

function probe(stack: Map<string, number> | undefined, key: string): boolean | undefined {
  return stack?.has(key);
}
function fetchKey(stack: Map<string, number> | undefined, key: string): number | undefined {
  return stack?.get(key);
}
function store(stack: Map<string, number> | undefined, key: string): void {
  stack?.set(key, 1);
}
function dropKey(stack: Map<string, number> | undefined, key: string): boolean | undefined {
  return stack?.delete(key);
}
function seenHas(seen: Set<string> | undefined, value: string): boolean | undefined {
  return seen?.has(value);
}

function optionalReceivers(): void {
  const m = new Map<string, number>();
  store(m, "k");
  store(undefined, "k");
  console.log(probe(m, "k"), probe(undefined, "k"), fetchKey(m, "k"), fetchKey(undefined, "k"));
  console.log(dropKey(m, "k"), dropKey(undefined, "k"), m.size);
  console.log(seenHas(new Set(["v"]), "v"), seenHas(undefined, "v"));
}

function fromOptional(xs?: string[]): Set<string> {
  return new Set(xs);
}
function fromSet(s: Set<number>): Set<number> {
  return new Set(s);
}

function collect(values: unknown): Set<number> {
  const results = new Set<number>();
  for (const value of values as Iterable<unknown>) {
    results.add(value as number);
  }
  return results;
}

function unique(values: string[]): number {
  const seen = new Set<string>();
  for (const value of values) {
    seen.add(value);
  }
  return seen.size;
}

function tag(value: unknown): string {
  return Object.prototype.toString.call(value);
}

function erasureKinds(): void {
  const m = new Map<string, number>([["a", 1]]);
  const asMap: unknown = m;
  const obj = { a: 1 };
  const asRecord: unknown = obj;
  console.log(tag(asMap), tag(asRecord), asMap instanceof Map, asRecord instanceof Map);
}

function nullishRows(): boolean[] {
  const pairs = [
    [null, undefined, false],
    [undefined, null, false],
  ];
  return pairs.map((pair) => pair[0] === pair[1]);
}

function identities(): void {
  const original: unknown = [1];
  const alias: unknown = original;
  const copy: unknown = [1];
  console.log(alias === original, copy === original, Object.is(alias, original));
  const data: number[] = [1, 2, 3];
  console.log((data as unknown) === (data as unknown));
  console.log(([1, 2] as unknown) === ([1, 2] as unknown));
  const notANumber: unknown = NaN;
  console.log(Object.is(NaN, NaN), Object.is(0, -0), notANumber === notANumber);
  const map = new Map<number, string>([[1, "a"]]);
  const mapAlias = map;
  const mapCopy = new Map<number, string>([[1, "a"]]);
  console.log(mapAlias === map, mapCopy === map);
  const set = new Set<number>([1]);
  const setAlias = set;
  console.log(setAlias === set, new Set<number>([1]) === set);
}

function main(): void {
  setMethods();
  mapMethods();
  erasedSlots();
  optionalReceivers();
  console.log(fromOptional(["a", "a", "b"]).size);
  const original = new Set<number>([1]);
  const copied = fromSet(original);
  copied.add(2);
  console.log(original.size, copied.size);
  console.log([...collect([1, 2, 1])].join());
  console.log(unique(["x", "y", "x"]));
  erasureKinds();
  console.log(nullishRows().join());
  const row = [1, 2, null];
  console.log(row.length, row[2] === null);
  identities();
}

main();
