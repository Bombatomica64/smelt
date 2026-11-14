// `??` over an erased `?.` chain: an absent link and a present `undefined`/
// `null` both take the fallback, and a present `0` is kept.
const erased: any = {};
const loaded: any = { locale: { options: { week: 1 } } };
function read(source: any, options?: { week?: number }): number {
  return options?.week ?? source.locale?.options?.week ?? 0;
}
console.log(read(erased), read(erased, { week: 5 }), read(loaded), read(loaded, {}));
const holder: Record<string, unknown> = { zero: 0, nil: null };
function look(key: string, first?: number): number { const v = holder as any; return first ?? v[key]?.inner ?? v[key] ?? -1; }
console.log(look("zero"), look("nil"), look("missing"));
