type Base = { test: (value: string) => boolean };
type Methods = { startsWith: (prefix: string) => Check; includes: (part: string) => Check };
export type Check = Base & Methods;
function make(test: (value: string) => boolean): Check {
  return {
    test,
    startsWith: (prefix) => make((value) => test(value) && value.startsWith(prefix)),
    includes: (part) => make((value) => test(value) && value.includes(part)),
  };
}
export const text: Check = make(() => true);
