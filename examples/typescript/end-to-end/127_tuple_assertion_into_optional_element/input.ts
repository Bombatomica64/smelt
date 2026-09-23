// A tuple asserted to a narrower element type (`as [number, Item]`) returned
// where the declared tuple element is optional (`[number, Item | undefined]`):
// each element converts to its declared slot, so the item is wrapped.
interface Item {
  name: string;
  weight: number;
}

const pick = (index: number): [number, Item | undefined] => {
  const item: Item = { name: "box", weight: 3 };
  if (index < 0) {
    return [0, undefined];
  }
  return [index + 1, item] as [number, Item];
};

const [next, found] = pick(4);
console.log(next);
console.log(found ? found.name + " " + found.weight : "none");
const [start, missing] = pick(-1);
console.log(start);
console.log(missing ? missing.name : "none");
