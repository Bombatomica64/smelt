// A closure in a nested block (a loop body or a bare block) that reads a
// `let`/`const` declared later in the same block reads that block's binding:
// it sees the value once the declaration ran, throws a catchable
// `ReferenceError` when called before, and does not see an outer binding of
// the same name.
const fns: (() => number)[] = [];
for (let i = 0; i < 3; i++) {
  const read = () => later * 10 + i;
  let later = i + 1;
  fns.push(read);
}
console.log(fns.map((f) => f()).join(","));
for (let i = 0; i < 2; i++) {
  const early = () => value;
  try {
    console.log(early());
  } catch (e) {
    console.log((e as Error).name, e instanceof ReferenceError);
  }
  const value = i * 7;
  console.log(early());
}
const shadowed = "outer";
{
  const readInner = () => shadowed;
  let caught = "none";
  try {
    readInner();
  } catch (e) {
    caught = (e as Error).name;
  }
  const shadowed = "inner";
  console.log(caught, readInner());
}
console.log(shadowed);
let n = 0;
while (n < 2) {
  const show = () => `${label}:${n}`;
  const label = "w";
  console.log(show());
  n++;
}
