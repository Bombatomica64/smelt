// Element reads on a value whose static type is a union of tuples/lists/records
// read each arm at its own type. The second tuple arm is SHORTER than the first,
// so `result[1]` is `undefined` on it: a truthiness test, `??` and an optional
// read must all see that (`[[h], stash] | [[h]]` is the shape Hono's router
// answers with).
type Indexed = Record<string, number>;
type Named = Record<string, string>;
type Outcome = [[string, Indexed][], string[]] | [[string, Named][]];

function indexed(path: string): Outcome {
  return [[['first', { id: 1 }], ['second', { id: 2 }]], ['', path, 'extra']];
}
function named(path: string): Outcome {
  return [[['only', { id: path }]]];
}

function describe(outcome: Outcome): string {
  const handlers = outcome[0];
  const head = handlers[0];
  const stash = outcome[1];
  const raw = head[1]['id'];
  const value = stash ? stash[raw as number] : (raw as string);
  // The test reads the element directly, as Hono's `#getParamValue` does.
  const direct = outcome[1] ? outcome[1][raw as number] : (raw as string);
  const fallback = outcome[1] ?? ['none'];
  const parts = [
    `handlers=${handlers.length}`,
    `head=${head[0]}`,
    `stash=${stash === undefined ? 'undefined' : stash.length}`,
    `value=${value}`,
    `direct=${direct}`,
    `fallback=${fallback[0]}`,
    `outcomeLength=${outcome.length}`,
  ];
  return parts.join(' ');
}

function label(choice: number): [string, number] | string[] {
  return choice === 0 ? ['tuple', 7] : ['a', 'bc', 'def'];
}

console.log(describe(indexed('/users/7')));
console.log(describe(named('9')));
for (const choice of [0, 1]) {
  const picked = label(choice);
  console.log(picked.length, picked[0], picked[1]);
}
