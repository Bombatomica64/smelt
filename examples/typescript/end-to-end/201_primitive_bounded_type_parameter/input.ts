// A function type parameter bounded by one primitive keyword is that
// primitive in Rust: TypeScript uses it to carry a literal type, a string or
// numeric enum, or a literal union through the signature, and every one of
// those instantiations is the same Rust type.
type Mode = "a" | "b";
enum Color {
  Red = "red",
  Green = "green",
}
enum Level {
  Low = 1,
  High = 2,
}
function echo<S extends string>(text: S): S {
  return text;
}
function pick<S extends string>(first: S, second: S, useFirst: boolean): S {
  return useFirst ? first : second;
}
function wrap<S extends string>(text: S): S[] {
  return [text, text];
}
function tagged<S extends string>(tag: S): { tag: S; size: number } {
  return { tag, size: tag.length };
}
function keep<N extends number>(value: N): N {
  return value;
}
function flag<B extends boolean>(value: B): B {
  return value;
}
function describe(mode: Mode): string {
  switch (mode) {
    case "a":
      return "first";
    case "b":
      return "second";
  }
}
function paint(color: Color): string {
  return color === Color.Red ? "warm" : "cool";
}
function rank(level: Level): string {
  return level === Level.High ? "high" : "low";
}
function withDefault<S extends string = "x">(text?: S): string {
  return text === undefined ? "none" : text;
}
function counts<K extends string>(keys: K[]): Record<K, number> {
  const out = {} as Record<K, number>;
  for (const key of keys) {
    out[key] = key.length;
  }
  return out;
}
const mode: Mode = "b";
const same: Mode = echo(mode);
console.log(describe(same));
console.log(describe(echo<Mode>("a")));
console.log(describe(pick<Mode>("a", "b", false)));
const modes: Mode[] = wrap(mode);
console.log(modes.map(describe).join(","));
const t = tagged(mode);
console.log(describe(t.tag), t.size);
const color: Color = echo(Color.Green);
console.log(paint(color), color);
console.log(paint(pick(Color.Red, Color.Green, true)));
const level: Level = keep(Level.High);
console.log(rank(level), level + 1);
const yes: true = flag(true);
console.log(yes ? "yes" : "no");
console.log(withDefault(), withDefault("y"));
const byMode: Record<Mode, number> = counts<Mode>(["a", "b"]);
console.log(byMode.a, byMode["b"], JSON.stringify(byMode));
const literal = echo("lit");
console.log(literal.toUpperCase(), keep(3) + 1);
function shout<S extends string>(text: S): string {
  return text.toUpperCase() + "!";
}
function double<N extends number>(value: N): number {
  return value * 2;
}
function negate<B extends boolean>(value: B): boolean {
  return !value;
}
function bump<I extends bigint>(value: I): bigint {
  return value + 1n;
}
function tagAll<T, K extends string>(items: T[], tag: K): string[] {
  return items.map((_item, index) => `${tag}${index}`);
}
console.log(shout("hi"), double(21), negate(true), bump(41n));
console.log(tagAll([1, 2], "n").join(","));
