// A record handed to a `Record<string, V>` slot is the same object there.
//
// TypeScript assigns an object type to `Record<K, V>` structurally, and
// JavaScript passes the object. The generated Rust has a struct on one side
// and a string-keyed dictionary on the other. The dictionary used to be built
// from the struct's fields, a copy: `double` below doubled a copy. It is now a
// VIEW of the struct (`smelt-runtime/src/runtime/live_view.rs`): a write
// through either is read through the other, and a property the dictionary
// adds or deletes belongs to the object, which enumerates it in JavaScript's
// own-key order from then on. `Size` stays a struct with `f64` fields.
//
// Every line below is diffed against Node.

type Size = { width: number; height: number };
type Margin = { top: number; left: number };

let kept: Record<string, number> = {};

function double(sizes: Record<string, number>): void {
  for (const key of Object.keys(sizes)) {
    sizes[key] = sizes[key] * 2;
  }
}

function widen(sizes: Record<string, number>): number {
  sizes["width"] += 1;
  return sizes["width"];
}

function keep(sizes: Record<string, number>): void {
  kept = sizes;
}

function reshape(sizes: Record<string, number>): void {
  // Add a property the record type does not declare, delete a declared one,
  // then set it again: it is a new property and enumerates last.
  sizes["depth"] = 7;
  delete sizes["width"];
  sizes["width"] = 30;
}

function main(): void {
  const size: Size = { width: 2, height: 3 };

  // The callee's writes are visible to the caller.
  double(size);
  console.log(size.width, size.height);
  console.log(widen(size), size.width);

  // The caller's write is visible through the dictionary the callee kept.
  keep(size);
  size.height = 40;
  console.log(kept["height"], kept["width"]);

  // A write through the kept dictionary reaches the record.
  kept["height"] += 2;
  console.log(size.height);

  // Key order after a dynamic add, a delete and a re-add through the view.
  reshape(size);
  console.log(size.width, JSON.stringify(size), JSON.stringify(kept));
  console.log(size);

  // A record no typed code ever writes is the caller's object all the same:
  // being handed to the dictionary parameter is what makes it shared.
  const margin: Margin = { top: 1, left: 2 };
  double(margin);
  console.log(margin.top, margin.left);

  // Another record of the same type is untouched.
  const other: Size = { width: 1, height: 1 };
  double(other);
  console.log(other, size.width);
}

main();
