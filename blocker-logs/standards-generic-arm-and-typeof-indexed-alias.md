# Standards round 19: the generic union arm, and what actually stops Hono's `context.ts`

Two separate defects sit behind the one symptom the demand loop reported
(`new Headers(arg.headers)` erasing at `src/context.ts:612`). The first was the
one the round's brief named and is fixed. The second is the one that actually
stops the build, and it is not the generic arm.

## 1. The generic record's instantiated field types (fixed)

A generic interface inside a union — Hono's
`interface ResponseInit<T extends StatusCode>` in
`StatusCode | ResponseInit<T> | Response` — generates its arm payload at the
INSTANTIATED type (`InitLike<f64>` in the reduced case), while two other places
kept the DECLARATION-time field type (`status?: T`, which erases):

* the record-to-struct adapter that rebuilds the arm from an erased object
  (`string_dict_record_adapter_text`), reached from the union's
  `from_smelt_unknown`;
* the field read itself (`emitter::types::place_ty`).

The two renderings of the same field then disagreed:
`expected Option<f64>, found Option<SmeltUnknown>` (E0308).

The root cause of the first is subtle and general: the emitter holds `&Mir` and
can only LOOK UP a substituted type (`existing_type_id`). `Optional<Float>` for
`status?: T` at `T = f64` existed in the type table only if something else in
the program happened to spell it; otherwise substitution silently answered the
UNSUBSTITUTED type, and no diagnostic fired. The fix is a MIR finalization pass
(`lower::passes::generic_records`) that interns the substituted field types of
every instantiated generic record in the program, following base classes and
extended interfaces, to a fixpoint. It only adds types, so no existing id
changes meaning. `place_ty` additionally substitutes the receiver's type
arguments the way `structural_record_fields` already did.

Fixture: `63_generic_init_union_arm` (record round-trip through the arm, plus a
typed read of the generic field, Node-verified).

## 2. What still stops `context.ts`: an unresolved `(typeof X)[keyof typeof X]` alias

With (1) fixed, the Hono probe still stops at the same place. The reason is
`ResponseHeadersInit`:

```ts
type ResponseHeadersInit =
  | [string, string][]
  | Record<'Content-Type', BaseMime>
  | Record<ResponseHeader, string>
  | Record<string, string>
  | Headers
```

and, in `src/utils/mime.ts`:

```ts
export type BaseMime = (typeof _baseMimes)[keyof typeof _baseMimes]
```

`BaseMime` is an INDEXED ACCESS over a `typeof` query, whose resolved value is
the union of the const object's value types — all string literals, i.e.
`string`. Smelt does not resolve that spelling and lowers the reference to a
nominal `Type::Class { name: BaseMime }`, which `is_erased_class_type` reports
as erased. So the arm `Dict<String, Class(BaseMime)>` is not concrete,
`union_member_is_concrete` fails for it, `concrete_union_members` answers
`None` for the whole `ResponseHeadersInit` union, and the value the union-arm
member read correctly produced — measured as
`Optional<Union[Headers, Dict<String, Class(BaseMime)>, Dict<String, String>, List<Tuple<String, String>>]>`
in the frontend — arrives at the emitter erased. `headers_conversion_text` then
refuses it, and the blocker prints the type as `SmeltUnknown`.

Minimal repro (fails), 16 lines, no Hono:

```ts
const mimes = { json: "application/json", text: "text/plain" } as const;
type Mime = (typeof mimes)[keyof typeof mimes];
type HeadersInitLike =
  | [string, string][]
  | Record<string, Mime>
  | Record<string, string>
  | Headers;

function headerOf(init: HeadersInitLike, name: string): string {
  const copied = new Headers(init);
  return copied.get(name) ?? "none";
}

console.log(headerOf({ "content-type": mimes.json }, "content-type"));
console.log(headerOf([["x-a", "1"]], "x-a"));
```

Node prints `application/json` / `1`. Smelt:

```
Error: EmitError { message: "`new Headers(init)` initializer type is not modeled: SmeltUnknown (initializer `init.clone()` in `header_of` ...)" }
```

Writing the same alias as the literal union it denotes
(`type Mime = "application/json" | "text/plain"`) transpiles and runs. So the
whole defect is the alias's resolution, not the union, not the `Headers`
conversion, and not the generic arm.

Two things to fix, in order of value:

1. **Resolve the spelling.** `keyof typeof O` over a source `const` object with
   an inferred literal type, and an indexed access over `typeof O`, both have
   statically knowable answers: the keys are the object's own property names and
   the values are its property types. That resolution is general — it is the
   "resolve the full alias path" rule, not a Hono special case — and it makes
   `BaseMime` the `string` it denotes.
2. **Do not lower an unresolved type reference to a nominal class.** A nominal
   class is the one shape that turns "I could not resolve this alias" into
   erasure of every union that contains it, silently and at a distance. An
   unresolved reference should either be a named blocker at the reference or
   resolve through the alias path; inventing a nominal class for it is what made
   a five-arm concrete union erase whole.

## Also in this round

