// A module-level `const` arrow that reads a module binding with reference
// identity must keep that binding, not a copy of it.
//
// Such an arrow used to be lifted into a named MIR function when another
// closure referenced it by value. A named function has no capture environment,
// so the binding it read was re-materialized from its initializer inside the
// function: a second, private list. Every write through the lifted arrow landed
// there, and the module's list never saw it -- this program printed `first`
// where Node prints `first,second`. Nothing failed to compile and nothing
// reported a blocker.
//
// (The counter-case, an arrow that reads only scalars and is therefore still
// free to lift, is covered by `keeps_lifting_a_module_arrow_over_scalars` in
// `part_7_tests.rs`: a lifted item's Rust name embeds the source path, so it
// cannot appear in a checked-in `expected.rs`.)
const rem: string[] = [];

const second = (): void => {
  rem.push('second');
};

const outer = (): void => {
  rem.push('first');
  take(second);
};

function take(f: () => void): void {
  f();
}

outer();
console.log(rem.join(','));
