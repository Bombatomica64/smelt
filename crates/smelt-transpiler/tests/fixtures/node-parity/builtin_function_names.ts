// Builtin functions read as values report the name and length Node gives:
// the property key they are installed under and their specified length.
const f = Math.max;
console.log(f.name, f.length);
const g = parseInt;
console.log(g.name, g.length);
const alias = g;
console.log(alias.name, alias.length);
console.log(String.name, String.length);
console.log(Number.name, Boolean.name, Array.name, Object.name);
const m = Math.floor;
console.log(m.name, m.length);
console.log(Math.abs.name, Math.abs.length, Array.isArray.name);
const isNan = Number.isNaN;
console.log(isNan.name, isNan.length);
const create = Object.create;
console.log(create.name, create.length);
const enc = encodeURIComponent;
console.log(enc.name, enc.length);
