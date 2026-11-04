// `break` and `continue` leave a `try` through its `finally`: the clause runs
// before control reaches the loop exit (or the next iteration), innermost
// clause first, and only the clauses between the jump and its loop run.
const log: string[] = []

function breakOutOfForOf(): void {
  for (const x of [1, 2, 3]) {
    try {
      if (x === 2) break
      log.push(`body ${x}`)
    } finally {
      log.push(`finally ${x}`)
    }
  }
  log.push('after for-of')
}

function continueInWhile(): void {
  let i = 0
  while (i < 3) {
    i += 1
    try {
      if (i === 2) continue
      log.push(`body ${i}`)
    } finally {
      log.push(`finally ${i}`)
    }
  }
  log.push('after while')
}

function nestedTry(): void {
  for (const x of [1, 2]) {
    try {
      try {
        if (x === 1) continue
        break
      } finally {
        log.push(`inner ${x}`)
      }
    } finally {
      log.push(`outer ${x}`)
    }
  }
  log.push('after nested')
}

function loopInsideTry(): void {
  try {
    for (let i = 0; i < 3; i++) {
      try {
        if (i === 1) break
      } finally {
        log.push(`loop finally ${i}`)
      }
    }
    log.push('still in try')
  } finally {
    log.push('outer finally')
  }
}

function breakFromCatch(): void {
  for (const x of [1, 2]) {
    try {
      if (x === 1) throw 'boom'
    } catch {
      log.push(`caught ${x}`)
      break
    } finally {
      log.push(`finally ${x}`)
    }
  }
  log.push('after catch')
}

// A throw out of a `finally` that a `return` runs is not caught by the same
// `try`'s `catch`: the clause runs after the `try` statement is left.
function finallyThrowSkipsOwnCatch(fail: boolean): string {
  try {
    try {
      return 'from try'
    } catch {
      return 'own catch'
    } finally {
      if (fail) throw 'late'
    }
  } catch {
    return 'outer catch'
  }
}

breakOutOfForOf()
continueInWhile()
nestedTry()
loopInsideTry()
breakFromCatch()
log.push(finallyThrowSkipsOwnCatch(true))
log.push(finallyThrowSkipsOwnCatch(false))
for (const line of log) {
  console.log(line)
}
