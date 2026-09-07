// `Blob` and `File` used to be marker-bearing records whose only observable
// members were `size` and `type`. They are now byte-backed concrete values with
// the spec's readers, and a `File` is a `Blob` whose name is present.
const blob = new Blob(["hello, ", "world"], { type: "text/plain" });
console.log(blob.size);
console.log(blob.type);
console.log(blob instanceof Blob);
console.log(blob instanceof File);

const file = new File(["report"], "report.csv", {
  type: "text/csv",
  lastModified: 42,
});
console.log(file.size);
console.log(file.type);
console.log(file.name);
console.log(file.lastModified);
console.log(file instanceof Blob);
console.log(file instanceof File);

// `slice` answers a Blob — never a File — and takes the content type from its
// own third argument rather than from the source.
const head = blob.slice(0, 5);
console.log(head.size, head.type, head instanceof File);
const tail = blob.slice(-5);
console.log(tail.size);
const retyped = blob.slice(0, 5, "text/html");
console.log(retyped.type);
// An inverted range is an empty blob, not an error.
console.log(blob.slice(9, 2).size);

// A nested blob part contributes its bytes. Blob parts here, string parts
// above: `BlobPart` is a union, and each modeled arm keeps its own type
// through to the constructor rather than routing through the erased walk.
const nested = new Blob([blob, new Blob(["!"])]);
console.log(nested.size);
console.log(nested.type);

// The body readers are async even though the bytes are already in memory.
console.log(await blob.text());
console.log(await head.text());
console.log(await nested.text());
console.log((await blob.arrayBuffer()).byteLength);
console.log((await file.bytes()).length);

const empty = new Blob([]);
console.log(empty.size, empty.type, await empty.text());

// Multi-byte content: `size` is a BYTE count, and the round trip is lossless.
const unicode = new Blob(["héllo 😀"]);
console.log(unicode.size);
console.log(await unicode.text());
