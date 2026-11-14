// Timer functions used as values (setTimeout, setInterval, clearInterval)
// fire like direct calls; every pending timer runs before exit; a binding a
// closure reads inside its own initializer is the binding's value.
const st = setTimeout;
st(() => console.log("fired"), 0);
const sched: (cb: () => void, ms: number) => unknown = setTimeout;
sched(() => console.log("fired2"), 10);
const later = (msg: string) => { setTimeout(() => console.log(msg), 20); };
later("from a closure");
const timers = { run: setTimeout };
timers.run(() => console.log("from a record"), 30);
const iv = setInterval;
let n = 0;
const h = iv(() => { n++; console.log("tick", n); if (n === 2) clearInterval(h as any); }, 12);
console.log("sync");
setTimeout(() => console.log("direct a"), 40);
setTimeout(() => console.log("direct b"), 50);
