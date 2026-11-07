class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  sound(): string {
    return "...";
  }
  intro(): string {
    return this.name + ": " + this.sound();
  }
}
class Dog extends Animal {
  sound(): string {
    return "woof " + super.sound();
  }
}
class Puppy extends Dog {
  sound(): string {
    return super.sound() + " yip";
  }
  intro(): string {
    return "[" + super.intro() + "]";
  }
}
console.log(new Dog("rex").intro());
console.log(new Puppy("bit").intro());
console.log(new Animal("cat").intro());
