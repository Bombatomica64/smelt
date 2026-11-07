// UTF-16 string index benchmark: the workload that measures the cost of a
// JavaScript string index in generated Rust.
//
// Each phase runs ROUNDS times over an 88 KB text, once ASCII and once with
// every word carrying a non-ASCII letter and one astral character (a
// surrogate pair). Every phase folds what it read into a checksum, so a wrong
// answer is visible next to its time. Run through `benchmarks/strings/run.py`,
// which builds the generated crate in release mode and compares it with Node.

const ROUNDS = 20;
const VOCABULARY = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet", "kilo", "lima", "mike"];

/** An 88 KB text of space-separated words, decorated when `wide` is set. */
function buildText(wide: boolean): string {
  let text = "";
  let index = 0;
  while (text.length < 88000) {
    let word = VOCABULARY[index % VOCABULARY.length];
    if (wide) {
      word = word + "é" + "😀";
    }
    text += word + " ";
    index += 1;
  }
  return text;
}

/** Folds `value` into `checksum`, staying an exact integer. */
function fold(checksum: number, value: number): number {
  return (checksum * 31 + value) % 1000000007;
}

/** `charCodeAt` over the first 4000 code units. */
function scan(text: string): number {
  let checksum = 0;
  for (let i = 0; i < 4000; i++) {
    checksum = fold(checksum, text.charCodeAt(i));
  }
  return checksum;
}

/** Every word, found with `indexOf` and cut with `slice`. */
function words(text: string): number {
  let checksum = 0;
  let from = 0;
  while (true) {
    const at = text.indexOf(" ", from);
    if (at < 0) {
      break;
    }
    const word = text.slice(from, at);
    checksum = fold(checksum, word.length + word.charCodeAt(word.length - 1));
    from = at + 1;
  }
  return checksum;
}

/** `s[i]` compared with a letter for the first 2000 code units. */
function chars(text: string): number {
  let checksum = 0;
  for (let i = 0; i < 2000; i++) {
    if (text[i] === "a" || text[i] === "é") {
      checksum = fold(checksum, i);
    }
  }
  return checksum;
}

/** A `+=` builder of `count` one-unit pieces, shaped like Hono's `encodeBase64`. */
function build(count: number): number {
  let out = "";
  for (let i = 0; i < count; i++) {
    out += String.fromCharCode(65 + (i % 26));
  }
  return fold(out.length, out.charCodeAt(count - 1));
}

/** Runs `phase` ROUNDS times and prints its time and checksum. */
function measure(name: string, text: string, phase: (text: string) => number): void {
  const start = Date.now();
  let checksum = 0;
  for (let round = 0; round < ROUNDS; round++) {
    checksum = fold(checksum, phase(text));
  }
  console.log(name + " ms=" + (Date.now() - start) + " checksum=" + checksum);
}

/** Runs the builder once at `count` pieces and prints its time and checksum. */
function measureBuild(count: number): void {
  const start = Date.now();
  const checksum = build(count);
  console.log("build n=" + count + " ms=" + (Date.now() - start) + " checksum=" + checksum);
}

const ascii = buildText(false);
const wide = buildText(true);
console.log("length ascii=" + ascii.length + " wide=" + wide.length);
measure("scan ascii", ascii, scan);
measure("scan wide", wide, scan);
measure("words ascii", ascii, words);
measure("words wide", wide, words);
measure("chars ascii", ascii, chars);
measure("chars wide", wide, chars);
measureBuild(1048576);
measureBuild(2097152);
measureBuild(4194304);
