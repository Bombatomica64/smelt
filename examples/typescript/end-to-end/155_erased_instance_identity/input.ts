// A class instance that crosses an erased boundary stays the SAME object.
//
// `throw` accepts any value and a `catch` binding is `unknown`, so a thrown
// instance crosses the erased exception channel and is narrowed back with
// `as`. JavaScript hands back the very object that was thrown: writes and
// mutating method calls through the narrowed value reach the thrower's
// instance, and `===` sees one object. The same holds for an instance that
// travels through an async rejection. A class compared by `===` has identity
// even when nothing mutates it.

class Context {
  res: string = "init";
  headers: string[] = [];
  count = 0;

  header(value: string): void {
    this.headers.push(value);
    this.count += 1;
  }
}

class Point {
  readonly x: number;
  readonly y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function fail(c: Context): void {
  throw c;
}

async function failLater(c: Context): Promise<void> {
  throw c;
}

async function run(): Promise<void> {
  const c = new Context();
  try {
    fail(c);
  } catch (err) {
    const back = err as Context;
    back.res = "from-catch";
    back.header("x-one");
    console.log(c.res, c.headers.join(","), c.count, back === c);
  }

  try {
    await failLater(c);
  } catch (err) {
    const back = err as Context;
    back.header("x-two");
    console.log(c.headers.join(","), c.count, back === c, back !== c);
  }

  const p = new Point(1, 2);
  const q = new Point(1, 2);
  const alias = p;
  console.log(p === alias, p === q, p !== q);
}

run();
