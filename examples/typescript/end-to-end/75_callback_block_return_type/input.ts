// A block-bodied callback's return type comes from its own `return`s.
//
// A callback the compact callback IR cannot model is retried through real
// closure-body lowering, with the CALLER's fallback return type. For `map` that
// fallback is `unknown` — the element type of the mapped list is exactly what
// the callback is supposed to answer, so the caller has nothing better to
// offer. An expression-bodied arrow already inferred its own type from its
// body; a block-bodied one kept the fallback, so its `map` produced a
// `List<Unknown>` even when every `return` in it is a string.
//
// A loop, a compound assignment, a `try`, a `switch` — any of them sends a
// callback down that path, and so does reading `this`. Hono's `buildRegExpStr`
// (`src/router/reg-exp-router/node.ts`) is the `this` shape, and its erasure
// surfaced two frames later as `list unshift item must match the list element
// type` with nothing about where. That is H54;
// `blocker-logs/hono-h54-callback-return-erasure.md` has the bisection.
//
// The rule is to read the body's own returns and join them the way a ternary's
// arms are joined, keeping the caller's fallback only when there is nothing to
// infer from (no `return` at all, a bare `return;`, or arms that do not unify).
//
// TYPES are the assertion here as much as values: a `List<Unknown>` prints the
// same text, so every mapped list below is pushed through an operation that
// only type-checks at the concrete element type — `unshift` of a string (the
// Hono blocker itself) or arithmetic on a number.
//
// The `this`-reading shape is deliberately NOT here: the closure it emits
// captures the method receiver and the emitter renders that as
// `let self = self.clone()`, which is not valid Rust. That is pre-existing —
// it fails identically without this fix — and recorded as
// `blocker-logs/hono-h57-closure-this-capture-name.md`.

// A loop: the compact IR has no loops, so this is the fallback path.
function joined(keys: string[]): string[] {
  return keys.map((key) => {
    let out = "";
    for (const ch of key) {
      out += ch;
    }
    return out;
  });
}

// A compound assignment to a local.
function shouted(keys: string[]): string[] {
  return keys.map((key) => {
    let out = key;
    out += "!";
    return out;
  });
}

// `try`/`catch`, with a `return` in each arm.
function upper(keys: string[]): string[] {
  return keys.map((key) => {
    try {
      return key + "?";
    } catch {
      return key;
    }
  });
}

// A `switch` whose arms return different expressions of the same type.
function classified(keys: string[]): string[] {
  return keys.map((key) => {
    switch (key) {
      case "a":
        return "A";
      default:
        return key;
    }
  });
}

// A numeric answer, so the inference is not string-shaped by accident.
function widths(keys: string[]): number[] {
  return keys.map((key) => {
    let total = 0;
    for (const ch of key) {
      total += ch.length;
    }
    return total;
  });
}

const first = joined(["ab", "c"]);
first.unshift("start");
console.log(first.join("|"));

const second = shouted(["a", "b"]);
second.unshift("start");
console.log(second.join("|"));

const third = upper(["a", "bc"]);
third.unshift("start");
console.log(third.join("|"));

const fourth = classified(["a", "z"]);
fourth.unshift("start");
console.log(fourth.join("|"));

const fifth = widths(["ab", "cde"]);
console.log(fifth.reduce((total, value) => total + value, 0));

// The two shapes that already worked, so the inference cannot have displaced
// them: an expression-bodied callback (inferred before) and a block-bodied one
// the compact IR models on its own.
// (An expression body whose whole value is a stdlib METHOD CALL —
// `key.toUpperCase()` — still types as `unknown`, for a reason that is not this
// rule and predates it: see
// `blocker-logs/hono-h58-callback-method-call-return.md`. The control here uses
// a concatenation so it asserts the shape this fixture is about.)
const sixth = ["a", "b"].map((key) => key + "-");
sixth.unshift("start");
console.log(sixth.join("|"));

const seventh = [1, 2].map((value) => {
  const step = value * 2;
  return step;
});
console.log(seventh.reduce((total, value) => total + value, 0));
