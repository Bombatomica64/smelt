// An overloaded callable-interface FIELD is called through the overload the
// arguments select, and a function stored into it is kept.
//
// Hono's `app.use` is a class field typed `MiddlewareHandlerInterface`, whose
// first overload is `(...handlers: Handler[])` and whose fourth is
// `(path: string, handler: Handler)`. Two rules meet here:
//
// * the constructor's `this.use = (arg1, ...handlers) => ..` stores a function
//   into the callable-interface record's `__smelt_call` slot (it used to be
//   replaced by the record's default, so every call did nothing);
// * `app.use('/p', h)` selects its overload from the argument count AND the
//   rest element type — a string is not a `Handler`, so the leading rest
//   overload does not claim it (the string was packed into the handler list,
//   E0308).
//
// Every line below is diffed against Node.

type Handler = (c: string) => string;

interface UseInterface {
  (...handlers: Handler[]): App;
  (path: string, handler: Handler): App;
}

class App {
  log: string[] = [];
  use: UseInterface;
  constructor() {
    this.use = ((arg1: string | Handler, ...handlers: Handler[]) => {
      if (typeof arg1 === 'string') {
        this.log.push('path ' + arg1 + ' ' + handlers.length);
      } else {
        this.log.push('handlers ' + (handlers.length + 1));
      }
      return this;
    }) as UseInterface;
  }
}

const app = new App();
const h: Handler = (c) => c;
app.use('/p', h);
app.use(h);
app.use(h, h);
console.log(app.log.join(','));
