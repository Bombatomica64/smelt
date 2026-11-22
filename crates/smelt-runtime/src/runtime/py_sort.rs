// Python's `list.sort` and `sorted`: a port of CPython 3.14's `listsort`
// (`Objects/listobject.c`), the adaptive merge sort ("timsort" with the
// powersort merge policy) that compares with `<` alone.
//
// A sort over a total order has one stable answer, which Rust's own stable
// sort already gives. Python's floats are not totally ordered: every
// comparison with a NaN is false, and the order `sorted([3.0, nan, 1.0])`
// leaves is whatever CPython's algorithm does with those answers. Following
// that order means running the same algorithm, comparison for comparison:
// the same run detection (`count_run`), binary insertion (`binarysort`),
// galloping merges (`merge_lo`/`merge_hi`, `gallop_left`/`gallop_right`,
// `min_gallop`) and merge schedule (`powerloop`, `found_new_run`,
// `merge_force_collapse`). Each function below names its CPython original.

// @smelt:item smelt_py_sort when py_sort
/// Sorts `items` in place as `CPython`'s `list.sort(reverse=...)` does,
/// comparing with `lt` (Python's `<`) only. `reverse` reverses the list
/// before and after an ascending sort, so equal items keep their order.
#[allow(dead_code)]
fn smelt_py_sort<T: Clone>(items: &mut [T], reverse: bool, mut lt: impl FnMut(&T, &T) -> bool) {
    if items.len() < 2 {
        return;
    }
    if reverse {
        items.reverse();
    }
    SmeltPySortState { lt: &mut lt, min_gallop: SMELT_PY_SORT_MIN_GALLOP, pending: Vec::new(), list_len: items.len() }.sort(items);
    if reverse {
        items.reverse();
    }
}

/// `list.sort(key=key, reverse=...)`: every key is computed once, in list
/// order, before any comparison (a raising key leaves the list untouched),
/// then the items sort by their keys as [`smelt_py_sort`] sorts.
#[allow(dead_code)]
fn smelt_py_sort_by_key<T: Clone, K: Clone>(
    items: &mut Vec<T>,
    reverse: bool,
    mut key: impl FnMut(&T) -> K,
    mut lt: impl FnMut(&K, &K) -> bool,
) {
    let mut keyed: Vec<(K, T)> = items.iter().map(|item| (key(item), item.clone())).collect();
    smelt_py_sort(&mut keyed, reverse, |left, right| lt(&left.0, &right.0));
    items.clear();
    items.extend(keyed.into_iter().map(|(_, item)| item));
}

/// `MIN_GALLOP`: the initial number of consecutive wins that switches a
/// merge into galloping mode.
const SMELT_PY_SORT_MIN_GALLOP: usize = 7;

/// `MAX_MINRUN`: runs shorter than `minrun` are extended by binary insertion.
const SMELT_PY_SORT_MAX_MINRUN: usize = 64;

/// A pending run (`struct s_slice`): where it starts, its length, and its
/// powersort node power.
#[derive(Clone, Copy)]
struct SmeltPySortRun {
    /// Index of the run's first item.
    base: usize,
    /// Number of items in the run.
    len: usize,
    /// The run's power (`powerloop`), compared when a new run arrives.
    power: u32,
}

/// `MergeState`: the comparison, the galloping threshold and the run stack.
struct SmeltPySortState<'a, T> {
    /// Python's `<` on two items.
    lt: &'a mut dyn FnMut(&T, &T) -> bool,
    /// The adaptive galloping threshold (`ms->min_gallop`).
    min_gallop: usize,
    /// Runs found and not yet merged (`ms->pending`).
    pending: Vec<SmeltPySortRun>,
    /// Length of the whole list (`ms->listlen`).
    list_len: usize,
}

