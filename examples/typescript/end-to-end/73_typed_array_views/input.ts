// The typed-array family as CONCRETE Rust types, with the erased byte-backed
// record as its boundary form.
//
// Before this, a source `Uint8Array` annotation and a `new Uint8Array(..)` both
// resolved to the erased record: the value existed inside a `SmeltUnknown`, and
// every read of it — `.length`, `view[0]`, `.subarray(1)` — crossed the dynamic
// boundary. Now the eleven view spellings are one generated `SmeltTypedArray`
// (a kind, shared byte storage, a byte offset, an element count) and
// `ArrayBuffer` is `SmeltArrayBuffer`, so all of that is ordinary Rust: the
// element kind is a runtime field of the value because
// `Object.prototype.toString.call(view)` reports it, and the eleven therefore
// share one Rust type while `Type::Class { name }` keeps the source spelling.
//
// `SharedArrayBuffer` and `DataView` deliberately stay erased: neither surface
// is modeled concretely, and a half-modeled concrete face answers `instanceof`
// correctly and then fails every method.
//
// Every line below is diffed against Node.

// Construction, in every spelling the constructor has.
const fromElements = new Uint8Array([1, 2, 3, 250]);
const fromLength = new Uint8Array(3);
const buffer = new ArrayBuffer(8);
const wholeBuffer = new Float64Array(buffer);
const window = new Uint8Array(buffer, 2, 4);
const converted = new Uint8Array(new Int8Array([-1]));

console.log(fromElements.length, fromElements.byteLength, fromElements.byteOffset);
console.log(fromLength.length, buffer.byteLength);
console.log(wholeBuffer.length, wholeBuffer.byteLength, wholeBuffer.byteOffset);
console.log(window.length, window.byteOffset, window.buffer.byteLength);
// An element source CONVERTS per element rather than re-viewing bytes, so a
// signed -1 becomes 255 and not the byte pattern of a wider element.
console.log(converted.length, converted[0]);

// Element width and signedness, which is the whole reason the kind is a value.
console.log(new Int8Array([200])[0], new Uint8Array([256])[0], new Uint8ClampedArray([300])[0]);
const wide = new Uint32Array([1, 2]);
console.log(wide.length, wide.byteLength, wide[1]);

// Indexed read and write, at the element's own width.
fromElements[0] = 9;
console.log(fromElements[0], fromElements[3]);

// `subarray` SHARES storage; `slice` copies. This is what makes the offset and
// the buffer real rather than decoration.
const shared = fromElements.subarray(1);
shared[0] = 42;
console.log(fromElements[1], shared[0], shared.byteOffset);
const copied = fromElements.slice(0, 2);
copied[0] = 0;
console.log(fromElements[0], copied[0], copied.length);

// `set` converts per element; `fill` writes one value across a range and
// answers the same view.
const target = new Uint8Array(4);
target.set(fromElements.subarray(0, 2), 1);
console.log(target[0], target[1], target[2]);
target.fill(7, 2);
console.log(target[1], target[2], target[3]);

// Identity, tags and stringification.
console.log(fromElements instanceof Uint8Array, fromElements instanceof Float64Array);
console.log(buffer instanceof ArrayBuffer, ArrayBuffer.isView(buffer));
console.log(ArrayBuffer.isView(fromElements), ArrayBuffer.isView(wholeBuffer));
console.log(Object.prototype.toString.call(fromElements));
console.log(Object.prototype.toString.call(wide));
// `TypedArray.prototype.toString` IS `Array.prototype.toString`.
console.log(String(fromElements));
console.log(`${wide}`);

// Iteration and spreading decode the elements once.
const collected: number[] = [];
for (const element of wide) {
  collected.push(element);
}
console.log(collected.join("-"));
console.log([...wide].join("-"));
console.log(Array.from(wide).join("-"));

// Enumeration answers from the concrete value: a view's own enumerable
// properties are exactly its element indices, and its values are numbers, so
// none of this goes through the erased record. Byte storage has no own
// properties at all, which is why it serializes as `{}`.
console.log(JSON.stringify(fromElements));
console.log(JSON.stringify(buffer));
console.log(Object.keys(fromElements).join(","));
console.log(Object.values(fromElements).join(","));
console.log(Object.entries(wide).map(([key, value]) => `${key}=${value}`).join(","));
console.log(Reflect.ownKeys(wide).join(","));

// The rest of the boundary — an erased view's `instanceof`, `isView`,
// `String()` and `JSON.stringify`, the recovery of a concrete view from an
// erased buffer, and the two byte hosts that stay erased (`DataView`,
// `SharedArrayBuffer`) — is exercised in the runtime tier
// (`crates/smelt-codegen-rust/tests/typed_array_runtime.rs`). It lives there
// rather than here because those flows are erased by construction and the
// examples corpus holds a hard `avoidable == 0` invariant; increment 5 of the
// plan folds them back once the enumeration path answers a concrete view
// without a record round trip.
