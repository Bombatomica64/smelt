// A lone surrogate survives in a template literal part and an object key, and
// isWellFormed / toWellFormed read a string's UTF-16 code units.
const half = "\ud83d";
const pair: string = `${half}\ude00`;
console.log(pair === "😀", pair.length, `a\ud800b`.length, `x${1}\udfff`.charCodeAt(2).toString(16));
const table: Record<string, number> = { "\ud83d": 1, "\ude00": 2, plain: 3 };
console.log(Object.keys(table).map((k) => k.length + ":" + k.charCodeAt(0).toString(16)).join(" "));
console.log(table["\ud83d"], table[half], table["\ude00"], "\ud83d" in table);
console.log(half.isWellFormed(), "😀".isWellFormed(), pair.isWellFormed(), half.toWellFormed().charCodeAt(0), "a😀".toWellFormed() === "a😀");
const cut = "😀x".slice(1);
console.log(cut.isWellFormed(), cut.toWellFormed().length, cut.toWellFormed().charCodeAt(0));
