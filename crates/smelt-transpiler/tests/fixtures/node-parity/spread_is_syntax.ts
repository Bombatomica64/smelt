// Spread is syntax: an argument is spread only when the call writes `...arg`,
// never because `...` appears inside a string literal argument.
function join(...parts: string[]): string { return parts.join("|"); }
const label = (prefix: string, ...rest: string[]): string => prefix + ":" + rest.length;
console.log(join("a...", "b"), label("x...", "y", "z"), Math.max("...".length, 2), String.fromCharCode("...".length + 60));
