// A program whose top level never awaits can still put work on the event
// loop through a function it calls. Node runs that work before exiting; the
// emitted `main` used to be synchronous when only a callee touched the loop,
// so a server started inside a helper panicked with "no reactor running" and a
// timer scheduled inside one never fired.
function schedule(label: string): void {
  setTimeout((): void => {
    console.log('fired', label);
  }, 0);
}

schedule('later');
console.log('main done');
