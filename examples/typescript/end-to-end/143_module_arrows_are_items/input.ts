// A module-level arrow const is a function item wherever it is called from:
// a class member, its own body (recursion), or another item. A call must run
// THAT function -- never a default-returning placeholder.

type Brand = { _brand: string };

// A modeled host class intersected with a type-level brand is still the host
// value itself.
const respond = (text: string, status?: number): Response & Brand =>
  new Response(text, { status: status ?? 200 }) as Response & Brand;

const add = (a: number, b?: number): number => a + (b ?? 1);

// Self-recursive arrows, one private and one exported whose annotation spells a
// different parameter list than the arrow itself.
const fact = (n: number): number => (n <= 1 ? 1 : n * fact(n - 1));
export const join: (...parts: string[]) => string = (head: string, ...rest: string[]): string =>
  rest.length ? `${head}/${join(...rest)}` : head;

class Counter {
  total = 0;
  bump(): number {
    this.total = add(this.total, 2);
    return add(this.total);
  }
  get label(): string {
    return join('c', String(this.total), String(fact(4)));
  }
  reply(): Response | Promise<Response> {
    return respond('ok:' + this.total, 201);
  }
}

// An `if` arm whose nested branch may return, then falls through to the rest
// of the function: the fall-through path must reach the loop below.
const scan = (url: string, key?: string, multiple?: boolean): string | undefined => {
  const encoded = url.indexOf('%') !== -1;
  if (!multiple && key && key.indexOf('%') === -1) {
    if (url.indexOf('?') === -1) {
      return undefined;
    }
  }
  let index = url.indexOf('?');
  let out = '';
  while (index !== -1) {
    const next = url.indexOf('&', index + 1);
    let eq = url.indexOf('=', index);
    if (eq > next && next !== -1) {
      eq = -1;
    }
    if (encoded) {
      out += '%';
    }
    if (eq === -1) {
      if (encoded) {
        out += '?';
      }
    }
    if (multiple) {
      out += 'm';
    }
    out += url.slice(index + 1, next === -1 ? undefined : next) + ';';
    index = next;
  }
  return out;
};

async function run(): Promise<void> {
  const counter = new Counter();
  console.log(counter.bump(), counter.bump());
  console.log(counter.label);
  const reply = await counter.reply();
  console.log(reply.status, await reply.text());
  console.log(fact(5), join('a', 'b', 'c'));
  console.log(scan('http://x/?a=1&b', 'a'));
  console.log(scan('http://x/', 'a'));
  console.log(scan('http://x/?z=1&y', undefined, true));
}

run();
