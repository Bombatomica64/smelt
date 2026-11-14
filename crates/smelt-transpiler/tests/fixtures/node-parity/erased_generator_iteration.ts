"use strict";
// A generator handed out through an erased value is still an iterable: spread,
// `for...of`, `Array.from` and destructuring call its `[Symbol.iterator]`, and
// a partly consumed generator resumes where it stopped.
function* count(limit: number) {
  for (let i = 1; i <= limit; i++) {
    yield i;
  }
}
const host: any = { count };
console.log("spread", [...host.count(3)]);
for (const value of host.count(2)) {
  console.log("for-of", value);
}
console.log("Array.from", Array.from(host.count(2)));
const [first, second] = host.count(5);
console.log("destructured", first, second);
const partly: any = host.count(4);
console.log("first next", partly.next().value);
console.log("rest", [...partly]);
console.log("after rest", partly.next().done);
const method: any = {
  *letters() {
    yield "a";
    yield "b";
  },
};
console.log("generator method", [...method.letters()]);
// A generator is its own iterator: `[Symbol.iterator]()` answers the same
// object, so the identity holds, and iterating it consumes the original.
const same: any = host.count(3);
console.log("self iterator", same[Symbol.iterator]() === same);
const iteratorOf: any = same[Symbol.iterator];
console.log("method read off", iteratorOf.call(same) === same);
same.next();
console.log("shared state", [...same[Symbol.iterator]()]);
async function* ticks() {
  yield "a";
}
const asyncHost: any = { ticks };
const asyncGenerator: any = asyncHost.ticks();
console.log("async self iterator", asyncGenerator[Symbol.asyncIterator]() === asyncGenerator);
// Destructuring steps the iterator only as far as the pattern binds: an
// infinite generator destructures, and a generator does not run past the last
// item it hands out.
function* naturals() {
  let n = 0;
  while (true) {
    n += 1;
    yield n;
  }
}
const lazyHost: any = { naturals, count };
const [one, two] = lazyHost.naturals();
console.log("infinite", one, two);
const stepped: string[] = [];
function* traced() {
  stepped.push("first");
  yield 1;
  stepped.push("second");
  yield 2;
  stepped.push("third");
  yield 3;
}
const tracedHost: any = { traced };
const [onlyFirst] = tracedHost.traced();
console.log("stepped", onlyFirst, stepped);
const [head, ...tail] = lazyHost.count(4);
console.log("rest", head, tail);
