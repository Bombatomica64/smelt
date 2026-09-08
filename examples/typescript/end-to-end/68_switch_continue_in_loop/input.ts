// `continue` inside a `switch` inside a loop targets the LOOP.
//
// A switch without fallthrough lowers to a HIR match, and a case body ends at
// the first statement that leaves it. The two ways of leaving differ in what
// they lower: a JS `break` exits the switch, which a match arm does by simply
// ending, so it emits nothing; a `continue` targets the enclosing loop and has
// to be emitted — which is exactly what Rust spells as `continue` inside a
// `match` arm inside a loop. Only the second was rejected ("switch continue
// lowering is not implemented yet"), so Hono's HTML escaper
// (`src/utils/html.ts`), whose `default` arm continues the scan loop, could not
// lower at all.
//
// The switch lowering that handles real FALLTHROUGH still rejects `continue`,
// and for a reason that is not conservatism: it builds a synthetic
// single-iteration loop, so a `continue` inside it would bind to that loop
// instead of the source one. That needs labeled loops, not this rule.

// The Hono shape: scan a string, and skip characters that need no escaping.
function escapeHtml(str: string): string {
  let out = "";
  let lastIndex = 0;
  for (let i = 0; i < str.length; i++) {
    const code = str.charCodeAt(i);
    let replacement = "";
    switch (code) {
      case 34:
        replacement = "&quot;";
        break;
      case 38:
        replacement = "&amp;";
        break;
      case 60:
        replacement = "&lt;";
        break;
      case 62:
        replacement = "&gt;";
        break;
      default:
        continue;
    }
    out += str.substring(lastIndex, i) + replacement;
    lastIndex = i + 1;
  }
  return out + str.substring(lastIndex, str.length);
}

console.log(escapeHtml('a"b&c<d>e'));
console.log(escapeHtml("nothing to escape"));

// `continue` from a LABELED case rather than the default, and inside a braced
// case body, so both statement shapes the case walker handles are covered.
function sumOdd(values: number[]): number {
  let total = 0;
  for (const value of values) {
    switch (value % 2) {
      case 0: {
        continue;
      }
      default:
        total += value;
        break;
    }
  }
  return total;
}

console.log(sumOdd([1, 2, 3, 4, 5]));

// `break` and `continue` in the same switch: the `break` case still falls out
// of the switch and runs the statement after it.
function classify(values: number[]): string {
  let seen = "";
  for (const value of values) {
    switch (value) {
      case 1:
        seen += "one";
        break;
      case 2:
        continue;
      default:
        seen += "?";
        break;
    }
    seen += ".";
  }
  return seen;
}

console.log(classify([1, 2, 3]));
