// A class method may implement an interface method with fewer parameters;
// the caller's extra arguments are not bound.
interface Visitor { visit(n: number, label: string): number }
class Counter implements Visitor { visit(): number { return 1; } }
class Doubler implements Visitor { visit(n: number): number { return n * 2; } }
class Labeler implements Visitor { visit(n: number, label: string): number { return n + label.length; } }
const visitors: Visitor[] = [new Counter(), new Doubler(), new Labeler()];
console.log(visitors.map((v) => v.visit(5, "abc")));
console.log(new Counter().visit(), new Doubler().visit(4));
