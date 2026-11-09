// Values that reach themselves survive erasure, `[]` stays an array when the other
// arm of its conditional is an object, and a callback selected by a conditional or
// `??` is the one that runs.
class Person {
  name: string;
  friends: Person[] = [];
  self?: Person;
  constructor(n: string) {
    this.name = n;
  }
}
const jake = new Person("jake");
jake.self = jake;
jake.friends = [jake, jake];
const asAny: any = jake;
console.log(asAny.self === asAny, asAny.friends[1] === asAny, asAny.friends[0].friends.length);

type Node_ = { name: string; age: number; friends?: Node_[] };
const jay: Node_ = { name: "j", age: 17, friends: [{ name: "c", age: 18 }] };
const erased: any = jay;
console.log(erased.friends[0].age, erased.friends["0"].name, Array.isArray(erased.friends));

const nextIsNum = /^\d+$/.test("0");
const holder: any = {};
holder.cards = nextIsNum ? [] : {};
holder.cards[0] = { v: 2 };
console.log(JSON.stringify(holder));
const shape = (b: boolean) => (b ? [] : {});
console.log(Array.isArray(shape(true)), Array.isArray(shape(false)));

const isEven = (n: number) => n % 2 === 0;
const isOdd = (n: number) => n % 2 === 1;
const pick = (b: boolean) => [1, 2, 3].filter(b ? isEven : isOdd);
console.log(pick(true), pick(false));
const maybe = null as ((n: number) => number) | null;
console.log([1, 2].map(maybe ?? ((n: number) => n + 1)));
