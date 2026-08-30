//! String collation helpers.
//!
//! The emitted names live in `smelt_stdlib::runtime_symbols::strings`, and the
//! `runtime_manifest_symbol_names_match` test in `smelt-codegen-rust` pins the
//! item text below to those constants so the definition and the call sites
//! cannot drift.

// @smelt:item smelt_locale_compare
/// JavaScript `String.prototype.localeCompare(other)` with no `locales` or
/// `options` arguments, i.e. the host's default collation.
///
/// Returns a negative number when `left` sorts before `right`, a positive
/// number when it sorts after, and `0` when they collate equal — the contract
/// `Array.prototype.sort` comparators rely on. V8 answers `-1`/`0`/`1`, so this
/// does too.
///
/// # What is modeled
///
/// The two levels of the Unicode root collation that observably differ from a
/// raw scalar comparison:
///
/// * **primary** — case-folded scalars, so `"a".localeCompare("B")` is negative
///   the way ICU orders it, where the scalar comparison `"a" < "B"` is not;
/// * **tertiary** — strings equal at the primary level break the tie with
///   lowercase before uppercase (the DUCET tertiary weights), so
///   `"a".localeCompare("A")` is negative.
///
/// Strings that differ in neither fall back to scalar order.
///
/// # What is NOT modeled
///
/// Locale arguments, `Intl.Collator` options (`sensitivity`, `numeric`,
/// `caseFirst`), the secondary (accent) level, and the full DUCET weighting of
/// punctuation and symbols — ICU sorts those before digits and letters, while
/// scalar order interleaves them. All of those need real CLDR collation tables;
/// a program that depends on them needs an `Intl` implementation, not this.
fn smelt_locale_compare(left: &str, right: &str) -> f64 {
    let primary = left
        .chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase));
    let ordering = primary.then_with(|| {
        left.chars()
            .map(|ch| (ch.is_uppercase(), ch))
            .cmp(right.chars().map(|ch| (ch.is_uppercase(), ch)))
    });
    match ordering {
        ::std::cmp::Ordering::Less => -1.0,
        ::std::cmp::Ordering::Greater => 1.0,
        ::std::cmp::Ordering::Equal => 0.0,
    }
}
// @smelt:item-end

#[cfg(test)]
mod tests {
    use super::smelt_locale_compare;

    /// Equal strings collate equal.
    #[test]
    fn equal_strings_collate_equal() {
        assert_eq!(smelt_locale_compare("abc", "abc"), 0.0);
        assert_eq!(smelt_locale_compare("", ""), 0.0);
    }

    /// The primary level is case-insensitive, unlike a scalar comparison.
    #[test]
    fn the_primary_level_ignores_case() {
        assert!("a" > "B", "scalar order puts uppercase first");
        assert!(
            smelt_locale_compare("a", "B") < 0.0,
            "collation compares the case-folded letters"
        );
        assert!(smelt_locale_compare("B", "a") > 0.0);
    }

    /// Case only breaks a tie, with lowercase first.
    #[test]
    fn case_breaks_a_primary_tie_lowercase_first() {
        assert!(smelt_locale_compare("a", "A") < 0.0);
        assert!(smelt_locale_compare("A", "a") > 0.0);
        assert!(smelt_locale_compare("ab", "aB") < 0.0);
    }

    /// A prefix sorts before the longer string.
    #[test]
    fn a_prefix_sorts_first() {
        assert!(smelt_locale_compare("ab", "abc") < 0.0);
        assert!(smelt_locale_compare("abc", "ab") > 0.0);
    }

    /// Ordinary lowercase ASCII keeps plain alphabetical order.
    #[test]
    fn lowercase_ascii_is_alphabetical() {
        assert!(smelt_locale_compare("a", "b") < 0.0);
        assert!(smelt_locale_compare("c", "b") > 0.0);
    }
}
