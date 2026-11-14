// An erased error enumerates the own keys Node does: `message`, `stack` and
// `cause` never (they are non-enumerable), and `name` only when a
// constructor assigned it (otherwise it is inherited from the prototype).

class PlainSubclass extends Error {
  code: number;
  constructor(message: string, code: number) {
    super(message);
    this.code = code;
  }
}

class NamedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NamedError";
  }
}

const values: any[] = [new Error("boom"), new PlainSubclass("bad", 7), new NamedError("named")];
for (const value of values) {
  const seen: string[] = [];
  for (const key in value) seen.push(key);
  console.log(JSON.stringify(Object.keys(value)), JSON.stringify(seen), JSON.stringify({ ...value }), value.name, value.message);
}
