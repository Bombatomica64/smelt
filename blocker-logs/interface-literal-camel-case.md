# An object literal against an interface destination drops every camelCase field

**Severity: silent wrong value.** Not erasure, not a compile break — the
program runs and prints the wrong answer. Found while implementing typed
`Response`/`Request` inits (standards round 4, item 1); it blocked the
source-interface half of that item's runtime tier, which now uses single-word
keys and points here.

Not fixed by the standards stream: it belongs to the object-literal-against-an-
interface-destination family already assigned to the Hono stream (the empty
literal `const c: Config = {}` erasing to a record is the same lowering site).

## Repro

`src/main.ts`:

```ts
interface Shape {
  plain?: number;
  camelCase?: string;
  statusText?: string;
}

const value: Shape = { plain: 1, camelCase: "a", statusText: "b" };
console.log(value.plain);
console.log(value.camelCase);
console.log(value.statusText);
```

`smelt build` succeeds. The generated program:

```rust
fn main() {
    let _smelt_tmp_1: Shape = Shape { plain: Some(1.0), camel_case: None::<String>, status_text: None::<String> };
    let value: Shape = _smelt_tmp_1;
    let _ = { println!("{}", match &value.plain.clone() { Some(value) => format!("{}", value), None => "undefined".to_owned() }); };
    let _ = { println!("{}", match &value.camel_case.clone() { Some(value) => format!("{}", value), None => "undefined".to_owned() }); };
    let _ = { println!("{}", match &value.status_text.clone() { Some(value) => format!("{}", value), None => "undefined".to_owned() }); };
    return;
}
```

The load-bearing line is the struct literal: **`camel_case: None::<String>` and
`status_text: None::<String>`**, though the source set both. Only the
single-word `plain` survives.

| | Node 22 | Smelt |
| --- | --- | --- |
| `value.plain` | `1` | `1` |
| `value.camelCase` | `a` | `undefined` |
| `value.statusText` | `b` | `undefined` |

## Where it is

`Rvalue::Struct`'s emitter matches the literal's field operands to the class's
declared fields **by symbol**
(`crates/smelt-codegen-rust/src/emitter/call_runtime.rs`, the
`Rvalue::Struct { class, fields }` arm):

```rust
if let Some((_, field_value)) = fields
    .iter()
    .find(|(field_name, _)| *field_name == field.name)
{
    parts.push(format!("{name}: {}", self.operand_text(field_value)?));
} else {
    parts.push(format!("{name}: {}", self.default_value_with_scoped_type_params(..)?));
}
```

The `else` branch is what produces `None::<String>`: no operand matched, so the
field takes its default. So the literal's key symbol and the declared field's
symbol are **not equal** for a camelCase name, and the emitter cannot tell "the
source omitted this key" from "the key is spelled differently here" — both
reach the same branch.

`intern_source_name` renders a source name to snake_case while recording the
original (`intern_rendered(name, &camel_to_snake(name))`), so a declared field
`statusText` interns as `status_text`. The literal's keys must be interned
through a different path that keeps the original spelling, so the two symbols
differ. That is the thing to find and unify.

## Why it is worth its own report

The same key survives one path and not the other, which narrows the fix:

* through a **concrete struct** (this bug) — dropped;
* through an **erased record** — kept. The ambient `ResponseInit` fixtures in
  `crates/smelt-codegen-rust/tests/fetch_init_runtime.rs` read `statusText`
  successfully, because an ambient interface has no runtime struct so the value
  crosses as a record whose keys keep their source spelling.

So the record path already agrees with the source; only the struct path
disagrees. Any interface with a multi-word optional field is affected, which is
most real interfaces.

## Suggested guard once fixed

An emitter that falls back to a field's default because no operand matched
cannot currently distinguish an omitted key from a mismatched symbol. Making
that branch assert that the field is genuinely absent from the literal — rather
than silently defaulting — would have turned this into a build failure instead
of a wrong value.
