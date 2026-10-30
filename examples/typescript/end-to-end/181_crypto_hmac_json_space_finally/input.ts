// WebCrypto HMAC keys, byte-source aliases, `fromCharCode(...bytes)`,
// `JSON.stringify`'s `space`, and `finally` on a rejection.
const algorithm = { name: 'HMAC', hash: 'SHA-256' }

const getKey = async (secret: string | BufferSource): Promise<CryptoKey> => {
  const bytes = typeof secret === 'string' ? new TextEncoder().encode(secret) : secret
  return await crypto.subtle.importKey('raw', bytes, algorithm, false, ['sign', 'verify'])
}

const sign = async (value: string, secret: string | BufferSource): Promise<string> => {
  const key = await getKey(secret)
  const tag = await crypto.subtle.sign(algorithm.name, key, new TextEncoder().encode(value))
  return btoa(String.fromCharCode(...new Uint8Array(tag)))
}

const verify = async (value: string, claimed: string, key: CryptoKey): Promise<boolean> => {
  const tag = await crypto.subtle.sign(algorithm.name, key, new TextEncoder().encode(claimed))
  return await crypto.subtle.verify(algorithm, key, new Uint8Array(tag), new TextEncoder().encode(value))
}

const cleanupAfter = async (work: () => Promise<void>): Promise<string> => {
  let cleaned = 'no'
  try {
    try {
      await work()
    } finally {
      cleaned = 'yes'
    }
  } catch {
    return `rejected, cleaned up: ${cleaned}`
  }
  return `resolved, cleaned up: ${cleaned}`
}

const failing = async (): Promise<void> => {
  throw 'boom'
}

// The value a `finally` holds while it runs is whatever was thrown: here a
// string on one path and an `Error` on the other, through the same clause.
// No static type covers both (the checker types the catch binding `unknown`),
// so the rethrow carries it at the dynamic value.
const rethrowThrough = (kind: number): string => {
  let ran = 0
  try {
    try {
      if (kind === 0) throw 'text'
      throw new RangeError('range')
    } finally {
      ran += 1
    }
  } catch (e) {
    const seen = e instanceof Error ? `error ${e.message}` : `value ${String(e)}`
    return `${seen}, finally ran ${ran}`
  }
}

async function run(): Promise<void> {
  const tag = await sign('macha', 'secret lucky charm')
  console.log(tag)
  const key = await getKey('secret lucky charm')
  console.log(await verify('macha', 'macha', key), await verify('macha', 'other', key))
  console.log(await sign('macha', new Uint8Array([1, 2, 3])))
  console.log(String.fromCharCode(...[72, 105]))
  console.log(JSON.stringify({ first: [1, 2], second: [3] }, null, 2))
  console.log(JSON.stringify(['a', 'b'], null, '--'))
  console.log(await cleanupAfter(failing))
  console.log(await cleanupAfter(async () => {}))
  console.log(rethrowThrough(0))
  console.log(rethrowThrough(1))
}
run()
