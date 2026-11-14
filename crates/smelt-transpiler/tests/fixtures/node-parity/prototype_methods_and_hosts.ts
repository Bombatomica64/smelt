// Methods live on the class prototype, shared by instances, clones and
// `Object.create(C.prototype)` objects; `.constructor` read before the program
// names the class is the class; host classes (`Date`, `Map`, `Blob`, the
// error family) have one prototype object each, chained like the builtins'.
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
  speak(): string {
    return `${this.name} woofs`;
  }
}

function constructorOf(value: unknown): unknown {
  return (value as any).constructor;
}

function main(): void {
  const rex: unknown = new Dog("rex");
  const early = constructorOf(rex);
  console.log("early constructor", early === Dog, early === Object.getPrototypeOf(rex).constructor);
  const a: any = rex;
  const b: any = new Dog("b");
  console.log("shared", a.bark === b.bark, Dog.prototype.bark === a.bark, Animal.prototype.speak === Dog.prototype.speak);
  const made: any = Object.create(Dog.prototype);
  made.name = "made";
  console.log("create", made.bark(), made.speak(), typeof Dog.prototype.bark);
  const copy: any = Object.assign(Object.create(Object.getPrototypeOf(rex)), rex);
  console.log("clone", copy.bark(), copy.bark === a.bark, copy.name);
  console.log("call", Dog.prototype.bark.call(b), Animal.prototype.speak.call(rex));

  const date: unknown = new Date(0);
  const blob: unknown = new Blob(["hi"]);
  const map: unknown = new Map();
  const error: unknown = new TypeError("x");
  console.log("host", Object.getPrototypeOf(date) === Date.prototype, Object.getPrototypeOf(blob) === Blob.prototype, Object.getPrototypeOf(map) === Map.prototype, Object.getPrototypeOf(error) === TypeError.prototype);
  console.log("host chain", Object.getPrototypeOf(TypeError.prototype) === Error.prototype, Object.getPrototypeOf(Date.prototype) === Object.prototype);
  console.log("host constructor", Date.prototype.constructor === (date as any).constructor, Blob.prototype.constructor === (blob as any).constructor);
  const blobCopy: any = Object.assign(Object.create(Blob.prototype), blob);
  console.log("host clone", blobCopy.size, Object.getPrototypeOf(blobCopy) === Blob.prototype);
}

main();
