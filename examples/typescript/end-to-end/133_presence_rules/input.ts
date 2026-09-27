// Presence rules behind Hono's body parser and trie router.

// 1. `'k' in value` answers from the value's whole instance surface: a host
//    class's spec members, a user class's methods as well as its fields.
class Wrapper {
  raw: Request;
  count = 0;
  constructor(raw: Request) {
    this.raw = raw;
  }
  header(): string {
    this.count++;
    return "x";
  }
}
const isRaw = (request: Wrapper | Request): request is Request => "headers" in request;
const req = new Request("https://x/", { method: "POST", body: "a", headers: { "Content-Type": "text/plain" } });
console.log(isRaw(req), isRaw(new Wrapper(req)));
console.log(isRaw(new Wrapper(req).raw), new Wrapper(req).header());
console.log("raw" in new Wrapper(req), "header" in new Wrapper(req), "nope" in new Wrapper(req));

// 2. A keyed read compared with `undefined` holds the absence, even for a
//    falsy-inhabited element type.
function segments(path: string): string[] {
  const parts = path.split("/");
  const out: string[] = [];
  let i = 0;
  for (const p of parts) {
    const next = parts[++i];
    out.push(next === undefined ? `${p}<last>` : p);
  }
  return out;
}
console.log(segments("a/b/c").join(","));

// 3. `rec[key] ?? fallback` takes the fallback for a missing key.
function pick(nodeParams: Record<string, string>, params: Record<string, string> | undefined, key: string): string {
  return nodeParams[key] ?? params?.[key];
}
function pickOr(nodeParams: Record<string, string>, key: string): string {
  return nodeParams[key] ?? "fallback";
}
console.log(pick({ id: "1" }, { action: "create" }, "action"), pick({ id: "1" }, undefined, "id"));
console.log(pickOr({}, "x"), pickOr({ x: "y" }, "x"));

// 4. `list[i] ||= []` stores into a missing slot.
function buckets(): string {
  const queue: string[][] = [];
  for (const [count, item] of [[1, "a"], [1, "b"], [0, "c"]] as [number, string][]) {
    const target = (queue[count] ||= []);
    target.push(item);
  }
  return JSON.stringify(queue);
}
console.log(buckets());

// 5. `.test` on a `RegExp | true` value runs the regex, and a RegExp built from
//    a string reports its source with `/` escaped.
function matches(matcher: RegExp | true, part: string): boolean {
  return matcher === true || matcher.test(part);
}
console.log(matches(/^[0-9]+$/, "123"), matches(/^[0-9]+$/, "abc"), matches(true, "x"));
console.log(new RegExp("^a(?=/next)").source, String(new RegExp("x/y")));
