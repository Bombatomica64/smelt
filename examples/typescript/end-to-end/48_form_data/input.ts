// `FormData` is a concrete ordered pair list whose entry values are the real
// union `string | File` — no tagged record, and no runtime member lookup.
// Names are case-SENSITIVE, unlike a `Headers` list's.
const form = new FormData();
form.append("a", "1");
form.append("a", "2");
form.append("b", "x");
console.log(form.get("a"));
console.log([...form.getAll("a")].length);
console.log(form.has("b"), form.has("B"));

// `set` replaces the first entry and drops the rest, keeping its POSITION.
form.set("a", "9");
console.log([...form.keys()].join(","));
for (const [name, value] of form.entries()) {
  console.log(name, value);
}
form.delete("a");
console.log([...form.keys()].join(","));
console.log(form.get("a") ?? "absent");

// A blob value becomes a File: named `blob` when no filename is given, and
// named by the third argument when one is. A File keeps its own name.
const withFiles = new FormData();
withFiles.append("plain", new Blob(["hello"], { type: "text/plain" }));
withFiles.append("named", new Blob(["hello"], { type: "text/plain" }), "note.txt");
withFiles.append("file", new File(["bye"], "given.txt", { type: "text/plain" }));
for (const entry of withFiles.values()) {
  if (typeof entry === "string") {
    console.log("text", entry);
  } else {
    console.log("file", entry.name, entry.type, entry.size, await entry.text());
  }
}

// Reading a body as a form: the `Content-Type` header picks the parser.
const urlencoded = new Response("a=1&b=hello+world&a=2", {
  headers: { "content-type": "application/x-www-form-urlencoded" },
});
const parsed = await urlencoded.formData();
for (const [name, value] of parsed.entries()) {
  console.log(name, value);
}
console.log(urlencoded.bodyUsed);

const boundary = "----SmeltBoundary";
const multipart = [
  "--" + boundary,
  'Content-Disposition: form-data; name="title"',
  "",
  "hello",
  "--" + boundary,
  'Content-Disposition: form-data; name="doc"; filename="a.txt"',
  "Content-Type: text/plain",
  "",
  "file body",
  "--" + boundary + "--",
  "",
].join("\r\n");
const request = new Request("https://forms.test/upload", {
  method: "POST",
  headers: { "content-type": "multipart/form-data; boundary=" + boundary },
  body: multipart,
});
const uploaded = await request.formData();
for (const [name, value] of uploaded.entries()) {
  if (typeof value === "string") {
    console.log(name, "text", value);
  } else {
    console.log(name, "file", value.name, value.type, value.size, await value.text());
  }
}

// A `Content-Type` naming neither encoding is the spec's `TypeError`, and a
// body is single-use whichever reader consumed it.
async function readOrReport(response: Response, label: string): Promise<void> {
  try {
    await response.formData();
    console.log(label, "read");
  } catch (error) {
    console.log(label, "threw");
  }
}
await readOrReport(
  new Response("plain", { headers: { "content-type": "text/plain" } }),
  "wrong type",
);
await readOrReport(urlencoded, "second read");
