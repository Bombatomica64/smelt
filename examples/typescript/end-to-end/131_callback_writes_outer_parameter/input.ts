// A closure that assigns to an OPTIONAL PARAMETER of the enclosing arrow, and
// a second closure that reads it: both must share the parameter's storage.
const makeQueue = () => {
  const run = (value: number, promise?: Promise<number>, resolve?: (result: number) => void): Promise<number> => {
    if (value < 0) {
      promise ||= new Promise<number>((r) => (resolve = r));
      setTimeout(() => run(-value, promise, resolve));
      return promise;
    }
    if (resolve) {
      resolve(value * 10);
      return promise as Promise<number>;
    }
    return Promise.resolve(value);
  };
  return run;
};

const enqueue = makeQueue();
console.log(await enqueue(2));
console.log(await enqueue(-3));
