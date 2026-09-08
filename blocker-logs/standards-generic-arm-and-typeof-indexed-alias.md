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
