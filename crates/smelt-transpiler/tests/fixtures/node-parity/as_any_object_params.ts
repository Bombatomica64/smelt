// Functions defined in an object literal cast `as any` are reached only
// through `any`: TypeScript checks no call of them, so their parameters
// receive whatever value the caller passes, whatever the annotation says.
const base = { kind: "string" };
const chain: any = Object.assign({ ...base }, {
  regex: (expr: string) => (value: string) => Boolean(value.match(expr)),
  label(text: string): string {
    return typeof text + ":" + String(text);
  },
  nested: { count: (n: number) => typeof n },
}) as any;
const isWord = chain.regex(/^[a-z]+$/);
console.log(isWord("abc"), isWord("a b c"), chain.label(/x/g), chain.label(42), chain.nested.count("7"));
const direct = {
  size: (value: string) => (Array.isArray(value) ? value.length : -1),
} as any;
console.log(direct.size([1, 2, 3]), direct.size("abc"));
// Not cast: the annotation holds and the parameter stays typed.
const typed = { twice: (n: number) => n * 2 };
console.log(typed.twice(21));
