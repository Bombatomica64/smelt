// A record handed to a slot of ANOTHER record type is the same object there.
//
// `Point` below is passed to parameters typed by an interface, by a structural
// subset with mixed property types, by the same properties declared under
// another name, and on from one of those to a further one. Each of those slots
// has its own struct in the generated Rust, and converting a `Point` to it
// used to build a new value: the callee wrote a copy, an alias it kept went
// stale, and the value it returned was another object with the slot type's
// key order. The slot now holds a view of the caller's record.
//
// For every conversion the program checks the four things a copy gets wrong:
// the callee's write is read by the caller, the caller's write is read through
// an alias the callee kept, the value that comes back is the same object, and
// its keys are the caller's.
//
// Every line below is diffed against Node.

type Point = { x: number; label: string; tags: string[] };

// An interface naming one property.
interface Named {
  label: string;
}

// A structural subset with mixed property types.
type Spot = { x: number; label: string };

// The same properties, declared under another name and in another order.
type Marker = { tags: string[]; label: string; x: number };

// A further view, reached from `Named`.
interface Titled {
  label: string;
}

let keptNamed: Named = { label: "nobody" };
let keptSpot: Spot = { x: 0, label: "nowhere" };
const roster: Named[] = [];

function rename(target: Named, label: string): Named {
  target.label = label;
  keptNamed = target;
  return target;
}

function move(target: Spot, by: number): Spot {
  target.x += by;
  target.label = target.label + "@" + target.x;
  keptSpot = target;
  return target;
}

function tag(target: Marker, extra: string): Marker {
  target.tags.push(extra);
  target.tags = target.tags.concat([extra + "!"]);
  target.x *= 2;
  return target;
}

function entitle(target: Titled): void {
  target.label = "<" + target.label + ">";
}

function throughNamed(target: Named): Named {
  entitle(target);
  return target;
}

function shout(all: Named[]): void {
  for (const each of all) {
    each.label = each.label.toUpperCase();
  }
}

function enroll(member: Named): number {
  roster.push(member);
  return roster.length;
}

function describe(target: Named): string {
  return JSON.stringify(target);
}

function main(): void {
  const point: Point = { x: 1, label: "a", tags: ["first"] };

  // An interface: the callee's write, the kept alias, identity, keys.
  const named = rename(point, "renamed");
  console.log(point.label);
  point.label = "later";
  console.log(keptNamed.label, named.label);
  keptNamed.label = "through";
  console.log(point.label);
  console.log(named === keptNamed, (named as Point) === point);
  console.log(describe(point), JSON.stringify(named));

  // A structural subset with mixed property types.
  const spot = move(point, 4);
  console.log(point.x, point.label);
  point.x = 40;
  console.log(keptSpot.x, spot.x);
  keptSpot.x += 2;
  console.log(point.x);
  console.log(spot === keptSpot, (spot as Point) === point);
  console.log(JSON.stringify(spot));

  // The same properties under another name: the caller's key order survives.
  const marker = tag(point, "second");
  console.log(point.x, point.tags.join(","));
  point.tags.push("third");
  console.log(marker.tags.length, (marker as Point) === point);
  console.log(JSON.stringify(marker));

  // A view handed on to a further view.
  const titled = throughNamed(point);
  console.log(point.label, titled === named);

  // Elements of an array, and a value stored in a module-level array.
  const points: Point[] = [
    { x: 1, label: "p", tags: [] },
    { x: 2, label: "q", tags: [] },
  ];
  shout(points);
  console.log(points[0].label, points[1].label);
  console.log(enroll(point), enroll(points[0]));
  const first = roster[0];
  first.label = "enrolled";
  console.log(point.label, roster[1] === (points[0] as Named));

  // A record built as the slot's own type is still its own object.
  const own: Named = { label: "own" };
  const renamed = rename(own, "mine");
  console.log(own.label, renamed === own, JSON.stringify(own), point.label);
}

main();
