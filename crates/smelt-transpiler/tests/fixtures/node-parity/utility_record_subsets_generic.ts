// A generic `K extends keyof T` is fixed by each call, so each call's result
// holds exactly the keys that call asked for.
interface Base {
  a: string;
  b: number;
  c?: boolean;
}

const pick = <T extends object, K extends keyof T>(obj: T, keys: K[]): Pick<T, K> => {
  const out = {} as Pick<T, K>;
  for (const key of keys) {
    out[key] = obj[key];
  }
  return out;
};

function omit<T, K extends keyof T>(obj: T, keys: K[]): Omit<T, K> {
  return keys.reduce(
    (acc, key) => {
      delete acc[key];
      return acc;
    },
    { ...obj },
  );
}

function loose<T>(value: T): Partial<T> {
  return value;
}

function report(label: string, keys: string[], json: string, spread: string): void {
  console.log(label, keys.join(","), json, spread);
}

function main(): void {
  const base: Base = { a: "a", b: 1 };
  const one = pick(base, ["a"]);
  report("pick-one", Object.keys(one), JSON.stringify(one), JSON.stringify({ ...one }));
  console.log(one.a);
  const none = pick(base, []);
  report("pick-none", Object.keys(none), JSON.stringify(none), JSON.stringify({ ...none }));
  const without = omit(base, ["a"]);
  report("omit-one", Object.keys(without), JSON.stringify(without), JSON.stringify({ ...without }));
  console.log(without.b);
  const anonymous = omit({ left: 1, right: "r" }, ["left"]);
  report("omit-anonymous", Object.keys(anonymous), JSON.stringify(anonymous), JSON.stringify({ ...anonymous }));
  const reversed = pick(base, ["b", "a"]);
  report("pick-order", Object.keys(reversed), JSON.stringify(reversed), JSON.stringify({ ...reversed }));
  const relaxed = loose(base);
  console.log(relaxed.a, relaxed.c === undefined);
}

main();
