// `Response.json()` and `Request.json()` read the body and parse it as JSON.
//
// Both are `text()` followed by `JSON.parse`: single-use body readers whose
// promise rejects with a catchable `SyntaxError` on malformed text. The parsed
// value keeps its source key order (`JSON.stringify` below prints `path`
// before `n`), as a JavaScript object does.
//
// Every line below is diffed against Node.

const handle = (req: Request): Response | Promise<Response> => {
  return new Response(JSON.stringify({ path: req.url, n: [1, 2] }), { status: 200 });
};

const main = async () => {
  const res = await handle(new Request('http://localhost/a'));
  console.log(res.status);
  const body = await res.json();
  console.log(JSON.stringify(body));
  const req = new Request('http://localhost/b', { method: 'POST', body: '{"x":true}' });
  console.log(JSON.stringify(await req.json()));
  try {
    await new Response('not json').json();
  } catch (e) {
    console.log((e as Error).name);
  }
};

main();
