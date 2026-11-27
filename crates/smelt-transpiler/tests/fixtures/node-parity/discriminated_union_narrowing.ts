// A discriminant comparison narrows a union to the arms whose literal field
// admits the compared value, in `if`, `else`, after an early exit, in each
// `switch` arm and in its `default`; reads on the narrowed value are typed.
interface Circle { kind: "circle"; r: number }
interface Rect { kind: "rect"; w: number; h: number }
interface Tri { kind: "tri"; base: number; height: number }
type Shape = Circle | Rect | Tri;

function area(s: Shape): number {
  if (s.kind === "circle") return 3 * s.r * s.r;
  if (s.kind === "rect") return s.w * s.h;
  return (s.base * s.height) / 2;
}

function perimeterish(s: Shape): number {
  switch (s.kind) {
    case "circle":
      return 6 * s.r;
    case "rect":
      return 2 * (s.w + s.h);
    default:
      return s.base + s.height;
  }
}

function isRound(s: Shape): string {
  if (s.kind !== "circle") {
    return "not round, kind " + s.kind;
  }
  return "round r=" + s.r;
}

// Grouped labels: the arm sees both arms that admit one of its labels.
function corners(s: Shape): number {
  switch (s.kind) {
    case "rect":
    case "tri":
      return s.kind === "rect" ? 4 : 3;
    case "circle":
      return 0;
  }
}

// Anonymous arms, one whose fields all share `string` (a dictionary shape
// unless its literal discriminant keeps it a record), keyed by `type`.
type Ev = { type: "num"; n: number } | { type: "text"; msg: string; lang: string };

function describe(e: Ev): string {
  if (e.type === "num") {
    return "n=" + (e.n + 1);
  }
  return e.lang + ":" + e.msg.toUpperCase();
}

// Number and boolean discriminants.
type Res = { ok: true; value: number } | { ok: false; error: string };
function unwrap(r: Res): string {
  if (r.ok === true) return "value " + r.value * 2;
  return "error " + r.error;
}
type Lvl = { level: 1; a: string } | { level: 2; b: number };
function lvl(l: Lvl): string {
  switch (l.level) {
    case 1:
      return "one " + l.a;
    case 2:
      return "two " + (l.b + 0.5);
  }
}

// A failed `in` test rules out arms that always carry the key.
interface Named { name: string; age: number }
interface Anon { id: number }
function label(x: Named | Anon): string {
  if ("name" in x) return x.name + "/" + x.age;
  return "#" + x.id;
}

// An arm whose discriminant is a plain `string` cannot be ruled out by a
// literal: it stays in both branches.
interface Loose { kind: string; note: string }
function loose(v: Circle | Loose): string {
  if (v.kind === "circle") return "kind " + v.kind;
  return "other " + v.kind;
}

const shapes: Shape[] = [
  { kind: "circle", r: 2 },
  { kind: "rect", w: 2, h: 5 },
  { kind: "tri", base: 4, height: 3 },
];
for (const s of shapes) {
  console.log(s.kind, area(s), perimeterish(s), isRound(s), corners(s));
}
console.log(describe({ type: "num", n: 41 }), describe({ type: "text", msg: "hi", lang: "en" }));
console.log(unwrap({ ok: true, value: 21 }), unwrap({ ok: false, error: "boom" }));
console.log(lvl({ level: 1, a: "x" }), lvl({ level: 2, b: 2 }));
console.log(label({ name: "ada", age: 36 }), label({ id: 7 }));
console.log(loose({ kind: "circle", r: 1 }), loose({ kind: "circle", note: "n" }), loose({ kind: "sq", note: "n" }));

// A nested literal argument picks its arm inside a nested field union.
type Input =
  | { type: "a"; value: { type: "img"; src: string } | { type: "text"; content: string; size: number } }
  | { type: "b"; value: number };
function show(input: Input): string {
  if (input.type === "b") return "b " + (input.value + 1);
  const v = input.value;
  return v.type === "img" ? "img " + v.src : "text " + v.content + "/" + v.size;
}
console.log(show({ type: "a", value: { type: "img", src: "cat.png" } }), show({ type: "a", value: { type: "text", size: 2, content: "yo" } }), show({ type: "b", value: 41 }));

// A key an arm does not declare rules that arm out (excess property).
type Rej = { tag: "rej"; fields: string[] } | { tag: "rej" };
function rej(r: Rej): string {
  return "fields" in r ? "fields " + r.fields.length : "bare";
}
console.log(rej({ tag: "rej", fields: ["a", "b"] }), rej({ tag: "rej" }));

// `in` on an arm whose property is optional tests the property, not the arm.
interface MaybeNamed { name?: string }
interface Numbered { id: number }
function named(v: MaybeNamed | Numbered): string {
  return "name" in v ? "named" : "unnamed";
}
console.log(named({}), named({ name: "x" }), named({ id: 1 }));

// `in` with a camelCase key narrows by the key's source spelling.
interface Responder { getResponse(): string }
interface Plain { message: string }
function respond(e: Responder | Plain): string {
  if ("getResponse" in e) return e.getResponse();
  return "plain " + e.message;
}
console.log(respond({ getResponse: () => "resp" }), respond({ message: "m" }));
