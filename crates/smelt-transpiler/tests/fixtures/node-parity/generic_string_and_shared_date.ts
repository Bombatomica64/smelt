// `String(x)` and a template on a value of a generic type `T` (a real Rust
// generic in `Box<T>`), and a concrete Date handed to a function that erases
// it: the callee's setter reaches the caller's Date, and a later change by the
// caller is visible through the value the callee returned.
class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
  describe(): string {
    return `box(${String(this.value)})`;
  }
}

function show<T>(value: T): string {
  return "[" + String(value) + "]";
}

function bump<D extends Date>(date: D | number, days: number): D {
  const d = (typeof date === "number" ? new Date(date) : date) as D;
  d.setUTCDate(d.getUTCDate() + days);
  return d;
}

function main(): void {
  console.log(new Box(3).describe(), new Box("s").describe(), new Box(false).describe());
  console.log(show(true), show([1, 2]), show(null), show(undefined));

  const base = new Date(Date.UTC(2024, 0, 30));
  const result = bump(base, 3);
  console.log(base.getUTCDate(), base.getUTCMonth(), result === base, result.getUTCDate());
  base.setUTCFullYear(2030);
  console.log(result.getUTCFullYear(), +result === +base);
  console.log(bump(Date.UTC(2024, 1, 28), 1).getUTCDate());
}

main();
