// A parameter binds inside its own function only: its neighbour still reads the
// module constant of that name.
type F = (s: string) => string;
const lower: F = (s) => s.toLowerCase();
const table: Record<string, F> = { A: lower };
function use(table: Record<string, F>, k: string): string { return table[k]("X"); }
function direct(k: string): string { const f = table[k]; return f ? f("Y") : "none"; }
function count(table: number[]): number { return table.length; }
console.log(use(table, "A"), direct("A"), direct("B"), count([1]));
console.log(Object.keys(table));
export const nums = { a: 1 };
function idx(nums: number[]): number { return nums[0]; }
console.log(nums.a, idx([9]));
