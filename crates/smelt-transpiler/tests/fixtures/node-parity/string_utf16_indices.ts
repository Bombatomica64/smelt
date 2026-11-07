// JavaScript string indices count UTF-16 code units: every operation that
// answers an index and every one that takes one agree on that unit, for
// ASCII, non-ASCII and astral (surrogate-pair) text.
const words: string[] = ["café!", "a😀b!", "naïve", "abc", "日本語!", "😀😀x"];
for (const s of words) {
  console.log(s.length, s.indexOf("!"), s.lastIndexOf("b"), s.search(/!/), s.charAt(2), s.slice(1, 3), s.substring(2), s.at(-1));
  console.log(s.charCodeAt(1), s.codePointAt(0), s.codePointAt(9), s[1], s.includes("é"), s.startsWith("é", 3), s.endsWith("!"), s.padEnd(8, "."), s.split("").length);
  const i = s.indexOf("b");
  console.log(i, s.slice(0, i), s.slice(i), s.substr(1, 2), s.indexOf("!", 2), s.lastIndexOf("x", 4), s.includes("x", 3), Array.from(s).length, [...s].length);
  console.log(s.slice(-2), s.substring(3, 1), s.charAt(-1) === "", s.padStart(9, "😀"), s.slice(0, s.indexOf("!")) + "|" + s.slice(s.indexOf("!") + 1));
}
const m = "x😀yz".match(/y/);
console.log(m?.index, "x😀yz".replace(/y/, "_"), "😀😀".lastIndexOf("😀"), "😀😀".indexOf("😀", 1));
const offsets: number[] = [];
"é😀a😀a".replace(/a/g, (match: string, offset: number) => { offsets.push(offset); return match; });
console.log(offsets);
const re = /a/g;
const subject = "😀a😀a";
let found = re.exec(subject);
while (found !== null) {
  console.log(found.index, re.lastIndex, subject.slice(found.index, re.lastIndex));
  found = re.exec(subject);
}
for (const hit of "ñ1😀2".matchAll(/\d/g)) {
  console.log(hit.index, hit[0]);
}
function erasedLength(value: unknown): unknown {
  return (value as { length: unknown }).length;
}
console.log(erasedLength("a😀"), erasedLength("café"));
const t: string = "ab😀cdef";
console.log(t.substring(3, 1), t.substring(4, -2), t.substring(NaN, 2), t.substring(5), t.substr(-3, 2), t.substr(1), t.substr(2, 2), t.substr(-20, 3), t.substr(1, -1) === "");
console.log(t.startsWith("ab", -3), t.endsWith("😀", 4), t.includes("a", -5));
