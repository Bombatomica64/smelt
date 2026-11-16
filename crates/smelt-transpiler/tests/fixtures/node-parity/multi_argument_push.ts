// `push` with several arguments evaluates them all, then appends them in
// order, wherever the call sits (a loop body, an expression).
function f(): number[] { let i = 0; const out: number[] = []; while (i < 5) { const j = i; out.push(j, j); i = i + 2; } return out; }
console.log(f().join(","));
const out: number[] = [];
const len = out.push(1, out.length, ...[7, 8], out.length);
console.log(out.join(","), len);
const words: string[] = [];
for (const w of ["a", "b"]) { const up = w.toUpperCase(); words.push(w, up); }
console.log(words.join(""));
const one: number[] = [];
console.log(one.push(5), one.push(...[6, 7]), one.join(","));
