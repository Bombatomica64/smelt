/**
 * Shared benchmark harness for the TypeScript (Node) side of the
 * "hand-written TS vs Smelt-generated Rust" comparison.
 *
 * Everything in this file has a byte-for-byte twin in `benchmarks/rust/smelt_bench_harness.rs`.
 * The two must stay in lockstep, because the comparison is only meaningful if both
 * sides see *identical* input data, do *identical* work, and are measured with an
 * *identical* protocol. Any divergence shows up as a checksum mismatch, which the
 * orchestrator treats as a hard failure rather than a result.
 */

/** xorshift32. Chosen because it is trivially reproducible in both JS and Rust:
 *  32-bit wrapping ops only, no floats, no library. */
export function makeRng(seed) {
  let s = seed >>> 0;
  if (s === 0) s = 0x9e3779b9;
  return function next() {
    s ^= (s << 13) >>> 0;
    s >>>= 0;
    s ^= s >>> 17;
    s ^= (s << 5) >>> 0;
    s >>>= 0;
    return s;
  };
}

/** A small fixed vocabulary so string workloads are deterministic and share
 *  the same distribution on both sides. */
export const VOCAB = [
  "alpha", "bravo_charlie", "delta-echo", "FoxTrot", "golf hotel",
  "india", "juliett_kilo", "lima-mike", "NovemberOscar", "papa quebec",
  "romeo", "sierra_tango", "uniform-victor", "WhiskeyXray", "yankee zulu",
  "someVeryLongIdentifierName", "HTTP_response_code", "parse-URL-string",
  "user_id", "created At",
];

/** `n` integers in [0, 1000) as JS numbers. Dup-heavy on purpose: the dedupe,
 *  group and set workloads are only interesting when collisions happen. */
export function numbers(n, seed) {
  const rng = makeRng(seed);
  const out = new Array(n);
  for (let i = 0; i < n; i++) out[i] = rng() % 1000;
  return out;
}

/** `n` strings drawn from VOCAB, with an index suffix on ~1/4 of them so the
 *  cardinality is high enough to matter but duplicates still occur. */
export function strings(n, seed) {
  const rng = makeRng(seed);
  const out = new Array(n);
  for (let i = 0; i < n; i++) {
    const w = VOCAB[rng() % VOCAB.length];
    out[i] = (rng() % 4 === 0) ? w + "_" + (rng() % 100) : w;
  }
  return out;
}

/** `n` flat records: the shape most `groupBy` / `sortBy` / `pick` workloads
 *  operate on in real code. */
export function records(n, seed) {
  const rng = makeRng(seed);
  const out = new Array(n);
  for (let i = 0; i < n; i++) {
    out[i] = {
      id: i,
      group: VOCAB[rng() % 8],
      value: rng() % 10000,
      flag: rng() % 2 === 0,
    };
  }
  return out;
}

// ---------------------------------------------------------------------------
// Checksums. These exist to (a) stop the optimizer deleting the benchmarked
// call and (b) prove the Rust and TS sides computed the same answer.
// All arithmetic is 32-bit wrapping so JS and Rust agree exactly.
// ---------------------------------------------------------------------------

/** Fold a u32 into a running 32-bit hash. */
export function mix(h, x) {
  return (Math.imul(h ^ (x >>> 0), 0x01000193) >>> 0);
}

export function hashNumber(h, v) {
  // Values in these workloads are integral and small; truncate to u32 so the
  // Rust side (which holds them as f64 too) folds the identical bits.
  return mix(h, Math.trunc(v) >>> 0);
}

export function hashString(h, s) {
  let acc = h;
  for (let i = 0; i < s.length; i++) acc = mix(acc, s.charCodeAt(i));
  return mix(acc, s.length);
}

export function hashBool(h, b) {
  return mix(h, b ? 1 : 0);
}

// ---------------------------------------------------------------------------
// Process-level footprint. Read straight out of procfs so the Node and Rust
// harnesses use the same source of truth rather than two different APIs with
// two different definitions of "memory used".
// ---------------------------------------------------------------------------

import { readFileSync } from "node:fs";

/** Peak resident set size of this process, in bytes (`VmHWM`). */
export function peakRssBytes() {
  try {
    const status = readFileSync("/proc/self/status", "utf8");
    const m = /^VmHWM:\s+(\d+)\s+kB/m.exec(status);
    return m ? Number(m[1]) * 1024 : -1;
  } catch {
    return -1;
  }
}

