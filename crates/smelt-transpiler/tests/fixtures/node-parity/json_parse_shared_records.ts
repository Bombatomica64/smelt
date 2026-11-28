// `JSON.parse` into an interface whose records are shared (mutated through an
// alias) decodes into fresh shared records.
interface Item { name: string; qty: number; tags: string[] }
const items: Item[] = JSON.parse('[{"name":"a","qty":1,"tags":["x"]},{"name":"b","qty":2,"tags":[]}]');
const first = items[0];
first.tags.push("y");
first.qty += 10;
console.log(items[0].qty, items[0].tags, items[1].name);
const again: Item[] = JSON.parse(JSON.stringify(items));
again[1].qty = 99;
console.log(items[1].qty, again[1].qty, again[0].tags.length);
