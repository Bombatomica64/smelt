# Hono phase 2: the generated crate under `cargo check`

Round 10, item 3. Phase 1 (source lowering) has been at zero blockers since
round 9; H19 (this round) got whole-crate MIR lowering to complete. Phase 2 is
the next gate: emit the crate and compile it.

## Where phase 2 actually stops

`smelt build` on `third_party/hono/Smelt.toml` no longer fails in MIR. It now
fails in the EMITTER:

```
Error: EmitError { message: "`new Headers(init)` initializer type is not modeled: SmeltUnknown" }
```

That is a standards-stream demand, not a general family: the initializer is
`SmeltUnknown` because `this.#res.headers` — `Response.headers` — is not typed,
and `Response`/`Headers` are owned by `blocker-logs/standards-tier-plan.md`
(campaign plan §6). Recorded in `blocker-logs/hono-fetch-demand.md` §1, whose
"blocking a source-lowering blocker" framing is now out of date for items 1-2:
they block EMISSION of the whole crate, which is strictly worse — one unmodeled
constructor argument in `src/context.ts` costs the entire crate.

`src/context.ts` is reachable from almost everything Hono exports, so it cannot
be excluded to see past it: excluding it would gut the crate rather than probe
it.

## So the crate was probed by slice

To get phase-2 information anyway, a manifest whose entry set is the ROUTERS and
`src/utils/url.ts` — the part of the closure that needs no fetch type — was
built and `cargo check`ed. That slice is 16 generated modules and includes the
trie, reg-exp, pattern and smart routers, which is where round 10's item-2
blockers came from in the first place.

