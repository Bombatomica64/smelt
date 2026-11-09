// A throw delivers exactly its operand, through one error channel, to any catch.

function classify(caught: unknown): string {
  if (caught instanceof TypeError) {
    return "TypeError:" + caught.message;
  }
  if (caught instanceof Error) {
    return "Error:" + caught.message;
  }
  if (typeof caught === "string") {
    return "string:" + caught;
  }
  if (typeof caught === "number") {
    return "number:" + String(caught);
  }
  if (typeof caught === "object" && caught !== null) {
    return "object:" + JSON.stringify(caught);
  }
  return "other";
}

function boom(value: unknown): void {
  throw value;
}

function observe(value: unknown): string {
  try {
    boom(value);
    return "no throw";
  } catch (error) {
    return classify(error);
  }
}

console.log(observe(new TypeError("bad")));
console.log(observe(new Error("plain")));
console.log(observe("a bare string"));
console.log(observe(7));
console.log(observe({ code: 42, tag: "record" }));

// The arm is chosen at run time and the catch is in another function.
function either(asRecord: boolean): void {
  if (asRecord) {
    throw { code: 42, tag: "record" };
  }
  throw "a bare string";
}

function runEither(asRecord: boolean): string {
  try {
    either(asRecord);
    return "no throw";
  } catch (error) {
    return classify(error);
  }
}

console.log(runEither(true));
console.log(runEither(false));

// The caught value is the thrown value: rethrowing keeps it.
function rethrow(value: unknown): string {
  try {
    try {
      boom(value);
    } catch (inner) {
      throw inner;
    }
    return "no throw";
  } catch (outer) {
    return classify(outer);
  }
}

console.log(rethrow(new RangeError("range")));
console.log(rethrow({ again: true }));

// Unrelated capturing closures do not disturb a throw in another function.
function manyClosures(values: number[]): number[] {
  const a = 1;
  const b = 2;
  const c = 3;
  const d = 4;
  return values.map((value) => value * a + b).map((value) => value * c + d);
}

function thrower(size: number): number {
  if (!Number.isInteger(size) || size <= 0) {
    throw new Error("Size must be an integer greater than zero.");
  }
  return size;
}

console.log(manyClosures([1, 2]).join(","));
console.log(thrower(3));
try {
  thrower(-1);
} catch (error) {
  console.log(classify(error));
}

// A try around a call that cannot throw runs its body and skips its catch.
function apply(f: (x: number) => number, value: number): number {
  return f(value);
}

function guarded(value: number): number {
  try {
    return apply((x) => x + 1, value);
  } catch {
    return 0;
  }
}

console.log(guarded(41));
