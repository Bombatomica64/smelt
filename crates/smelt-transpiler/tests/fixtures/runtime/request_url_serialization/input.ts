// `request.url` is the WHATWG serialization of the input. The runtime skips
// the parser for an input that is already serialized (every request a served
// program builds from `http://<host><path>`), so these inputs pin both sides of
// that test: ones it may pass through, and ones the parser must rewrite —
// case, default ports, leading zeros, IPv4 shorthand, dot segments, characters
// in a percent-encode set, and backslashes.
const inputs: string[] = [
  'http://localhost:3000/todos/abc',
  'http://127.0.0.1:3110/todos?x=1&y=2',
  'https://ex-ample.com:8443/a/b?c=d',
  'http://example.com/%zz/%41',
  'http://LOCALHOST:3000/Path',
  'http://example.com:80/a',
  'https://example.com:443/a',
  'http://example.com:0080/x',
  'http://example.com',
  'http://example.com/a/./b/../c',
  'http://example.com/a/%2e%2E/c',
  'http://example.com/.well-known/x',
  'http://example.com/a b',
  'http://example.com/a"b<c>d`e{f}g',
  'http://example.com/a\\b',
  "http://example.com/a?q='x'",
  'http://example.com/a|b',
  'http://example.com/x#frag',
  'http://127.1/x',
  'http://0x7f.0.0.1/x',
  'http://010.0.0.1/x',
  'http://example.com./x',
];

for (const input of inputs) {
  console.log(new Request(input).url);
}
