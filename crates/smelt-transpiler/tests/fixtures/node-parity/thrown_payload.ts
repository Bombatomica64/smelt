// A thrown value crosses every channel whole: a catch sees the same value that
// was thrown, whether it was thrown synchronously, from a callback, or rejected.

class Custom extends Error {
  code: number;
  constructor(message: string, code: number) {
    super(message);
    this.code = code;
    this.name = "Custom";
  }
}

function boom(value: unknown): void {
  throw value;
}

function describe(caught: unknown): string {
  if (caught instanceof Custom) return `Custom:${caught.message}:${caught.code}`;
  if (caught instanceof TypeError) return `TypeError:${caught.message}`;
  if (caught instanceof Error) return `Error:${caught.message}`;
  return `value:${String(caught)}:${typeof caught}`;
}

function capture(run: () => void): string {
  try {
    run();
    return "no throw";
  } catch (error) {
    return describe(error);
  }
}

console.log(capture(() => boom(new TypeError("bad"))));
console.log(capture(() => boom(new Error("plain"))));
console.log(capture(() => boom(new Custom("mine", 7))));
console.log(capture(() => boom("text")));
console.log(capture(() => boom(42)));
console.log(capture(() => boom(null)));
console.log(capture(() => boom({ a: 1 })));

console.log(capture(() => [1, 2].forEach(() => boom(new TypeError("in callback")))));

try {
  try {
    boom(new Custom("inner", 1));
  } catch (error) {
    throw error;
  }
} catch (error) {
  console.log(describe(error));
}

function finallyOrder(): string {
  const seen: string[] = [];
  try {
    try {
      boom(new Error("x"));
    } finally {
      seen.push("finally");
    }
  } catch (error) {
    seen.push(describe(error));
  }
  return seen.join(",");
}
console.log(finallyOrder());

const error = new TypeError("shown");
console.log(String(error));
console.log(`${new Error("e")}`);

async function reject(value: unknown): Promise<void> {
  throw value;
}

async function main(): Promise<void> {
  try {
    await reject(new Custom("async", 3));
  } catch (caught) {
    console.log(describe(caught));
  }
  try {
    await Promise.reject("rejected text");
  } catch (caught) {
    console.log(describe(caught));
  }
}

main();
