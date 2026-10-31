// Three runtime rules Hono's `timeout` middleware depends on:
// - a sleep started inside nested async calls does not move the clock while
//   those calls are still running their synchronous prefix, so a deadline
//   armed before them still wins the race;
// - `for...of` over a `Headers` iterates its `[name, value]` entries;
// - a `Response` built from another response's `body` keeps its bytes.
const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

async function slowHandler(): Promise<string> {
  await sleep(1100)
  return 'handler finished'
}
async function layer1(): Promise<string> {
  return await slowHandler()
}
async function layer2(): Promise<string> {
  return await layer1()
}
async function layer3(): Promise<string> {
  return await layer2()
}
async function layer4(): Promise<string> {
  return await layer3()
}
async function dispatchOuter(): Promise<string> {
  return await layer4()
}

async function withDeadline(ms: number): Promise<string> {
  const deadline = sleep(ms).then(() => `deadline ${ms} won`)
  return await Promise.race([dispatchOuter(), deadline])
}

async function run(): Promise<void> {
  console.log(await withDeadline(1000))
  console.log(await withDeadline(1500))

  const headers = new Headers({ 'X-One': '1', 'x-two': '2' })
  for (const [name, value] of headers) {
    console.log(`${name}=${value}`)
  }

  const original = new Response('payload text', { status: 504 })
  const copy = new Response(original.body, { status: original.status })
  console.log(copy.status, await copy.text())
}
run()
