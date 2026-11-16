// Typed deep equality: `toEqual` and the structural `==` over generated records.
//
// A generated record's own `PartialEq` answers `===` where the record is a
// JavaScript object with identity (a shared cell compares by address), and a
// by-value struct's derived `PartialEq` asks that same question of every
// record it holds. Deep equality asks another one: do the two values have the
// same own properties, compared the same way. `SmeltDeepEq` is that question
// at the value's own type: the code generator writes one impl per record that
// a deep comparison reaches (`smelt-codegen-rust/src/emitter/deep_equality.rs`),
// comparing field with field, and nothing is erased to be compared.
//
// The impls below are the leaves a record's TYPE ARGUMENT can be
// (`Box<number>`, `Box<string[]>`): inside `impl<T: SmeltDeepEq> .. for Box<T>`
// a field of type `T` can only be compared through the trait. Every other
// position is compared by the expression the generator writes for its type.

// @smelt:item SmeltDeepEq when deep_eq
/// Deep (own-property) equality of two values of one type.
//~
//~ `seen` holds the pairs of shared cells a comparison is inside of, which is
//~ how a cycle ends (see `SmeltDeepSeen::enter`).
pub trait SmeltDeepEq {
    fn smelt_deep_eq(&self, other: &Self, seen: &mut SmeltDeepSeen) -> bool;
}

/// The pairs of record cells a deep comparison is inside of, by address.
//~
//~ A reference record can hold itself, and comparing such a pair again while
//~ it is being compared would never end. The rule is the one of vitest's
//~ `equals` (its `aStack`/`bStack`): a cell met again while it is being
//~ compared is equal exactly when it is paired with the cell it is being
//~ compared to, so two cycles are equal when they close at the same depth.
//~ A by-value struct has no address and takes no part in this.
#[derive(Default)]
pub struct SmeltDeepSeen(Vec<(usize, usize)>);
impl SmeltDeepSeen {
    //~ Start comparing the cells at `left` and `right`. `Some(answer)` when
    //~ either is already being compared, and nothing is entered then;
    //~ otherwise the pair is entered until `leave`.
    pub fn enter(&mut self, left: usize, right: usize) -> Option<bool> {
        for pair in self.0.iter().rev() {
            if pair.0 == left { return Some(pair.1 == right); }
            if pair.1 == right { return Some(false); }
        }
        self.0.push((left, right));
        None
    }
    //~ The pair entered last has been compared.
    pub fn leave(&mut self) { self.0.truncate(self.0.len().saturating_sub(1)); }
}

/// Deep equality of `left` and `right`, as one comparison of its own.
#[allow(dead_code)]
fn smelt_deep_eq<T: SmeltDeepEq + ?Sized>(left: &T, right: &T) -> bool {
    left.smelt_deep_eq(right, &mut SmeltDeepSeen::default())
}

//~ A borrowed value compares as the value: the generated walks hand out
//~ items by reference (`iter().zip(..)`, `as_ref()`) and by value alike.
impl<T: SmeltDeepEq + ?Sized> SmeltDeepEq for &T {
    fn smelt_deep_eq(&self, other: &Self, seen: &mut SmeltDeepSeen) -> bool { (**self).smelt_deep_eq(&**other, seen) }
}
//~ A number leaf is compared with `Object.is` less the sign of zero, the rule
//~ of every nested number (`structural_leaf_equality_expr`): `NaN` equals `NaN`.
impl SmeltDeepEq for f64 {
    fn smelt_deep_eq(&self, other: &Self, _seen: &mut SmeltDeepSeen) -> bool { self == other || (self.is_nan() && other.is_nan()) }
}
impl SmeltDeepEq for bool {
    fn smelt_deep_eq(&self, other: &Self, _seen: &mut SmeltDeepSeen) -> bool { self == other }
}
impl SmeltDeepEq for String {
    fn smelt_deep_eq(&self, other: &Self, _seen: &mut SmeltDeepSeen) -> bool { self == other }
}
impl<T: SmeltDeepEq> SmeltDeepEq for Option<T> {
    fn smelt_deep_eq(&self, other: &Self, seen: &mut SmeltDeepSeen) -> bool {
        match (self, other) { (Some(left), Some(right)) => left.smelt_deep_eq(right, seen), (None, None) => true, _ => false }
    }
}
impl<T: SmeltDeepEq> SmeltDeepEq for Vec<T> {
    fn smelt_deep_eq(&self, other: &Self, seen: &mut SmeltDeepSeen) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|(left, right)| left.smelt_deep_eq(right, seen))
    }
}
// @smelt:if js_string
impl SmeltDeepEq for JsString {
    fn smelt_deep_eq(&self, other: &Self, _seen: &mut SmeltDeepSeen) -> bool { self == other }
}
// @smelt:end
// @smelt:if smelt_list
impl<T: SmeltDeepEq> SmeltDeepEq for SmeltList<T> {
    fn smelt_deep_eq(&self, other: &Self, seen: &mut SmeltDeepSeen) -> bool {
        self.len() == other.len() && self.borrow().iter().zip(other.borrow().iter()).all(|(left, right)| left.smelt_deep_eq(right, seen))
    }
}
// @smelt:end
// @smelt:if unknown
//~ A value the SOURCE types `unknown`/`any` is already erased; its deep
//~ equality is the erased structural walk. Nothing is erased here.
impl SmeltDeepEq for SmeltUnknown {
    fn smelt_deep_eq(&self, other: &Self, _seen: &mut SmeltDeepSeen) -> bool { self == other }
}
// @smelt:end

// @smelt:item-end
