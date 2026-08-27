/**
 * Node benchmark runner: one library, one case, one process.
 *
 *   node run.mjs list <lib>          -> print the case names this library can run
 *   node run.mjs run <lib> <case>    -> run one case, print a single JSON object
 *   node run.mjs baseline <lib>      -> import the library, do nothing, report footprint
 *
 * One case per process, deliberately: peak RSS is a process-lifetime high-water mark,
 * so running several cases in one process would attribute the largest case's memory to
 * all of them. It also gives every case a cold, unpolluted JIT — the same starting
 * conditions the Rust binary gets. The Rust runner has the same rule.
 */

import { buildCases, hashValue } from "./cases.mjs";
import { measure, records, peakRssBytes, currentRssBytes, cpuSeconds, mix } from "./harness.mjs";

/** Shared case id -> each library's spelling of the same operation. */
const NAMES = {
  remeda: {
    chunk: "chunk", unique: "unique", flatten: "flat", zip: "zip",
    difference: "difference", intersection: "intersection",
    group_by: "groupBy", count_by: "countBy", unique_by: "uniqueBy",
    sum_by: "sumBy", partition: "partition", sort_by: "sortBy",
    deep_equal: "isDeepEqual", clone_deep: "clone",
    camel_case: "toCamelCase", kebab_case: "toKebabCase",
  },
  "es-toolkit": {
    chunk: "chunk", unique: "uniq", flatten: "flatten", zip: "zip",
    difference: "difference", intersection: "intersection",
    group_by: "groupBy", count_by: "countBy", unique_by: "uniqBy",
    sum_by: "sumBy", partition: "partition", sort_by: "sortBy",
    deep_equal: "isEqual", clone_deep: "cloneDeep",
    camel_case: "camelCase", kebab_case: "kebabCase",
  },
};

const VENDOR = {
  remeda: "./vendor/remeda.mjs",
  "es-toolkit": "./vendor/es-toolkit.mjs",
};

const N = 10000;

/**
 * `sortBy` is the one case whose *call shape* differs between the libraries:
 * remeda takes a key function, es-toolkit takes a list of criteria. Both sides'
 * Rust twins call the same shapes, so this is where the difference lives.
 */
function sortByCase(libName, lib) {
  const data = records(N, 8);
  if (libName === "remeda") {
    return measure(() => lib.sortBy(data, (x) => x.value), (out) => hashValue(0, out));
  }
  return measure(() => lib.sortBy(data, ["value"]), (out) => hashValue(0, out));
}

async function main() {
  const [cmd, libName, caseName] = process.argv.slice(2);
  if (!VENDOR[libName]) {
    console.error(`unknown library: ${libName}`);
    process.exit(2);
  }
  const lib = await import(VENDOR[libName]);

  if (cmd === "baseline") {
    const cpu = cpuSeconds();
    console.log(JSON.stringify({
      kind: "baseline",
      peak_rss_bytes: peakRssBytes(),
      rss_bytes: currentRssBytes(),
      cpu_user_s: cpu.user,
      cpu_sys_s: cpu.sys,
    }));
    return;
  }

  const cases = buildCases(lib, NAMES[libName]);
  cases.sort_by = () => sortByCase(libName, lib);

  if (cmd === "list") {
    for (const name of Object.keys(cases)) console.log(name);
    return;
  }

  const run = cases[caseName];
  if (!run) {
    console.error(`unknown case: ${caseName}`);
    process.exit(2);
  }
  const m = run();
  const cpu = cpuSeconds();
  console.log(JSON.stringify({
    kind: "result",
    case: caseName,
    ...m,
    peak_rss_bytes: peakRssBytes(),
    rss_bytes: currentRssBytes(),
    cpu_user_s: cpu.user,
    cpu_sys_s: cpu.sys,
  }));
}

main().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exit(1);
});
