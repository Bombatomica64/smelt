//! es-toolkit benchmark cases, Rust side.
//!
//! Each case here is the exact twin of a case in the es-toolkit entry of `benchmarks/ts/cases.mjs`:
//! same input data, same library call, same checksum fold. Inputs are built once,
//! outside the timed closure, so what is measured is the library call and nothing else.
//!
//! es-toolkit's generated surface is mostly *typed* — `camelCase` comes out as
//! `fn(String) -> String`, `chunk` as `fn(SmeltList<SmeltUnknown>, f64)`, and there is
//! a family of generic
//! `fn f<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown>(...)`. Where a
//! function is generic, two cases are registered: the `SmeltUnknown` instantiation
//! that erased callers get, and (suffix `_typed`) the `f64` instantiation a caller
//! holding a concrete `number[]` gets. The gap between them is the cost of erasure.

use super::smelt_bench_harness::*;
use super::smelt_bench_entry::*;
use super::*;

/// Fold an erased list of erased values.
fn hash_list(h: u32, list: &SmeltList<SmeltUnknown>) -> u32 {
    let mut acc = mix(h, list.len() as u32);
    for v in list.iter() {
        acc = hash_unknown(acc, v);
    }
    acc
}

/// Fold a JS Map the way the JS side folds the object/Map it gets back:
/// insertion order, key then value.
fn hash_map_of_lists(h: u32, map: &SmeltJsMap<SmeltUnknown, SmeltList<SmeltUnknown>>) -> u32 {
    let mut acc = mix(h, map.len() as u32);
    for (k, v) in map.iter() {
        acc = hash_unknown(acc, &k);
        acc = hash_list(acc, &v);
    }
    acc
}

fn hash_map_of_numbers(h: u32, map: &SmeltJsMap<SmeltUnknown, f64>) -> u32 {
    let mut acc = mix(h, map.len() as u32);
    for (k, v) in map.iter() {
        acc = hash_unknown(acc, &k);
        acc = hash_number(acc, v);
    }
    acc
}

const N: usize = 10_000;
const N_SMALL: usize = 2_000;
const N_STR: usize = 5_000;

