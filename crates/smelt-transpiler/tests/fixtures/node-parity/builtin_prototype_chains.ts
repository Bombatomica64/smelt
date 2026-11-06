// `instanceof` walks the builtin prototype chain: `DOMException` extends `Error`,
// `File` extends `Blob`, a `TypeError` is an `Error`, and a union arm answers for
// its base classes.
class Plain extends Error {
  constructor(m: string) { super(m); }
}
class Direct extends DOMException {
  constructor(m = 'dflt') { super(m); }
}
class Sub extends Plain {}
function isErr(x: unknown): boolean { return x instanceof Error; }
function isDom(x: unknown): boolean { return x instanceof DOMException; }
const p = new Plain('a');
console.log(p instanceof Error, p instanceof Plain, isErr(p), isDom(p));
const s = new Sub('a');
console.log(s instanceof Error, s instanceof Plain, s instanceof Sub, isErr(s));
const d = new Direct();
console.log(d instanceof Error, d instanceof DOMException, d instanceof Direct, isErr(d), isDom(d), d.message);
console.log(isErr(new DOMException('x')), isDom(new DOMException('x')), new DOMException('x') instanceof Error);
console.log(isDom(new Error('x')), isDom(1));
class T1 extends TypeError {}
const t = new T1('x');
console.log(t instanceof Error, t instanceof TypeError, t instanceof RangeError, isErr(t));
class B1 { k = 1; }
class B2 extends B1 {}
function un(x: B2 | number): boolean { return x instanceof B1; }
console.log(un(new B2()), un(1));
const de: DOMException = new DOMException('q');
console.log(de instanceof Error, de instanceof DOMException);
const ee: Error = new Direct();
console.log(ee instanceof DOMException, ee instanceof Error);
function isBlob(x: unknown): boolean { return x instanceof Blob; }
function isFile(x: unknown): x is File { return isBlob(x) && x instanceof File; }
const items: Array<File | number> = [new File([''], 'a.txt'), 1, new File([''], 'b.txt'), 2];
const files = items.filter(isFile);
console.log(files.length, files[0] instanceof File, files[1] instanceof File, files[0] instanceof Blob);
const blob = new Blob(['a']);
console.log(blob instanceof File, blob instanceof Blob, isFile(blob), isBlob(blob));
const u: unknown = new File(['a'], 'n.txt');
console.log(u instanceof File, u instanceof Blob);
