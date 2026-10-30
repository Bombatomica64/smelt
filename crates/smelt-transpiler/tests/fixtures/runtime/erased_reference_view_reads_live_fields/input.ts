// An erased view of a class instance is the same JavaScript object as the
// instance: narrowing it back yields that instance, and reading it through the
// erased value sees its fields, own-key order, JSON form and methods. The view
// builds its field snapshot on first use, so these reads also pin that the
// deferred snapshot is complete and taken from the live instance.
class Box {
  value = 1;
  label = 'a';
  bump = (): number => {
    this.value += 1;
    return this.value;
  };
}

function carry(value: unknown): unknown {
  return value;
}

function run(): void {
  const box = new Box();
  const back = carry(box) as Box;
  console.log(back === box, back.bump(), box.value);

  const view = carry(box) as { value: number; label: string };
  console.log(view.value, view.label);
  console.log(Object.keys(carry(box) as object).join(','));
  console.log(JSON.stringify(carry(box)));

  const untouched = carry(box);
  box.label = 'b';
  console.log((untouched as Box).label, (untouched as Box) === box);
}

run();
