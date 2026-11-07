const table = { a: 123, b: "x", c: true };
function pick(key: "a" | "b"): string | number {
  return table[key];
}
console.log(table.b + table.a);
console.log(table.a + table.b);
console.log(table.a + 1, table.b + "y", table.c);
console.log(pick("a"), pick("b"));
