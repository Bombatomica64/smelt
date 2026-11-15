// A generic class read as a value carries its static methods, named and
// symbol-keyed, like any class (ts-pattern's matcher protocol: `static
// [P.matcher]()` on `class Some<const T>`). A class value is a `function`.
const matcher = Symbol.for("demo/matcher");

class Some<const T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
  static [matcher](): { kind: string } {
    return { kind: "some" };
  }
  static of(value: number): Some<number> {
    return new Some(value);
  }
}

class Plain {
  static [matcher](): { kind: string } {
    return { kind: "plain" };
  }
}

function protocolKind(pattern: unknown): string {
  const candidate = pattern as { [matcher]?: () => { kind: string } };
  return candidate && candidate[matcher] ? candidate[matcher]!().kind : "none";
}

console.log(protocolKind(Some), protocolKind(Plain), protocolKind({}), protocolKind(new Some(1)));
console.log(typeof Some, typeof (Some as unknown), typeof (Some as unknown) === "object");
console.log(Some.of(3).value, typeof (Some as any).of, (Some as any).of(4).value);
