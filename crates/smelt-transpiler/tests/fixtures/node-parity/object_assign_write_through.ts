// `Object.assign(target, ..sources)` writes ONTO its target and returns it,
// whether the target is an interface record, a member of an object, or a
// parameter, and whatever interfaces the target's type extends.
interface Named {
  name: string;
}
interface Counted {
  count: number;
}
interface Data extends Named, Counted {
  tag: string;
}

class Holder {
  data: Data = { name: "a", count: 0, tag: "t" };
  inner: { data: Data } = { data: { name: "i", count: 0, tag: "t" } };
  bump(patch: Named): void {
    Object.assign(this.data, patch);
  }
}

const h = new Holder();
Object.assign(h.data, { count: 1 });
console.log(h.data.count, h.data.name);
h.bump({ name: "b" });
console.log(h.data.count, h.data.name);
Object.assign(h.inner.data, { tag: "deep" }, { count: 3 });
console.log(h.inner.data.tag, h.inner.data.count);

const d: Data = { name: "x", count: 0, tag: "t" };
Object.assign(d, { name: "y" });
console.log(d.count, d.name);

function patch(target: Data, change: Counted): Data {
  return Object.assign(target, change);
}
const e: Data = { name: "e", count: 5, tag: "t" };
const r = patch(e, { count: 6 });
console.log(e.count, r.count, e.name, r === e);

interface Options {
  label?: string;
  size: number;
}
const opts: Options = { label: "keep", size: 1 };
const partial: { label?: string; size?: number } = { size: 2 };
Object.assign(opts, partial);
console.log(opts.label, opts.size);
const relabel: { label?: string; size?: number } = { label: "z" };
Object.assign(opts, relabel);
console.log(opts.label, opts.size);
