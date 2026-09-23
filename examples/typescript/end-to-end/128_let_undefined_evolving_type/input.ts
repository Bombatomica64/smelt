// An unannotated `let x = undefined` has TypeScript's evolving type: the
// binding's type is the union of what is later assigned to it, not `undefined`.
const PAIR = /^([^:]*):(.*)$/;

function split(input: string): string {
  let pair = undefined;
  try {
    pair = PAIR.exec(input);
  } catch {}
  if (!pair) {
    return "no pair";
  }
  return pair[1] + " / " + pair[2];
}

function firstLong(words: string[]): string {
  let found = undefined;
  for (const word of words) {
    if (word.length > 3) {
      found = word;
      break;
    }
  }
  return found === undefined ? "none" : found.toUpperCase();
}

console.log(split("user:secret"));
console.log(split("nocolon"));
console.log(firstLong(["a", "bb", "castle", "dragon"]));
console.log(firstLong(["a"]));
