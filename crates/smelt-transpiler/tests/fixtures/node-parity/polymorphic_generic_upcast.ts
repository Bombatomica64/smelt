// A class of a polymorphic hierarchy viewed as its generic base: the upcast
// builds the base handle through a turbofish path (`Base::<JsString>(..)`),
// spelled from the type's path and arguments rather than its rendered text.
class Base<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  describe(): string { return "base"; }
}
class Derived extends Base<string> {
  constructor(value: string) { super(value); }
  describe(): string { return "derived " + this.value; }
}
function show(item: Base<string>): string { return item.describe(); }
const items: Base<string>[] = [new Base("a"), new Derived("b")];
console.log(items.map(show).join(","));
const up: Base<string> = new Derived("c");
console.log(show(up));
