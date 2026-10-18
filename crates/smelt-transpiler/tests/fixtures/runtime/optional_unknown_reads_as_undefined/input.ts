// An optional value read at `unknown` keeps its absence: `let output: unknown =
// data` with `data: object | null | undefined` is `undefined` when `data` is.
// The generated code asserted presence instead, which panicked as soon as a
// caller passed `undefined` as an absent value (remeda `prop(undefined, 'a')`).
type NonPropertyKey = object | null | undefined;

function propImplementation(data: NonPropertyKey, ...keys: readonly PropertyKey[]): unknown {
  let output: unknown = data;
  for (const key of keys) {
    if (output === undefined || output === null) {
      return undefined;
    }
    // @ts-expect-error the key is dynamic, as in remeda's `prop`
    output = output[key];
  }
  return output;
}

console.log(String(propImplementation(undefined, 'a')), String(propImplementation(null, 'a')), String(propImplementation({ a: 1 }, 'a')));
