// An object literal's getter runs on every read, dotted or keyed, whatever
// spells its key: a plain name, a constant holding a string, or a symbol.
const SYMBOL_NAME = "label";
const tag = Symbol("tag");
let reads = 0;
const obj = {
  get count(): number { reads += 1; return reads; },
  get [SYMBOL_NAME](): string { return "from getter"; },
  get [tag](): number { return 7; },
  plain: 1,
};
console.log(obj.count, obj.count, obj.label, obj[SYMBOL_NAME], obj[tag], obj.plain, Object.keys(obj));
const key: string = "count";
console.log(obj[key as "count"], reads);
