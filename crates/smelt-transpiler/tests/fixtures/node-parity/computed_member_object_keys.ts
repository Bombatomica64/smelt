// An object key computed from a member read of an `any` receiver uses that member's value.
function model(identifiers: any, entityId: string) {
  return {
    [identifiers.ID_COLUMN]: { type: "increments" },
    [entityId]: { type: "integer" },
  };
}

const first = model({ ID_COLUMN: "id" }, "entity");
console.log(Object.keys(first).join(","));
console.log(JSON.stringify(first));
const second = model({ ID_COLUMN: "pk" }, "owner");
console.log(Object.keys(second).join(","));
console.log(JSON.stringify(second));

// A symbol key stays on the object and is absent from JSON.
const tag = Symbol("tag");
const mixed = { [tag]: 1, ["plain"]: 2 };
console.log(mixed[tag]);
console.log(JSON.stringify(mixed));
