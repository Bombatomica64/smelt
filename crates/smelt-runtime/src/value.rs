//! Value-core runtime items: JavaScript reference identity.
//!
//! Every JavaScript object, array, function, `Map`, `Set`, `Promise` and regexp
//! match is a *reference* value: `a === b` asks whether two bindings name the
//! same object, not whether their contents match. Generated Rust models that by
//! stamping each such value with a process-unique `usize` minted here, so
//! identity survives the erase/extract round trip through `SmeltUnknown`.
//!
//! The id counter is the crate-root end of the dependency chain: descendant
//! modules (`value::list` and, in later phases, the erased-value core) call
//! [`smelt_next_object_id`] through `use super::…`, which the emitted form does
//! not need because every runtime item lands in one flat generated module.

pub mod list;

// @smelt:item SMELT_NEXT_OBJECT_ID
thread_local! {
    static SMELT_NEXT_OBJECT_ID: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(1) };
}
// @smelt:item-end

// @smelt:item smelt_next_object_id
#[allow(dead_code)]
fn smelt_next_object_id() -> usize {
    SMELT_NEXT_OBJECT_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id })
}
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::smelt_next_object_id;

    /// Ids are minted strictly increasing, so no two live values share one.
    ///
    /// This is the property JS reference equality rests on: `[] === []` is
    /// `false` because the two arrays get different ids.
    #[test]
    fn object_ids_are_distinct_and_increasing() {
        let first = smelt_next_object_id();
        let second = smelt_next_object_id();
        let third = smelt_next_object_id();
        assert!(
            first < second && second < third,
            "ids must increase: {first}, {second}, {third}"
        );
    }

    /// The counter starts above zero, so `0` is usable as a "no identity" value.
    #[test]
    fn object_ids_are_never_zero() {
        assert_ne!(
            smelt_next_object_id(),
            0,
            "0 is reserved for the absence of an identity"
        );
    }
}
