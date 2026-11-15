// `"key" in record` on a declared record: an optional property is present only
// when the value carries it, and the key is compared by its JavaScript spelling
// (date-fns' year setter: `!("era" in flags) || flags.era === 1`).
interface ParseFlags {
  timestampIsSet?: boolean;
  era?: number;
}

function year(flags: ParseFlags, value: number): number {
  return !("era" in flags) || flags.era === 1 ? value : 1 - value;
}

const flags: ParseFlags = {};
console.log(year(flags, 2022), "era" in flags, "timestampIsSet" in flags);
Object.assign(flags, { timestampIsSet: true });
console.log("timestampIsSet" in flags, flags.timestampIsSet, "era" in flags);
Object.assign(flags, { era: 0 });
console.log(year(flags, 2022), "era" in flags);

class Point {
  x = 1;
  fullName = "p";
  label?: string;
  move(): void {}
}
const point = new Point();
console.log("x" in point, "fullName" in point, "label" in point, "move" in point, "toString" in point, "y" in point);
point.label = "p";
console.log("label" in point);
