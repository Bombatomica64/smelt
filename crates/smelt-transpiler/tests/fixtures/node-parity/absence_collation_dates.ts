// Values JavaScript reads as absent or orders by locale:
// - an out-of-range `number[]` or typed-array read is `undefined` (falsy, NaN in
//   arithmetic, `null` in JSON), and `NaN` is falsy;
// - a `T | null` holding `null` is never `=== undefined` and prints `null`;
// - `localeCompare` orders by the root collation (accents beside their letter);
// - an ISO date-time without an offset is local time.
const arr = [1, 2, 3];
console.log(arr[5], arr[5] === undefined ? "missing" : "present");
const e: number[] = []; const last = e.length - 1; console.log(last, e[last]);
const holes: number[] = Array(3);
console.log(holes.join(), holes.length, JSON.stringify([arr[9]]), arr[9] + 1);
function portOf(ports: number[]): number { return ports[0] || 80; }
console.log(portOf([8080]), portOf([0]), portOf([]));
const u = new Uint8Array(2); console.log(u[5], u[1], new Float64Array(1)[3]);
const n = parseFloat("x"); console.log(n || 7, n ? "t" : "f", !n, [NaN, 1].filter((v) => v).length);

const m = "x".match(/y/);
console.log(m === null, m === undefined, m == null, String(m), `${m}`);
console.log(m);
console.log("m:", m, 1 + 1);
const h = "xy".match(/y/);
console.log(h && h[0], h === null);

console.log("ä".localeCompare("z"), ["z", "ä", "a"].sort((p, q) => p.localeCompare(q)).join(","));
console.log("a".localeCompare("B"), "a".localeCompare("A"), "résumé".localeCompare("resume"), "10".localeCompare("9"));

// A date-time without an offset is read in local time, so the local fields
// read back what was written whatever the zone; one with an offset, and a
// date-only string (UTC midnight), name one instant.
for (const t of ["2020-01-01T10:20:30", "2020-06-15T23:05", "2020-01-01T00:00:00.250"]) {
  const d = new Date(t);
  console.log(t, d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds());
}
for (const t of ["2020-01-01", "2020-01-01T00:00:00Z", "2020-01-01T00:00:00+02:00", "nonsense"]) {
  console.log(t, new Date(t).getTime());
}

// An object-literal method that takes a callback and passes an arrow on.
const Obj = { filter(xs: number[], f: (x: number) => boolean) { return xs.filter((x) => !f(x)); } };
console.log(Obj.filter([1, 2, 3], (x) => x > 1).join(","));
// An interface whose method is optional builds from `{}`.
interface P { name?: string; greet?(): string }
const p: P = {};
console.log(p.name === undefined, p.greet === undefined);
