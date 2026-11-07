class Point {
  x = 1;
  y = 2;
}
function blank<T>(value: T): T {
  return new ((value as object).constructor as { new (): T })();
}
function blankAny(value: any): any {
  return new value.constructor();
}
console.log(blank([1, 2]), Array.isArray(blank([1])));
console.log(blank({ a: 1 }), blankAny({ b: 2 }));
console.log(blank(new Point()) instanceof Point);
console.log(blankAny([1]).length, new ([].constructor as any)(3).length);
