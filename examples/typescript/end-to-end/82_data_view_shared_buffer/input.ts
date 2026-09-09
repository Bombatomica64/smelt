// `DataView` and `SharedArrayBuffer` as concrete members of the byte family.
//
// The two were deliberately left erased when the eleven views and
// `ArrayBuffer` became concrete, and each needed a different shape.
//
// `SharedArrayBuffer` is the SAME class as `ArrayBuffer`: the two differ in
// their `[object X]` tag, their `instanceof` answer and the growth members, and
// in nothing about the bytes — storage every view writes through is what
// `ArrayBuffer` already is, and Smelt has no worker threads, so the one
// behaviour a hand-written Rust pair would differ on cannot be observed. One
// Rust type with a species flag, then; answering "ArrayBuffer" for a
// `SharedArrayBuffer` WOULD be observable, which is why the flag is not
// optional.
//
// `DataView` is its own class, because its element kind is an argument of every
// accessor rather than a property of the value: the same view answers
// `getInt16(0)` and `getFloat64(0)`. Its byte ORDER is a parameter too, and its
// default is BIG-endian — the opposite of every typed array. The widths and
// signednesses are still the kind table's, reached by reversing the window for
// a big-endian call, so each has one definition.
//
// Every line below is diffed against Node.

// Storage, and the species that separates the two constructors.
const shared = new SharedArrayBuffer(8);
const plain = new ArrayBuffer(8);
console.log(shared.byteLength, plain.byteLength);
console.log(shared instanceof SharedArrayBuffer, shared instanceof ArrayBuffer);
console.log(plain instanceof ArrayBuffer, plain instanceof SharedArrayBuffer);
console.log(String(shared), String(plain));

// A view over shared storage reports THAT storage back.
const overShared = new Uint8Array(shared);
overShared[0] = 7;
console.log(overShared[0], overShared.buffer instanceof SharedArrayBuffer);

// `slice` copies and keeps the species.
const copied = shared.slice(0, 4);
console.log(copied.byteLength, copied instanceof SharedArrayBuffer);

// A `DataView` addresses BYTES: it has `byteLength`, `byteOffset` and
// `buffer`, and no element count.
const buffer = new ArrayBuffer(16);
const view = new DataView(buffer);
console.log(view.byteLength, view.byteOffset, view.buffer === buffer);
console.log(view instanceof DataView, String(view), ArrayBuffer.isView(view));

// The default byte order is big-endian, which is what makes these two reads of
// the same two bytes disagree.
view.setInt16(0, -2);
console.log(view.getInt16(0), view.getUint16(0));
console.log(view.getUint8(0), view.getUint8(1));
view.setInt16(2, -2, true);
console.log(view.getUint8(2), view.getUint8(3));
console.log(view.getInt16(2, true), view.getInt16(2, false));

// The width comes from the accessor, not the view, so one view reads the same
// bytes at every width.
view.setUint32(4, 4294967295);
console.log(view.getUint32(4), view.getInt32(4));
view.setFloat64(8, 1.5);
console.log(view.getFloat64(8));

// A window is an offset and a length into the SAME storage, so a write through
// one is visible through the other.
const windowed = new DataView(buffer, 8, 4);
console.log(windowed.byteOffset, windowed.byteLength);
windowed.setInt8(0, 7);
console.log(view.getInt8(8), windowed.getInt8(0));

// A `DataView` over shared storage reports the shared buffer, as a typed array
// over it does.
const sharedView = new DataView(shared);
console.log(sharedView.buffer instanceof SharedArrayBuffer, sharedView.byteLength);
console.log(sharedView.getUint8(0));

// Crossing the dynamic boundary keeps the identity: a `DataView` is a view with
// no index keys, which is exactly why es-toolkit's `isTypedArray` has to
// exclude it from `ArrayBuffer.isView`. The `Object.prototype.toString` tag and
// the `instanceof` answer are read here THROUGH the concrete value, which needs
// no erasure at all.
console.log(JSON.stringify(view), Object.keys(view).length);
console.log(Object.prototype.toString.call(view), Object.prototype.toString.call(shared));
console.log(view instanceof DataView ? view.byteLength : -1);

// The same two questions asked through an `unknown` PARAMETER — the shape
// es-toolkit's `isTypedArray` has — are erased by construction, so they live in
// the runtime tier rather than here: the examples corpus holds a hard
// `avoidable == 0` invariant, and the precedent is `74_typed_array_views`,
// whose erased half is in the same tier
// (`crates/smelt-codegen-rust/tests/typed_array_runtime.rs`,
// `the_erased_face_is_reached_only_through_the_boundary_adapters`).
