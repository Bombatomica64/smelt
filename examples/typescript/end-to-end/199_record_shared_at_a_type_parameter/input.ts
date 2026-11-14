// A record passed at a type parameter is the same object on the other side.
//
// `T` here is a real Rust generic, so the record crosses the call as its own
// struct type and nothing converts. This example pins that a generic helper
// neither copies the record nor loses its identity, alone or inside a generic
// container, next to the dictionary and subset views of examples 197 and 198.
//
// Every line below is diffed against Node.

type Point = { x: number; label: string };

function identity<T>(value: T): T {
  return value;
}

function pair<T>(value: T): T[] {
  return [value, value];
}

function first<T>(values: T[]): T {
  return values[0];
}

function main(): void {
  const point: Point = { x: 1, label: "a" };

  // Identity across a round trip through `T`.
  const same = identity(point);
  console.log(same === point);

  // A write through the returned value reaches the caller's record.
  same.x = 30;
  console.log(point.x);

  // Both elements of the generic container are the caller's record.
  const both = pair(point);
  both[0].label = "first";
  console.log(both[1].label, point.label, both[0] === both[1]);

  // The caller's write is visible through what the helper returned.
  point.x = 4;
  console.log(first(both).x, first(both) === point);
  console.log(JSON.stringify(point));
}

main();
