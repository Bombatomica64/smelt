import { test, expect } from "vitest";

// The returned closure emits T as SmeltUnknown, even though its MIR retains T.
function dispatch<T>(callback: (...values: unknown[]) => unknown): (data: T[]) => unknown {
  return (data) => callback(data);
}

// Exercise the value-return erasure path as well as callback argument packing.
function eraseArray<T>(data: T[]): unknown {
  return data;
}

// Concrete elements still require a real representation conversion.
function concreteArray(data: number[]): unknown {
  return data;
}

test("erased generic callback arrays share identity and mutations", () => {
  const original: unknown[] = [1];
  const callback = (...values: unknown[]): unknown => {
    const received = values[0] as unknown[];
    expect(received).toBe(original);
    received.push(2);
    expect(original.length).toBe(2);
    original.push(3);
    expect(received.length).toBe(3);
    return received;
  };
  const result = dispatch(callback)(original);
  expect(result).toBe(original);
  expect(original).toEqual([1, 2, 3]);
});

test("erased generic return arrays share identity and mutations", () => {
  const original: unknown[] = [1];
  const received = eraseArray(original) as unknown[];
  expect(received).toBe(original);
  received.push(2);
  expect(original.length).toBe(2);
  original.push(3);
  expect(received).toEqual([1, 2, 3]);
});

test("concrete numeric arrays still erase their elements", () => {
  const received = concreteArray([1, 2, 3]) as unknown[];
  expect(received).toEqual([1, 2, 3]);
});
