// A type parameter bounded by an interface keeps its type when it reaches the
// function inside a container (`T[]`, `Record<string, T>`, `[T, string]`): the
// call site's argument binds it, the body reads its members through the
// bound's trait, and the result is the caller's own type.
interface HasId {
  id: number;
}
interface User extends HasId {
  name: string;
}

function maxId<T extends HasId>(xs: T[]): T {
  let best = xs[0];
  for (const x of xs) {
    if (x.id > best.id) {
      best = x;
    }
  }
  return best;
}

function idsOf<T extends HasId>(byKey: Record<string, T>): number[] {
  const ids: number[] = [];
  for (const key of Object.keys(byKey)) {
    ids.push(byKey[key].id);
  }
  return ids;
}

function labelled<T extends HasId>(pair: [T, string]): string {
  return pair[1] + "#" + pair[0].id;
}

const users: User[] = [
  { id: 1, name: "ada" },
  { id: 3, name: "grace" },
  { id: 2, name: "edsger" },
];
console.log(maxId(users).name);
console.log(idsOf({ a: users[0], b: users[1] }).join(","));
console.log(labelled([users[2], "third"]));
