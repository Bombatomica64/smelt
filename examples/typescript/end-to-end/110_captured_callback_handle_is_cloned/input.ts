// A callback handle rebuilt inside a nested closure is CLONED, not moved.
//
// `next` is a parameter of the middleware closure, bound by value as an owned
// `Rc<dyn Fn() -> string>`. The inner `again` closure captures it and hands it
// on to `call`, which needs its own owned handle — so the adapter wraps it in a
// fresh `move` closure. Naming the capture inside that `move` gives it away,
// and `again` is an `Fn` that may run more than once, so the give-away is
// E0507. The handle is cloned in front of the wrapper instead, which is the
// discipline the sibling adapters already use for the callable they bind.
//
// `wrapped` is called twice on purpose: once would not distinguish a move from
// a clone.

type Next = () => string;
type Middleware = (label: string, next: Next) => string;

function wrap(call: Middleware): Middleware {
  return (label: string, next: Next) => {
    const again = () => call(label, next);
    return again() + '|' + again();
  };
}

const wrapped = wrap((label: string, next: Next) => label + '/' + next());
console.log(wrapped('a', () => 'z'));
console.log(wrapped('b', () => 'y'));
