// Round-6 rules: a missing key/index is `undefined`, a type assertion does not
// change the runtime arm, lib aliases are their unions, and a sleep under
// `Promise.race` is a timer like any other.

// 1. `rec[k] === undefined` is a presence test on a `Record<string, string>`.
function makeRecord(keys: string[]): Record<string, string> {
  const rec: Record<string, string> = {};
  for (const key of keys) rec[key] = key + '!';
  return rec;
}
const rec = makeRecord(['a']);
console.log(rec['id'] === undefined, rec['a'] === undefined, rec['a'] !== undefined);
const pairs: [string, Record<string, string>][] = [['x', makeRecord([])], ['y', makeRecord(['id'])]];
console.log(pairs[0][1]['id'] === undefined, pairs[1][1]['id'] == null);
const xs: string[] = ['q'];
console.log(xs[3] === undefined, xs[0] === undefined);

// 2. A string index read is total.
function startsWithSlash(parts: string[]): boolean {
  return parts[0][0] === '/';
}
console.log(startsWithSlash(['']), startsWithSlash(['/a']));

// 3. `buf as ArrayBuffer` over a byte-source union digests the live arm.
async function digestLength(data: string | ArrayBufferView | ArrayBuffer): Promise<number> {
  let source: ArrayBufferView | ArrayBuffer;
  if (typeof data === 'string') {
    source = new TextEncoder().encode(data);
  } else {
    source = data;
  }
  const buffer = await crypto.subtle.digest('SHA-256', source as ArrayBuffer);
  return new Uint8Array(buffer)[0];
}

// 4. `FormDataEntryValue` is `File | string`.
function collect(into: (string | File)[], value: FormDataEntryValue): number {
  return into.push(value);
}
const entries: (string | File)[] = ['a'];
collect(entries, 'b');
console.log(entries.length, typeof entries[1], entries[1]);

// 5. A constructed-only `RegExp` still carries its runtime.
console.log(new RegExp('a/b').source);

// 6. A sleeping racer due first beats a slower one: under `Promise.race` a
//    sleep is a timer, so polling one racer cannot advance the clock past
//    another racer's deadline.
async function slow(ms: number, value: string): Promise<string> {
  await new Promise((resolve) => setTimeout(resolve, ms));
  return value;
}
async function run(): Promise<void> {
  console.log(await digestLength('abc'), await digestLength(new Uint8Array([1, 2, 3])));
  console.log(await Promise.race([slow(100, 'slow'), slow(30, 'fast')]));
  console.log(await Promise.race([slow(110, 'late'), slow(50, 'timed out')]));
  console.log(await Promise.race([slow(20, 'early'), slow(60, 'late timeout')]));
}
run();

// 7. Generic read-only `Array.prototype` methods over an array-like, and a
//    radix inside a callback body.
const digestBytes = new Uint8Array([1, 171, 255]);
console.log(Array.prototype.join.call(digestBytes, '-'), Array.prototype.indexOf.call(digestBytes, 171));
console.log([10, 255].map((x) => x.toString(16)).join(','));

// 8. A ternary in argument position narrows its present arm.
const queue: string[][] = [['a', 'b'], ['c']];
const head = queue.shift();
console.log(head ? head.join(',') : 'none');