impl<T: Clone> SmeltPySortState<'_, T> {
    /// The main loop of `listsort_impl`: find each run, extend a short one to
    /// `minrun`, push it while keeping the powersort invariant, then merge
    /// everything left.
    fn sort(&mut self, items: &mut [T]) {
        let minrun = smelt_py_sort_minrun(items.len());
        let mut lo = 0;
        let mut remaining = items.len();
        while remaining > 0 {
            let mut run = smelt_py_sort_count_run(self.lt, &mut items[lo..lo + remaining]);
            if run < minrun {
                let force = remaining.min(minrun);
                smelt_py_sort_binarysort(self.lt, &mut items[lo..lo + force], run);
                run = force;
            }
            self.found_new_run(items, run);
            self.pending.push(SmeltPySortRun { base: lo, len: run, power: 0 });
            lo += run;
            remaining -= run;
        }
        // `merge_force_collapse`.
        while self.pending.len() > 1 {
            let mut at = self.pending.len() - 2;
            if at > 0 && self.pending[at - 1].len < self.pending[at + 1].len {
                at -= 1;
            }
            self.merge_at(items, at);
        }
    }

    /// `found_new_run`: merge the stack's top runs while their power exceeds
    /// the power of the boundary the new run of `new_len` items makes.
    fn found_new_run(&mut self, items: &mut [T], new_len: usize) {
        let Some(last) = self.pending.last().copied() else {
            return;
        };
        let power = smelt_py_sort_powerloop(last.base, last.len, new_len, self.list_len);
        while self.pending.len() > 1 && self.pending[self.pending.len() - 2].power > power {
            let at = self.pending.len() - 2;
            self.merge_at(items, at);
        }
        if let Some(top) = self.pending.last_mut() {
            top.power = power;
        }
    }

    /// `merge_at`: merge the pending runs at `at` and `at + 1`, first
    /// trimming the items already in place at either end by galloping.
    fn merge_at(&mut self, items: &mut [T], at: usize) {
        let SmeltPySortRun { base: mut base_a, len: mut len_a, .. } = self.pending[at];
        let SmeltPySortRun { base: base_b, len: len_b, .. } = self.pending[at + 1];
        self.pending[at].len = len_a + len_b;
        self.pending.remove(at + 1);
        let skip = smelt_py_sort_gallop_right(self.lt, &items[base_b], &items[base_a..base_a + len_a], 0);
        base_a += skip;
        len_a -= skip;
        if len_a == 0 {
            return;
        }
        let len_b = smelt_py_sort_gallop_left(self.lt, &items[base_a + len_a - 1], &items[base_b..base_b + len_b], len_b - 1);
        if len_b == 0 {
            return;
        }
        if len_a <= len_b {
            self.merge_lo(items, base_a, len_a, len_b);
        } else {
            self.merge_hi(items, base_a, len_a, len_b);
        }
    }

    /// `merge_lo`: merge the adjacent runs `a` (`len_a` items from `base_a`,
    /// the shorter) and `b` that follows it, left to right, with `a` copied
    /// aside.
    fn merge_lo(&mut self, items: &mut [T], base_a: usize, mut len_a: usize, mut len_b: usize) {
        let aside: Vec<T> = items[base_a..base_a + len_a].to_vec();
        let mut dest = base_a;
        let mut pa = 0;
        let mut pb = base_a + len_a;
        items[dest] = items[pb].clone();
        dest += 1;
        pb += 1;
        len_b -= 1;
        // `true` when the merge ends by copying `b`'s rest then `a`'s last
        // item (`CopyB`), `false` when `a`'s rest closes it (`Succeed`).
        let copy_b = 'merge: {
            if len_b == 0 {
                break 'merge false;
            }
            if len_a == 1 {
                break 'merge true;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut a_count = 0;
                let mut b_count = 0;
                loop {
                    if (self.lt)(&items[pb], &aside[pa]) {
                        items[dest] = items[pb].clone();
                        dest += 1;
                        pb += 1;
                        b_count += 1;
                        a_count = 0;
                        len_b -= 1;
                        if len_b == 0 {
                            break 'merge false;
                        }
                        if b_count >= min_gallop {
                            break;
                        }
                    } else {
                        items[dest] = aside[pa].clone();
                        dest += 1;
                        pa += 1;
                        a_count += 1;
                        b_count = 0;
                        len_a -= 1;
                        if len_a == 1 {
                            break 'merge true;
                        }
                        if a_count >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                loop {
                    min_gallop -= usize::from(min_gallop > 1);
                    self.min_gallop = min_gallop;
                    let k = smelt_py_sort_gallop_right(self.lt, &items[pb], &aside[pa..pa + len_a], 0);
                    a_count = k;
                    if k > 0 {
                        items[dest..dest + k].clone_from_slice(&aside[pa..pa + k]);
                        dest += k;
                        pa += k;
                        len_a -= k;
                        if len_a == 1 {
                            break 'merge true;
                        }
                        if len_a == 0 {
                            break 'merge false;
                        }
                    }
                    items[dest] = items[pb].clone();
                    dest += 1;
                    pb += 1;
                    len_b -= 1;
                    if len_b == 0 {
                        break 'merge false;
                    }
                    let k = smelt_py_sort_gallop_left(self.lt, &aside[pa], &items[pb..pb + len_b], 0);
                    b_count = k;
                    if k > 0 {
                        for offset in 0..k {
                            items[dest + offset] = items[pb + offset].clone();
                        }
                        dest += k;
                        pb += k;
                        len_b -= k;
                        if len_b == 0 {
                            break 'merge false;
                        }
                    }
                    items[dest] = aside[pa].clone();
                    dest += 1;
                    pa += 1;
                    len_a -= 1;
                    if len_a == 1 {
                        break 'merge true;
                    }
                    if a_count < SMELT_PY_SORT_MIN_GALLOP && b_count < SMELT_PY_SORT_MIN_GALLOP {
                        break;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        if copy_b {
            for offset in 0..len_b {
                items[dest + offset] = items[pb + offset].clone();
            }
            items[dest + len_b] = aside[pa].clone();
        } else if len_a > 0 {
            items[dest..dest + len_a].clone_from_slice(&aside[pa..pa + len_a]);
        }
    }

    /// `merge_hi`: merge the adjacent runs `a` (`len_a` items from `base_a`)
    /// and the shorter `b` that follows it, right to left, with `b` copied
    /// aside. Positions are signed: a cursor steps one before its run's start
    /// as the run empties.
    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    fn merge_hi(&mut self, items: &mut [T], base_a: usize, mut len_a: usize, mut len_b: usize) {
        let base_b = base_a + len_a;
        let aside: Vec<T> = items[base_b..base_b + len_b].to_vec();
        let at = |index: isize| index as usize;
        let mut dest = (base_b + len_b - 1) as isize;
        let mut pa = (base_a + len_a - 1) as isize;
        let mut pb = (len_b - 1) as isize;
        items[at(dest)] = items[at(pa)].clone();
        dest -= 1;
        pa -= 1;
        len_a -= 1;
        // `true` when the merge ends by moving `a`'s rest then `b`'s first
        // item (`CopyA`), `false` when `b`'s rest closes it (`Succeed`).
        let copy_a = 'merge: {
            if len_a == 0 {
                break 'merge false;
            }
            if len_b == 1 {
                break 'merge true;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut a_count = 0;
                let mut b_count = 0;
                loop {
                    if (self.lt)(&aside[at(pb)], &items[at(pa)]) {
                        items[at(dest)] = items[at(pa)].clone();
                        dest -= 1;
                        pa -= 1;
                        a_count += 1;
                        b_count = 0;
                        len_a -= 1;
                        if len_a == 0 {
                            break 'merge false;
                        }
                        if a_count >= min_gallop {
                            break;
                        }
                    } else {
                        items[at(dest)] = aside[at(pb)].clone();
                        dest -= 1;
                        pb -= 1;
                        b_count += 1;
                        a_count = 0;
                        len_b -= 1;
                        if len_b == 1 {
                            break 'merge true;
                        }
                        if b_count >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                loop {
                    min_gallop -= usize::from(min_gallop > 1);
                    self.min_gallop = min_gallop;
                    let k = len_a - smelt_py_sort_gallop_right(self.lt, &aside[at(pb)], &items[base_a..base_a + len_a], len_a - 1);
                    a_count = k;
                    if k > 0 {
                        dest -= k as isize;
                        pa -= k as isize;
                        for offset in (1..=k as isize).rev() {
                            items[at(dest + offset)] = items[at(pa + offset)].clone();
                        }
                        len_a -= k;
                        if len_a == 0 {
                            break 'merge false;
                        }
                    }
                    items[at(dest)] = aside[at(pb)].clone();
                    dest -= 1;
                    pb -= 1;
                    len_b -= 1;
                    if len_b == 1 {
                        break 'merge true;
                    }
                    let k = len_b - smelt_py_sort_gallop_left(self.lt, &items[at(pa)], &aside[..len_b], len_b - 1);
                    b_count = k;
                    if k > 0 {
                        dest -= k as isize;
                        pb -= k as isize;
                        let (to, from) = (at(dest + 1), at(pb + 1));
                        items[to..to + k].clone_from_slice(&aside[from..from + k]);
                        len_b -= k;
                        if len_b == 1 {
                            break 'merge true;
                        }
                        if len_b == 0 {
                            break 'merge false;
                        }
                    }
                    items[at(dest)] = items[at(pa)].clone();
                    dest -= 1;
                    pa -= 1;
                    len_a -= 1;
                    if len_a == 0 {
                        break 'merge false;
                    }
                    if a_count < SMELT_PY_SORT_MIN_GALLOP && b_count < SMELT_PY_SORT_MIN_GALLOP {
                        break;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        if copy_a {
            for offset in (1 - len_a as isize..=0).rev() {
                items[at(dest + offset)] = items[at(pa + offset)].clone();
            }
            dest -= len_a as isize;
            items[at(dest)] = aside[0].clone();
        } else if len_b > 0 {
            let to = at(dest - (len_b as isize - 1));
            items[to..to + len_b].clone_from_slice(&aside[..len_b]);
        }
    }
}

/// `merge_compute_minrun`: the run length every short run is extended to.
fn smelt_py_sort_minrun(mut len: usize) -> usize {
    let mut extra = 0;
    while len >= SMELT_PY_SORT_MAX_MINRUN {
        extra |= len & 1;
        len >>= 1;
    }
    len + extra
}

/// `powerloop`: the powersort power of the boundary between the run of `n1`
/// items at `s1` and the `n2` items after it, in a list of `n` items.
fn smelt_py_sort_powerloop(s1: usize, n1: usize, n2: usize, n: usize) -> u32 {
    let mut result = 0;
    let mut a = 2 * s1 + n1;
    let mut b = a + n1 + n2;
    loop {
        result += 1;
        if a >= n {
            a -= n;
            b -= n;
        } else if b >= n {
            break;
        }
        a <<= 1;
        b <<= 1;
    }
    result
}

/// `count_run`: the length of the run at the start of `items`, made
/// ascending in place. A descending stretch is reversed, with each block of
/// equal items reversed first so the reversal keeps them in order.
fn smelt_py_sort_count_run<T>(lt: &mut dyn FnMut(&T, &T) -> bool, items: &mut [T]) -> usize {
    let remaining = items.len();
    let mut n = 1;
    while n < remaining {
        if lt(&items[n], &items[n - 1]) {
            break;
        }
        n += 1;
    }
    if n == remaining {
        return n;
    }
    if n > 1 {
        if lt(&items[0], &items[n - 1]) {
            return n;
        }
        items[..n].reverse();
    }
    n += 1;
    let mut equal = 0;
    while n < remaining {
        if lt(&items[n], &items[n - 1]) {
            if equal > 0 {
                items[n - equal - 1..n].reverse();
                equal = 0;
            }
        } else if lt(&items[n - 1], &items[n]) {
            break;
        } else {
            equal += 1;
        }
        n += 1;
    }
    if equal > 0 {
        items[n - equal - 1..n].reverse();
    }
    items[..n].reverse();
    while n < remaining {
        if lt(&items[n], &items[n - 1]) {
            break;
        }
        n += 1;
    }
    n
}

/// `binarysort`: extend the sorted prefix of `sorted` items to all of
/// `items` by binary insertion, each new item after any it compares equal to.
fn smelt_py_sort_binarysort<T>(lt: &mut dyn FnMut(&T, &T) -> bool, items: &mut [T], sorted: usize) {
    for next in sorted.max(1)..items.len() {
        let (mut low, mut high) = (0, next);
        while low < high {
            let middle = (low + high) >> 1;
            if lt(&items[next], &items[middle]) {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        items[low..=next].rotate_right(1);
    }
}

/// `gallop_left`: where `key` goes in the sorted `items`, before every item
/// equal to it, searching outward from `hint`.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
fn smelt_py_sort_gallop_left<T>(lt: &mut dyn FnMut(&T, &T) -> bool, key: &T, items: &[T], hint: usize) -> usize {
    let n = items.len() as isize;
    let hint = hint as isize;
    let at = |index: isize| &items[index as usize];
    let mut last_ofs: isize = 0;
    let mut ofs: isize = 1;
    if lt(at(hint), key) {
        let max_ofs = n - hint;
        while ofs < max_ofs {
            if lt(at(hint + ofs), key) {
                last_ofs = ofs;
                ofs = (ofs << 1) + 1;
            } else {
                break;
            }
        }
        ofs = ofs.min(max_ofs);
        last_ofs += hint;
        ofs += hint;
    } else {
        let max_ofs = hint + 1;
        while ofs < max_ofs {
            if lt(at(hint - ofs), key) {
                break;
            }
            last_ofs = ofs;
            ofs = (ofs << 1) + 1;
        }
        ofs = ofs.min(max_ofs);
        let k = last_ofs;
        last_ofs = hint - ofs;
        ofs = hint - k;
    }
    last_ofs += 1;
    while last_ofs < ofs {
        let middle = last_ofs + ((ofs - last_ofs) >> 1);
        if lt(at(middle), key) {
            last_ofs = middle + 1;
        } else {
            ofs = middle;
        }
    }
    ofs as usize
}

/// `gallop_right`: where `key` goes in the sorted `items`, after every item
/// equal to it, searching outward from `hint`.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
fn smelt_py_sort_gallop_right<T>(lt: &mut dyn FnMut(&T, &T) -> bool, key: &T, items: &[T], hint: usize) -> usize {
    let n = items.len() as isize;
    let hint = hint as isize;
    let at = |index: isize| &items[index as usize];
    let mut last_ofs: isize = 0;
    let mut ofs: isize = 1;
    if lt(key, at(hint)) {
        let max_ofs = hint + 1;
        while ofs < max_ofs {
            if lt(key, at(hint - ofs)) {
                last_ofs = ofs;
                ofs = (ofs << 1) + 1;
            } else {
                break;
            }
        }
        ofs = ofs.min(max_ofs);
        let k = last_ofs;
        last_ofs = hint - ofs;
        ofs = hint - k;
    } else {
        let max_ofs = n - hint;
        while ofs < max_ofs {
            if lt(key, at(hint + ofs)) {
                break;
            }
            last_ofs = ofs;
            ofs = (ofs << 1) + 1;
        }
        ofs = ofs.min(max_ofs);
        last_ofs += hint;
        ofs += hint;
    }
    last_ofs += 1;
    while last_ofs < ofs {
        let middle = last_ofs + ((ofs - last_ofs) >> 1);
        if lt(key, at(middle)) {
            ofs = middle;
        } else {
            last_ofs = middle + 1;
        }
    }
    ofs as usize
}
// @smelt:item-end
