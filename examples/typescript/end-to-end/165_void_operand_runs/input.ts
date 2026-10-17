// `void X` evaluates X and discards the result. The operand used to be
// dropped entirely, so `void handle(req, res)` — the idiom for starting a
// promise on purpose without awaiting it — never ran, and a `node:http`
// server written that way accepted connections and answered none.
let count = 0;

function bump(): number {
  count += 1;
  return count;
}

async function later(label: string): Promise<void> {
  await Promise.resolve();
  console.log('async ran', label);
}

void bump();
const discarded = void bump();
console.log(count, discarded === undefined);
['from callback'].forEach((label: string): void => {
  void later(label);
});
void later('top');
