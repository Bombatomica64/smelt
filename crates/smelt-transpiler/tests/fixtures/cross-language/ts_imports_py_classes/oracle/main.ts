import { Counter } from "./counter.ts";
const c = new Counter(2);
c.inc();
c.stepSize = 3;
console.log(c.inc(), c.count, c.bumpBy(10), c.stepSize);
