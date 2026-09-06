// A floating top-level promise. JavaScript does not exit while work is queued —
// Node drains its microtask queue and runs due timers before exiting — so
// `later();` still prints. Smelt returned from `main` the moment the module
// body ended, so this program printed only its synchronous lines and the
// queued work was silently discarded.
async function greet(name: string): Promise<string> {
  return `hello ${name}`;
}

async function later(): Promise<void> {
  const message = await greet("queued");
  console.log(message);
}

// Nothing awaits this: only the drain at exit can run it.
later();

console.log("sync tail");
