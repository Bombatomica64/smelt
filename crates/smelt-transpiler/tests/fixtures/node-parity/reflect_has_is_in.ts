// `Reflect.has(target, key)` is `key in target`: own and inherited properties.
const base = { inherited: 1 };
const child = Object.create(base);
child.own = 2;
console.log(Reflect.has(child, "own"), Reflect.has(child, "inherited"), Reflect.has(child, "missing"), "inherited" in child);
const record: Record<string, number> = { a: 1 };
console.log(Reflect.has(record, "a"), Reflect.has(record, "b"));
