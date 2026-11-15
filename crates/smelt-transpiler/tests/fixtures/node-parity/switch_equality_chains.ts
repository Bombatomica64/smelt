// `switch` is a chain of `===` tests. One whose discriminant is erased or
// optional-erased, or whose labels are not literals (`case undefined`), runs
// as that chain (date-fns zh-CN `ordinalNumber`: `switch (options?.unit)`).
type Unit = "date" | "hour" | "minute";
type LocalizeFn = (value: number, options?: { unit?: Unit }) => string;

const ordinal: LocalizeFn = (value, options) => {
  switch (options?.unit) {
    case "date":
      return value + "日";
    case "hour":
      return value + "时";
    default:
      return "第 " + value;
  }
};

function describe(value: unknown): string {
  switch (value) {
    case "date":
      return "text";
    case 3:
      return "three";
    case null:
      return "null";
    case undefined:
      return "undefined";
    case true:
      return "true";
    default:
      return "other";
  }
}

function literalsOnly(value: unknown): number {
  switch (value) {
    case "a":
      return 1;
    case 2:
      return 2;
    default:
      return 0;
  }
}

function optionalErased(holder?: { unit?: unknown }): string {
  switch (holder?.unit) {
    case "x":
      return "X";
    case undefined:
      return "none";
    default:
      return "?";
  }
}

const Kind = { A: "a", B: "b" } as const;
function kindOf(kind: string): number {
  let seen = 0;
  switch (kind) {
    case Kind.A:
      seen += 1;
    // falls through
    case Kind.B:
      seen += 10;
      break;
    default:
      seen = -1;
  }
  return seen;
}

console.log(ordinal(1, { unit: "date" }), ordinal(2, { unit: "hour" }), ordinal(3), ordinal(4, {}));
console.log(describe("date"), describe(3), describe("3"), describe(null), describe(undefined), describe(true), describe(1));
console.log(optionalErased({ unit: "x" }), optionalErased(), optionalErased({ unit: 1 }), optionalErased({}));
console.log(kindOf("a"), kindOf("b"), kindOf("c"));
console.log(literalsOnly("a"), literalsOnly(2), literalsOnly("2"), literalsOnly(null));
