class Counter {
  private static stored = 0;
  static get count(): number { return Counter.stored; }
  static set count(v: number) { Counter.stored = v * 10; }
  static bump(): void { Counter.stored++; }
}
Counter.bump();
console.log(Counter.count);
Counter.count = 3;
console.log(Counter.count);
Counter.count += 1;
console.log(Counter.count);
Counter.count++;
console.log(Counter.count);

class Config {
  static storedame = "base";
  static get label(): string { return "[" + Config.storedame + "]"; }
  static rename(n: string): void { Config.storedame = n; }
}
console.log(Config.label);
Config.rename("x");
console.log(Config.label.length, Config.label);
