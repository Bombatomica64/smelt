import { three } from "./helpers.ts";

function countdown(from: number): Iterator<number, void, undefined> {
  let current = from;
  return {
    next(): IteratorResult<number, void> {
      if (current <= 0) {
        return { done: true, value: undefined };
      }
      current -= 1;
      return { done: false, value: current + 1 };
    },
  };
}
const steps = countdown(three);
console.log(steps.next().value, steps.next().value, steps.next().value, steps.next().done);
