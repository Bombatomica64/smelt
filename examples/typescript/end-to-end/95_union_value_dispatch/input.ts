// A value whose static type is a GENERATED union is dispatched through its own
// enum arms, never through the `SmeltUnknown` runtime-shape `match`.
//
// A union whose arms are all concrete lowers to a tagged `SmeltUnion…` enum, so
// the tag already says which arm is live. Two emitter paths used to ignore that
// and emit the erased `match value { SmeltUnknown::String(..) => .. }` form
// against the enum, which type-checks against nothing:
//
//  * a `BodyInit` at the body position (`string | Uint8Array` below): each arm
//    names a concrete type, so each takes that type's own extraction rule — a
//    string arm is text, a buffer-source arm contributes its bytes;
//  * a dotted property write through a union-typed receiver: when every arm
//    declares the property, the write is dispatched on the tag and each arm
//    performs its own member's typed struct-field assignment, so the value
//    never has to be erased to reach the slot.

// Every arm of this `BodyInit` is concrete, so the conversion is per arm rather
// than a runtime shape test.
async function bodyText(init: string | Uint8Array): Promise<string> {
  return await new Response(init).text();
}

// Both arms declare `tag`, so `value.tag = next` is a typed field write inside
// each arm of the generated enum.
interface Keyed {
  tag: string;
  kind: string;
}

interface Noted {
  tag: string;
  note: string;
}

type Tagged = Keyed | Noted;

function retag(value: Tagged, next: string): string {
  value.tag = next;
  return value.tag;
}

async function run(): Promise<void> {
  console.log(await bodyText("hello"));
  console.log(await bodyText(new Uint8Array([104, 105, 33])));
  const counted: Keyed = { tag: "first", kind: "k" };
  const noted: Noted = { tag: "third", note: "n" };
  console.log(retag(counted, "second"));
  console.log(retag(noted, "fourth"));
}

await run();
