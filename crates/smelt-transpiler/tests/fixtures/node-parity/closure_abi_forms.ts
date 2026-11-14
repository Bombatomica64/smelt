// Callback ABI forms that did not compile or lost writes: a callback writing
// its parameter's field, a rest callback in forEach/map, === on borrowed rest
// callbacks, a borrowed erased-rest callback forwarded to a typed slot, a
// switch with no default in an arrow, a method at a generic interface field.

interface Flags { era: number; name: string }
interface Setter { set: (flags: Flags) => void }
function fieldWrite(): void {
  const setter: Setter = { set: (flags) => { flags.era = 7; flags.name = "x"; } };
  const fl: Flags = { era: 1, name: "a" };
  setter.set(fl);
  console.log(fl.era, fl.name);
}

interface Opt { era?: number }
interface OptSetter { set: (flags: Opt) => number | [number, Opt] }
function forward(flags: Opt, setter: OptSetter): number | [number, Opt] { return setter.set(flags); }
function forwarded(setter: OptSetter): number { const flags: Opt = {}; forward(flags, setter); return flags.era!; }
function direct(setter: OptSetter): number { const flags: Opt = {}; setter.set(flags); return flags.era!; }

function restCallbacks(): void {
  const out: string[] = [];
  [1, 2, 3].forEach((...args) => { out.push(String(args[0]) + ":" + String(args[1]) + ":" + args.length); });
  console.log(out.join(","));
  console.log(["a", "b"].map((first, ...rest) => first + rest[0] + rest[1].length).join(" "));
}

function same(a: (...args: unknown[]) => void, b: (...args: unknown[]) => void): boolean { return a === b; }
function sameTyped(a: (n: number) => number, b: (n: number) => number): boolean { return a === b && !(a !== b); }

function apply(cb: (n: number) => string | undefined, n: number): string { return cb(n) ?? "none"; }
function forwardLoose(fn: (...args: any[]) => any): string { return apply(fn, 3); }

const pick = (k: "a" | "b" | "c"): number => {
  switch (k) {
    case "a": return 1;
    case "b": return 2;
    case "c": return 3;
  }
};
const label = (n: number): string => {
  switch (n) { case 1: return "one"; case 2: { const s = "tw"; return s + "o"; } }
  return "many";
};

interface Acc<T> { add(m: number, h: T): number }

function main(): void {
  fieldWrite();
  console.log(forwarded({ set: (f) => { f.era = 3; return 1; } }), direct({ set: (f) => { f.era = 5; return [2, f]; } }));
  restCallbacks();
  const f = (...args: unknown[]) => { console.log(args.length); };
  const g = (...args: unknown[]) => {};
  const inc = (n: number) => n + 1;
  const dec = (n: number) => n - 1;
  console.log(same(f, f), same(f, g), sameTyped(inc, inc), sameTyped(inc, dec));
  console.log(forwardLoose((n: number) => n > 2 ? "big" : undefined), forwardLoose(() => undefined));
  console.log(pick("a"), pick("c"), label(1), label(2), label(9));
  const acc: Acc<string> = { add(m, h) { return m + h.length; } };
  console.log(acc.add(1, "abc"));
}
main();
