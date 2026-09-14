// A module-top-level `try` whose body AWAITS, with statements after it.
//
// Both arms of the emitted `match` re-emit the code that FOLLOWS the
// `try`/`catch`, and each arm is its own Rust lexical scope — so a temporary
// the tail needs has to be declared in whichever arm is emitting it. It used to
// be declared in the first arm only, and the second arm assigned to a name its
// scope had never declared; the crate did not compile. The awaits after the
// `catch`, and the module's own exit drain, are exactly that tail.
//
// The catch bindings are omitted deliberately. A caught payload is the
// exception-payload ABI — a genuine dynamic boundary, since `throw` accepts any
// JavaScript value — and naming one here would put an erased local in a corpus
// whose whole job is to hold none. What this fixture is about is the TAIL, and
// which arm ran is observable without the payload.

async function work(flag: boolean): Promise<string> {
  if (flag) {
    throw new Error("rejected");
  }
  return "resolved";
}

let outcome = "not run";
try {
  outcome = await work(false);
} catch {
  outcome = "caught";
}
console.log(outcome);

// The tail after the catch: more awaits, and more statements needing
// temporaries of their own.
const after = await work(false);
console.log(after);
console.log(after.length + outcome.length);

// The throwing path takes the other arm, and its tail must compile too.
let thrown = "not run";
try {
  thrown = await work(true);
} catch {
  thrown = "caught";
}
console.log(thrown);

const last = await work(false);
console.log(last, thrown.length);

// A nested try inside a try, both awaiting, so an arm's tail is itself a
// two-armed match.
let nested = "not run";
try {
  try {
    nested = await work(true);
  } catch {
    nested = "inner";
  }
  nested = nested + "|" + (await work(false));
} catch {
  nested = "outer";
}
console.log(nested);

// A try whose tail is only the module's exit drain, which is the shape the
// generated entry point appends after the last statement.
let final = "not run";
try {
  final = await work(true);
} catch {
  final = "last caught";
}
console.log(final);
