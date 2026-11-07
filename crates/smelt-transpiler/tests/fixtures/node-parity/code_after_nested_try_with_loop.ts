// The code after a `try` whose `catch` may rethrow runs, a loop included,
// even though lowering numbered its first block before the `try`'s own.
function find(key: string): number {
  if (key === "bad") {
    throw new Error(key);
  }
  return key.length;
}

function isError(e: unknown): boolean {
  return e instanceof Error;
}

try {
  try {
    find("bad");
  } catch (e) {
    if (isError(e)) {
      console.log("inner caught");
    } else {
      throw e;
    }
  }
} catch (e) {
  if (isError(e)) {
    console.log("outer caught");
  } else {
    throw e;
  }
}
console.log("middle");
for (const x of [1, 2]) {
  console.log("tick", x);
}
console.log("after");
