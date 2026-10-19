// A listener registered on the emitter it captures.
//
// `e.on('ping', () => e.emit('pong'))` stores the listener in `e` while the
// listener owns `e`: a cycle JavaScript collects and a reference-counted port
// must break (`emitter::weak_receiver`). The listener holds the emitter weakly
// and must still see it whenever it runs, including when it emits on it.
// It lives here rather than in `examples/` because an `EventEmitter`'s
// arguments cross its erased listener ABI, which the examples corpus counts.
import { EventEmitter } from "node:events";

function echo(): string[] {
  const seen: string[] = [];
  const e = new EventEmitter();
  e.on("ping", (name: string) => {
    seen.push(`ping ${name}`);
    e.emit("pong", name);
  });
  e.on("pong", (name: string) => {
    seen.push(`pong ${name}`);
  });
  e.emit("ping", "a");
  e.emit("ping", "b");
  return seen;
}

function attach(): EventEmitter {
  const e = new EventEmitter();
  e.once("ping", () => {
    e.emit("done");
  });
  return e;
}

console.log(echo().join(", "));
const kept = attach();
let done = 0;
kept.on("done", () => {
  done += 1;
});
kept.emit("ping");
kept.emit("ping");
console.log(done);
