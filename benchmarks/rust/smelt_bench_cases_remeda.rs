//! Remeda benchmark cases, Rust side.
//!
//! Each case here is the exact twin of a case in the remeda entry of `benchmarks/ts/cases.mjs`:
//! same input data, same library call, same checksum fold. Inputs are built once,
//! outside the timed closure, so what is measured is the library call and nothing else.
//!
//! Remeda's public surface is built on `purry`, the runtime data-first/data-last
//! dispatcher: `R.chunk(data, size)` and `R.chunk(size)(data)` are the same export,
//! told apart at runtime by inspecting `arguments`. Smelt lowers that faithfully, so
//! every entry point here has the shape
//! `fn(args: SmeltList<SmeltUnknown>) -> Result<SmeltUnknown, _>` -- the arguments
//! are erased because in the source they genuinely are. That is a property of
//! remeda's API design, not a Smelt lowering choice, and it is why the remeda rows
//! and the es-toolkit rows are not directly comparable to each other.

use super::smelt_bench_harness::*;
use super::smelt_bench_entry::*;
use super::*;

/// Fold an erased array-valued result.
fn hash_result(h: u32, value: &SmeltUnknown) -> u32 {
    hash_unknown(h, value)
}

const N: usize = 10_000;
const N_SMALL: usize = 2_000;
const N_STR: usize = 5_000;

/// Run one named case, or return `None` if this crate does not define it.
pub fn run_case(name: &str) -> Option<Measurement> {
    Some(match name {
        // --- array shaping ---
        "chunk" => {
            let data = arr(numbers(N, 1));
            measure(
                || entry_chunk(args(vec![data.clone(), SmeltUnknown::Number(7.0)])).unwrap(),
                |out| hash_result(0, out),
            )
        }
        "unique" => {
            let data = arr(numbers(N, 2));
            measure(|| entry_unique(args(vec![data.clone()])).unwrap(), |out| hash_result(0, out))
        }
        "flatten" => {
            let nested = arr((0..1000).map(|i| arr(numbers(10, 3 + i as u32))).collect());
            measure(|| entry_flat(Some(nested.clone()), Some(1.0)), |out| hash_result(0, out))
        }
        "zip" => {
            let a = arr(numbers(N, 4));
            let b = arr(numbers(N, 5));
            measure(
                || entry_zip(args(vec![a.clone(), b.clone()])).unwrap(),
                |out| hash_result(0, out),
            )
        }

        // --- set-like ---
        "difference" => {
            let a = arr(numbers(N / 2, 6));
            let b = arr(numbers(N / 2, 7));
            measure(
                || entry_difference(args(vec![a.clone(), b.clone()])).unwrap(),
                |out| hash_result(0, out),
            )
        }
        "intersection" => {
            let a = arr(numbers(N / 2, 6));
            let b = arr(numbers(N / 2, 7));
            measure(
                || entry_intersection(args(vec![a.clone(), b.clone()])).unwrap(),
                |out| hash_result(0, out),
            )
        }

        // --- keyed aggregation (callback-taking) ---
        "group_by" => {
            let data = arr(records(N, 8));
            measure(
                || {
                    let key = js_fn(|a: Vec<SmeltUnknown>| field(&arg(&a, 0), "group"));
                    entry_group_by(args(vec![data.clone(), key])).unwrap()
                },
                |out| hash_result(0, out),
            )
        }
        "count_by" => {
            let data = arr(records(N, 8));
            measure(
                || {
                    let key = js_fn(|a: Vec<SmeltUnknown>| field(&arg(&a, 0), "group"));
                    entry_count_by(args(vec![data.clone(), key])).unwrap()
                },
                |out| hash_result(0, out),
            )
        }
        "unique_by" => {
            let data = arr(records(N, 8));
            measure(
                || {
                    let key = js_fn(|a: Vec<SmeltUnknown>| field(&arg(&a, 0), "group"));
                    entry_unique_by(args(vec![data.clone(), key])).unwrap()
                },
                |out| hash_result(0, out),
            )
        }
        "sum_by" => {
            let data = arr(records(N, 8));
            measure(
                || {
                    let get = js_fn(|a: Vec<SmeltUnknown>| field(&arg(&a, 0), "value"));
                    entry_sum_by(args(vec![data.clone(), get])).unwrap()
                },
                |out| hash_result(0, out),
            )
        }
        "partition" => {
            let data = arr(records(N, 8));
            measure(
                || {
                    let pred = js_fn(|a: Vec<SmeltUnknown>| field(&arg(&a, 0), "flag"));
                    entry_partition(args(vec![data.clone(), pred])).unwrap()
                },
                |out| hash_result(0, out),
            )
        }
        "sort_by" => {
            let data = arr(records(N, 8));
            measure(
                || {
                    let key = js_fn(|a: Vec<SmeltUnknown>| field(&arg(&a, 0), "value"));
                    entry_sort_by(args(vec![data.clone(), key]))
                },
                |out| hash_result(0, out),
            )
        }

        // --- deep structural work ---
        "deep_equal" => {
            let a = arr(records(N_SMALL, 9));
            let b = arr(records(N_SMALL, 9));
            measure(
                || entry_is_deep_equal(args(vec![a.clone(), b.clone()])).unwrap(),
                |out| hash_result(0, out),
            )
        }
        "clone_deep" => {
            let a = arr(records(N_SMALL, 9));
            measure(|| entry_clone(args(vec![a.clone()])).unwrap(), |out| hash_result(0, out))
        }

        // --- strings ---
        "camel_case" => {
            let data = strings(N_STR, 10);
            measure(
                || data.iter().map(|s| entry_to_camel_case(s.clone(), None)).collect::<Vec<_>>(),
                |out: &Vec<SmeltUnknown>| {
                    let mut h = mix(0, out.len() as u32);
                    for s in out {
                        h = hash_unknown(h, s);
                    }
                    h
                },
            )
        }
        "kebab_case" => {
            let data = strings(N_STR, 10);
            measure(
                || {
                    data.iter()
                        .map(|s| entry_to_kebab_case(args(vec![s.clone()])).unwrap())
                        .collect::<Vec<_>>()
                },
                |out: &Vec<SmeltUnknown>| {
                    let mut h = mix(0, out.len() as u32);
                    for s in out {
                        h = hash_unknown(h, s);
                    }
                    h
                },
            )
        }

        _ => return None,
    })
}

/// Every case this crate knows how to run, in report order.
pub const CASES: &[&str] = &[
    "chunk", "unique", "flatten", "zip",
    "difference", "intersection",
    "group_by", "count_by", "unique_by", "sum_by", "partition", "sort_by",
    "deep_equal", "clone_deep",
    "camel_case", "kebab_case",
];
