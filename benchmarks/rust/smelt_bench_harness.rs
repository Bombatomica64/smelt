//! Shared benchmark harness for the Rust side of the
//! "hand-written TS vs Smelt-generated Rust" comparison.
//!
//! This is the twin of `benchmarks/ts/harness.mjs`. The two must stay in lockstep:
//! identical PRNG, identical datasets, identical checksum folding, identical timing
//! protocol. A divergence in any of those makes the comparison meaningless, so the
//! checksum is reported on both sides and the orchestrator refuses to publish a row
//! whose checksums disagree.
//!
//! This file is injected into the *generated* crate by `benchmarks/prepare.py` rather
//! than living in it, so the generated output stays exactly what `smelt build` emits.

use super::*;

// ---------------------------------------------------------------------------
// Deterministic data generation. Mirrors harness.mjs exactly.
// ---------------------------------------------------------------------------

/// xorshift32, the same generator the JS harness uses.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 0x9e37_79b9 } else { seed })
    }
    pub fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }
}

/// The fixed vocabulary shared with the JS harness.
pub const VOCAB: [&str; 20] = [
    "alpha", "bravo_charlie", "delta-echo", "FoxTrot", "golf hotel",
    "india", "juliett_kilo", "lima-mike", "NovemberOscar", "papa quebec",
    "romeo", "sierra_tango", "uniform-victor", "WhiskeyXray", "yankee zulu",
    "someVeryLongIdentifierName", "HTTP_response_code", "parse-URL-string",
    "user_id", "created At",
];

/// `n` integers in [0, 1000), erased as JS numbers.
pub fn numbers(n: usize, seed: u32) -> Vec<SmeltUnknown> {
    let mut rng = Rng::new(seed);
    (0..n).map(|_| SmeltUnknown::Number((rng.next() % 1000) as f64)).collect()
}

/// `n` strings drawn from VOCAB, ~1/4 of them index-suffixed.
pub fn strings(n: usize, seed: u32) -> Vec<SmeltUnknown> {
    let mut rng = Rng::new(seed);
    (0..n)
        .map(|_| {
            let w = VOCAB[(rng.next() % VOCAB.len() as u32) as usize];
            let s = if rng.next() % 4 == 0 {
                format!("{}_{}", w, rng.next() % 100)
            } else {
                w.to_owned()
            };
            SmeltUnknown::String(s.into())
        })
        .collect()
}

/// `n` flat `{ id, group, value, flag }` records, erased as JS objects.
pub fn records(n: usize, seed: u32) -> Vec<SmeltUnknown> {
    let mut rng = Rng::new(seed);
    (0..n)
        .map(|i| {
            let group = VOCAB[(rng.next() % 8) as usize].to_owned();
            let value = (rng.next() % 10000) as f64;
            let flag = rng.next() % 2 == 0;
            SmeltUnknown::Object(SmeltObject::new(vec![
                ("id".to_owned(), SmeltUnknown::Number(i as f64)),
                ("group".to_owned(), SmeltUnknown::String(group.into())),
                ("value".to_owned(), SmeltUnknown::Number(value)),
                ("flag".to_owned(), SmeltUnknown::Bool(flag)),
            ]))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Checksums. 32-bit wrapping FNV-style folding, identical to the JS side.
// ---------------------------------------------------------------------------

pub fn mix(h: u32, x: u32) -> u32 {
    (h ^ x).wrapping_mul(0x0100_0193)
}

pub fn hash_number(h: u32, v: f64) -> u32 {
    // `as u32` on f64 saturates in Rust while JS `>>> 0` wraps modulo 2^32.
    // Every value these workloads produce is a small non-negative integer, so
    // the two agree; `trunc` first keeps the comparison honest for any that
    // arrive with a fractional part.
    mix(h, v.trunc() as i64 as u32)
}

pub fn hash_string(h: u32, s: &str) -> u32 {
    let mut acc = h;
    // JS `charCodeAt` yields UTF-16 code units, so fold code units, not chars.
    for unit in s.encode_utf16() {
        acc = mix(acc, unit as u32);
    }
    mix(acc, s.encode_utf16().count() as u32)
}

pub fn hash_bool(h: u32, b: bool) -> u32 {
    mix(h, if b { 1 } else { 0 })
}

/// Fold an arbitrary erased value. The JS side has no single twin for this;
/// each case's JS checksum function is written to fold the same sequence.
pub fn hash_unknown(h: u32, value: &SmeltUnknown) -> u32 {
    match value {
        SmeltUnknown::Number(v) => hash_number(h, *v),
        SmeltUnknown::String(s) => hash_string(h, s),
        SmeltUnknown::Bool(b) => hash_bool(h, *b),
        SmeltUnknown::Null | SmeltUnknown::Undefined => mix(h, 0xffff_ffff),
        SmeltUnknown::Array(a) => {
            let mut acc = mix(h, a.len() as u32);
            for v in a.iter() {
                acc = hash_unknown(acc, &v);
            }
            acc
        }
        SmeltUnknown::Object(o) => {
            let mut acc = mix(h, o.len() as u32);
            for (k, v) in o.iter() {
                acc = hash_string(acc, &k);
                acc = hash_unknown(acc, &v);
            }
            acc
        }
        _ => mix(h, 0xdead_beef),
    }
}

// ---------------------------------------------------------------------------
// Process-level footprint, read from the same procfs fields as the JS harness.
// ---------------------------------------------------------------------------

fn proc_status_kb(field: &str) -> i64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else { return -1 };
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            if let Some(kb) = rest.split_whitespace().next() {
                if let Ok(v) = kb.parse::<i64>() {
                    return v;
                }
            }
        }
    }
    -1
}

