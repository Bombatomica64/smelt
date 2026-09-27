// User-defined type predicates (`value is T`, `asserts value is T`) narrow a
// union argument in the guarded branch AND narrow the other branch to the
// remaining members, whichever callable carries the predicate: a function
// declaration, an arrow bound to a const, a class method (on an instance or
// on `this`), or a block-local arrow.

interface Req {
  kind: "req";
  url: string;
}

interface RawReq {
  kind: "raw";
  raw: number;
}

function isRaw(r: Req | RawReq): r is RawReq {
  return r.kind === "raw";
}

const isReq = (r: Req | RawReq): r is Req => r.kind === "req";

// Positive branch sees `RawReq`, the else branch sees `Req`.
function describe(request: Req | RawReq): string {
  if (isRaw(request)) {
    return "raw " + request.raw;
  } else {
    return "req " + request.url;
  }
}

// Arrow predicate; the early return leaves `RawReq` behind.
function describeArrow(request: Req | RawReq): string {
  if (isReq(request)) {
    return "req " + request.url;
  }
  return "raw " + request.raw;
}

// Negated guard: the then-branch sees `Req`, the fall-through `RawReq`.
function earlyReturn(request: Req | RawReq): string {
  if (!isRaw(request)) {
    return "req " + request.url;
  }
  return "raw " + (request.raw + 1);
}

function isString(value: string | number): value is string {
  return typeof value === "string";
}

// `&&` carries the guard's fact into the rest of the condition and the branch.
function lengthOr(value: string | number, big: boolean): number {
  if (isString(value) && big && value.length > 2) {
    return value.length;
  }
  if (!isString(value)) {
    return value * 10;
  }
  return 0;
}

class Checker {
  isRaw(r: Req | RawReq): r is RawReq {
    return r.kind === "raw";
  }

  label(request: Req | RawReq): string {
    if (this.isRaw(request)) {
      return "this-raw " + request.raw;
    }
    return "this-req " + request.url;
  }
}

function describeMethod(checker: Checker, request: Req | RawReq): string {
  if (checker.isRaw(request)) {
    return "m-raw " + request.raw;
  }
  return "m-req " + request.url;
}

function describeLocal(request: Req | RawReq): string {
  const localIsRaw = (candidate: Req | RawReq): candidate is RawReq =>
    candidate.kind === "raw";
  if (localIsRaw(request)) {
    return "l-raw " + request.raw;
  }
  return "l-req " + request.url;
}

function assertRaw(r: Req | RawReq): asserts r is RawReq {
  if (r.kind !== "raw") {
    throw new Error("not raw");
  }
}

function asserted(request: Req | RawReq): number {
  assertRaw(request);
  return request.raw * 2;
}

const plainRequest: Req = { kind: "req", url: "/home" };
const rawRequest: RawReq = { kind: "raw", raw: 7 };
console.log(describe(plainRequest));
console.log(describe(rawRequest));
console.log(describeArrow(plainRequest));
console.log(describeArrow(rawRequest));
console.log(earlyReturn(plainRequest));
console.log(earlyReturn(rawRequest));
console.log(lengthOr("hello", true));
console.log(lengthOr("hi", true));
console.log(lengthOr(5, true));
const checker = new Checker();
console.log(describeMethod(checker, plainRequest));
console.log(describeMethod(checker, rawRequest));
console.log(checker.label(plainRequest));
console.log(checker.label(rawRequest));
console.log(describeLocal(plainRequest));
console.log(describeLocal(rawRequest));
console.log(asserted(rawRequest));
