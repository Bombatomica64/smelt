# H18 — source setters, the private-name namespace, and the last two source blockers

Round 9, Hono implementer. This note covers the four rules that took the probe
from **258 files / 1 with blockers** to **258 / 0**, each unmasked by the one
before it.

## 1. A source `set` accessor lowers (`context.ts:414`)

```ts
set res(_res: Response | undefined) { ... }
```

was `setters are not lowered yet` — a hard blocker at the class declaration, so
the whole file could not lower. Everything needed already existed:
`class_function_impl` already prefixes an accessor method (`__smelt_get_x` /
`__smelt_set_x`) and already types a setter's return as `None`; the MIR
descriptor carries a `setter` slot; and codegen's `descriptor_setter_statement`
already emits the call for a field write. Only the SOURCE-level registration was
missing — a plain getter pushed a `Descriptor { getter: Some(..) }` and a setter
pushed nothing.

Now a getter and a setter of the same name build ONE descriptor carrying both
halves, in either source order (`read_ty` from the getter's return, `write_ty`
from the setter's parameter). A set-only property is a descriptor with no
getter; its `read_ty` mirrors the written type rather than interning `Unknown`
(see the interning note below).

## 2. A private name is its own namespace

The parser hands the bare text for `#res` and for `res`, and both interned to
one symbol. That aliased three genuinely different members of Hono's `Context`:
`#res` / `get res()` / `set res()`, and also `#newResponse` (method) beside
`newResponse` (public field arrow). Consequences, in order of discovery:

* the accessor pair was shadowed by the private slot
  (`descriptor_for_field`'s instance-data-property rule), so a read skipped the
  getter's `||=` lazy initialization and a write skipped the setter;
* once setters lowered, `set res(v) { this.#res = v }` dispatched its own body's
  write back into itself: **stack overflow** in the generated crate;
* `this.#newResponse(...)` could resolve to the public `newResponse` arrow whose
  body calls the private method — unbounded recursion.

A private name now interns as `#res` (`intern_private_name`), so its symbol
identity carries the `#`. The Rust rendering is the sanitized form (`_res`), so
a private slot and a public property of the same source name are distinct struct
fields. Three interning sites: the class member key (`property_key_symbol`),
private field reads and writes (`private_field_member`), and the private
member-call name in `dispatch_call`.

Blast radius, measured before doing it: es-toolkit and remeda contain no `this.#`
at all, `examples/` has one fixture, and the affected tests are behavioural
runtime tiers rather than name snapshots — so no golden churn.

`place.rs`'s shadowing rule additionally now ignores fields that are
`Visibility::Hidden` ("not an own property at runtime" — a private name or a
synthesized storage slot). With distinct symbols that is belt-and-braces for
private names, and correct in its own right for the synthesized slots.

## 3. Calling a private FIELD that holds a function (`request.ts`, `context.ts`)

```ts
#cachedBody = (key: keyof Body) => { ... }
this.#cachedBody('text')            // request.ts, six call sites
#renderer: Renderer | undefined
this.#renderer(...args)             // context.ts:450
```

A private name can denote a method OR a field, and a field can hold a closure.
The public spelling has always been lowered as a call of a FIELD
(`callable_static_member_call`); the private spelling had no such path, so it
lowered as `ExprKind::Method` and MIR failed with `class method '#cached_body'
is not resolvable` — there is no method of that name. **This was not caused by
(2): the same failure reproduces with the pre-change bare name**, it was simply
masked while `context.ts` and `request.ts` still had frontend blockers.

`private_callable_field_call` mirrors the public path: the receiver's class must
declare a FIELD of that private name (a private name is never both a method and
a field, so the field's existence is the whole discriminator — routing a private
METHOD here would replace its direct receiver-first call with the erased
callable-value ABI), and then the callable is extracted with the same
`function_member_type` the public path uses, so `Renderer | undefined` works.
When the field's type carries no callable at all — Hono's `Renderer` resolves
through `ContextRenderer extends Function ? ... : ...`, which lowers to
`unknown` — the ABI is synthesized from the argument types, again exactly as the
public path does for the same shape. The receiver is restricted to an identifier
or `this`, the only shapes whose lowering has no side effect, because the method
fallback lowers the receiver again.

## 4. `for...in` over a union of record-like arms (`context.ts:629`, `:637`)

`HeaderRecord` is three `Record<..., ...>` arms and `for (const k in headers)`
was rejected with `for...in is only lowered for record-like objects`. `for...in`
yields property NAMES, and each arm spells its names with the same key type, so
the projection is the same operation whichever arm the value holds — there is
nothing to narrow. The arms differ only in their VALUE types, which the loop
never reads (`headers[k]` is a separate, union-typed expression). Lowered
through the same key projection the erased-receiver arm uses, guarded on every
arm being a `Dict` with a `String` key.

## One trap worth recording: interning `Type::Unknown` is crate-wide

The first version of (1) interned `Unknown` once per class method to use as the
"no annotation" fallback. That single extra entry in the type table flipped
crate-wide erasure decisions — `emits_missing_class_record_reads_as_optional_values`
started emitting `.get(&key)` instead of `.get(&key).cloned()` for a crate with
no erased value anywhere. It is the same hazard H14 names. `Unknown` is now
interned only on the branch that genuinely has no type.

## Test

`crates/smelt-codegen-rust/tests/class_accessor_runtime.rs` (new tier, `host`
shard of `.github/workflows/runtime-tiers.yml`): a write running the setter body
with a `#name` slot beside the accessor pair (the Hono shape), a setter writing
through another accessor, per-instance state, a setter that transforms/validates
and keeps side state, and a set-only property. `private_member_call_runtime`
covers the private-field-arrow and private-method calls.

`crates/smelt-frontend-ts/src/tests/part06_tests.rs` no longer asserts the
setter rejection; it asserts the getter/setter/private-slot class lowers and
validates.

```sh
cargo test -p smelt-codegen-rust --test class_accessor_runtime -- --ignored
cargo test -p smelt-codegen-rust --test private_member_call_runtime -- --ignored
```

## Probe, and what phase 2 looks like from here

**258 files scanned, 0 with blockers.** Phase 1's metric (14 -> 0) is met.

`smelt check` on the whole manifest is clean. The whole-crate BUILD is not: with
the frontend blockers gone, MIR lowering now reaches a chain of gaps that were
never reachable before. In order encountered and fixed this round:
`#cachedBody` (item 3), `#renderer` (item 3 again, erased callable). The next
one, NOT fixed, is

```text
field and index reads currently require a local receiver
  third_party/hono/src/router/trie-router/node.ts:  for (const child of node.#patterns)
```

a private field read whose receiver is not a local — a MIR limitation unrelated
to naming (it reproduces for the public spelling of the same shape). Phase 2 —
`cargo check` of the generated non-test crate — cannot start until the crate can
be emitted, so it is not reached in this round: the emitter never runs.

## Also found, NOT fixed

* **A source class named `Box` (or any prelude name) collides in the generated
  crate.** A fixture class `Box` made `type SmeltPromiseFuture = Pin<Box<dyn
  Future<..>>>` resolve `Box` to the user's struct: E0107, dozens of times, in
  the prelude. Generated user types need to be kept out of the prelude's
  namespace (a module, or a name mangle). The tier's fixture was renamed to
  `Gauge` to work around it.
* **A union of `Record` arms erases at the ABI.** `function keysOf(headers:
  HeaderRecord)` emits `headers: SmeltUnknown` — the union of dicts has no
  concrete Rust spelling, so it erases at the parameter. Item 4 above only fixes
  the key iteration; the parameter type is a separate, pre-existing erasure.
* **An inline anonymous object type lowers to `Record<string, union>`** — see
  `blocker-logs/hono-h16-nested-callback-destructuring.md`; silent wrong value.