* `super(options?.message)`: `Error.message` is a `string` and
  `new Error(undefined).message` is `""`, so an optional message forwarded to
  `super` is a DEFAULTING. `ERROR_MARKER_FIELDS` now carries each inherited
  slot's spec default and the optional case coalesces against it, so the
  absent-argument path and the supplied-but-optional path write the same value.
  Fixture `64_error_subclass_optional_message`; before it, every `Error`
  subclass constructed without a message panicked with "optional value was
  absent after narrowing".
* `cause: SmeltUnknown` is now a legitimate boundary in `classify_line`
  (`cause?: unknown` is the canonical source-level `unknown`), which is what
  lets an `Error` subclass live in the zero-erasure examples corpus at all.


---

# Round 20 follow-up: the cause was upstream of the alias, and (b) rejects valid code

## What actually dropped `BaseMime`

Round 19 read the symptom right (`Class(BaseMime)` erasing the whole
`ResponseHeadersInit` union) and the mechanism right (nominal stand-in ->
erased member -> non-concrete union), but the CAUSE was one level further up:
`src/utils/mime.ts` was never in the crate.

`typescript_import_statements` (`crates/smelt-transpiler/src/manifest.rs`)
buffered a multi-line import until a line ended with `;`. Hono is formatted
without semicolons, so that line never comes and the buffer swallowed the rest
of the file. Measured on `src/context.ts`: 3 of its 6 import statements were
scanned — everything after the multi-line `import type { ... } from './types'`
was lost, including `./utils/mime` (type-only) AND `./utils/html` (a value
import). Those modules never entered the dependency closure, so their aliases
were never predeclared, so `BaseMime` had nothing to resolve against.

Fixed, with `a_multiline_import_without_semicolons_does_not_swallow_later_imports`
pinning Hono's own header shape.

## The alias itself

`typeof <binding>` now resolves to the type of the value it names (const item,
module binding, or function signature). `keyof T` was already `string` and an
indexed access over a `Dict` was already its value type, so the query was the
only missing link; `(typeof mimes)[keyof typeof mimes]` is now `string`, in the
same file or across modules. Fixture `65_typeof_keyof_alias_union`.

## Item 1(b) as written rejects valid TypeScript — three measured cases

"An unresolved type reference must never lower to a nominal `Type::Class`; it
is a named blocker naming the alias" was implemented and measured against the
corpora. It rejects programs `tsc` accepts, in three distinct ways:

1. **A cyclic type-only import of a class that IS in the crate.** Hono's
   `src/types.ts` does `import type { Context } from './context'` while
   `context.ts` imports `types.ts`; `types.ts` lowers first, so at the
   reference the class is not registered yet even though it will be. The
   blocker fires 7x on `Context`/`HonoBase` in `src/types.ts` alone. The
   whole-crate alias PREPASS covers type aliases but not classes/interfaces,
   which is why aliases survive this and classes do not.
2. **A types-only external package.** remeda's
   `internal/types/UpsertProp.ts` imports `Simplify` from `type-fest`, which is
   not a crate module at all and never will be. Blocking it stops remeda at the
   first such file.
3. **The documented `[sources] exclude` contract.** The compat overlays state
   that "type-only imports and `export type` re-exports from an excluded module
   stay free — excluding a module removes its implementation, not its type
   surface". A blanket blocker withdraws that guarantee, so every exclude would
   have to grow a type-shim.

So the fallback stays, and the guard is NOT landed. The narrow form that would
be safe — and would still have caught `BaseMime` — is: blocker only when the
specifier is RELATIVE, resolves to a file that is in the manifest's source set,
is not excluded, and declares nothing of that name. That needs a crate-wide
"module is in the source set" fact in `HirCtx` (the per-module export table
cannot answer it, because a cyclic import has not recorded its exports yet).
Cheap to build in the existing prepass; it is a decision for the next round
rather than something to slip in, since it changes what the corpora accept.

## Hono's whole-crate stop now

`smelt build` on `third_party/hono` at the pinned ref with the committed
compat overlay:

    Error: Custom { kind: InvalidData, error: "…/src/utils/crypto.ts:\n[\n
      ManifestDiagnostic { file: \"…/src/utils/crypto.ts\",
        category: UnsupportedLowering, code: \"smelt::unsupported-ts\",
        message: \"JSON.stringify() value must be JSON-serializable
          (got Some(Union([TypeId(29), TypeId(196), TypeId(197), TypeId(531),
          TypeId(533)])))\" } ]" }

Two things to note. The stop moved forward twice this round: at this merge head
it was `src/utils/cookie.ts` (`unresolved identifier crypto`, `atob`), which is
what the head answers with these changes reverted; the closure fix pulls in the
modules that were previously dropped, which both reveals more of the crate and
reorders what is reached first. And `context.ts:612` is no longer the stop —
but that is not yet proof the Headers erasure is gone, because the frontend now
aborts before the emitter runs. Probing `src/context.ts` as its own entry is
NOT a valid check either: with a single-file entry the closure is 12 modules and
`utils/mime.ts` is legitimately absent, so the probe reproduces the erasure by
construction.

That JSON.stringify blocker also prints raw `TypeId`s instead of the union's
member types, which is the same "name the culprit" problem this note is about.
