# TS7 parity, phase 2, round 3: follow-ups from round 2

Branch `feat/hono-url-model`. This round closes the four items round 2 listed
as found and not fixed
(`ts7-parity-phase2-round2-hono.md`, "Found and not fixed"), plus three
defects found on the way. Every rule has an end-to-end example whose output
is compared against Node.

Hono v5 `0f5ad7b6`: **413 passed / 41 failed** of 454. This is unchanged from
round 2, and no test that passed fails. `newResponse` is now typed fixed
parameters instead of an erased list.

## The four round-2 items

1. **A class method with a rest parameter** (`count(...args: T[])`) failed:
   "unresolved identifier `args`".
   - `class_function_impl` never bound the rest parameter.
   - `method_signature_function_type` left it out of the method's type and
     recorded `rest` at the last FIXED parameter.
   - The `MethodSig` sidecar had no `rest` either.
   - All three now carry it. Instance and static method calls pack the tail
     into the list, and pass a single list spread through.
   - Example 176.
2. **A list spread into a call on an overloaded callable value**
   (`ctx.respond(...xs)` through a getter) passed the list as parameter 0.
   - The function-typed field path now picks the signature without an arity
     and distributes the list, as the other fixed-arity paths do.
   - So does `new Ctor(..)(..)`.
   - Example 176.
3. **`string | null` printed `undefined`.** `Option<T>` has one absent case.
   - The user decided: keep `Option<T>`. A type that admits BOTH `null` and
     `undefined` stays a known limit (memory: strict types over JS nullish).
   - For a type that admits ONLY `null`, the TS7 checker's type of the
     expression decides how the absent case reads (`lowering/null_absent.rs`):
     - string conversions (`${x}`, `String(x)`, `+`, and `console.log` of a
       string or boolean) read it as `"null"`;
     - `x === undefined` folds to `false`.
   - An interface field declared `p: T | null` erases its absent case to
     `null` (from `FieldDecodeFacts`), so `JSON.stringify` prints `"p":null`.
   - Example 176.
4. **Tuple rest parameters** (`lowering/tuple_rest.rs`). A rest parameter
   typed as a fixed-width tuple is lowered as FIXED PARAMETERS, and the body
   binds the rest name to their tuple: `fn pair(args__0: String, args__1:
   Option<f64>)` with `let args = (args__0, args__1)`.
   - It applies to annotations and function-type annotations, call and
     method signatures, module predeclarations, functions, methods, arrows
     and callback closures.
   - An unannotated rest in a fixed-arity slot takes the tuple of the slot's
     remaining parameters, which is what TypeScript gives it. That tuple, or
     the checker's, is used only when every element keeps a type. Splitting
     an erased context (`(x: any, y: any)`) adds erased values and buys
     nothing.
   - Supporting changes:
     - a tuple asserted to a list type keeps the tuple;
     - a spread sees through an assertion to the operand's own tuple width;
     - the emitter reads `tuple?.[i]`. It used to answer `None` for a tuple
       receiver.
   - Example 177.

## Found on the way and fixed

- A static method named like an array builtin (`Bag.join(..)`) was claimed
  by `Array.prototype.join` lowering.
- An object type literal's record value type was the raw member union.
  `{ a: string; b?: string }` gave `String | Optional<String>`, which erased
  every record of the alias. It is now the flat `Optional<String>`.
- A user class named after a Rust prelude item (`Box`, `Vec`, `Option`,
  `Result`, `String`, `Some`, `None`, `Ok`, `Err`) shadowed it for the whole
  crate (E0107). Such names are renamed like keywords.

## Ratchets

| baseline | before | after |
| --- | ---: | ---: |
| examples SmeltUnknown avoidable (invariant) | 0 | 0 |
| es-toolkit SmeltUnknown avoidable | 22,341 | 22,338 |
| examples typedness | | two new programs (176, 177) |
| es-toolkit typedness | | one record updated by hand (below) |

`src/function/memoize.ts`, values: erased 32 to 33. Updated by hand, with the
user's approval:

- `{ cache?: X; getCacheKey?: fn }` record values are now `Option<..>`, so
  presence is an `Option` instead of an `Undefined` sentinel.
- The narrowed `getCacheKey(arg)` call unwraps that `Option` into one more
  erased temp.

## Still open

- **Object-shaped type aliases** (`type R = { a: string }`) are referenced as
  `Class { R }` but only classes and interfaces become structs. So the alias
  is an erased record. In a crate with no other erasure, reading `r.b` then
  aborts the build ("cannot type the read of `b` on `SmeltUnknown`").
  Interfaces work. This is a broad erasure family (es-toolkit's
  `TruncateOptions` is one instance).
- **Mount (13 Hono failures).** `(c.req as unknown)[GET_MATCH_RESULT]`
  hides a symbol-keyed getter behind a type-only `unknown`.
- `T | null | undefined` cannot distinguish its two absent cases (by design;
  see item 3).
