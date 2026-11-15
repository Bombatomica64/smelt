// A record put into an erased collection is the caller's object there: the
// collection finds it again by identity, and a write through an element
// reaches it. No call hands it over here; `push`, `add` and an array literal
// do.

type Point = { x: number; label: string };

function main(): void {
  const origin: Point = { x: 1, label: "o" };

  const seen = new Set<unknown>();
  seen.add(origin);
  console.log(seen.has(origin), seen.size);

  const registry: unknown[] = [];
  registry.push(origin);
  console.log(registry.indexOf(origin), registry.includes(origin));

  const all: Record<string, unknown>[] = [origin];
  all[0]["label"] = "renamed";
  console.log(origin.label);

  // An equal record is another object.
  const other: Point = { x: 1, label: "renamed" };
  console.log(seen.has(other), registry.indexOf(other));
}

main();
