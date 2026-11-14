// A class has one prototype object with identity: `.constructor` is the
// class, the chain runs through the base class's prototype to
// `Object.prototype`, and `Object.create`, `Object.setPrototypeOf`,
// `isPrototypeOf` and `instanceof` agree with it. Values are held as
// `unknown`, so every answer comes from the erased representation.
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak(): string {
    return `${this.name} speaks`;
  }
}
class Dog extends Animal {
  bark(): string {
    return `${this.name} barks`;
  }
}
class Plant {
  kind = "fern";
}

function main(): void {
  const rex: unknown = new Dog("rex");
  const tom: unknown = new Animal("tom");
  const fern: unknown = new Plant();
  const pDog = Object.getPrototypeOf(rex);
  console.log("same class", pDog === Object.getPrototypeOf(new Dog("b") as unknown));
  console.log("other class", pDog === Object.getPrototypeOf(fern));
  console.log("is Dog.prototype", pDog === Dog.prototype);
  console.log("proto.constructor", pDog.constructor === Dog, Dog.prototype.constructor === Dog);
  console.log("instance.constructor", (rex as any).constructor === Dog, (tom as any).constructor === Dog);
  console.log("chain", Object.getPrototypeOf(pDog) === Animal.prototype);
  console.log("root", Object.getPrototypeOf(Animal.prototype) === Object.prototype);
  console.log("end", Object.getPrototypeOf(Object.getPrototypeOf(Animal.prototype)));
  console.log("isPrototypeOf", Animal.prototype.isPrototypeOf(rex as object), Dog.prototype.isPrototypeOf(tom as object));
  console.log("Object.prototype.isPrototypeOf", Object.prototype.isPrototypeOf(rex as object));
  const made = Object.create(Dog.prototype);
  console.log("create", made instanceof Dog, made instanceof Animal, made instanceof Plant, Object.getPrototypeOf(made) === Dog.prototype);
  const plain: any = { name: "p" };
  Object.setPrototypeOf(plain, Animal.prototype);
  console.log("setPrototypeOf", plain instanceof Animal, plain instanceof Dog, Object.getPrototypeOf(plain) === Animal.prototype);
  Object.setPrototypeOf(plain, {});
  console.log("setPrototypeOf plain", plain instanceof Animal);
  const again = new pDog.constructor("fido");
  console.log("rebuild", again instanceof Dog, again.name, again.bark());
  const copy = Object.assign(Object.create(Object.getPrototypeOf(rex)), rex);
  console.log("clone", copy instanceof Dog, copy !== rex, copy.name, copy.bark());
  console.log("class identity", (Dog as unknown) === (Dog as unknown));
  console.log("keys", Object.keys(Dog.prototype).length, JSON.stringify(copy));
}

main();
