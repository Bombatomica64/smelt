// A value at a `Headers` slot is the spec's `HeadersInit` conversion of it.
//
// WHATWG builds a header list from a `Headers`, a `Record<string, string>` or a
// sequence of name/value pairs, and Smelt already emits exactly that conversion
// for `new Headers(init)` and for every init-dictionary `headers` key. A
// coercion seam reaches the same pairing whenever an init arm flows into a slot
// Smelt has typed `Headers` — which is what `preferred ?? extra` below does,
// since the `??` takes its non-nullish left type. Without the rule the record
// was assigned to the header list unconverted.

function make(
  preferred: Headers | undefined,
  extra: Record<string, string> | undefined
): Response {
  return new Response('ok', {
    status: 201,
    headers: preferred ?? extra,
  })
}

function show(response: Response): string {
  return `${response.status}:${response.headers.get('x-token') ?? '-'}`
}

const preset = new Headers([['x-token', 'from-headers']])

console.log(show(make(preset, undefined)))
console.log(show(make(undefined, { 'x-token': 'from-record' })))
console.log(show(make(undefined, undefined)))
