// `setTimeout(callback)` omits the delay, which defaults to 0: the callback
// runs after the current synchronous work, ordered with other zero timers.
const log: string[] = [];

setTimeout(() => {
  log.push("first timer");
});
setTimeout(() => {
  log.push("second timer");
  console.log(log.join(", "));
}, 0);
log.push("sync");
console.log(log.join(", "));
