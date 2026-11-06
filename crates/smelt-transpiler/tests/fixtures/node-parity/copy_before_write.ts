// A binding copied before a closure writes the source keeps the value it copied.

function snapshotAcrossClosureWrite(): void {
  let x = 1;
  const increment = (): void => {
    x = x + 1;
  };
  const before = x;
  increment();
  console.log(before);
  console.log(x);
}

function snapshotAcrossNestedClosureWrite(): void {
  let x = 1;
  const outer = (): void => {
    const inner = (): void => {
      x = x + 1;
    };
    inner();
  };
  const before = x;
  outer();
  console.log(before);
  console.log(x);
}

function snapshotInsideClosure(): void {
  let x = 1;
  const increment = (): void => {
    x = x + 1;
  };
  const read = (): number => {
    const before = x;
    increment();
    return before * 10 + x;
  };
  console.log(read());
}

snapshotAcrossClosureWrite();
snapshotAcrossNestedClosureWrite();
snapshotInsideClosure();
