// An assignment is an expression whose VALUE is the assigned value and whose
// EFFECT is the store, and a logical assignment performs that store only when
// its test says to. Both halves are checked here for every target shape:
// locals, object fields and record elements.
class Leaf {
  tag: string;

  constructor(tag: string) {
    this.tag = tag;
  }
}

// A plain assignment used as an expression value stores.
let local = 0;
const localValue = (local = 5);
console.log('local: ' + String(localValue) + ' ' + String(local));

const holder = { field: 0 };
const fieldValue = (holder.field = 5);
console.log('field: ' + String(fieldValue) + ' ' + String(holder.field));

const numbers: Record<string, number> = {};
const elementValue = (numbers['a'] = 5);
console.log(
  'element: ' + String(elementValue) + ' ' + Object.keys(numbers).join(',') + ' ' + String(numbers['a'])
);

// `||=` in expression position stores too, on every target shape.
let falsyLocal = 0;
const orLocal = (falsyLocal ||= 7);
console.log('or local: ' + String(orLocal) + ' ' + String(falsyLocal));

const falsyHolder = { field: 0 };
const orField = (falsyHolder.field ||= 7);
console.log('or field: ' + String(orField) + ' ' + String(falsyHolder.field));

// A record of objects: the constructed value is both stored and handed back.
const leaves: Record<string, Leaf> = {};
const leaf = (leaves['a'] ||= new Leaf('first'));
console.log('or element: ' + leaf.tag + ' ' + Object.keys(leaves).join(','));

// The second time the key is present, so the right side is not evaluated and
// the existing value is kept.
const kept = (leaves['a'] ||= new Leaf('second'));
console.log('or element again: ' + kept.tag + ' ' + Object.keys(leaves).join(','));

// An absent record key reads as `undefined`, so `??=` stores.
const nullish: Record<string, number> = {};
nullish['a'] ??= 2;
console.log('nullish absent: ' + Object.keys(nullish).join(',') + ' ' + String(nullish['a']));

// ... and `&&=` does not store, so the key stays absent.
const conjunction: Record<string, number> = {};
conjunction['a'] &&= 9;
console.log('and absent: ' + String(Object.keys(conjunction).length));

// A present truthy key is what `&&=` does store over.
const present: Record<string, number> = { a: 3 };
present['a'] &&= 9;
console.log('and present: ' + String(present['a']));

// `??=` on a local and on an optional field, both undefined.
let absentLocal: number | undefined = undefined;
absentLocal ??= 4;
console.log('nullish local: ' + String(absentLocal));

const optional: { field?: number } = {};
optional.field ??= 4;
console.log('nullish field: ' + String(optional.field));

// A truthy target keeps its value and is not re-stored.
let truthy = 1;
truthy ||= 8;
console.log('or truthy: ' + String(truthy));
