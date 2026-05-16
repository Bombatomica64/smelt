class Counter {
  public value: number;

  constructor(value: number) {
    this.value = value;
  }

  inc(delta: number): number {
    this.value = this.value + delta;
    return this.value;
  }
}
let counter = new Counter(4);
console.log(counter.inc(3));
