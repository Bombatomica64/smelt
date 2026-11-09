// Typed arrays, Buffer and the arguments object.

function tag(value: unknown): string {
  return Object.prototype.toString.call(value);
}

function tags(): void {
  const views: unknown[] = [
    new Int8Array(1), new Uint8Array(1), new Uint8ClampedArray(1),
    new Int16Array(1), new Uint16Array(1), new Int32Array(1), new Uint32Array(1),
    new Float32Array(1), new Float64Array(1), new BigInt64Array(1), new BigUint64Array(1),
    Buffer.from([1]),
  ];
  for (const view of views) {
    console.log(tag(view));
  }
}

function isView(value: any): boolean {
  return ArrayBuffer.isView(value);
}

function kinds(): void {
  console.log(isView(Buffer.from([1])), isView(new Uint8Array(1)), isView(new DataView(new ArrayBuffer(1))));
  console.log(isView(new ArrayBuffer(1)), isView([1]), isView({}));
  const u: unknown = new Uint8Array(1);
  const b: unknown = Buffer.from([1]);
  const f: unknown = new Float64Array(1);
  console.log(u instanceof Uint8Array, b instanceof Uint8Array, f instanceof Uint8Array, f instanceof Float64Array);
}

function codec(): void {
  const buffer = new ArrayBuffer(8);
  const bytes = new Uint8Array(buffer);
  bytes.fill(255);
  console.log(new Int8Array(buffer)[0], new Uint8Array(buffer)[0]);
  console.log(new Int16Array(buffer)[0], new Uint16Array(buffer)[0]);
  console.log(new Int32Array(buffer)[0], new Uint32Array(buffer)[0]);
  console.log(new BigInt64Array(buffer)[0], new BigUint64Array(buffer)[0]);
  bytes.fill(0);
  new Float32Array(buffer)[0] = 1.5;
  console.log(new Float32Array(buffer)[0], new Uint8Array(buffer)[3]);
  new Float64Array(buffer)[0] = 0.1;
  console.log(new Float64Array(buffer)[0]);
  const clamped = new Uint8ClampedArray([300, -5, 0.5, 1.5, 2.5]);
  console.log(Array.from(clamped).join());
  const wrapped = new Uint8Array([300, -1]);
  console.log(Array.from(wrapped).join());
}

function lengths(): void {
  const eight = new ArrayBuffer(8);
  console.log(new Float64Array(eight).length, new Float32Array(eight).length, new Uint8Array(eight).length);
  console.log(new Float64Array(eight).byteLength, new Int16Array(3).byteLength, new Int16Array(3).length);
  const view = new Uint16Array(eight, 2, 2);
  console.log(view.byteOffset, view.length, view.buffer === eight);
  const erased: any = new Int32Array([1, 2, 3, 4]);
  console.log(erased.slice(1, 3).length, erased.slice(-1)[0], erased.subarray(1).length);
}

function construction(): void {
  const buffer = new ArrayBuffer(8);
  const reviewed = new Float32Array(buffer);
  reviewed[0] = 2;
  console.log(new Float32Array(buffer)[0]);
  const converted = new Uint8Array(new Int8Array([-1]));
  console.log(converted[0]);
  const any1: any = buffer;
  console.log(new Float32Array(any1).length);
  const any2: any = new Int8Array([-1, 2]);
  console.log(new Uint8Array(any2)[0], new Uint8Array(any2).length);
  console.log(new Uint8Array(new ArrayBuffer(2)).length, new Float64Array(3).length);
  const first = new Uint8Array([10, 20, 30]);
  console.log(first[0], first[2], first.length);
}

function ownKeys(value: any): string {
  return Object.keys(value).join();
}
function ownValues(value: any): unknown[] {
  return Object.values(value);
}
function ownEntries(value: any): [string, unknown][] {
  return Object.entries(value);
}
function hasKey(value: any, key: string): boolean {
  return Object.hasOwn(value, key);
}

function keysOf(): void {
  console.log(ownKeys(new Uint8Array(2)));
  const asRecord: Record<string, unknown> = new Uint8Array(1) as unknown as Record<string, unknown>;
  console.log(Object.keys(asRecord).join());
  console.log(ownValues(new Uint8Array([4, 5])).join(), ownEntries(new Uint8Array([4])).map(([k, v]) => k + ":" + String(v)).join());
  console.log(hasKey(new Uint8Array(1), "0"), hasKey(new Uint8Array(1), "1"), hasKey(new Uint8Array(1), "length"));
}

function hex(bytes: Uint8Array): string {
  let out = "";
  for (let index = 0; index < bytes.length; index = index + 1) {
    out = out + Number(bytes[index]).toString(16);
  }
  return out;
}

function args(a: number, b: number): any {
  const unused = a + b;
  return arguments;
}
function argsList(a: number, b: number): unknown[] {
  const unused = a + b;
  return Array.from(arguments as any);
}

function argumentsObject(): void {
  const object = args(1, 2);
  console.log(Object.keys(object).join(), tag(object), object.length, object[1]);
  console.log(argsList(3, 4).join());
}

function main(): void {
  tags();
  kinds();
  codec();
  lengths();
  construction();
  keysOf();
  console.log(hex(new Uint8Array([1, 171])));
  argumentsObject();
}

main();
