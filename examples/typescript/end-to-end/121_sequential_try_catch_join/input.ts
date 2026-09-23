// Sequential try/catch statements rejoin ONE shared continuation.
//
// A throwing call is emitted as a `match` with an `Ok`, an error, and a panic
// arm. Each arm used to carry the whole rest of the body, so N sequential
// try/catch statements emitted 3^N copies of the tail (Hono's
// `utils/cookie.test.ts`, 12 `expect(..).toThrow()` assertions, produced a
// 616 MB module). The arms now end where the normal and the catch paths
// meet, and the rest of the body is emitted once — the generated Rust is
// linear in the number of statements. Catch arms that branch, a `try` whose
// body returns early, and awaited calls all take the same rule.

function parse(input: string): number {
  if (input.length === 0) {
    throw new Error('empty input')
  }
  return input.length
}

async function parseLater(input: string): Promise<number> {
  return parse(input)
}

function firstParsed(inputs: string[]): number {
  for (const input of inputs) {
    try {
      return parse(input)
    } catch (error) {
      if (error instanceof Error) {
        console.log('skip:', error.message)
      }
    }
  }
  return -1
}

function sequential(): number {
  let caught = 0
  let total = 0
  try { total += parse('a') } catch (error) { caught += 1 }
  try { total += parse('') } catch (error) { if (error instanceof Error) { caught += 1 } }
  try { total += parse('bb') } catch (error) { caught += 1 }
  try { total += parse('') } catch (error) { if (error instanceof Error) { caught += 1 } else { caught += 100 } }
  try { total += parse('ccc') } catch (error) { caught += 1 }
  try { total += parse('') } catch (error) { caught += 1 }
  try { total += parse('dddd') } catch (error) { caught += 1 }
  try { total += parse('') } catch (error) { if (error instanceof Error) { caught += 1 } }
  console.log('total', total, 'caught', caught)
  return caught
}

async function sequentialAwaited(): Promise<number> {
  let caught = 0
  try { await parseLater('') } catch (error) { caught += 1 }
  try { await parseLater('x') } catch (error) { caught += 1 }
  try { await parseLater('') } catch (error) { if (error instanceof Error) { caught += 1 } }
  try { await parseLater('') } catch (error) { caught += 1 }
  console.log('awaited caught', caught)
  return caught
}

async function run(): Promise<void> {
  console.log('first', firstParsed(['', '', 'abc', 'z']))
  console.log('sequential', sequential())
  console.log('awaited', await sequentialAwaited())
}

run()
