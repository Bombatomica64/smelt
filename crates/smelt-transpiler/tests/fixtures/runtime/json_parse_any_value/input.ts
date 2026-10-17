// An untyped `JSON.parse(text)` is `any`: the value is whatever the text
// holds. It used to be typed as a string-keyed record, which turned
// `JSON.parse('null')` into `{}`, so a `body === null` check never fired.
function check(body: unknown): string {
  if (typeof body !== 'object' || body === null) {
    return 'not object';
  }
  return Array.isArray(body) ? 'array' : 'object';
}

async function viaResponse(text: string): Promise<string> {
  const body: unknown = await new Response(text).json();
  return check(body);
}

async function run(): Promise<void> {
  console.log(check(JSON.parse('null')), check(JSON.parse('{}')), check(JSON.parse('[1]')), check(JSON.parse('3')));
  console.log(await viaResponse('null'), await viaResponse('{"a":1}'));
}

await run();
