// The effects of a comma expression, a `void` and an interpolated `void` call
// run where the expression is evaluated: after the operands before it and
// before the ones after it, never ahead of the whole statement.

function viaSet(box: { items: Set<number> }, s: Set<number>): number[] {
  return [box.items.size, (s.add(9), s.size), box.items.size];
}

function viaVoid(list: number[]): string {
  return `${list.length}:${void list.push(4)}:${list.length}`;
}

function viaComma(counter: { n: number }): number[] {
  let k = 0;
  const bump = (): number => ++counter.n;
  return [counter.n, (k++, bump(), counter.n), k, counter.n];
}

function inCall(xs: number[]): number {
  return Math.max(xs.length, (xs.push(1, 2, 3), 0), xs.length * 10);
}

function inCondition(xs: number[]): string {
  const out: string[] = [];
  for (let i = 0; i < 3; i++) {
    if (xs.length > 1 && (xs.pop(), xs.length) >= 1) {
      out.push(`kept${xs.length}`);
    } else {
      out.push(`left${xs.length}`);
    }
  }
  return out.join(",");
}

const seen: string[] = [];
function note(x: string): void {
  seen.push(x);
}

const shared = new Set<number>([1]);
console.log(viaSet({ items: shared }, shared).join(","));
console.log(viaVoid([1, 2]));
console.log(viaComma({ n: 5 }).join(","));
console.log(inCall([7]));
console.log(inCondition([1, 2, 3, 4]));
console.log(`a${note("x")}b${seen.length}${void note("y")}${seen.join("")}`);