/// Run one named case, or return `None` if this crate does not define it.
pub fn run_case(name: &str) -> Option<Measurement> {
    Some(match name {
        // --- array shaping ---
        "chunk" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(numbers(N, 1));
            measure(
                || entry_chunk(data.clone(), 7.0).unwrap(),
                |out: &SmeltList<SmeltList<SmeltUnknown>>| {
                    let mut h = mix(0, out.len() as u32);
                    for inner in out.iter() {
                        h = hash_list(h, inner);
                    }
                    h
                },
            )
        }
        "unique" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(numbers(N, 2));
            measure(|| entry_uniq::<SmeltUnknown>(data.clone()), |out| hash_list(0, out))
        }
        "unique_typed" => {
            // The instantiation a caller with a real `number[]` gets: no tag, no
            // erasure, monomorphized comparisons.
            let data: SmeltList<f64> = SmeltList::from(
                numbers(N, 2).into_iter().map(|v| match v { SmeltUnknown::Number(n) => n, _ => 0.0 }).collect::<Vec<_>>(),
            );
            measure(
                || entry_uniq::<f64>(data.clone()),
                |out: &SmeltList<f64>| {
                    let mut h = mix(0, out.len() as u32);
                    for v in out.iter() {
                        h = hash_number(h, *v);
                    }
                    h
                },
            )
        }
        "flatten" => {
            let nested: SmeltList<SmeltUnknown> = SmeltList::from(
                (0..1000)
                    .map(|i| arr(numbers(10, 3 + i as u32)))
                    .collect::<Vec<_>>(),
            );
            measure(
                || entry_flatten(nested.clone(), Some(SmeltUnknown::Number(1.0))),
                |out| hash_list(0, out),
            )
        }
        "zip" => {
            let pair: SmeltList<SmeltList<SmeltUnknown>> = SmeltList::from(vec![
                SmeltList::from(numbers(N, 4)),
                SmeltList::from(numbers(N, 5)),
            ]);
            measure(
                || entry_zip(pair.clone()),
                |out: &SmeltList<SmeltList<SmeltUnknown>>| {
                    let mut h = mix(0, out.len() as u32);
                    for inner in out.iter() {
                        h = hash_list(h, inner);
                    }
                    h
                },
            )
        }

        // --- set-like ---
        "difference" => {
            let a: SmeltList<SmeltUnknown> = SmeltList::from(numbers(N / 2, 6));
            let b: SmeltList<SmeltUnknown> = SmeltList::from(numbers(N / 2, 7));
            measure(
                || entry_difference::<SmeltUnknown>(a.clone(), b.clone()),
                |out| hash_list(0, out),
            )
        }
        "intersection" => {
            let a: SmeltList<SmeltUnknown> = SmeltList::from(numbers(N / 2, 6));
            let b: SmeltList<SmeltUnknown> = SmeltList::from(numbers(N / 2, 7));
            measure(
                || entry_intersection::<SmeltUnknown>(a.clone(), b.clone()),
                |out| hash_list(0, out),
            )
        }

        // --- keyed aggregation (callback-taking) ---
        "group_by" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(records(N, 8));
            let key = |item: SmeltUnknown, _i: f64, _all: SmeltList<SmeltUnknown>| field(&item, "group");
            measure(|| entry_group_by(data.clone(), &key), |out| hash_map_of_lists(0, out))
        }
        "count_by" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(records(N, 8));
            let key = |item: SmeltUnknown, _i: f64, _all: SmeltList<SmeltUnknown>| field(&item, "group");
            measure(|| entry_count_by(data.clone(), &key), |out| hash_map_of_numbers(0, out))
        }
        "unique_by" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(records(N, 8));
            let key = |item: SmeltUnknown, _i: f64, _all: SmeltList<SmeltUnknown>| field(&item, "group");
            measure(|| entry_uniq_by(data.clone(), &key), |out| hash_list(0, out))
        }
        "sum_by" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(records(N, 8));
            let get = |item: SmeltUnknown, _i: f64| match field(&item, "value") {
                SmeltUnknown::Number(n) => n,
                _ => 0.0,
            };
            measure(
                || entry_sum_by::<SmeltUnknown, _>(data.clone(), &get),
                |out: &f64| hash_number(0, *out),
            )
        }
        "partition" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(records(N, 8));
            let pred = |item: SmeltUnknown, _i: f64, _all: SmeltList<SmeltUnknown>| {
                matches!(field(&item, "flag"), SmeltUnknown::Bool(true))
            };
            measure(
                || entry_partition::<SmeltUnknown, _>(data.clone(), &pred),
                // es-toolkit's `partition` returns a 2-tuple here but a 2-element
                // array in JS, so fold the outer length first to match the JS side's
                // `hashValue` on `[truthy, falsy]`.
                |(yes, no): &(SmeltList<SmeltUnknown>, SmeltList<SmeltUnknown>)| {
                    hash_list(hash_list(mix(0, 2), yes), no)
                },
            )
        }
        "sort_by" => {
            let data: SmeltList<SmeltUnknown> = SmeltList::from(records(N, 8));
            let criteria: SmeltList<SmeltUnknown> =
                SmeltList::from(vec![SmeltUnknown::String("value".to_owned())]);
            measure(
                || entry_sort_by(data.clone(), criteria.clone()),
                |out| hash_list(0, out),
            )
        }

        // --- deep structural work ---
        "deep_equal" => {
            let a = arr(records(N_SMALL, 9));
            let b = arr(records(N_SMALL, 9));
            measure(|| entry_is_equal(a.clone(), b.clone()), |out: &bool| hash_bool(0, *out))
        }
        "clone_deep" => {
            let a = arr(records(N_SMALL, 9));
            measure(|| entry_clone_deep(a.clone()).unwrap(), |out| hash_unknown(0, out))
        }

        // --- strings ---
        "camel_case" => {
            let data: Vec<String> = strings(N_STR, 10)
                .into_iter()
                .map(|v| match v { SmeltUnknown::String(s) => s, _ => String::new() })
                .collect();
            measure(
                || data.iter().map(|s| entry_camel_case(s.clone())).collect::<Vec<_>>(),
                |out: &Vec<String>| {
                    let mut h = mix(0, out.len() as u32);
                    for s in out {
                        h = hash_string(h, s);
                    }
                    h
                },
            )
        }
        "kebab_case" => {
            let data: Vec<String> = strings(N_STR, 10)
                .into_iter()
                .map(|v| match v { SmeltUnknown::String(s) => s, _ => String::new() })
                .collect();
            measure(
                || data.iter().map(|s| entry_kebab_case(s.clone())).collect::<Vec<_>>(),
                |out: &Vec<String>| {
                    let mut h = mix(0, out.len() as u32);
                    for s in out {
                        h = hash_string(h, s);
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
    "chunk", "unique", "unique_typed", "flatten", "zip",
    "difference", "intersection",
    "group_by", "count_by", "unique_by", "sum_by", "partition", "sort_by",
    "deep_equal", "clone_deep",
    "camel_case", "kebab_case",
];
