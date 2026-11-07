// JavaScript string indices count UTF-16 code units: every operation that
// answers an index and every one that takes one agree on that unit, for
// ASCII, non-ASCII and astral (surrogate-pair) text. A lone surrogate half is
// never printed on its own here: Smelt gives a cut pair to the piece on the
// cut's left, so the pieces still join back into the string.
const words: string[] = ["café!", "naïve", "abc", "日本語!"];
for (const s of words) {
  console.log(s.length, s.indexOf("!"), s.lastIndexOf("b"), s.search(/!/), s.charAt(2), s.slice(1, 3), s.substring(2), s.at(-1));
  console.log(s.charCodeAt(1), s.codePointAt(0), s.codePointAt(9), s[1], s.includes("é"), s.startsWith("é", 3), s.endsWith("!"), s.padEnd(8, "."), s.split("").length);
  const i = s.indexOf("b");
  console.log(i, s.slice(0, i), s.slice(i), s.substr(1, 2), s.indexOf("!", 2), s.lastIndexOf("x", 4), s.includes("x", 3), Array.from(s).length, [...s].length);
  console.log(s.slice(-2), s.substring(3, 1), s.charAt(-1) === "", s.padStart(s.length + 4, "😀"), s.slice(0, s.indexOf("!")) + "|" + s.slice(s.indexOf("!") + 1));
}
const astral: string[] = ["a😀b!", "😀😀x", "x😀"];
for (const s of astral) {
  console.log(s.length, s.indexOf("!"), s.lastIndexOf("x"), s.search(/x/), s.charCodeAt(1), s.codePointAt(0), s.codePointAt(1), s.split("").length, [...s].length);
  const at = s.indexOf("😀");
  console.log(s.slice(at, at + 2), s.slice(at + 2), s.substring(0, at), s.indexOf("😀", 1), s.includes("x", 2), s.endsWith("😀", 4), s.padEnd(7, "."));
  let joined = "";
  for (let i = 0; i < s.length; i++) joined += s[i];
  let cuts = 0;
  for (let k = 0; k <= s.length; k++) if (s.slice(0, k) + s.slice(k) === s) cuts++;
  console.log(joined === s, cuts, s.split("").join("") === s, s.charAt(0).toUpperCase() + s.slice(1) === s.toUpperCase());
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
console.log(t.substring(4, 1), t.substring(4, -2), t.substring(NaN, 2), t.substring(5), t.substr(-3, 2), t.substr(1), t.substr(2, 2), t.substr(-20, 4), t.substr(1, -1) === "");
console.log(t.startsWith("ab", -3), t.endsWith("😀", 4), t.includes("a", -5));
