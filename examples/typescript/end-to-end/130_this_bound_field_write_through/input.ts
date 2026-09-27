// A class field that stores a plain function taking an explicit `this`
// (Hono's `match: typeof match<..> = match`). Calling it as a method supplies
// the instance as `this`, and the function replaces itself on that instance
// the first time it runs, so later calls take the fast path.
interface Counter {
  label: string;
  builds: number;
  describe(prefix: string): string;
}

function describe(this: Counter, prefix: string): string {
  this.builds = this.builds + 1;
  const label = this.label;
  const fast = (next: string): string => next + ' ' + label + ' (cached)';
  this.describe = fast;
  return prefix + ' ' + label + ' (built)';
}

class Tally {
  label: string;
  builds: number = 0;
  describe: (prefix: string) => string = describe;
  constructor(label: string) {
    this.label = label;
  }
}

const tally = new Tally('apples');
console.log(tally.describe('first'));
console.log(tally.describe('second'));
console.log(tally.describe('third'));
console.log(tally.builds);
const other = new Tally('pears');
console.log(other.describe('solo'));
console.log(other.builds, tally.builds);
