/**
 * TypeScript benchmark cases for both libraries.
 *
 * Every case here is the twin of a case in `benchmarks/rust/smelt_bench_cases_*.rs`:
 * same input data (same PRNG, same seeds, same sizes), the same library call, and a
 * checksum folded in the same order over the same values. Inputs are built once,
 * outside the timed closure, so what is measured is the library call and nothing else.
 *
 * The library under test is the *bundled* source at the pinned ref (see
 * `benchmarks/ts/vendor/`), not the npm release, so both sides start from identical
 * TypeScript.
 */

import {
  mix, hashNumber, hashString, hashBool, numbers, strings, records, measure,
} from "./harness.mjs";

const N = 10000;
const N_SMALL = 2000;
const N_STR = 5000;

/**
 * Fold an arbitrary JS value exactly the way `hash_unknown` folds a `SmeltUnknown`:
 * length first, then elements / entries in iteration order.
 */
export function hashValue(h, v) {
  if (v === null || v === undefined) return mix(h, 0xffffffff);
  if (typeof v === "number") return hashNumber(h, v);
  if (typeof v === "string") return hashString(h, v);
  if (typeof v === "boolean") return hashBool(h, v);
  if (Array.isArray(v)) {
    let acc = mix(h, v.length);
    for (const item of v) acc = hashValue(acc, item);
    return acc;
  }
  if (v instanceof Map) {
    let acc = mix(h, v.size);
    for (const [k, val] of v) {
      acc = hashValue(acc, k);
      acc = hashValue(acc, val);
    }
    return acc;
  }
  if (typeof v === "object") {
    const keys = Object.keys(v);
    let acc = mix(h, keys.length);
    for (const k of keys) {
      acc = hashString(acc, k);
      acc = hashValue(acc, v[k]);
    }
    return acc;
  }
  return mix(h, 0xdeadbeef);
}

/** Nested input shared by the `flatten` case on both sides. */
function nested() {
  return Array.from({ length: 1000 }, (_, i) => numbers(10, 3 + i));
}

/**
 * Build the case table for one library.
 *
 * `lib` is the imported module namespace; `names` maps the shared case id to that
 * library's spelling of the function, because remeda and es-toolkit disagree on
 * names for the same operation (`unique`/`uniq`, `isDeepEqual`/`isEqual`,
 * `toCamelCase`/`camelCase`, ...).
 */
export function buildCases(lib, names) {
  const f = (id) => {
    const fn = lib[names[id]];
    if (typeof fn !== "function") throw new Error(`missing export ${names[id]} for case ${id}`);
    return fn;
  };
  const cases = {};

  // --- array shaping ---
  cases.chunk = () => {
    const chunk = f("chunk");
    const data = numbers(N, 1);
    return measure(() => chunk(data, 7), (out) => hashValue(0, out));
  };
  cases.unique = () => {
    const unique = f("unique");
    const data = numbers(N, 2);
    return measure(() => unique(data), (out) => hashValue(0, out));
  };
  cases.flatten = () => {
    const flatten = f("flatten");
    const data = nested();
    return measure(() => flatten(data, 1), (out) => hashValue(0, out));
  };
  cases.zip = () => {
    const zip = f("zip");
    const a = numbers(N, 4);
    const b = numbers(N, 5);
    return measure(() => zip(a, b), (out) => hashValue(0, out));
  };

  // --- set-like ---
  cases.difference = () => {
    const difference = f("difference");
    const a = numbers(N / 2, 6);
    const b = numbers(N / 2, 7);
    return measure(() => difference(a, b), (out) => hashValue(0, out));
  };
  cases.intersection = () => {
    const intersection = f("intersection");
    const a = numbers(N / 2, 6);
    const b = numbers(N / 2, 7);
    return measure(() => intersection(a, b), (out) => hashValue(0, out));
  };

  // --- keyed aggregation (callback-taking) ---
  cases.group_by = () => {
    const groupBy = f("group_by");
    const data = records(N, 8);
    return measure(() => groupBy(data, (x) => x.group), (out) => hashValue(0, out));
  };
  cases.count_by = () => {
    const countBy = f("count_by");
    const data = records(N, 8);
    return measure(() => countBy(data, (x) => x.group), (out) => hashValue(0, out));
  };
  cases.unique_by = () => {
    const uniqueBy = f("unique_by");
    const data = records(N, 8);
    return measure(() => uniqueBy(data, (x) => x.group), (out) => hashValue(0, out));
  };
  cases.sum_by = () => {
    const sumBy = f("sum_by");
    const data = records(N, 8);
    return measure(() => sumBy(data, (x) => x.value), (out) => hashValue(0, out));
  };
  cases.partition = () => {
    const partition = f("partition");
    const data = records(N, 8);
    return measure(() => partition(data, (x) => x.flag), (out) => hashValue(0, out));
  };

  // --- deep structural work ---
  cases.deep_equal = () => {
    const isEqual = f("deep_equal");
    const a = records(N_SMALL, 9);
    const b = records(N_SMALL, 9);
    return measure(() => isEqual(a, b), (out) => hashValue(0, out));
  };
  cases.clone_deep = () => {
    const cloneDeep = f("clone_deep");
    const a = records(N_SMALL, 9);
    return measure(() => cloneDeep(a), (out) => hashValue(0, out));
  };

  // --- strings ---
  cases.camel_case = () => {
    const camelCase = f("camel_case");
    const data = strings(N_STR, 10);
    return measure(
      () => data.map((s) => camelCase(s)),
      (out) => {
        let h = mix(0, out.length);
        for (const s of out) h = hashString(h, s);
        return h;
      },
    );
  };
  cases.kebab_case = () => {
    const kebabCase = f("kebab_case");
    const data = strings(N_STR, 10);
    return measure(
      () => data.map((s) => kebabCase(s)),
      (out) => {
        let h = mix(0, out.length);
        for (const s of out) h = hashString(h, s);
        return h;
      },
    );
  };

  return cases;
}
