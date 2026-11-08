// A JavaScript string is UTF-16 code units: a lone surrogate is a value of its
// own, a cut through a surrogate pair leaves one, and strings sort by code unit.
const half: string = "\ud83d";
const emoji: string = "😀";
const cut = emoji.slice(0, 1);
console.log(half.length, cut.length, cut === half, cut === "\ud83d", half === emoji);
console.log(half, cut, half + "\ude00" === emoji, (half + "\ude00").length);
console.log(half.charCodeAt(0), cut.charCodeAt(0).toString(16), emoji.charCodeAt(1).toString(16));
const words = ["😀", "￿", "a", "é"];
words.sort();
console.log(words.map((w) => w.codePointAt(0)!.toString(16)).join(","), "😀" < "￿");
console.log([half, emoji.slice(1)].map((s) => s.length).join("+"), `${half}x`.length);
