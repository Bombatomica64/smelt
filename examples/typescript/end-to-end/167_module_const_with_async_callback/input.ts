// A top-level `const` whose initializer passes an async callback.
//
// A module-level constant's initializer is lowered once to learn its type
// before the declaration itself is lowered. A closure inside it used to leave
// an orphan body behind from that first pass, and for an ASYNC closure the
// orphan's `await` sat in a function nothing marked async, so the whole build
// failed. The callback here is stored, then awaited, so its behavior is also
// checked.
type Job = { run: (input: string) => Promise<string>; label: string };

function makeJob(job: Job): Job {
  return job;
}

const job = makeJob({
  label: "upper",
  run: async (input: string): Promise<string> => {
    const trimmed = await Promise.resolve(input.trim());
    return trimmed.toUpperCase();
  },
});

console.log(job.label, await job.run("  hello  "));
