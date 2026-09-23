// A subclass value flowing into a parameter declared with its base class type.
// Overridden methods must dispatch to the subclass implementation.
class Animal {
  name: string;
  legs: number;
  constructor(name: string, legs: number) {
    this.name = name;
    this.legs = legs;
  }
  speak(): string {
    return this.name + " makes a sound";
  }
  describe(): string {
    return this.name + " has " + this.legs + " legs";
  }
}

class Dog extends Animal {
  breed: string;
  constructor(name: string, breed: string) {
    super(name, 4);
    this.breed = breed;
  }
  speak(): string {
    return this.name + " the " + this.breed + " barks";
  }
}

function introduce(animal: Animal): string {
  return animal.speak() + "; " + animal.describe();
}

const generic = new Animal("Snake", 0);
const dog = new Dog("Rex", "beagle");
console.log(introduce(generic));
console.log(introduce(dog));
console.log(dog.breed);

// A mutated class is emitted as a shared reference handle: the base view of a
// subclass must still dispatch overrides to the subclass object.
class Counter {
  count: number;
  constructor() {
    this.count = 0;
  }
  bump(): void {
    this.count = this.count + 1;
  }
  label(): string {
    return "counter at " + this.count;
  }
}

class NamedCounter extends Counter {
  title: string;
  constructor(title: string) {
    super();
    this.title = title;
  }
  label(): string {
    return this.title + " at " + this.count;
  }
}

function report(counter: Counter): string {
  return counter.label();
}

const plain = new Counter();
plain.bump();
const named = new NamedCounter("clicks");
named.bump();
named.bump();
console.log(report(plain));
console.log(report(named));
