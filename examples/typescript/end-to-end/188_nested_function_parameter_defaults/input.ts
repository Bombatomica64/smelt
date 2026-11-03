// A nested `function` declaration takes the same defaulted-parameter ABI as a
// top-level one: a call that omits a defaulted or optional argument keeps the
// arguments it does pass and the callee applies its own defaults.

function blockNested(n: number): number {
  if (n > 0) {
    function inner(a: number = 10, b: number = 10): number {
      return a + b;
    }
    return inner(n);
  }
  return 0;
}

function bodyLevel(n: number): number {
  function inner(a: number, b: number = a * 2, c: number = 0): number {
    return a + b + c;
  }
  return inner(n) * 100 + inner(n, 1) * 10 + inner(n, 1, 1);
}

function hoistedCall(n: number): number {
  const early = combine(n);
  function combine(a: number, label: string = "x", flag?: boolean): number {
    return a + label.length + (flag === undefined ? 100 : 0);
  }
  return early + combine(n, "abc", true);
}

function recursive(n: number): number {
  function count(k: number, acc: number = 1): number {
    return k <= 0 ? acc : count(k - 1, acc * 2);
  }
  return count(n);
}

console.log(blockNested(3));
console.log(bodyLevel(4));
console.log(hoistedCall(5));
console.log(recursive(4));
