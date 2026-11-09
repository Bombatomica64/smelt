// Inheritance, reference classes, getters, reassigned methods and host constructors
// reached through dynamic reads, observed end to end.

const smallNumbers = [0, 1].concat([2, 3, 4].slice(1));
function numberCount(): number {
  const values = smallNumbers;
  return values.length;
}
console.log(numberCount(), smallNumbers.join(","));

class BaseMsg {
  message: string;
  constructor(message: string) { this.message = message; }
}
class ChildMsg extends BaseMsg {
  constructor(message: string) { super(message); }
}
console.log(new ChildMsg("hello").message);

class Level1 { a: number; constructor() { this.a = 1; } }
class Level2 extends Level1 { b: number; constructor() { super(); this.b = 2; } }
class Level3 extends Level2 { c: number; constructor() { super(); this.c = 3; } }
const leaf = new Level3();
console.log(leaf.a, leaf.b, leaf.c);

class ClosureBase {
  name: string = "";
  msg: string = "x";
  greet: () => string;
  constructor() { this.greet = () => "base:" + this.name + ":" + this.msg; }
}
class Smart extends ClosureBase {
  msg = "hi";
  constructor() { super(); this.name = "smart"; }
}
console.log(new Smart().greet());

class Tally {
  total: number = 1;
  bump(by: number): void { this.total = this.total + by; }
  twice(): number { this.bump(this.total); return this.total; }
}
console.log(new Tally().twice());

class Plain { x: number; constructor() { this.x = 1; } }
class Mutated extends Plain { bump(): void { this.x = this.x + 1; } }
const mutated = new Mutated();
mutated.bump();
console.log(mutated.x);

class ParseFailure extends TypeError {
  constructor(message: string) { super(message); }
}
const failure = new ParseFailure("bad input");
console.log(failure.name, failure.message, failure instanceof TypeError, failure instanceof Error);
try { throw new ParseFailure("thrown"); } catch (e) { console.log((e as Error).message); }

class Greeting {
  text: string;
  constructor(text = "hi there") { this.text = text; }
}
console.log(new Greeting().text, new Greeting("given").text);

function globalRead(name: any): any { return (globalThis as any)[name]; }
const dynamicError = new (globalRead("TypeError"))("dyn");
console.log(dynamicError.message, dynamicError instanceof TypeError, globalRead("NoSuchGlobal") === undefined);
function blobRef(): any { return Blob; }
function ctorOf(value: any): any { return value.constructor; }
console.log(blobRef() === blobRef(), ctorOf(new Blob([])) === blobRef());

class Handler {
  cb: (x: number) => number;
  constructor(cb: (x: number) => number) { this.cb = cb; }
}
class SubHandler extends Handler { tag: string = "sub"; }
function makeBase(cb: (x: number) => number): Handler { return new Handler(cb); }
function makeSub(cb: (x: number) => number): SubHandler { return new SubHandler(cb); }
console.log(makeBase((x) => x + 1).cb(1), makeSub((x) => x + 2).cb(1), makeSub((x) => x).tag);

class Stepper {
  total = 0;
  step(flag: boolean, amount: number): number {
    let next: number;
    if (flag) { next = this.total + amount; } else { next = this.total - amount; }
    this.total = next;
    return next;
  }
}
const stepper = new Stepper();
console.log(stepper.step(true, 2), stepper.step(false, 5));

class TrieNode { key = ""; }
function has(nodes: Record<string, TrieNode>, key: string): boolean {
  const found: TrieNode | undefined = nodes[key];
  const present = !!found;
  return present;
}
console.log(has({}, "a"), has({ a: new TrieNode() }, "a"));

class Store {
  entries: Record<string, number> = {};
  put(key: string, value: number): void { this.entries[key] = value; }
}
class CountingStore extends Store {
  reads = 0;
  total(): number {
    let sum = 0;
    for (const key of Object.keys(this.entries)) { sum += this.entries[key]; }
    return sum;
  }
}
const countingStore = new CountingStore();
countingStore.put("a", 2);
countingStore.put("b", 5);
console.log(countingStore.total());

class Holder {
  inner: string;
  constructor(inner: string) { this.inner = inner; }
  private get active(): string {
    if (this.inner === "") { throw new Error("empty"); }
    return this.inner;
  }
  label(): string { return this.active + "!"; }
  rethrow(): string {
    if (this.inner === "x") { throw new Error("x"); }
    return this.active;
  }
}
console.log(new Holder("ab").label(), new Holder("a").rethrow());
try { new Holder("").rethrow(); } catch (e) { console.log((e as Error).message); }
try { new Holder("x").rethrow(); } catch (e) { console.log((e as Error).message); }

class Picker {
  tag: unknown = 0;
  pick(value: string): string { return value; }
  other(value: string): string { return value + "?"; }
  run(value: string): string {
    this.pick = (v: string) => v + "!";
    return this.pick(value) + this.other(value);
  }
}
const picker = new Picker();
console.log(picker.pick("a"), picker.run("b"), picker.pick("c"));

class Point2 {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
}
type MakePoint = (...args: ConstructorParameters<typeof Point2>) => Point2;
const makePoint: MakePoint = (x, y) => new Point2(x, y);
console.log(makePoint(1, 2).x, makePoint(3, 4).y);

type ClientRequestOptions = {
  webSocket?: (...args: ConstructorParameters<typeof WebSocket>) => WebSocket;
};
const clientOptions: ClientRequestOptions = {};
console.log(clientOptions.webSocket === undefined);

class Shape {
  describe(): string { return `${this.name()} ${this.sides()}`; }
  name(): string { return "shape"; }
  sides(): number { return 0; }
}
class Square extends Shape {
  name(): string { return "square"; }
  sides(): number { return 4; }
}
console.log(new Square().describe(), new Shape().describe());
