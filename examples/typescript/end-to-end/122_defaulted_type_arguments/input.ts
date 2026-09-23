// A generic class with three defaulted type parameters, referenced with 0, 1
// and 3 type arguments.
//
// Every reference site fills its omitted trailing arguments from the
// declaration's defaults before arity is checked, so `Ctx`, `Ctx<string>` and
// `Ctx<string, string, number>` all name the same three-parameter Rust type.

class Ctx<E = string, P = string, I = number> {
  env: E;
  path: P;
  input: I;
  constructor(env: E, path: P, input: I) {
    this.env = env;
    this.path = path;
    this.input = input;
  }
  where(): P {
    return this.path;
  }
}

function none(c: Ctx): string {
  return c.env + ":" + c.where();
}

function one(c: Ctx<string>): number {
  return c.input * 2;
}

function three(c: Ctx<string, string, number>): number {
  return c.input + 1;
}

const c = new Ctx("root", "/a", 41);
console.log(none(c));
console.log(one(c));
console.log(three(c));
