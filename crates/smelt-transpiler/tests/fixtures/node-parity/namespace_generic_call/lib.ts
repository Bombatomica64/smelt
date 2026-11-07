export function iterate<T>(count: number, step: (value: T, index: number) => T, start: T): T {
  let value = start;
  for (let index = 0; index < count; index++) {
    value = step(value, index);
  }
  return value;
}
export function first<T>(values: T[]): T | undefined {
  return values[0];
}
export function wrap<T>(value: T): { value: T } {
  return { value };
}
