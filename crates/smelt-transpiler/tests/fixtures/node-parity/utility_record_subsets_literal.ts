// `Pick`/`Omit`/`Partial`/`Required` with a key set the annotation fixes name
// real subsets of the base's properties: enumeration sees exactly those.
interface Base {
  a: string;
  b: number;
  c?: boolean;
}
interface Derived extends Base {
  d: number;
}
class Counter {
  x = 1;
  y = "s";
}
interface Box<T> {
  v: T;
  w: number;
}
type OnlyA = "a";
type Generic<K extends keyof Base> = Pick<Base, K>;

function keep<T>(box: Box<T>): Pick<Box<T>, "v"> {
  return { v: box.v };
}

function report(label: string, keys: string[], json: string, spread: string): void {
  console.log(label, keys.join(","), json, spread);
}

function main(): void {
  const picked: Pick<Base, "a"> = { a: "p" };
  report("pick", Object.keys(picked), JSON.stringify(picked), JSON.stringify({ ...picked }));
  const omitted: Omit<Base, "a" | "c"> = { b: 2 };
  report("omit", Object.keys(omitted), JSON.stringify(omitted), JSON.stringify({ ...omitted }));
  const viaAlias: Pick<Base, OnlyA> = { a: "alias" };
  report("alias-key", Object.keys(viaAlias), JSON.stringify(viaAlias), JSON.stringify({ ...viaAlias }));
  const viaGeneric: Generic<"a"> = { a: "generic" };
  report("alias-generic", Object.keys(viaGeneric), JSON.stringify(viaGeneric), JSON.stringify({ ...viaGeneric }));
  const all: Pick<Base, keyof Base> = { a: "k", b: 3 };
  report("keyof", Object.keys(all), JSON.stringify(all), JSON.stringify({ ...all }));
  const inherited: Pick<Derived, "a" | "d"> = { a: "i", d: 4 };
  report("inherited", Object.keys(inherited), JSON.stringify(inherited), JSON.stringify({ ...inherited }));
  const boxed: Omit<Box<string>, "w"> = { v: "b" };
  report("generic-base", Object.keys(boxed), JSON.stringify(boxed), JSON.stringify({ ...boxed }));
  const fromClass: Pick<Counter, "x"> = { x: 5 };
  report("class", Object.keys(fromClass), JSON.stringify(fromClass), JSON.stringify({ ...fromClass }));
  const partial: Partial<Derived> = { d: 6 };
  report("partial", Object.keys(partial), JSON.stringify(partial), JSON.stringify({ ...partial }));
  const required: Required<Base> = { a: "r", b: 7, c: true };
  report("required", Object.keys(required), JSON.stringify(required), JSON.stringify({ ...required }));
  const kept = keep({ v: 8, w: 9 });
  report("generic-subset", Object.keys(kept), JSON.stringify(kept), JSON.stringify({ ...kept }));
  console.log(required.c === true, partial.a === undefined);
}

main();
