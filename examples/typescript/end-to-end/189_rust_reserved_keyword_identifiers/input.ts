// Source identifiers that are Rust keywords -- including `gen`, which only
// the Rust 2024 edition reserves -- are renamed in every position: free
// functions, parameters, locals, class fields, methods and object fields.

function gen(n: number): number {
  return n + 1;
}

function become(abstract: number, final: number): number {
  const box = abstract * 10;
  let virtual = box + final;
  virtual += 1;
  return virtual;
}

class Machine {
  gen: number;
  type: string;
  constructor(gen: number) {
    this.gen = gen;
    this.type = "unit";
  }
  match(override: number): number {
    return this.gen + override;
  }
  loop(): string {
    return this.type + ":" + this.gen;
  }
}

interface Fields {
  gen: number;
  dyn: string;
  priv: boolean;
}

function readFields(fields: Fields): string {
  return fields.dyn + fields.gen + (fields.priv ? "!" : "?");
}

function shadowed(): number {
  const self = 2;
  const unsized = 3;
  const macro = (impl: number): number => impl * self * unsized;
  return macro(4);
}

const machine = new Machine(gen(4));
console.log(gen(1));
console.log(become(2, 3));
console.log(machine.match(7));
console.log(machine.loop());
console.log(readFields({ gen: 9, dyn: "d", priv: true }));
console.log(shadowed());
