// A plain object narrowed to a record type is that record from then on: one
// identity, shared writes, and the object's own key order and extra
// properties.

type Point = { x: number; label: string };
type Mixed = { zeta: number; alpha: string; mid: boolean };

function sortKeys<T extends Record<string, any>>(value: T): T {
  const sorted: Record<string, any> = {};
  for (const key of Object.keys(value).sort()) {
    sorted[key] = value[key];
  }
  return sorted as T;
}

function main(): void {
  const plain: any = { label: "p", x: 3, more: [1] };
  const typed: Point = plain;
  typed.x = 4;
  plain.label = "q";
  console.log(plain.x, typed.label, typed === plain);
  console.log(Object.keys(typed), typed);

  // A value a callee rebuilds in another key order keeps that order.
  const unordered: Mixed = { zeta: 1, alpha: "two", mid: true };
  const sorted: Mixed = sortKeys(unordered);
  console.log(Object.keys(sorted).join(","), Object.keys(unordered).join(","));
  console.log(sorted.alpha, sorted.mid, sorted.zeta, sorted === unordered);
  console.log(JSON.stringify(sorted));
  console.log(sorted);
}

main();
