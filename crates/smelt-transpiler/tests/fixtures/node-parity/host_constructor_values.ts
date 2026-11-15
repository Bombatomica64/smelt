// A host constructor read as a value is the one constructor object: `typeof`
// is "function", it is `===` across reads, to `instance.constructor` and to
// `X.prototype.constructor`, `new` through it builds the host value, and an
// erased Map or Set built that way answers its methods by name, its
// `keys()`/`values()`/`entries()` being live, single-pass iterators.
function make(ctor: unknown, ...args: unknown[]): any {
  return new (ctor as any)(...args);
}

function main(): void {
  const c: unknown = Date;
  const m: unknown = Map;
  const e: unknown = TypeError;
  console.log(typeof c, typeof m, typeof e, c === Date, m === Map, e === TypeError);
  console.log(Date.prototype.constructor === Date, Map.prototype.constructor === Map, TypeError.prototype.constructor === TypeError);
  console.log((new Date(0) as any).constructor === Date, (new TypeError("x") as any).constructor === TypeError);

  const Cache: any = undefined;
  const cache = new (Cache || Map)();
  cache.set("k", 1).set(NaN, 2);
  console.log(cache.get("k"), cache.get(NaN), cache.has("z"), cache instanceof Map, cache.size);
  cache.delete("k");
  console.log(cache.size, JSON.stringify(cache.keys()), Object.keys(cache.keys()).length);
  const keys = cache.keys();
  console.log(keys.next().value, Array.from(keys).length, Array.from(keys).length, keys[Symbol.iterator]() === keys);
  const live = cache.values();
  cache.set("late", 3);
  console.log(Array.from(live).join(","), JSON.stringify(Array.from(cache.entries())));

  const set = make(Set, [1, 1, 2]);
  set.add(3);
  console.log(set.size, set.has(2), set instanceof Set);
  const date = make(Date, 86400000);
  console.log(date instanceof Date, date.getTime());

  const ctors: unknown[] = [Date, Map, Set, TypeError, Error, Uint8Array];
  console.log(ctors.map((k: any) => k.name).join(","));
  const built = make(e, "boom");
  console.log(built instanceof TypeError, built instanceof Error, built.message);
}

main();
