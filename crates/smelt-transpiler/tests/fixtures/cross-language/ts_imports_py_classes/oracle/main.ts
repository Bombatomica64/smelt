import { Counter } from "./counter.ts";
const c = new Counter(2);
c.inc();
console.log(c.inc(), c.count);
