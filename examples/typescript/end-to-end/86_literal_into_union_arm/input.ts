// A literal passed where a UNION is expected is contextually typed by the arm
// that can hold it, exactly as it is when bound to a typed local first.
//
// The literal used to lower with no hint at all — a union is neither a list nor
// a tuple — so its elements erased and the argument became
// `SmeltList<SmeltUnknown>`, while the SAME literal bound to a typed local was
// injected as the union's arm. The values agreed; the types did not, and the
// erased spelling is what a hand-written Rust port would never produce.
type Slot = number | [string, number][];

function describe(slot: Slot): string {
  if (typeof slot === 'number') {
    return 'number:' + String(slot);
  }
  return 'pairs:' + slot.map((pair) => pair[0] + '=' + String(pair[1])).join(',');
}

// The literal passed straight in.
console.log('direct: ' + describe([['a', 1], ['b', 2]]));

// The same literal bound to a typed local: the control that always worked, and
// which must keep emitting the same union arm.
const pairs: [string, number][] = [['a', 1], ['b', 2]];
console.log('bound: ' + describe(pairs));

// The other arm still resolves.
console.log('other arm: ' + describe(7));

// A nested literal: the arm's element type contextually types the inner
// literals too, so a tuple arm stays a tuple all the way down.
type Rows = string | [number, number][];

function total(rows: Rows): string {
  if (typeof rows === 'string') {
    return 'text:' + rows;
  }
  let sum = 0;
  for (const row of rows) {
    sum += row[0] * row[1];
  }
  return 'sum:' + String(sum);
}

console.log('nested: ' + total([[2, 3], [4, 5]]));
console.log('string arm: ' + total('none'));

// Two list arms are ambiguous, so the literal's own elements decide — the
// no-hint path, unchanged.
type Either = string[] | number[];

function firstOf(values: Either): string {
  return values.length === 0 ? 'empty' : String(values[0]);
}

console.log('ambiguous: ' + firstOf(['a', 'b']));
