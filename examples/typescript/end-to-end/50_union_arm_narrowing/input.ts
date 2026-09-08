// Two ways a union arm has to survive: a conditional that BUILDS one, and an
// `instanceof` that reads one back out.

class Doc {
  name: string;

  constructor(name: string) {
    this.name = name;
  }
}

// A ternary whose branches are a declared class and a string. Both arms are
// concrete, and the join is the union TypeScript already gives it — not one
// arm's type carrying the other arm's value.
function pick(flag: boolean): string | Doc {
  return flag ? new Doc("doc") : "text";
}

// The same union built through if/else returns, which always worked, so the two
// spellings can be compared.
function branches(flag: boolean): string | Doc {
  if (flag) {
    return new Doc("doc");
  }
  return "text";
}

// Reading either back out narrows through the arm.
function label(value: string | Doc): string {
  if (typeof value === "string") {
    return "text:" + value;
  }
  return "doc:" + value.name;
}

console.log(label(pick(true)), label(pick(false)));
console.log(label(branches(true)), label(branches(false)));

// A ternary through a local annotated with the union, which reaches the same
// unification with a type hint in hand.
const chosen: string | Doc = pick(true);
console.log(label(chosen));

// `instanceof` on an OPTIONAL union: the entry is `string | File | null`, so
// the check has to ask about the arm one optional deep. An absent value is not
// a `File`, which is also what `null instanceof File` answers in JavaScript.
const form = new FormData();
form.append("doc", new File(["hi"], "note.txt", { type: "text/plain" }));
form.append("text", "plain");

const entry = form.get("doc");
if (entry instanceof File) {
  console.log(entry.name, entry.type, entry.size);
} else {
  console.log("not a file");
}

const text = form.get("text");
if (text instanceof File) {
  console.log("unexpectedly a file");
} else {
  console.log("not a file");
}

const missing = form.get("absent");
if (missing instanceof File) {
  console.log("unexpectedly a file");
} else {
  console.log("absent is not a file");
}

// The non-optional union reads the same way, one arm shallower.
for (const value of form.values()) {
  if (value instanceof File) {
    console.log("file", value.name);
  } else {
    console.log("text", value);
  }
}
