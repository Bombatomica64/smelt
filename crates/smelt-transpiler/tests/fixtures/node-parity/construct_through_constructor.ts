class Point {
  x = 1;
  y = 2;
  label: string;
  constructor(label?: string) {
    this.label = (label ?? "p") + "!";
  }
}
class Counter {
  count = 10;
  steps: number[] = [];
  bump(): number {
    this.count += 1;
    return this.count;
  }
}
function blank<T>(value: T): T {
  return new ((value as object).constructor as { new (): T })();
}
function blankAny(value: any): any {
  return new value.constructor();
}
function withLabel(value: any, label: string): any {
  return new value.constructor(label);
}
console.log(blank([1, 2]), Array.isArray(blank([1])));
console.log(blank({ a: 1 }), blankAny({ b: 2 }));
const point = new Point("q");
point.x = 9;
const fresh = blank(point);
console.log(fresh instanceof Point, fresh.x, fresh.y, fresh.label);
console.log(withLabel(point, "r").label, withLabel(point, "r").x);
const counter = new Counter();
counter.bump();
const again = blankAny(counter);
console.log(again.count, again.steps.length, again.bump(), counter.count);
console.log(blankAny([1]).length, new ([].constructor as any)(3).length);
