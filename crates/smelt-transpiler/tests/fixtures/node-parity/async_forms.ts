// Async functions, closures, generators and timers. Each section awaits the one
// before, so the output order does not depend on how independent chains interleave.

const tryit = <Args extends unknown[], Return>(func: (...args: Args) => Return) => {
  return (...args: Args): unknown => func(...args);
};

async function erasedAsyncThrow(): Promise<void> {
  const wrapped = tryit(async () => {
    throw new Error("failure");
  });
  try {
    await (wrapped() as Promise<unknown>);
    console.log("not rejected");
  } catch (error) {
    console.log("erased reject", (error as Error).message);
  }
}

async function erasedResolve(func: (...args: unknown[]) => unknown): Promise<unknown> {
  return func();
}

function worker(done: (values: unknown[]) => void): () => Promise<void> {
  return async () => {
    const values: unknown[] = [];
    while (true) {
      if (values.length === 0) return done(values);
    }
  };
}

async function attemptAsync<T, E>(func: () => Promise<T>): Promise<[null, T] | [E, null]> {
  try {
    const result = await func();
    return [null, result];
  } catch (error) {
    return [error as E, null];
  }
}

async function firstEven(values: number[]): Promise<number> {
  for (const value of values) {
    if (value % 2 === 0) {
      return value;
    }
  }
  return -1;
}

class Cell {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}
function pick(sync: boolean): Cell | Promise<Cell> {
  return sync ? new Cell(1) : Promise.resolve(new Cell(2));
}

async function* sequence(): AsyncGenerator<number, string, unknown> {
  yield 1;
  await Promise.resolve(2);
  yield 2;
  return "done";
}

class Good<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> {
      yield 7;
      return value;
    })();
  }
}
class Bad<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> {
      yield 8;
      return value;
    })();
  }
}
type Outcome<T> = Good<T> | Bad<T>;
async function* unwrap(promise: Promise<Outcome<string>>): AsyncGenerator<number, string, unknown> {
  return yield* await promise.then((result) => result.safeUnwrap());
}

function attempt(fn: () => unknown): unknown {
  return fn();
}

async function load(): Promise<number> {
  return 1;
}
function retain(callback: () => Promise<number>): () => Promise<number> {
  return callback;
}

async function shadowed(): Promise<number> {
  const objectize =
    (callback: (value: { num: number }) => Promise<number>) =>
    async (num: number) =>
      callback({ num });
  return await objectize(async (value) => value.num)(1);
}

async function factoryTwice(): Promise<number> {
  const factory = (callback: (value: number) => Promise<number>) => async () => callback(5);
  const callback = async (value: number): Promise<number> => value;
  const generated = factory(callback);
  return (await generated()) + (await generated());
}

async function flaky(n: number): Promise<number> {
  if (n < 3) {
    throw new Error("nope");
  }
  return n;
}
async function retryLoop(limit: number): Promise<number> {
  for (let i = 0; i <= limit; i++) {
    try {
      return await flaky(i);
    } catch (err) {
      // retry
    }
  }
  return -1;
}

async function closureTry(n: number): Promise<number> {
  const inner = async (limit: number): Promise<number> => {
    for (let i = 0; i <= limit; i++) {
      try {
        return await flaky(i);
      } catch (err) {}
    }
    return -1;
  };
  return await inner(n);
}

function fireAndForget(cb: () => Promise<string>): void {
  cb().then((value) => console.log("borrowed future", value));
}

async function throwingValue(): Promise<string> {
  const boom = async (): Promise<number> => {
    throw new Error("boom");
  };
  const pending = boom();
  try {
    await pending;
    return "no";
  } catch (error) {
    return (error as Error).message;
  }
}

function greet(name: string, count: number): void {
  console.log("timer extras", name, count);
}
function note(prefix: string, suffix?: string): void {
  console.log("timer optional", prefix, suffix);
}
function handle(value: unknown): void {
  console.log("timer unknown", value);
}
function fire(payload: unknown): void {
  setTimeout(handle, 1, payload);
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main(): Promise<void> {
  await erasedAsyncThrow();
  const erasedValue = await erasedResolve(async () => 1);
  console.log("erased resolve", await erasedValue);
  await worker((values) => console.log("worker", values.length))();
  console.log("attempt ok", JSON.stringify(await attemptAsync(async () => 2)));
  const failed = await attemptAsync<number, Error>(async () => {
    throw new Error("e");
  });
  console.log("attempt fail", (failed[0] as Error).message, failed[1]);
  console.log("first even", await firstEven([1, 2]), await firstEven([1]));
  console.log("await union", (await pick(true)).value, (await pick(false)).value);

  const iterator = sequence();
  const first = await iterator.next();
  const second = await iterator.next();
  const completed = await iterator.next();
  console.log("async gen", first.value, second.value, completed.value, completed.done);

  const outcome: Promise<Outcome<string>> = Promise.resolve(new Good<string>("ok"));
  const delegating = unwrap(outcome);
  const yielded = await delegating.next();
  const returned = await delegating.next();
  console.log("delegate", yielded.value, returned.value, returned.done);

  const syncSlot = attempt(async () => 1) as Promise<number>;
  console.log("sync slot", await syncSlot);
  console.log("retain", await retain(async () => (await load()) + 1)());
  console.log("shadowed", await shadowed());
  console.log("factory", await factoryTwice());
  console.log("retry loop", await retryLoop(5), await retryLoop(1));
  console.log("closure try", await closureTry(4), await closureTry(0));
  fireAndForget(async () => "s");
  await sleep(1);
  console.log("throwing value", await throwingValue());

  setTimeout(greet, 1, "hi", 3);
  setTimeout(note, 2, "with-optional", "extra");
  fire("payload");
  await sleep(5);

  let ticks = 0;
  await new Promise<void>((resolve) => {
    const id = setInterval(() => {
      ticks = ticks + 1;
      if (ticks === 3) {
        clearInterval(id);
        resolve();
      }
    }, 1);
  });
  await sleep(5);
  console.log("interval ticks", ticks);

  let fired = false;
  setTimeout(() => {
    fired = true;
  }, 2);
  await sleep(20);
  console.log("timer before sleep end", fired);

  const timed = AbortSignal.timeout(2);
  console.log("signal before", timed.aborted);
  await sleep(20);
  console.log("signal after", timed.aborted, (timed.reason as Error).name);
}

main();
