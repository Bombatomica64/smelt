// An erased value converted to a declared record: present fields convert,
// optional absent fields stay absent, a parameter the body tests for presence
// accepts null, every member a JavaScript read would find is found on an
// instance (fields, getters, methods, a host object's members), `#private`
// state survives the round trip, and source validation is observed by `try/catch`.
interface Data {
  count: number;
  name: string;
  note?: string;
  when: number | undefined;
}

function raw(value: unknown): any {
  return value;
}

const present = raw({ count: 1, name: "a", note: "n", when: 3 }) as Data;
console.log(present.count, present.name, present.note, present.when);

const optional = raw({ count: 2, name: "b" }) as Data;
console.log(optional.count, optional.note, optional.when);

interface Options {
  times?: number;
}
function times(options: Options): number {
  return options?.times ?? 3;
}
console.log(times(raw(null)));
console.log(times(raw({ times: 7 })));

interface Sized {
  size: number;
}
class Bag {
  items: number[] = [1, 2];
  get size(): number {
    return this.items.length;
  }
}
function converts(value: unknown): string {
  const sized = value as Sized;
  return sized ? "converted" : "absent";
}
console.log(converts(new Bag()));
console.log(converts(new Map<string, number>([["a", 1]])));

function validated(value: any): Data {
  if (typeof value !== "object" || value === null || typeof value.count !== "number") {
    throw new TypeError("not a Data");
  }
  return value as Data;
}
try {
  validated({ name: "x" });
} catch (error) {
  console.log(error instanceof TypeError, (error as Error).message);
}
console.log(validated({ count: 9, name: "ok" }).count);

// getter is read through the view
function sizeOf(value: unknown): number {
  return (value as Sized).size;
}
console.log(sizeOf(new Bag()));
console.log(sizeOf(new Map<string, number>([["a", 1]])));
console.log((new Bag() as any).size, Object.keys(new Bag() as any).join(","), JSON.stringify(new Bag()));

class Secret {
  #hidden = 41;
  bump(): number {
    this.#hidden += 1;
    return this.#hidden;
  }
}
function roundTrip(value: unknown): Secret {
  return value as Secret;
}
const secret = roundTrip(new Secret());
console.log(secret.bump(), (secret as any)["#hidden"], Object.keys(secret).length, JSON.stringify(secret));

interface Svc {
  run(): Promise<number>;
  name: string;
}
class Impl {
  name = "impl";
  async run(): Promise<number> {
    return 1;
  }
}
function svc(value: unknown): Svc {
  return value as Svc;
}
console.log(svc(new Impl()).name, typeof svc(new Impl()).run);

function guarded(o: Options): number {
  if (!o) return -1;
  return o.times ?? 0;
}
console.log(guarded(raw(undefined)), guarded({ times: 2 }));
const arrow = (o: Options): number => o?.times ?? 5;
console.log(arrow(raw(null)));
class Guard { check(o: Options): number { return o?.times ?? 11; } }
console.log(new Guard().check(raw(null)));
function apply(f: (o: Options) => number): number { return f({ times: 4 }); }
function tested(o: Options): number { return o?.times ?? 12; }
console.log(apply(tested), tested({}));

class Tag {
  #value: number;
  constructor(value: number) {
    this.#value = value;
  }
  read(): number {
    return this.#value;
  }
}
function tagBack(value: unknown): Tag {
  return value as Tag;
}
console.log(tagBack(new Tag(7)).read());
