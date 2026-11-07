// parseInt and parseFloat read the longest valid prefix after leading
// whitespace; parseInt without a radix detects a 0x prefix.
const inputs: string[] = ["12px", "3.7", " 4", "0x10", "-0x1F", "  -12abc", "abc", "", "+7", "1e3", "08", "Infinity", " \t9", "99999999999999999999", "-0"];
for (const s of inputs) {
  console.log(JSON.stringify(s), parseInt(s), parseInt(s, 10), parseInt(s, 16), parseFloat(s), Number(s));
}
console.log(Number.parseInt("42rem"), Number.parseFloat(".5e1x"), parseFloat("1e"), parseFloat("-.5e-1z"), parseInt("z", 36), parseInt("10", 37));
const xs: string[] = ["10", "10", "10", "10"];
console.log(xs.map(parseInt));
console.log(xs.map(parseFloat));
const parse = parseInt;
console.log(parse("0x1f"), ["7px", "0x8"].map((s) => parse(s)));
function group(s: string): number {
  const m = s.match(/a(\d+)?/);
  return m === null ? -1 : parseInt(m[1], 36);
}
console.log(group("a12b"), group("ab"), group("b"));
let calls = 0;
function text(): string { calls += 1; console.log("text", calls); return "11"; }
function radix(): number { calls += 1; console.log("radix", calls); return 2; }
console.log(parseInt(text(), radix()));
