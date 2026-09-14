// A module-level `const` holding a modeled host value, read from FUNCTIONS
// rather than only from the module body.
//
// This is the shape that used to fabricate the declared type's default — an
// empty erased record cast to the class — instead of the value the module
// evaluated. It was wrong twice over: the initializer's value was silently
// gone, and for `Headers` the recovery cast was emitted at the record type
// rather than at `SmeltUnknown`, so the generated crate did not compile.
//
// A class-typed module binding read from a hoisted item body now lifts to a
// module-global slot, so all three functions below see the SAME object. The
// mutating case is the one that distinguishes a slot from re-running the
// initializer per use site: `addHeader` writes and `extra` reads, and only a
// shared slot lets the read observe the write.

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const headers = new Headers({ "content-type": "text/plain" });
const params = new URLSearchParams("a=1&b=2");

function encodedLength(text: string): number {
  return encoder.encode(text).length;
}

function roundTrip(text: string): string {
  return decoder.decode(encoder.encode(text));
}

function contentType(): string {
  return headers.get("content-type") ?? "none";
}

function addHeader(): void {
  headers.set("x-extra", "added");
}

function extra(): string {
  return headers.get("x-extra") ?? "missing";
}

function paramA(): string {
  return params.get("a") ?? "none";
}

function appendParam(): void {
  params.append("c", "3");
}

console.log(encodedLength("hello"));
console.log(encodedLength("héllo"));
console.log(roundTrip("round trip"));
console.log(contentType());

// Before the write, the header is absent; after it, every reader sees it.
console.log(extra());
addHeader();
console.log(extra());

// The same for a second modeled class, to show the rule is the TYPE and not
// one class's special handling.
console.log(paramA());
appendParam();
console.log(params.toString());

// The module body reads the same binding through its ordinary local, so the
// slot and the local have to agree.
console.log(headers.get("content-type") ?? "none");
