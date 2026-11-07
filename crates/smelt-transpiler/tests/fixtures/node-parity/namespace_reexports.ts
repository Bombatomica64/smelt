// A namespace re-exported under a name (`export { Ns as P }`, `export * as Q`)
// is that module's namespace in the importer: its members are the module's
// exports, a member named like an array method is the module's function, and
// a re-exported symbol const keys a class member.
import { P, Q, Pattern } from "./index.ts";
console.log(P.map("a", P.one), Q.map("b", 2), Pattern.one, P.boxed.n);
class Some {
  constructor(public value: number) {}
  [P.matcher](): number { return this.value; }
  static check(input: unknown): boolean { return input instanceof Some; }
}
console.log(new Some(4)[P.matcher](), Some.check(new Some(1)), Some.check(1));
const local = () => {
  class Inner { static is(input: unknown): boolean { return input instanceof Inner; } }
  return [Inner.is(new Inner()), Inner.is("x")];
};
console.log(local());
