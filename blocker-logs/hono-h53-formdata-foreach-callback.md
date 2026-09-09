# H53 — `FormData.forEach` gives its callback (value, INDEX), not (value, key)

Hono blocker 5 of the post-import-closure list, **diagnosed and not taken**:
the cause is inside the FormData model, which the standards stream is finishing
this round, so touching it here would collide.

## The blocker as reported

```
src/utils/body.ts   string prefix/suffix methods require string receiver and argument
```

Reported at `body.ts:168`, inside `convertFormDataToBodyData`:

```ts
formData.forEach((value, key) => {
  const shouldParseAllValues = options.all || key.endsWith('[]')
  ...
})
```

The message points at `endsWith` and reads like a union/optional receiver
problem. It is not: `key` is lowered as a NUMBER.

## Cause

`formData.forEach(cb)` does not go through the FormData model's own `ForEach`
op. It is lowered as an ARRAY iteration: the receiver is asserted to
`List<Unknown>` and the callback's second parameter becomes the loop INDEX. The
HIR for a four-line repro shows it plainly:

```
#7:  FormData = %0
#8:  List<Unknown> = assert_type #7        <- receiver erased to a list
#14: Float = %3                            <- `key` is the index
```

So `key.endsWith('[]')` asks for a string method on an `f64` receiver, and the
affix lowering correctly refuses. Minimal repro, no Hono involved:

```ts
const formData = new FormData();
formData.append('a[]', '1');
formData.forEach((value, key) => {
  console.log(key.endsWith('[]') ? 'list' : 'single');
});
```

WHATWG spells the callback `(value, key, parent)`, and
`crates/smelt-frontend-ts/src/lowering/stdlib/form_data.rs` already models
`FormDataOp::ForEach` with `form_data_value_type()` for the value — the
`forEach` CALL just is not being claimed by that path, so it falls through to
the generic array-iteration lowering.

## Why it is not taken here

`form_data.rs` is the file the standards agent is working in right now
("finish FormData"). The fix belongs with the rest of that model: claim
`forEach` for `FormDataOp::ForEach` and give the callback the spec's three
parameters, with the value typed `string | File` (the union
`form_data_value_type` already interns) and the key typed `string`.

## Note on the reported message

Worth keeping in mind beyond this blocker: the diagnostic named the WRONG layer.
Nothing about `endsWith` is unimplemented; a mis-typed callback parameter three
frames away produced a plausible-looking stdlib complaint. When an affix/search
lowering refuses a receiver, the receiver's provenance is the thing to print.
