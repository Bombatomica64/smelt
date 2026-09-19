// Reading a union value to inspect it is a READ, not a move.
//
// A union's cloneability is its RUST representation's, not its arms'. A union
// with concrete storage lowers to a generated tagged enum that derives `Clone`,
// and a union that erases lowers to the runtime carrier, which is `Clone` too.
// The emitter decided otherwise by walking the union's ARMS: one `Promise` arm
// made the whole value look like something that could only be moved, so the
// first read of a union-typed parameter moved out of it and every later read in
// the same function was `use of moved value` — five of them in Hono's
// `html.rs`, on a `string | Promise<string> | HtmlEscapedString` parameter that
// the generated Rust holds as one plain runtime value.
//
// Each line of `label` reads the same parameter: the guard tests it, and then
// whichever branch was taken reads it again for its answer. The value has to
// survive the guard for either branch to have anything to say.

async function label(value: string | Promise<string>): Promise<string> {
  if (typeof value === "string") {
    return `direct:${value}`;
  }
  return `deferred:${await value}`;
}

console.log(await label("here"));
console.log(await label(Promise.resolve("later")));
