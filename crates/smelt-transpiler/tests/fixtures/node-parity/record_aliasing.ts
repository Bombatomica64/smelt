// A record is an object: a second binding, an array element and a function
// parameter all name the same object, so a write through one is seen through
// the others.
function setKey(target: Record<string, number>, key: string, value: number): void {
  target[key] = value;
}

const counts: Record<string, number> = { a: 1 };
const alias = counts;
alias["b"] = 2;
setKey(counts, "c", 3);
console.log(counts["a"], counts["b"], counts["c"], alias["c"]);

const rows: Record<string, number>[] = [counts, counts];
rows[0]["shared"] = 4;
console.log(rows[1]["shared"], counts["shared"]);

const numbered: Record<string, string> = { b: "bee", "2": "two", "1": "one" };
numbered["0"] = "zero";
for (const key in numbered) {
  console.log(key, numbered[key]);
}
