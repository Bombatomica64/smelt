// A conditional's arms keep their own types: a primitive consequent does not
// convert a different primitive in the alternate.
function segment(quoted: string | undefined, raw: string): string | number {
  return quoted !== undefined ? quoted : /^\d+$/.test(raw) ? Number(raw) : raw;
}
const out: (string | number)[] = [];
for (const raw of ["12", "x"]) out.push(raw === "q" ? "quoted" : /^\d+$/.test(raw) ? Number(raw) : raw);
out.push(segment(undefined, "34"), segment("k", "56"));
console.log(out, out.map((value) => typeof value));
