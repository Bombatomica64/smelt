// A source `Error` subclass without its own `toString` stringifies through
// `Error.prototype.toString`: "name: message", or whichever half is non-empty.
class AppError extends Error {
  constructor(message: string, public code: number) { super(message); this.name = "AppError"; }
}
class NotFound extends AppError {
  constructor(what: string) { super(`${what} not found`, 404); this.name = "NotFound"; }
}
class Quiet extends TypeError { constructor() { super(""); } }
class Unnamed extends Error { constructor() { super("only message"); this.name = ""; } }
class Custom extends Error { toString(): string { return "custom!"; } }
const e = new NotFound("user");
console.log(String(e), `${new AppError("boom", 1)}`, "e: " + new Quiet(), String(new Unnamed()));
console.log(e.toString(), new Quiet().toString(), String(new Custom("z")));
const errors: AppError[] = [new AppError("a", 1), new NotFound("b")];
console.log(errors.map((x) => String(x)).join(" | "));
