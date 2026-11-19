// An element read past the end of a list or a typed-array view is
// `undefined` wherever the reader's type can hold it: an optional slot,
// `=== undefined`, `??`, an optional return, an erased parameter.
const u8 = new Uint8Array(2);
const xs: number[] = [1];
const a: number | undefined = u8[5];
const b: number | undefined = xs[5];
console.log(a, b, u8[5] === undefined, xs[5] === undefined, u8[5] ?? -1, xs[5] ?? -1, xs.at(5));
function first(values: number[]): number | undefined { return values[3]; }
function firstByte(values: Uint8Array): number | undefined { return values[3]; }
console.log(first(xs), firstByte(u8), u8[1], xs[0]);
function erased(value: unknown): string { return value === undefined ? "undefined" : String(value); }
console.log(erased(u8[5]), erased(u8[1]), erased(xs[5]));
const big = new BigInt64Array(1);
const missing: bigint | undefined = big[3];
console.log(missing, big[0]);
// The same tests bound before printing: a miss is `undefined` in either spelling.
const viewMiss = u8[5] === undefined;
const listMiss = xs[5] === undefined;
const viewHit = u8[1] !== undefined;
console.log(viewMiss, listMiss, viewHit);
// Two holes are `undefined === undefined`; a computed NaN still equals nothing.
const allocated: number[] = Array(2);
const empty: number[] = [];
const notANumber = 0 / 0;
console.log(allocated[0] === empty[5], allocated[0] === allocated[1], allocated[0] !== empty[5], notANumber === notANumber, notANumber === empty[3]);
