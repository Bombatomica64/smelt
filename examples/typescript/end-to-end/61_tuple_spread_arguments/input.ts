// Spreading a tuple into a call distributes its elements positionally onto the
// callee's parameters.
//
// This has to be checked at RUNTIME. The broken lowering dropped the spread and
// padded every parameter with a default, which COMPILED -- so a compile-only
// assertion would have passed while every argument was wrong. Each line below
// prints what the callee actually received.
type Route = [string, string, number];

// 1. A named top-level function.
function record(method: string, path: string, weight: number): string {
  return method + " " + path + " @" + weight;
}

const routes: Route[] = [
  ["GET", "/a", 1],
  ["POST", "/b", 2],
];

for (let i = 0; i < routes.length; i++) {
  console.log(record(...routes[i]));
}

// Leading fixed arguments still line up around a spread.
const tail: [string, number] = ["/c", 7];
console.log(record("DELETE", ...tail));

// 2. A closure held in a local.
const viaClosure = (method: string, path: string, weight: number): string =>
  "closure " + record(method, path, weight);
console.log(viaClosure(...routes[0]));

// 3. A method on a class.
class Sink {
  take(method: string, path: string, weight: number): string {
    return "method " + record(method, path, weight);
  }
}
const sink = new Sink();
console.log(sink.take(...routes[1]));

// 4. A spread of a call result must evaluate that call exactly once.
let evaluations = 0;
function makeRoute(): Route {
  evaluations = evaluations + 1;
  return ["PUT", "/once", 3];
}
console.log(record(...makeRoute()));
console.log("evaluations: " + evaluations);

// 5. A list element read. Hono's `router.add(...routes[i])` is spelled this
// way. The read resolves its own optionality before the spread sees it (the
// array-hole path defaults a miss), so the spread receives a plain tuple --
// which is why an out-of-range read prints defaults rather than throwing. That
// is a separate, pre-existing gap, recorded in the note; what this line pins is
// that the in-range read distributes correctly.
const oneRoute: Route[] = [["PATCH", "/maybe", 4]];
console.log(record(...oneRoute[0]));
