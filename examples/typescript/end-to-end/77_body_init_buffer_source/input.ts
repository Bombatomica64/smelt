// `BodyInit`'s remaining arms, now that the byte family is concrete.
//
// `new Response(arrayBuffer)` and `new Request(url, { body: view })` used to
// stop the whole crate in the EMITTER — "body must be a string or null; this
// `BodyInit` arm is not modeled yet: SmeltArrayBuffer" — because the arms were
// only reachable as erased host records before, and a concrete one had no
// conversion. Every arm the spec lists now extracts a body:
//
// * a `BufferSource` (an `ArrayBuffer` or any of the eleven element views)
//   contributes its BYTES, and a view contributes its OWN WINDOW rather than
//   the whole buffer, which is what makes `new Response(view.subarray(1, 3))`
//   two bytes long;
// * a `URLSearchParams` contributes its query string with the content type the
//   spec fixes for it;
// * a `FormData` contributes `multipart/form-data`, whose boundary is part of
//   the content type rather than of the bytes — so the encoder mints one and
//   the header carries it, which is what makes the round trip through the
//   parser that already existed work;
// * a `Blob` already did (its bytes plus its own type).
//
// Neither `BufferSource` arm sets a content type: the spec's "extract a body"
// step leaves it unset for one, unlike a blob or a form.
//
// The two BYTE readers came with them. `arrayBuffer()` answers storage and
// `bytes()` an element view, which is one spec member each and a distinction
// only observable since the family became concrete — `ArrayBuffer.isView`
// separates them.

async function run(): Promise<void> {
  const buffer = new ArrayBuffer(4);
  const bytes = new Uint8Array(buffer);
  bytes[0] = 104;
  bytes[1] = 105;
  bytes[2] = 33;
  bytes[3] = 63;

  // Byte storage as a body, read back as storage.
  const fromBuffer = new Response(buffer);
  console.log((await fromBuffer.arrayBuffer()).byteLength);

  // A VIEW as a body contributes its own window, not the buffer's bytes.
  console.log(await new Response(bytes.subarray(0, 2)).text());
  const window = await new Response(bytes.subarray(1, 3)).bytes();
  console.log(window.length, window[0], ArrayBuffer.isView(window));

  // A wider view contributes the bytes it spans, not its element count.
  const wide = new Uint32Array([1]);
  console.log((await new Response(wide).arrayBuffer()).byteLength);

  // A `BufferSource` body sets no content type. Compared against `null`
  // rather than printed: `Headers.get` answers `string | null`, which Smelt
  // models as an `Optional<String>`, and printing an absent one gives
  // `undefined` where Node gives `null` — a pre-existing divergence in how a
  // NULL-origin optional prints, unrelated to the body arms, recorded rather
  // than papered over.
  console.log(new Response(buffer).headers.get("content-type") === null);

  // A `URLSearchParams` body is its query string, with the spec's type.
  const params = new URLSearchParams();
  params.append("a", "1");
  params.append("b", "two");
  console.log(await new Response(params).text());
  console.log(new Response(params).headers.get("content-type"));

  // A `FormData` body round-trips through the multipart parser, and its
  // content type carries the boundary the encoder minted.
  const form = new FormData();
  form.append("name", "smelt");
  form.append("kind", "codec");
  const parsed = await new Request("https://a.test", { method: "POST", body: form }).formData();
  console.log(parsed.get("name"), parsed.get("kind"));
  const formType = new Request("https://a.test", { method: "POST", body: form }).headers.get("content-type") ?? "";
  console.log(formType.startsWith("multipart/form-data; boundary="));

  // A `Blob` body still contributes its own type, which the byte arms do not.
  const blob = new Blob(["hi"], { type: "text/plain" });
  console.log(new Response(blob).headers.get("content-type"));
  console.log((await new Response(blob).bytes()).length);

  // Both readers on a request, and both consume the body.
  const request = new Request("https://a.test", { method: "POST", body: bytes });
  const asBuffer = await request.arrayBuffer();
  console.log(asBuffer.byteLength, ArrayBuffer.isView(asBuffer));
  const second = new Request("https://a.test", { method: "POST", body: bytes });
  console.log((await second.bytes()).length, second.bodyUsed);
}

run();
