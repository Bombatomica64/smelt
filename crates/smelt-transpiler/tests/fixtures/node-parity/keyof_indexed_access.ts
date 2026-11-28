interface Settings { volume: number; name: string; muted: boolean }

function getSetting<K extends keyof Settings>(s: Settings, k: K): Settings[K] {
  return s[k];
}
function setSetting<K extends keyof Settings>(s: Settings, k: K, v: Settings[K]): void {
  s[k] = v;
}
function readPlain(s: Settings, k: keyof Settings): Settings[keyof Settings] {
  return s[k];
}

type Point = { x: number; y: number };
function axis<K extends keyof Point>(p: Point, k: K): number {
  return p[k];
}
function bump<K extends keyof Point>(p: Point, k: K, by: number): void {
  p[k] = p[k] + by;
}

class Pair {
  first = "a";
  second = "b";
}
function side<K extends keyof Pair>(pair: Pair, k: K): string {
  return pair[k];
}

const st: Settings = { volume: 5, name: "x", muted: false };
setSetting(st, "volume", 9);
setSetting(st, "name", "loud");
setSetting(st, "muted", true);
console.log(getSetting(st, "volume"), getSetting(st, "name"), getSetting(st, "muted"));
console.log(st.volume, st.name, st.muted);
const keys: (keyof Settings)[] = ["name", "volume", "muted"];
for (const k of keys) console.log(k, readPlain(st, k));

const p: Point = { x: 1, y: 2 };
bump(p, "y", 10);
console.log(axis(p, "x"), axis(p, "y"), p.y);
const pair = new Pair();
console.log(side(pair, "first") + side(pair, "second"));
