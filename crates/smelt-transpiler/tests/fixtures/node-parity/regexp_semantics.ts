// Regular expressions: replacement callbacks, flags, match values and lastIndex.

function localize(enNumber: number): string {
  const suffix = "!";
  return enNumber.toString().replace(/\d/g, match => match + suffix);
}
console.log(localize(120));

function replaceTable(str: string, table: unknown): string {
  return str.replace(/[&<]/g, match => (table as Record<string, any>)[match]);
}
console.log(replaceTable("a&b<c", { "&": "AMP", "<": 7 }));
console.log(replaceTable("a&b", {}));

function matchUnknown(value: unknown): string[] | null {
  return (value as any).match(/a+/);
}
console.log(matchUnknown("baab")![0], matchUnknown("b") === null);

function parts(value: string): string[] | null {
  const tokens = /a+|b/g;
  return value.match(tokens);
}
console.log(parts("aab c b")!.join("|"), parts("z") === null);

function words(text: string): number {
  return text.match(/[a-z]+/g)?.length ?? 0;
}
console.log(words("ab cd 12"), words("12"));

const patterns = [/x/u, /x/gimu];
const pattern = patterns[1];
console.log(pattern.global, pattern.ignoreCase, pattern.multiline, pattern.flags, patterns[0].global);

function cloneRe(obj: unknown): RegExp {
  const regExp = obj as RegExp;
  return new RegExp(regExp.source, regExp.flags);
}
const cloned = cloneRe(/a.c/gi);
console.log(cloned.source, cloned.flags, cloned.test("ABC"));

type Args = {
  parsePatterns: Record<string, readonly RegExp[]>;
  defaultParseWidth: string;
};
function selectPatterns(args: Args): readonly RegExp[] {
  return args.parsePatterns[args.defaultParseWidth];
}
const parseMonthPatterns = {
  narrow: [/^j/i, /^f/i] as const,
  any: [/^ja/i, /^f/i] as const,
};
const selected = selectPatterns({ parsePatterns: parseMonthPatterns, defaultParseWidth: "any" });
console.log(selected.length, selected[0].source, selected[0].test("JAN"), selected[0].test("JUN"));

const indexed = "a,b.c".matchAll(/,|\./gu);
for (const { index } of indexed) {
  console.log(index);
}

const re = /(?<year>\d{4})-(\d{2})/;
const m = re.exec("2024-07");
console.log(m === null, m![0], m![1], m![2], m!.index, m!.input, m!.groups!.year);
console.log(re.exec("nope") === null);

for (const found of "a1 b".matchAll(/(?<letter>[a-z])(\d)?/g)) {
  const [full, letter] = found;
  console.log(found[0], found.groups!.letter, found[2], full, letter, found.index, found.input);
}

const boxedMatch = /(?<letter>[a-z])/.exec("a");
if (boxedMatch !== null) {
  const boxed: unknown = boxedMatch;
  console.log(boxedMatch.groups!.letter, Array.isArray(boxed), (boxed as any)[0]);
}

function inner(value: string): string[] | null {
  return value.match(/^'([^]*?)'?$/);
}
console.log(inner("'a\nb'")![1]);

function lastIndexRoundTrip(): number {
  const regex = /a/g;
  regex.lastIndex = 10;
  return regex.lastIndex;
}
console.log(lastIndexRoundTrip());

const stateful = /a/g;
console.log(stateful.test("aa"), stateful.lastIndex, stateful.test("aa"), stateful.lastIndex, stateful.test("aa"), stateful.lastIndex);

const CASE_SPLIT_PATTERN = /[0-9]+/g;
function firstSplit(text: string): number {
  return text.split(CASE_SPLIT_PATTERN).length;
}
function secondSplit(text: string): number {
  return text.split(CASE_SPLIT_PATTERN).length;
}
console.log(firstSplit("a1b2c"), secondSplit("x"));
