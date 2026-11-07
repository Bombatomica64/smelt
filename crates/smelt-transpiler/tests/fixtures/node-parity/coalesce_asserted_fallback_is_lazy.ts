// `a ?? b!` evaluates `b!` only when `a` is absent: a non-null assertion is a
// runtime check, so it is not hoisted ahead of the test.
function pick(a: string | undefined, b: string | undefined): string { return a ?? b!; }
console.log(pick("left", undefined), pick(undefined, "right"));
const m = /(?<a>x)|(?<b>y)/.exec("x");
if (m !== null) {
  const { a, b } = m.groups!;
  console.log(a ?? b!);
}
