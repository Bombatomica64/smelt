// At the host edges a lone surrogate is what JavaScript makes of it:
// JSON.stringify writes its escape, JSON.parse reads it back, the URI encoders
// throw a URIError, and TextEncoder writes U+FFFD.
const half = "\ud83d";
console.log(JSON.stringify(half), JSON.stringify({ a: half, b: "😀", c: "q\"\n\u0001" }), JSON.stringify([half + "x"]));
try {
  encodeURIComponent(half);
  console.log("no throw");
} catch (error) {
  console.log((error as Error).name, (error as Error).message);
}
try {
  console.log(encodeURI("a b😀"), encodeURIComponent("😀/?"));
  encodeURI("x" + "\udc00");
} catch (error) {
  console.log((error as Error).name);
}
console.log(Array.from(new TextEncoder().encode(half)).join(","), Array.from(new TextEncoder().encode("é😀")).join(","));
const parsed = JSON.parse('"\\ud83d"') as string;
console.log(parsed.length, parsed.charCodeAt(0).toString(16));
