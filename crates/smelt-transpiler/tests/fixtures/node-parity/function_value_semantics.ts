// Captures, receivers, function values as objects, and the argument ABI of calls.

class Counter {
  value = 1;
  read(): number {
    const get = () => this.value;
    this.value = 2;
    return get();
  }
}
console.log("receiver capture", new Counter().read());

function nestedShared(): number {
  let count = 0;
  const outer = () => {
    const inner = () => {
      count = count + 1;
    };
    inner();
    inner();
  };
  outer();
  count = count + 10;
  outer();
  return count;
}
console.log("nested cell", nestedShared());

function recursive(): number {
  const walk = (n: number): number => (n <= 0 ? 0 : 1 + walk(n - 1));
  return walk(3);
}
console.log("recursive closure", recursive());

async function asyncShared(): Promise<number> {
  let count = 0;
  const bump = async () => {
    const again = async () => {
      count = count + 1;
    };
    await again();
  };
  await bump();
  await bump();
  return count;
}

function methodReceiver(): unknown {
  const callee = function (this: any, a: number) {
    return (a + this.b) as number;
  };
  const object = { m: callee, b: 2 };
  return (object as any).m(1);
}
console.log("method receiver", methodReceiver());

const erasedMap: any = new Map<string, number>();
erasedMap.set("a", 1);
console.log("erased map", erasedMap.get("a"), erasedMap.size);

function forwardRead(): number {
  const read = () => later + 1;
  const later = 41;
  return read();
}
console.log("forward read", forwardRead());

function arm(ms: number): void {
  const onFire = () => {
    clearTimeout(timeoutId);
    console.log("timeout handle reached the handler");
  };
  const timeoutId = setTimeout(onFire, ms);
}
arm(0);

function Maker(this: any, x: number) {
  this.x = x;
}
Maker.prototype.twice = function (this: any) {
  return this.x * 2;
};
const made = new (Maker as any)(3);
console.log("construct", made.x, made.twice(), made instanceof (Maker as any));
console.log("instanceof other", {} instanceof (Maker as any));

function wrapped(func: (...args: unknown[]) => unknown): unknown {
  const wrapper = function (...args: unknown[]) {
    return func(...args);
  };
  if ((func as any).prototype) {
    wrapper.prototype = Object.create((func as any).prototype);
  }
  return wrapper;
}
const wrappedMaker = wrapped(Maker as any) as any;
console.log("function property", Boolean(wrappedMaker.prototype));

const proto = { greet: () => "hi" };
const child = Object.create(proto);
console.log("create", child.greet(), Object.getPrototypeOf(child) === proto);

type Next = () => string;
type Middleware = (label: string, next: Next) => string;
function wrap(call: Middleware): Middleware {
  return (label: string, next: Next) => {
    const again = () => call(label, next);
    return again() + again();
  };
}
console.log("handle rebuilt", wrap((label, next) => label + next())("a", () => "b"));

type Handler = (key: string) => string | Promise<string>;
async function runHandler(handler: Handler): Promise<string> {
  return await handler("k");
}
async function asyncSection(): Promise<void> {
  console.log("promise arm", await runHandler(async (key: string) => key + "!"));
  console.log("value arm", await runHandler((key: string) => key + "?"));
  console.log("async cell", await asyncShared());
}
asyncSection();

setTimeout(() => {
  console.log("zero delay fired");
});

function callIt(fn: Function): void {
  fn();
}
try {
  const boom = () => {
    throw new Error("boom");
  };
  callIt(boom);
  console.log("not reached");
} catch (error) {
  console.log("never callback", (error as Error).message);
}

class Failure {
  message: string;
  constructor(message: string) {
    this.message = message;
  }
}
const rethrow = (err: Failure): never => {
  throw err;
};
const handle = (err: Failure): number => err.message.length;
function chooseHandler(strict: boolean): (err: Failure) => number {
  const handler = strict ? rethrow : (err: Failure) => handle(err);
  return handler;
}
console.log("join lenient", chooseHandler(false)(new Failure("four")));
try {
  chooseHandler(true)(new Failure("x"));
} catch (error) {
  console.log("join strict", (error as Failure).message);
}

interface PlainInit {
  status?: number;
}
function pickStatus(arg?: number | PlainInit | Response): number {
  return typeof arg === "number" ? arg : (arg?.status ?? 200);
}
console.log(
  "union read",
  pickStatus(404),
  pickStatus({ status: 201 }),
  pickStatus({}),
  pickStatus(new Response(null, { status: 500 })),
  pickStatus(),
);

function fillWith<T>(xs: T[], make: (index: number) => T): void {
  xs.push(make(0));
}
function useFill(): number[] {
  const nums: number[] = [1, 2];
  fillWith(nums, (i: number) => i + 10);
  return nums;
}
console.log("mut generic", useFill().join(","));

function applyTo<T>(xs: T[], fn: (item: T) => T): T[] {
  return xs.map(fn);
}
function useApply(): number[] {
  const nums: number[] = [1, 2, 3];
  return applyTo(nums, (n: number) => n + 1);
}
console.log("borrowed callback", useApply().join(","));

type Row = { id: number };
class Collector {
  #addRow(into: Row[], row: Row, tag?: string): void {
    into.push(row);
    if (tag) {
      into.push({ id: row.id + 1 });
    }
  }
  collect(): string {
    const sink: Row[] = [];
    this.#addRow(sink, { id: 1 });
    this.#addRow(sink, { id: 5 }, "twice");
    return sink.map((row) => row.id).join(",");
  }
}
console.log("method abi", new Collector().collect());