The slice is a probe, not a gate: `.github/compat/hono/Smelt-routers.toml` is
committed (copy it next to the probe checkout's `Smelt.toml`) so the
measurement can be repeated, and it is NOT the campaign's in-scope manifest.

| | errors | warnings |
| --- | ---: | ---: |
| before this round's fixes | **19 829** | 39 |
| after H22 (method-body locals) | 547 | 39 |
| after H23 (optional place bases) + H24 (bool coercion) | **497** | 99 |

Full grouped report: `blocker-logs/hono-routers-diagnostics.md` (regenerate with
`smelt rust-diagnostics --cargo-manifest third_party/hono/dist-routers/Cargo.toml`).

## Fixed this round

### H22 — a method body had no function-scope local declarations

19 053 × E0425 (`cannot find value _smelt_tmp_N in this scope`), plus most of
96 × E0070.

MIR locals are function-scoped; generated Rust branch bodies are lexically
scoped. `emit_body` (free functions, `main`, closures) has always predeclared
the locals first assigned outside the entry block — `emit_method` never did. So
a temporary first assigned inside one `if` arm was declared *inside* that arm
with `let mut`, and the sibling arm's assignment to the same MIR local named
something out of scope. Any branchy method hit it; Hono's routers are nothing
but branchy methods. `emit_method` now calls `emit_mutable_local_preludes`, so a
method body and a function body agree.

Two constructor assertions moved with it (`part_7_tests`): a `super()` result is
now `let __smelt_super: A;` plus an assignment rather than one initialized
`let`, which is what every free-function body already looked like.

### H23 — a place whose base is still optional had no lvalue, and its read named the wrong field

144 × E0609 (`no field _pattern on type &Node<T>`) and 96 × E0070 (`invalid
left-hand side of assignment`).

`child.#pattern = pattern` where `child: Node<T> | undefined`: `tsc` accepts it
because the preceding assignment narrows `child`, and MIR keeps the receiver's
declared type on the place base. The emitter then had no arm for an optional
base, so `assignment_place_text` fell through to the field READ expression
(`child.as_ref().and_then(|v| v._pattern.clone())`) and used it as an lvalue.
The lvalue now unwraps in place — `as_mut().expect("optional value was absent
after narrowing")` — which is the only spelling that keeps the write inside the
value the binding holds; an unwrapped copy would type-check and silently drop
it. A reference class keeps its handle (`.0.borrow_mut().field`).

The matching READ had the same gap in the other direction: the structural-record
arm spelled `_smelt_value.field` for a reference class, whose fields live behind
`Rc<RefCell<Inner>>`. Both `place_text` and `field_access_text` now go through
the handle.

### H24 — a value asked for at `bool` was handed back unchanged

48 × E0308 (`expected bool, found Node<T>`).

Wherever JavaScript expects a boolean it applies truthiness, and for a class
instance the answer is "present". `value_at_type_text` had no rule for a `bool`
target, so the optional-map arm rendered `Option<Node<T>>` at `bool` as
`.map_or(false, |value| value)`. It now defers to `value_truthy_text`, which is
the one truthiness notion the emitter already has (`Type::Unknown` and a type
parameter keep their existing erased narrowing paths).

## Open, numbered, not mine to fix this round

| # | family | evidence | count |
| --- | --- | --- | ---: |
| H25 | a call to a class method whose parameter is by-reference passes it BY VALUE, and an optional trailing parameter is omitted from the call | `self._push_handler_sets(handler_sets.clone(), ..)`: `expected &mut SmeltList<SmeltUnknown>, found SmeltList<SmeltUnknown>` + `takes 5 arguments but 4 were supplied` | 400 × E0308 + 19 × E0061 |
| H26 | a generated union (`SmeltUnion127`) is matched with `SmeltUnknown::` arms, and passed where `SmeltUnknown` is expected, without its `IntoSmeltUnknown` boundary | `src/prepared_router.rs:349-357` | 10 × E0308 |
| H27 | a dict read whose value type is already optional yields `Option<Option<T>>` where `Option<T>` is expected | `src/matcher.rs:37`, `:69` | 6 × E0308 |
| H28 | a throwing call in statement position renders as `()` where the function's `Result<SmeltUnknown, Box<dyn Error>>` is expected | | 6 × E0308 |
| H29 | a generic method body that inspects a `T` does not carry the bounds its body needs (`T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown`), and a tuple of `T` has no `into_smelt_unknown` | `src/main.rs:3597`, `:3640`, `:3681`, `:5348` | 19 × E0277 + 7 × E0599 |
| H30 | remaining struct/field mismatches: `PatternRouter<SmeltUnknown>` has no field `_routes` (E0560 ×6), a field read on an anonymous class handle (E0609 ×9 — `no field 0 on __smelt_anon_class_3050<T>`), E0615 ×3, E0369 ×1 | | 19 |

## H31 — a write through a projected VALUE-class receiver is lost (silent wrong value)

Found while verifying H23, and the only family here with NO rustc error at all.

```ts
class Node {
  children: Record<string, Node> = {};
  pattern = '';
  insert(key: string, pattern: string): void {
    const child = new Node();
    this.children[key] = child;   // writes into a COPY of this.children
    child.pattern = pattern;
  }
  patternOf(key: string): string {
    const child = this.children[key];
    return child ? child.pattern : 'missing';
  }
}
const root = new Node();
root.insert('a', 'p1');
console.log(root.patternOf('a'));   // Node 22: p1   Smelt: missing
```

`lower_place` roots an index place by materializing the receiver into a local
(`this.children` → a temp), which is correct for Smelt's handle types — a
`SmeltList`/`SmeltRecord` clone shares its storage — and wrong for a value
representation, here a plain `HashMap` field of a value class: the insert lands
in the temp and is never written back. It predates this round (the `a.b[i] = v`
path has always worked this way); H19 only made the shape reachable, and it is
independent of optionals (the repro above uses public, non-optional fields).

The honest fix is a NESTED place (`Place::Field` of `Place::Index`, rather than
base + one projection) or a writeback like `lower_mutation_receiver`'s. Hono
itself is unaffected — its `Node<T>` is classified as a reference class — but
any value-class program with this shape gets a wrong answer with no diagnostic,
which makes H31 the most serious entry in this file.

## H32 — a labeled statement is not lowered

`src/router/linear-router/router.ts` uses `ROUTES_LOOP: for (…) { … continue
ROUTES_LOOP }`; `statement kind is not lowered yet: LabeledStatement`. It is
OUTSIDE the campaign's in-scope closure (`src/index.ts` reaches the smart router,
not the linear one), which is why phase 1 reports zero blockers, and the probe
manifest above drops that entry to get past it. Still a plain language feature
with no support.
