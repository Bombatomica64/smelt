// String collation helpers.
//
// The emitted names live in `smelt_stdlib::runtime_symbols::strings`, and the
// `runtime_manifest_symbol_names_match` test in `smelt-codegen-rust` pins the
// item text below to those constants so the definition and the call sites
// cannot drift.

// @smelt:item smelt_locale_compare when locale_compare
/// JavaScript `String.prototype.localeCompare(other)` with no `locales` or
/// `options` arguments, i.e. the host's default collation.
//~
//~ Returns a negative number when `left` sorts before `right`, a positive
//~ number when it sorts after, and `0` when they collate equal — the contract
//~ `Array.prototype.sort` comparators rely on. V8 answers `-1`/`0`/`1`, so this
//~ does too.
//~
//~ The order is the CLDR root collation, which JavaScript's default locale
//~ (`en-US` in Node) uses untailored: ICU4X's `icu_collator`, the collator
//~ Node's own ICU implements. All three levels apply — base letters, then
//~ accents (`"a" < "ä" < "z"`), then case (`"a" < "A"`) — and punctuation
//~ and digits order as ICU orders them. One collator per thread is built from
//~ the compiled-in data on first use. `locales` and `Intl.Collator` options are
//~ not modelled.
fn smelt_locale_compare(left: &(impl SmeltHostText + ?Sized), right: &(impl SmeltHostText + ?Sized)) -> f64 { let left = left.host_text(); let left: &str = &left; let right = right.host_text(); let right: &str = &right;
    thread_local! {
        //~ The root collator, `None` only if the compiled-in data could not
        //~ load, in which case the order falls back to scalar order.
        static SMELT_COLLATOR: Option<icu_collator::CollatorBorrowed<'static>> = icu_collator::Collator::try_new(icu_collator::CollatorPreferences::default(), icu_collator::options::CollatorOptions::default()).ok();
    }
    let ordering = SMELT_COLLATOR.with(|collator| collator.as_ref().map_or_else(|| left.cmp(right), |collator| collator.compare(left, right)));
    match ordering {
        ::std::cmp::Ordering::Less => -1.0,
        ::std::cmp::Ordering::Greater => 1.0,
        ::std::cmp::Ordering::Equal => 0.0,
    }
}

// @smelt:item-end

#[cfg(test)]
mod strings_tests {
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

    /// An accent sorts after its base letter and before the next letter, as
    /// the root collation (and Node) orders it; a raw comparison put every
    /// accented letter after `z`.
    #[test]
    fn accents_sort_beside_their_base_letter() {
        assert!(smelt_locale_compare("ä", "z") < 0.0, "ä before z");
        assert!(smelt_locale_compare("a", "ä") < 0.0, "a before ä");
        assert!(smelt_locale_compare("résumé", "resume") > 0.0, "the accent breaks the tie");
    }
}
