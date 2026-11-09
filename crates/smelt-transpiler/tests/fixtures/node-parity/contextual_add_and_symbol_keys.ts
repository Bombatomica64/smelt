// `+` on contextually typed parameters concatenates, across a module boundary too,
// and symbol keys from Reflect.ownKeys stay symbols through a keyof assertion.
function zip<K extends string | number | symbol, V>(
  keys: K[],
  values: V | ((key: K, idx: number) => V) | V[],
): Record<K, V> {
  const isFn = typeof values === "function";
  return keys.reduce((acc, key, idx) => {
    acc[key] = isFn ? (values as any)(key, idx) : (values as any);
    return acc;
  }, {} as Record<K, V>);
}
console.log(zip(["a", "b"], (k, i) => k + i));
const prefix = <A>(x: A, cb: A | ((a: A) => string)) => (typeof cb === "function" ? (cb as any)(x) : cb);
console.log(prefix("s", (a) => a + 1));
const sum = (ns: number[], cb: (n: number, i: number) => number) => ns.map(cb);
console.log(sum([1], (n, i) => n + i));

const sk = Symbol("k");
function ownTypes<T>(x: T): string[] {
  const keys = Reflect.ownKeys(x as unknown as object) as (keyof typeof x)[];
  const out: string[] = [];
  for (let i = 0; i < keys.length; i++) {
    out.push(typeof keys[i]);
    if ((x as any)[keys[i]] === undefined) out.push("missing");
  }
  return out;
}
console.log(ownTypes({ [sk]: "s", a: 1 }));
function same<T>(x: T, y: T): boolean {
  const kx = Reflect.ownKeys(x as unknown as object) as (keyof typeof x)[];
  for (let i = 0; i < kx.length; i++) {
    if (!Reflect.has(y as unknown as object, kx[i])) return false;
    if (x[kx[i]] !== y[kx[i]]) return false;
  }
  return true;
}
const rec: any = { [sk]: "s", x: 1 };
console.log(same(rec, { ...rec }), same(rec, { x: 1 }));
const stringKeys = Object.keys({ a: 1 }) as (keyof { a: number })[];
console.log(stringKeys);
