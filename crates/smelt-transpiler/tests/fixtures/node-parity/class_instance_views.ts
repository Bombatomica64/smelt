// A class instance handed to an interface-typed slot is the SAME object there
// (docs/design/record-identity.md, step 1): a write through either handle is
// read through the other, `===` holds, and whole-object reads (keys, JSON,
// inspection, `instanceof`) answer as the class it was built as. A leaf class,
// subclasses, and a class with subclasses (a polymorphic handle), in arrays,
// at a wider property type and through casts back.

class Pt {
  constructor(public x: number, public label: string) {}
  norm(): number { return this.x * 2; }
}
interface Labeled { label: string }
interface HasX { x: number }
interface Wide { x: number | string; label?: string }

function rename(l: Labeled): void { l.label = "renamed"; }
const p = new Pt(1, "a");
rename(p);
console.log(p.label);
const kept: Labeled = p;
p.label = "z";
console.log(kept.label, kept === p);
console.log(kept);
console.log(JSON.stringify(kept), Object.keys(kept).join(","));
const back = kept as Pt;
back.x = 4;
console.log(back.norm(), back === p, p.x);
const xs: HasX[] = [p, { x: 7 }];
xs[0].x = 11;
console.log(p.x, xs[1].x);
const w: Wide = p;
w.x = 3;
console.log(p.x, w.label);
const u: unknown = kept;
console.log(u === p, u instanceof Pt);

function relabel(items: Labeled[]): void { for (const item of items) item.label += "!"; }
const pts = [new Pt(1, "p"), new Pt(2, "q")];
relabel(pts);
console.log(pts.map((q) => q.label).join(","));

class Animal {
  constructor(public name: string) {}
  speak(): string { return "..."; }
}
class Dog extends Animal {
  constructor(name: string, public tricks: number) { super(name); }
  speak(): string { return "woof"; }
}
class Puppy extends Dog { age = 1; }
interface Named { name: string }

const d = new Dog("rex", 2);
const nd: Named = d;
nd.name = "max";
console.log(d.name, nd === d, d.speak());
const a: Animal = d;
const an: Named = a;
console.log(an.name, an === nd, (an as Animal) === a, (an as Dog).tricks);
an.name = "zed";
console.log(d.name, a.name);
const zoo: Animal[] = [new Animal("cat"), new Puppy("bit", 0), d];
const names: Named[] = zoo;
names.forEach((n) => { n.name = n.name.toUpperCase(); });
console.log(zoo.map((z) => z.name + ":" + z.speak()).join(","));
console.log(names[1] === zoo[1], (names[1] as Animal) instanceof Puppy);
console.log(names[1]);
const erased: unknown = names[2];
console.log(erased === d, erased instanceof Dog);
