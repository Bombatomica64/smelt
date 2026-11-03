// A postfix update inside a declaration's initializer stores its new value
// where JavaScript evaluates it. Smelt may only move the store after the
// binding when nothing later in the same initializer can observe it.

class Echo {
  m(x: number): number {
    return x;
  }
}

function laterCallArgument(n: number): number {
  let i = n;
  const v = i++ + new Echo().m(i);
  return v * 100 + i;
}

function laterRead(n: number): number {
  let i = n;
  const v = i++ + i;
  return v * 100 + i;
}

function twoUpdates(n: number): number {
  let i = n;
  const v = i++ * 10 + i++;
  return v * 100 + i;
}

function calleeReadsCapturedVariable(n: number): number {
  let i = n;
  const peek = (): number => i;
  const v = i++ * 10 + peek();
  return v * 100 + i;
}

function indexedRead(n: number): number {
  const values = [10, 20, 30, 40];
  let i = n;
  const v = values[i++] + values[i];
  return v * 100 + i;
}

function laterElementRead(n: number): number {
  let i = n;
  const pair = [i++, i];
  return pair[0] * 100 + pair[1] * 10 + i;
}

function trailingUpdateStillDefers(n: number): number {
  let i = n;
  const v = i * 10 + i++;
  return v * 100 + i;
}

let g = 5;
const top = g++ + new Echo().m(g);

console.log(laterCallArgument(5));
console.log(laterRead(5));
console.log(twoUpdates(5));
console.log(calleeReadsCapturedVariable(5));
console.log(indexedRead(1));
console.log(laterElementRead(5));
console.log(trailingUpdateStillDefers(5));
console.log(top, g);
