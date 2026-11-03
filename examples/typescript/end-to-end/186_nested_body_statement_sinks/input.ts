// Every nested body starts with its own statement sink and postfix-update
// mode. A class lowered while an enclosing statement fills a block of the
// OUTER body used to push its own synthesized stores (a parameter default's
// `a++`, a field initializer's `this.a++`) into that outer block id.

function methodDefault(n: number): number {
  if (n > 0) {
    class K {
      v: number;
      constructor(a: number) {
        let b = a;
        b++;
        this.v = b;
      }
      get(a: number, x: number = a++): number {
        return a + x + this.v;
      }
    }
    return new K(n).get(n);
  }
  return 0;
}

function fieldInitializer(n: number): number {
  if (n > 0) {
    class Counter {
      a: number = 3;
      b: number = this.a++;
    }
    const counter = new Counter();
    return counter.a * 10 + counter.b;
  }
  return 0;
}

function methodDefaultInLoop(n: number): number {
  let total = 0;
  for (let r = 0; r < n; r++) {
    class Scaled {
      get(a: number, x: number = a++): number {
        return a * 10 + x;
      }
    }
    total += new Scaled().get(r);
  }
  return total;
}

function classExpressionInDeferringDeclaration(n: number): number {
  let i = n;
  const g = i++,
    C = class {
      m(a: number, x: number = a++): number {
        return a * 10 + x;
      }
    };
  return g * 1000 + new C().m(i) + i;
}

function constructorDefault(n: number): number {
  if (n > 0) {
    class Pair {
      v: number;
      constructor(a: number, x: number = a++) {
        this.v = a * 10 + x;
      }
    }
    return new Pair(n).v;
  }
  return 0;
}

console.log(methodDefault(3));
console.log(fieldInitializer(1));
console.log(methodDefaultInLoop(3));
console.log(classExpressionInDeferringDeclaration(3));
console.log(constructorDefault(2));
