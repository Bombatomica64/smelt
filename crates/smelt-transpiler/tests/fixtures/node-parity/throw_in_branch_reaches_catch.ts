// A `throw` inside an `if` in a `try` jumps to the `catch`: the branch's own
// region (the handler) runs before control rejoins after the `try`, also
// when an outer `try`/`finally` wraps it.
function check(n: number): void {
  let completed = false;
  try {
    try {
      if (n > 1) {
        throw new Error("too big");
      }
      completed = true;
    } catch (e) {
      if (e instanceof Error) {
        console.log("caught in function", e.message);
      } else {
        throw e;
      }
    }
    if (completed) {
      console.log("no error for", n);
    }
  } finally {
    console.log("done", n);
  }
}

check(1);
check(5);
