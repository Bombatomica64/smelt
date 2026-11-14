// Optional calls on erased receivers short-circuit as `?.` does, and computed
// member calls read the member the key names.

function attempt(label: string, run: () => unknown): void {
  try {
    console.log(label, run());
  } catch (error) {
    console.log(label, (error as Error).name, (error as Error).message);
  }
}

const a: any = [1, 2];
const n: any = undefined;
const z: any = null;
const o: any = { b: [3, 4] };
let evaluated = 0;
const touch = (): number => {
  evaluated++;
  return 1;
};

// A nullish receiver ends the call, and its arguments are not evaluated.
attempt("present", () => a?.map((x: number) => x * 2));
attempt("undefined receiver", () => n?.map((x: number) => x));
attempt("null receiver", () => z?.map((x: number) => x));
attempt("args skipped", () => [n?.concat(touch()), evaluated]);
attempt("args evaluated", () => [a?.concat(touch()), evaluated]);

// A nullish link anywhere in the chain ends the whole chain.
attempt("deeper chain present", () => o?.b.map((x: number) => x + 1));
attempt("deeper chain absent", () => n?.b.map((x: number) => x + 1));

// `?.()` tests the member itself and keeps the receiver.
attempt("optional call present", () => a.map?.((x: number) => -x));
attempt("optional call missing", () => a.nope?.((x: number) => x));

// Computed members name the member by key, literal or dynamic.
const key: any = "map";
const index: any = "1";
const s: any = "hello";
const num: any = 2.5;
attempt("computed optional", () => [a?.["map"]((x: number) => x), n?.["map"]((x: number) => x)]);
attempt("computed literal", () => [a["map"]((x: number) => x + 10), s["slice"](1), num["toFixed"](1)]);
attempt("computed dynamic", () => [a[key]((x: number) => x * 3), a[index], s[index]]);