/// Peak resident set size in bytes (`VmHWM`).
pub fn peak_rss_bytes() -> i64 {
    let kb = proc_status_kb("VmHWM:");
    if kb < 0 { -1 } else { kb * 1024 }
}

/// Current resident set size in bytes (`VmRSS`).
pub fn current_rss_bytes() -> i64 {
    let kb = proc_status_kb("VmRSS:");
    if kb < 0 { -1 } else { kb * 1024 }
}

/// (user, system) CPU seconds from /proc/self/stat.
pub fn cpu_seconds() -> (f64, f64) {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else { return (-1.0, -1.0) };
    let Some(paren) = stat.rfind(')') else { return (-1.0, -1.0) };
    let tail: Vec<&str> = stat[paren + 2..].split(' ').collect();
    let hz = 100.0; // Linux USER_HZ
    let user = tail.get(11).and_then(|v| v.parse::<f64>().ok()).unwrap_or(-1.0);
    let sys = tail.get(12).and_then(|v| v.parse::<f64>().ok()).unwrap_or(-1.0);
    (user / hz, sys / hz)
}

/// Live entry counts of the runtime prelude's process-wide identity tables.
///
/// These exist so a benchmark can tell a workload's real memory footprint apart from
/// runtime bookkeeping that never gets released. `SMELT_LIST_IDENTITIES` and friends
/// are insert-only maps keyed on an allocation's address; nothing ever removes an
/// entry, so they grow for the lifetime of the process. If a case's peak RSS rises
/// with iteration count, these counters say whether that is why.
///
/// The JS side has no twin for this and does not need one — it is measuring the
/// generated runtime, not the language.
pub fn identity_table_sizes() -> (usize, usize, usize) {
    let lists = SMELT_LIST_IDENTITIES.with(|m| m.borrow().len());
    let promises = SMELT_PROMISE_IDENTITIES.with(|m| m.borrow().len());
    let functions = SMELT_FUNCTION_ORIGINS.with(|m| m.borrow().len());
    (lists, promises, functions)
}

// ---------------------------------------------------------------------------
// The timing protocol. Same phases, same budgets, same statistic as the JS side.
// ---------------------------------------------------------------------------

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub struct Measurement {
    pub ns_per_op_median: f64,
    pub ns_per_op_best: f64,
    pub ops_per_sec: f64,
    pub samples: usize,
    pub iterations: u64,
    pub checksum: u32,
}

