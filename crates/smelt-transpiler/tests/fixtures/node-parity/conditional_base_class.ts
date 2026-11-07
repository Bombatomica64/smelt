class Cat {
  static kind = "cat";
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  sound(): string {
    return "meow";
  }
  static make(): string {
    return "a cat";
  }
}
class Dog {
  static kind = "dog";
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  sound(): string {
    return "woof";
  }
  static make(): string {
    return "a dog";
  }
}

function describe(loud: boolean, name: string): string {
  const Base = loud ? Dog : Cat;
  class Pet extends Base {
    static legs = 4;
    volume = 3;
    constructor(name: string) {
      super(name.toUpperCase());
    }
    sound(): string {
      return super.sound() + "!";
    }
    describe(): string {
      return this.name + " says " + this.sound() + " at " + this.volume;
    }
    static info(): string {
      return Pet.legs + " legs";
    }
  }
  const pet = new Pet(name);
  pet.name = pet.name + "-" + Pet.legs;
  return [pet.describe(), pet.name, Pet.legs, Pet.info(), Pet.kind, Pet.make(), pet instanceof Pet, pet instanceof Dog].join(" | ");
}

console.log(describe(true, "rex"));
console.log(describe(false, "tom"));
