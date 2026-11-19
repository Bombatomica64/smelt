// A typed list handed to an `unknown[]` slot is the SAME array there
// (docs/design/record-identity.md, step 2): a write through the `unknown[]`
// view (index store, push, splice, length) is read through the typed list and
// the other way round, however long both are held, and identity holds.

function fill(xs: unknown[]): void {
  xs[0] = 42;
  xs.push(7);
}
const nums: number[] = [1, 2, 3];
fill(nums as unknown[]);
console.log(nums.length, nums[0], nums[3], nums.join(","));

const alias: unknown[] = nums;
alias[1] = 99;
console.log(nums[1], alias === nums);
nums.push(5);
console.log(alias.length, alias[alias.length - 1]);
alias.splice(0, 2);
console.log(nums.join(","), alias.join(","));
alias.length = 2;
console.log(nums.length, nums.join(","));

const words: string[] = ["a", "b"];
function shout(items: unknown[]): void {
  for (let i = 0; i < items.length; i++) items[i] = String(items[i]).toUpperCase();
}
shout(words);
console.log(words.join(""));

interface Point { x: number; y: number }
const pts: Point[] = [{ x: 1, y: 2 }];
const erasedPts: unknown[] = pts;
erasedPts.push({ x: 3, y: 4 });
console.log(pts.length, pts[1].x + pts[1].y);

function count(xs: unknown[]): number { return xs.length; }
const flags: boolean[] = [true];
flags.push(false);
console.log(count(flags), (flags as unknown[]) === (flags as unknown[]));
