function make(): number {
  class Counter {
    static start = 10;
    static label: string = "n";
    static next(): number {
      return Counter.start + 1;
    }
    read(): string {
      return Counter.label + Counter.start;
    }
  }
  console.log(new Counter().read());
  return Counter.next();
}
class Top {
  static base = 2;
  twice(): number {
    return Top.base * 2;
  }
}
console.log(make(), new Top().twice(), Top.base);
