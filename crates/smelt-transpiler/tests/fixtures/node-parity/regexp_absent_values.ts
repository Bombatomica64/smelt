// What a regular expression and its match answer when a value is absent: a
// group that did not take part is `undefined`, `indices` exists only with
// the `d` flag, a property the value does not have is `undefined`, and the
// lib's own match types name the match value.
const m = "ac".match(/(a)(b)?(c)/);
if (m !== null) {
  console.log(m[2], (m[2] as string | undefined) === undefined, m.length, m.index);
  console.log(m.indices, (m as any).foo);
}
const re = /x/g;
console.log(re.sticky, re.unicode, re.hasIndices, re.unicodeSets, (re as any).nope, re.global);
const d = /(a)(b)?/d.exec("😀a");
console.log(d?.indices?.[0], d?.indices?.[2], d?.index);
for (const hit of "a1😀b2".matchAll(/([a-z])(\d)/dg)) {
  console.log(hit.index, hit.indices?.[0], hit.indices?.[2], hit.indices?.length);
}
function group(mm: RegExpMatchArray | null): string | undefined { return mm === null ? "none" : mm[1]; }
function execGroup(mm: RegExpExecArray | null): number { return mm === null ? -1 : mm.index; }
console.log(group("xa".match(/(a)/)), group("x".match(/(a)?/)), group(null), execGroup(/b/.exec("ab")));
