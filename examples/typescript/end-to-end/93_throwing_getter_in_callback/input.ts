// Reading a THROWING getter is a fallible operation, and which fallible form
// the generated Rust uses depends on the enclosing item.
//
// A getter whose body can throw is emitted returning `Result`, so the read has
// to do something with that `Result`. `?` only travels where the enclosing
// function returns `Result` — a closure whose contextual callback type is a
// plain `Fn(..) -> T` is not such a function, and a `?` in its body is
// `error[E0277]: the `?` operator can only be used in a closure that returns
// `Result``. Three of Hono's closures hit exactly that, reading
// `c.executionCtx` (a getter that throws when the context has none) inside a
// callback the framework types as infallible.
//
// So: a fallible reader propagates with `?`, and a non-fallible one takes the
// panic path — the program stops at the throw rather than continuing with a
// fabricated value. Nothing here makes the getter actually throw, because a
// throw that escapes an infallible callback is a process-level stop rather
// than a catchable error, which is the floor this records rather than a
// behaviour to diff against Node.
//
// Every line below is diffed against Node.

class Registry {
  private slots: string[] = [];

  get first(): string {
    if (this.slots.length === 0) {
      throw new Error("registry is empty");
    }
    return this.slots[0];
  }

  add(name: string): void {
    this.slots.push(name);
  }

  // A FALLIBLE reader: it throws on its own, so the getter read propagates.
  describe(): string {
    if (this.slots.length > 3) {
      throw new Error("too many");
    }
    return `first=${this.first}`;
  }

  // A NON-fallible reader: a plain method with no throw of its own.
  label(): string {
    return "[" + this.first + "]";
  }

  // The closure shape: the callback's type is a plain mapper, so the read
  // inside it cannot propagate.
  tagged(prefix: string[]): string[] {
    return prefix.map((tag) => tag + ":" + this.first);
  }
}

const registry = new Registry();
registry.add("alpha");
registry.add("beta");

console.log(registry.first);
console.log(registry.describe());
console.log(registry.label());
console.log(registry.tagged(["a", "b"]).join(","));

// The fallible reader's OWN throw is still catchable, which is what makes the
// two forms different rather than interchangeable.
registry.add("gamma");
registry.add("delta");
// The binding-less `catch` is deliberate: `catch (error)` binds a genuine
// `unknown`, and the examples corpus holds a hard "no avoidable erasure"
// invariant, so the erased spelling lives in the runtime tiers instead (the
// same split `74_typed_array_views` documents).
let caught = "none";
try {
  registry.describe();
  caught = "no throw";
} catch {
  caught = "threw";
}
console.log(caught);
