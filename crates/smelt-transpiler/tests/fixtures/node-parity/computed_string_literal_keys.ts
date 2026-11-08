// A computed string-literal key is the string's value, escapes decoded and a
// lone surrogate kept: not the source text between the quotes.
const show = (key: string): string => key.length + ":" + key.charCodeAt(0).toString(16);
const computed = { ["\ud83d"]: 1, ["a\tb"]: 2, ["\ude00"]: 3 } as Record<string, number>;
console.log(Object.keys(computed).map(show).join(" "));
console.log(computed["\ud83d"], computed["a\tb"], computed["\ude00"]);