/** Current resident set size of this process, in bytes (`VmRSS`). */
export function currentRssBytes() {
  try {
    const status = readFileSync("/proc/self/status", "utf8");
    const m = /^VmRSS:\s+(\d+)\s+kB/m.exec(status);
    return m ? Number(m[1]) * 1024 : -1;
  } catch {
    return -1;
  }
}

/** (user, system) CPU seconds consumed by this process, from /proc/self/stat. */
export function cpuSeconds() {
  try {
    const stat = readFileSync("/proc/self/stat", "utf8");
    // Fields after the (possibly paren-wrapped) comm: utime is field 14, stime 15.
    const tail = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    const hz = 100; // Linux USER_HZ; constant on every platform we run on.
    return { user: Number(tail[11]) / hz, sys: Number(tail[12]) / hz };
  } catch {
    return { user: -1, sys: -1 };
  }
}

// ---------------------------------------------------------------------------
// The timing protocol.
// ---------------------------------------------------------------------------

const WARMUP_MS = Number(process.env.SMELT_BENCH_WARMUP_MS ?? 500);
const MEASURE_MS = Number(process.env.SMELT_BENCH_MEASURE_MS ?? 3000);
const MIN_SAMPLES = Number(process.env.SMELT_BENCH_MIN_SAMPLES ?? 10);
// Hard ceiling on the sampling loop. Some generated-Rust cases take seconds per op,
// where MIN_SAMPLES alone would mean minutes per case; the ceiling bounds the sweep
// while still giving slow cases several samples. Both harnesses use the same value.
const MAX_MS = Number(process.env.SMELT_BENCH_MAX_MS ?? 20000);

function nowNs() {
  return process.hrtime.bigint();
}

/**
 * Somewhere for results to go. Storing each result on a live global object keeps
 * the benchmarked call observable, so neither V8 nor rustc can delete it, without
 * adding per-iteration work of our own. The Rust side uses `std::hint::black_box`
 * for the same purpose.
 */
export const sinkHolder = { value: undefined };

/**
 * Run `op` under the shared protocol: warm up for WARMUP_MS, then collect batches
 * until MEASURE_MS has elapsed and at least MIN_SAMPLES batches exist. Batch size is
 * auto-scaled so a batch takes roughly a millisecond, which keeps clock overhead off
 * the measurement without letting a single batch swallow the whole budget.
 *
 * `op` returns the library call's result; `checksumOf` folds that result into a u32.
 * The fold runs exactly ONCE, outside the timed region — walking a 10,000-element
 * result is real work, and charging it to the library would flatter or penalise
 * whichever side happens to walk structures faster.
 *
 * Returns per-op nanoseconds (median and best) plus that checksum.
 */
export function measure(op, checksumOf) {
  // The reported checksum comes from a single call, so it never depends on how many
  // iterations happened — the two languages run different counts in the same budget.
  const checksum = checksumOf(op()) >>> 0;

  // --- warmup, also used to estimate a batch size ---
  let warmIters = 0;
  const warmStart = nowNs();
  const warmDeadline = warmStart + BigInt(WARMUP_MS) * 1000000n;
  while (nowNs() < warmDeadline) {
    sinkHolder.value = op();
    warmIters++;
  }
  const warmElapsed = Number(nowNs() - warmStart);
  const nsPerOpEstimate = Math.max(1, warmElapsed / Math.max(1, warmIters));
  // Target ~1ms per batch, clamped so we neither spin the clock nor make one
  // batch longer than the whole measurement window.
  const batch = Math.max(1, Math.min(1 << 22, Math.round(1000000 / nsPerOpEstimate)));

  // --- measurement ---
  const samples = [];
  let totalIters = 0;
  const measStart = nowNs();
  const measDeadline = measStart + BigInt(MEASURE_MS) * 1000000n;
  const hardDeadline = measStart + BigInt(MAX_MS) * 1000000n;
  while ((nowNs() < measDeadline || samples.length < MIN_SAMPLES) && nowNs() < hardDeadline) {
    const t0 = nowNs();
    for (let i = 0; i < batch; i++) sinkHolder.value = op();
    const t1 = nowNs();
    samples.push(Number(t1 - t0) / batch);
    totalIters += batch;
    if (samples.length > 10000) break; // pathological safety valve
  }

  samples.sort((a, b) => a - b);
  const median = samples[Math.floor(samples.length / 2)];
  return {
    ns_per_op_median: median,
    ns_per_op_best: samples[0],
    ops_per_sec: 1e9 / median,
    samples: samples.length,
    iterations: totalIters,
    checksum: checksum >>> 0,
  };
}
