// A binding written two closure levels down is shared through BOTH levels.
//
// `val = 1` sits inside a callback that is created inside another callback.
// The middle closure never writes `val` itself, but it has to hand the inner
// closure the same storage the outer function reads, so its own capture must
// be shared too. It used to capture a private copy, and the inner closure
// referred to a shared cell nobody had created (rustc E0425).

function defer(body: (register: (cleanup: () => void) => void) => void): void {
  const cleanups: Array<() => void> = []
  body((cleanup) => {
    cleanups.push(cleanup)
  })
  for (const cleanup of cleanups) {
    cleanup()
  }
}

function run(): number {
  let val = 0
  let calls = 0
  defer((register) => {
    register(() => {
      val = 1
      calls += 1
    })
    register(() => {
      calls += 1
    })
  })
  return val * 10 + calls
}

console.log(run())
