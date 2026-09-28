// A promise and a function keep their JavaScript runtime shape when the
// static types around them say less.
//
// `new Promise(executor)` with no type argument and no contextual type
// resolves with whatever the executor passes to `resolve` — through a nested
// callback, on every branch, or on only some of them — so the future is
// typed by that value (optional where a branch resolves nothing) rather than
// by `void`.
//
// A function asserted to a signature with fewer parameters is still the same
// function: called through the narrower type it simply receives fewer
// arguments.

class Reply {
  body: string;
  constructor(body: string) {
    this.body = body;
  }
}

function defer(task: () => void): void {
  task();
}

function later() {
  return new Promise((resolve) => defer(() => resolve(new Reply("late reply"))));
}

function counted(flag: boolean) {
  return new Promise((resolve) => {
    if (flag) {
      resolve(3);
    } else {
      resolve(4);
    }
  });
}

function maybe(flag: boolean) {
  return new Promise((resolve) => (flag ? resolve(1) : resolve()));
}

type Greet = (name: string) => string;

async function entry(): Promise<void> {
  const reply = await later();
  console.log(reply.body);
  console.log(await counted(true), await counted(false));
  const present = await maybe(true);
  const absent = await maybe(false);
  console.log(present, absent === undefined);

  const greet = ((name: string, suffix?: string) => `hello ${name}${suffix ?? "!"}`) as Greet;
  console.log(greet("smelt"));
}

entry();
