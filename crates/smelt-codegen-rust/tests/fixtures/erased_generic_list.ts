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

// A twin of `eraseArray` under a different type-parameter name.
function eraseOther<U>(data: U[]): unknown {
  return data;
}

// A generic class whose methods call the erased generics above. Type parameters
// are name-interned, so the class's `T` and `eraseArray`'s `T` are one symbol:
// the caller's own scope spells it, yet the callee's slot is still
// `SmeltList<SmeltUnknown>`. The argument must be judged at the callee's scope.
class Holder<T> {
  value: T;

  constructor(value: T) {
    this.value = value;
  }

  // An erased array into the callee's `T[]`: already the slot's representation.
  share(items: unknown[]): unknown {
    return eraseArray(items);
  }

  // The class's own `T[]` is a real Rust `SmeltList<T>`, so its elements still
  // need converting into the callee's erased `U[]`.
  shareOwn(items: T[]): unknown {
    return eraseOther(items);
  }
}

test("a generic class passes an erased array to an erased generic by reference", () => {
  const original: unknown[] = [1];
  const received = new Holder<number>(0).share(original) as unknown[];
  expect(received).toBe(original);
  received.push(2);
  expect(original.length).toBe(2);
  expect(original).toEqual([1, 2]);
});

test("a generic class still converts its own concrete elements for an erased generic", () => {
  const received = new Holder<number>(0).shareOwn([1, 2]) as unknown[];
  expect(received).toEqual([1, 2]);
});
