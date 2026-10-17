// A named function referenced as a value is ONE function object: every
// reference is `===` to every other. Typed references used to build a fresh
// closure each time, so `app.errorHandler === errorHandler` was false and Hono
// wrapped every mounted handler in an error-composing adapter.
type ErrorHandler = (err: string, tag: string) => string;

const defaultHandler = (err: string, tag: string): string => tag + ':' + err;

function namedHandler(err: string, tag: string): string {
  return tag + '!' + err;
}

class App {
  errorHandler: ErrorHandler = defaultHandler;
  other: ErrorHandler = namedHandler;
}

const a = new App();
console.log(a.errorHandler === defaultHandler, a.other === namedHandler, a.errorHandler === namedHandler);
const b = new App();
b.errorHandler = (err: string, tag: string): string => 'custom ' + tag + err;
console.log(b.errorHandler === defaultHandler, b.errorHandler('e', 't'), a.errorHandler('e', 't'));
