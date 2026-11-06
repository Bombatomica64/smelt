// `.replace(re, (m) => m.toUpperCase())` is an ordinary replacer callback: it
// sees the whole match, runs once for a non-global pattern and once per match
// for a global one, and a match of nothing still calls it.
const upper = (text: string): string => text.replace(/[a-c]+/, (m) => m.toUpperCase());
console.log(upper("xabcx abc"));
console.log(upper("none"));
console.log(upper(""));
console.log("abc abc".replace(/b/g, (m) => m.toUpperCase()));
console.log("ßx".replace(/ß/, (m) => m.toUpperCase()));
console.log("hello world".replace(/^\w/, (c) => c.toUpperCase()));
console.log("kebab-case".replace(/-(\w)/, (m) => m.toUpperCase()));
console.log("abc".replace(/(?:)/, (m) => m.toUpperCase() + "!"));
