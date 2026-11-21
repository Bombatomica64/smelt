// `JSON.parse` of deeply nested arrays and objects, and dropping the result,
// at a depth that a recursive parse or a recursive drop would overflow the
// stack with (200 000 levels).
const depth = 200000;
const arrays: unknown = JSON.parse("[".repeat(depth) + "]".repeat(depth));
console.log(Array.isArray(arrays));
const objects: unknown = JSON.parse('{"a":'.repeat(depth) + "1" + "}".repeat(depth));
console.log(typeof objects);
const mixed: unknown = JSON.parse('[{"k":'.repeat(depth) + "0" + "}]".repeat(depth));
console.log(Array.isArray(mixed));
function dropScoped(): string {
  const local: unknown = JSON.parse("[".repeat(depth) + "1" + "]".repeat(depth));
  return typeof local;
}
console.log(dropScoped());
console.log("done");
