// A body reader picked by a run-time key (`raw[key]()`, Hono's request body
// cache) is the same member a static read names. A computed read on a class
// or host value used to answer `undefined`, so the call returned nothing and
// every request body parsed as `null`.
type BodyKey = 'text' | 'json';

async function read(raw: Request, key: BodyKey): Promise<string> {
  const value: unknown = await raw[key]();
  return JSON.stringify(value);
}

async function run(): Promise<void> {
  console.log(await read(new Request('http://x/', { method: 'POST', body: '{"a":1}' }), 'text'));
  console.log(await read(new Request('http://x/', { method: 'POST', body: '{"a":1}' }), 'json'));
}

await run();
