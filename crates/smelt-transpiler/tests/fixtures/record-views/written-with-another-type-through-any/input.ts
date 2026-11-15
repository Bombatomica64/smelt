// A write through an `any` view of a value the property's type cannot hold
// stays on the object as written: JavaScript does not check a write. Every
// view reads the written value, in the property's place, and a later write of
// a value of the declared type is the record's field again.

type Settings = { name: string; count: number; enabled: boolean };

function overwrite(target: any): void {
  target.name = { first: "a" };
  target.count = "many";
}

function restore(target: any): void {
  target.name = "back";
}

function mergeInto(target: Record<string, unknown>, source: Record<string, unknown>): Record<string, unknown> {
  for (const key of Object.keys(source)) {
    target[key] = source[key];
  }
  return target;
}

function main(): void {
  const settings: Settings = { name: "s", count: 1, enabled: true };
  const erased: any = settings;
  overwrite(settings);
  console.log(JSON.stringify(erased), typeof erased.name, erased.count);
  console.log(Object.keys(erased));

  restore(settings);
  console.log(settings.name, JSON.stringify(erased));

  // The shape a merge helper leaves behind: every property replaced by an
  // object, read back through what the helper returned.
  const target: Settings = { name: "t", count: 2, enabled: false };
  const merged = mergeInto(target, { name: { x: 1 }, count: { y: 2 }, enabled: { z: 3 } });
  console.log(JSON.stringify(merged), merged === (target as unknown));
}

main();
