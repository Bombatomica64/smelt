// A class the program declares is that class, even when it is spelled like a
// host constructor (`URL`, `Date`, `RegExp`, an error class, `Map`).
class URL {
  href: string;
  constructor(href: string) { this.href = href; }
  describe(): string { return "user URL " + this.href; }
}
class Date {
  day: number;
  constructor(day: number) { this.day = day; }
  getTime(): number { return this.day * 10; }
}
class RegExp {
  source: string;
  constructor(source: string) { this.source = source; }
  test(text: string): boolean { return text === this.source; }
}
class TypeError {
  message: string;
  constructor(message: string) { this.message = message; }
}
class Map {
  size = 42;
}
const u = new URL("x");
const d = new Date(3);
const r = new RegExp("abc");
const e = new TypeError("oops");
const m = new Map();
console.log(u.describe(), u.href, d.getTime(), r.test("abc"), r.test("a.c"), e.message, e instanceof TypeError, m.size);
console.log(u instanceof URL, d instanceof Date, r instanceof RegExp, m instanceof Map);
class Set {
  items: number[] = [];
  add(item: number): Set { this.items.push(item * 2); return this; }
}
const s = new Set().add(1).add(2);
console.log(s.items, s instanceof Set);