/// Run `op` under the shared protocol and report per-op timings plus the
/// single-call checksum used for cross-language parity checking.
///
/// `op` returns the library call's result; `checksum_of` folds it into a u32. The
/// fold runs exactly ONCE, outside the timed region — walking a 10,000-element result
/// is real work, and charging it to the library would flatter or penalise whichever
/// side happens to walk structures faster. The JS harness does the same.
pub fn measure<T, F: FnMut() -> T, C: Fn(&T) -> u32>(mut op: F, checksum_of: C) -> Measurement {
    let warmup_ms = env_f64("SMELT_BENCH_WARMUP_MS", 500.0);
    let measure_ms = env_f64("SMELT_BENCH_MEASURE_MS", 3000.0);
    let min_samples = env_f64("SMELT_BENCH_MIN_SAMPLES", 10.0) as usize;
    // Hard ceiling on the sampling loop. Some generated cases take seconds per op,
    // where `min_samples` alone would mean minutes per case; the ceiling bounds the
    // sweep while still giving slow cases several samples. Mirrors the JS harness.
    let max_ms = env_f64("SMELT_BENCH_MAX_MS", 20000.0);

    // Reported checksum comes from one call, so it never depends on iteration count.
    let checksum = checksum_of(&op());

    // --- warmup, also used to size a batch ---
    let warm_start = std::time::Instant::now();
    let warm_budget = std::time::Duration::from_secs_f64(warmup_ms / 1000.0);
    let mut warm_iters: u64 = 0;
    while warm_start.elapsed() < warm_budget {
        std::hint::black_box(op());
        warm_iters += 1;
    }
    let ns_per_op_estimate =
        (warm_start.elapsed().as_nanos() as f64 / warm_iters.max(1) as f64).max(1.0);
    // Target ~1ms per batch, same clamp as the JS harness.
    let batch = ((1_000_000.0 / ns_per_op_estimate).round() as u64).clamp(1, 1 << 22);

    // --- measurement ---
    let mut samples: Vec<f64> = Vec::new();
    let mut total_iters: u64 = 0;
    let meas_start = std::time::Instant::now();
    let meas_budget = std::time::Duration::from_secs_f64(measure_ms / 1000.0);
    let hard_budget = std::time::Duration::from_secs_f64(max_ms / 1000.0);
    while (meas_start.elapsed() < meas_budget || samples.len() < min_samples)
        && meas_start.elapsed() < hard_budget
    {
        let t0 = std::time::Instant::now();
        for _ in 0..batch {
            std::hint::black_box(op());
        }
        samples.push(t0.elapsed().as_nanos() as f64 / batch as f64);
        total_iters += batch;
        if samples.len() > 10000 {
            break;
        }
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[samples.len() / 2];
    Measurement {
        ns_per_op_median: median,
        ns_per_op_best: samples[0],
        ops_per_sec: 1e9 / median,
        samples: samples.len(),
        iterations: total_iters,
        checksum,
    }
}

// ---------------------------------------------------------------------------
// Small helpers for calling into the generated library surface.
// ---------------------------------------------------------------------------

/// Wrap a Rust closure as an erased JS callable, the way generated code passes
/// callbacks to library functions like `sumBy` or `groupBy`.
pub fn js_fn<F>(f: F) -> SmeltUnknown
where
    F: Fn(Vec<SmeltUnknown>) -> SmeltUnknown + 'static,
{
    SmeltUnknown::Function(std::rc::Rc::new(move |args| Ok(f(args))))
}

/// Read argument `i` of an erased callback invocation.
pub fn arg(args: &[SmeltUnknown], i: usize) -> SmeltUnknown {
    args.get(i).cloned().unwrap_or(SmeltUnknown::Undefined)
}

/// Read a named field off an erased object, or `Undefined`.
pub fn field(value: &SmeltUnknown, key: &str) -> SmeltUnknown {
    match value {
        SmeltUnknown::Object(o) => o.get(key).unwrap_or(SmeltUnknown::Undefined),
        _ => SmeltUnknown::Undefined,
    }
}

/// Erase a `Vec<SmeltUnknown>` into a JS array value.
pub fn arr(values: Vec<SmeltUnknown>) -> SmeltUnknown {
    SmeltUnknown::Array(SmeltArray::new(values))
}

/// Build the positional argument list the generated data-first entry points take.
pub fn args(values: Vec<SmeltUnknown>) -> SmeltList<SmeltUnknown> {
    SmeltList::from(values)
}
