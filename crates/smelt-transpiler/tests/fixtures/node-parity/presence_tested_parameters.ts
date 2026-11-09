// A parameter the body tests for presence accepts an absent value whatever its
// annotation says, so `null as any` reaches it unchanged; `null as unknown as T`
// stays null through a binding.
interface Options {
  times?: number;
}
function raw(value: unknown): any {
  return value;
}
function chained(options: Options): number {
  return options?.times ?? 3;
}
function guarded(options: Options): number {
  if (!options) return -1;
  return options.times ?? 0;
}
function compared(options: Options): string {
  return options == null ? "absent" : "present";
}
const arrow = (options: Options): number => options?.times ?? 5;
class Runner {
  run(options: Options): number {
    return options?.times ?? 11;
  }
}
function inline(options: { times?: number }): number {
  return options?.times ?? 13;
}
console.log(chained(raw(null)), chained(raw(undefined)), chained({ times: 7 }), chained({}));
console.log(guarded(raw(null)), guarded({ times: 2 }));
console.log(compared(raw(undefined)), compared({}));
console.log(arrow(raw(null)), new Runner().run(raw(null)), inline(raw(null)));
const NULL = null as unknown as Options;
console.log(chained(NULL), NULL === null);
function apply(f: (options: Options) => number): number {
  return f({ times: 4 });
}
console.log(apply(chained));
