// A parameter typed as a union of a construct signature and a call signature
// (date-fns `transpose`'s `constructor` argument) that the function STORES:
// `seen.push(make)` keeps a handle on the caller's callable past the call, so
// the parameter enters owned (it was a borrowed `&dyn Fn`, and wrapping a
// borrow into the stored `'static` handle was E0521).
class Box {
  value = 7;
}
type Make = (new () => Box) | (() => Box);
const seen: unknown[] = [];
function remember(make: Make): number {
  seen.push(make);
  return seen.length;
}
const kept = new Map<string, () => number>();
function keep(name: string, read: () => number): void {
  kept.set(name, read);
}
function build(make: Make, isClass: boolean): Box {
  return isClass ? new (make as new () => Box)() : (make as () => Box)();
}
console.log(remember(Box), remember(() => new Box()), typeof seen[0], typeof seen[1]);
keep("one", () => 1);
keep("two", () => 2);
console.log(kept.size, kept.get("two")?.(), build(Box, true).value, build(() => ({ value: 3 }), false).value);
