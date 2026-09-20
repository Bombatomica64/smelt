// `await` over `T | Promise<T>` unwraps the promise arms and passes the others
// through, then joins: the result is `T`, not an erased value.
//
// The awaited value used to erase to the runtime carrier, so a fully known type
// was reported as erased -- Hono's
// `new Response(null, await this.#dispatch(..))` on `Response | Promise<Response>`
// is that shape. The value arm matters as much as the promise arm: asserting the
// whole union to be a future would take the half that needs no waiting and turn
// it into an unreachable branch.
class Cell {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}

function pick(sync: boolean): Cell | Promise<Cell> {
  return sync ? new Cell(1) : Promise.resolve(new Cell(2));
}

function pickNumber(sync: boolean): number | Promise<number> {
  return sync ? 10 : Promise.resolve(20);
}

async function run(): Promise<string> {
  const direct = await pick(true);
  const deferred = await pick(false);
  const directNumber = await pickNumber(true);
  const deferredNumber = await pickNumber(false);
  return `${direct.value + deferred.value}/${directNumber + deferredNumber}`;
}

console.log(await run());
