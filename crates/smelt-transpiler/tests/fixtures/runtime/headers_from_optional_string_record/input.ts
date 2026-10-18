// A `Record<string, string | undefined>` is a valid `HeadersInit`: WebIDL runs
// ToString on each value. The emitter had no conversion for it, so Hono's
// `#cachedBody` rebuild (`new Response(body, { headers: contentType ? {
// 'Content-Type': contentType } : undefined })`) stopped the whole build.
async function rebuild(raw: Request, kind: string, body: string): Promise<string> {
  const contentType = kind === 'formData' ? undefined : raw.headers.get('content-type');
  const response = new Response(body, {
    headers: contentType ? { 'Content-Type': contentType } : undefined,
  });
  return `${String(response.headers.get('content-type'))} ${await response.text()}`;
}

async function run(): Promise<void> {
  const request = new Request('http://x/', { headers: { 'content-type': 'text/plain' } });
  console.log(await rebuild(request, 'text', 'hi'));
  console.log(await rebuild(request, 'formData', 'raw'));
}

await run();
