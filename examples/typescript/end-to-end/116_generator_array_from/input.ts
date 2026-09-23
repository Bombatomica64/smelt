// `Array.from(gen)` and `[...gen]` drain a synchronous generator through its
// iterator protocol, so the result is a list of the generator's own yield
// type. The collection stays typed end to end: no erased iterator object.

function* range(start: number, end: number, step: number = 1): Generator<number, void, undefined> {
  for (let i = start; i <= end; i += step) {
    yield i
  }
}

function* labels(count: number): Generator<string, void, undefined> {
  for (let i = 0; i < count; i++) {
    yield `item-${i}`
  }
}

const fromRange = Array.from(range(1, 5))
console.log(fromRange.length, fromRange[0], fromRange[4])

const spreadRange = [...range(0, 10, 5)]
console.log(spreadRange.join(','))

const names = [...labels(3)]
console.log(names.join(' '))

// A drained generator is a fresh array each time.
const again = Array.from(range(2, 3))
console.log(again.length, again[0] + again[1])
