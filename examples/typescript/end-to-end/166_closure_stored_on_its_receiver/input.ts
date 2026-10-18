// A closure stored on the instance it captures.
//
// A class-field arrow (`json = () => this.x`) lives in the instance it
// captures. JavaScript collects the pair; a reference-counted port must break
// the cycle without changing what any program can observe
// (`emitter::weak_receiver`). Every way the closure can outlive the instance's
// other handles is exercised: returned from a function after the instance's
// last handle is gone, a nested closure created by the field closure and kept,
// and the inherited field of a subclass. The listener form of the same cycle
// is the runtime fixture `listener_captures_its_own_emitter`.

class Ctx {
  status = 200;
  json = (body: string): string => `${this.status}:${body}`;
  later = (): (() => string) => () => `later ${this.status}`;
  constructor(code: number) {
    this.status = code;
  }
}

class Sub extends Ctx {
  extra = 1;
  constructor(code: number) {
    super(code);
    this.extra = code * 2;
  }
}

function useOnce(code: number): string {
  const c = new Ctx(code);
  c.status = c.status + 1;
  return c.json("x");
}

function escapeField(): (body: string) => string {
  const c = new Ctx(5);
  c.status = 6;
  return c.json;
}

function escapeNested(): () => string {
  const c = new Ctx(8);
  c.status = 9;
  return c.later();
}

function subclassField(): string {
  const s = new Sub(3);
  s.status = 4;
  return `${s.json("s")} ${s.extra}`;
}

let total = 0;
for (let i = 0; i < 3; i++) {
  total += useOnce(i).length;
}
const field = escapeField();
const nested = escapeNested();
console.log(total, field("z"), nested());
console.log(subclassField());
