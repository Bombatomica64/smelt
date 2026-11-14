// A `let`/`const` binding is in its temporal dead zone from the start of its
// scope until its declaration runs: reading it there throws a catchable
// `ReferenceError`, through a closure called early or a `switch` case that
// skipped the declaration. After the declaration the same reads succeed, and
// a recursive arrow or a hoisted function that runs only after the binding it
// reads is initialized is not affected.

function readThroughClosureEarly(): string {
  const read = (): number => late * 2
  let first: string
  try {
    first = 'read ' + read()
  } catch (e) {
    first = 'caught ' + (e instanceof ReferenceError) + ' ' + (e as Error).message
  }
  const late = 21
  return first + ' / then ' + read()
}

function switchCaseSkipsDeclaration(k: number): string {
  switch (k) {
    case 0:
      let v: string | undefined = 'zero'
      return v
    case 1:
      return 'one ' + v
  }
  return 'none'
}

function hoistedCallsLaterBindingAfterInit(): number {
  const base = 10
  let total = 0
  for (let i = 0; i < 3; i++) {
    total = total + addStep(i)
  }
  return total
  function addStep(i: number): number {
    return base + i
  }
}

function hoistedCallsLaterBindingBeforeInit(): string {
  try {
    return 'value ' + early()
  } catch (e) {
    return 'caught ' + (e as Error).name + ': ' + (e as Error).message
  }
  function early(): number {
    return later + 1
  }
  const later = 3
}

function recursiveArrow(n: number): number {
  const fact = (k: number): number => (k <= 1 ? 1 : k * fact(k - 1))
  return fact(n)
}

// `isEven` captures `isOdd` before `isOdd` is initialized; it only runs after.
function mutualRecursion(n: number): string {
  const isEven = (k: number): boolean => (k === 0 ? true : isOdd(k - 1))
  const isOdd = (k: number): boolean => (k === 0 ? false : isEven(k - 1))
  return isEven(n) + ' ' + isOdd(n)
}

console.log(readThroughClosureEarly())
console.log(switchCaseSkipsDeclaration(0))
try {
  console.log(switchCaseSkipsDeclaration(1))
} catch (e) {
  console.log('switch caught ' + (e instanceof ReferenceError) + ' ' + (e as Error).message)
}
console.log(switchCaseSkipsDeclaration(2))
console.log(hoistedCallsLaterBindingAfterInit())
console.log(hoistedCallsLaterBindingBeforeInit())
console.log(recursiveArrow(5))
console.log(mutualRecursion(10))
console.log(mutualRecursion(7))
