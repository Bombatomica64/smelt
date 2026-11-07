// SameValueZero and strict equality as traits over typed and erased values.

// The hash side of the key-equality trait is a DEFAULTED method rather than a
// separate `SmeltJsKeyHash` trait: a second trait would have to be threaded
// through every `+ SmeltJsKeyEq` bound codegen appends (see
// `classes::class_impl_generics_text`), and any bound it was missed on would
// fail to type-check. As one defaulted method, every existing bound already
// carries it and an impl that only defines equality stays correct — just
// unindexed, falling back to the linear scan.
// @smelt:item SmeltJsKeyEq when unknown
pub trait SmeltJsKeyEq {
    fn same_js_key(&self, other: &Self) -> bool;
    /// A hash consistent with `same_js_key`: whenever two keys compare equal
    /// they MUST hash the same, so a hash index can narrow a lookup without
    /// changing its answer. The converse is not required — a collision only
    /// costs a `same_js_key` comparison. `None` means the key has no stable
    /// hashable identity and must keep the linear scan.
    fn js_key_hash(&self) -> Option<u64> { None }
}

impl SmeltJsKeyEq for SmeltUnknown {
    fn js_key_hash(&self) -> Option<u64> { smelt_js_member_hash_key(self) }
    fn same_js_key(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) if left.is_nan() && right.is_nan() => true, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => smelt_same_erased_function(left, right), (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id, (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right, _ => self == other } }
}

impl SmeltJsKeyEq for String { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
impl SmeltJsKeyEq for JsString { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
impl SmeltJsKeyEq for bool { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
impl SmeltJsKeyEq for i64 { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }
impl SmeltJsKeyEq for f64 { fn same_js_key(&self, other: &Self) -> bool { (self.is_nan() && other.is_nan()) || self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(&(if self.is_nan() { f64::NAN.to_bits() } else if *self == 0.0 { 0_f64.to_bits() } else { self.to_bits() }))) } }
//~ A record/object used as a collection key compares by JavaScript
//~ reference identity (its stable object `id`), matching `same_js_key`'s
//~ object arm on the erased value. This lets a `Set`/`Map`/cache keyed by
//~ a concrete `SmeltRecord` resolve `SmeltJsKeyEq` without erasing to
//~ `SmeltUnknown` (was E0599: unsatisfied `SmeltJsKeyEq` bound).
impl<K, V> SmeltJsKeyEq for SmeltRecord<K, V> { fn same_js_key(&self, other: &Self) -> bool { self.id == other.id } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(&self.id)) } }

//~ The fourth equality kind: JavaScript strict equality (`===`/`!==`).
//~ Distinct from the other three — objects/arrays/functions compare by
//~ REFERENCE identity (id/ptr), primitives by value, and `NaN !== NaN`
//~ (unlike `same_js_key`'s SameValueZero and `Object.is`'s SameValue, which
//~ both treat NaN as equal). `+0 === -0` holds because `f64 ==` is true for
//~ them. Unlike `PartialEq` (`smelt_unknown_structural_eq`, intentionally
//~ deep for `isDeepEqual`), this never recurses into object/array contents.
//~ A trait (not an inherent method) with primitive impls so erased operands
//~ that lower to a concrete `String`/`bool`/number still resolve the method.
pub trait SmeltJsStrictEq {
    fn js_strict_eq(&self, other: &Self) -> bool;
}
impl SmeltJsStrictEq for SmeltUnknown {
    fn js_strict_eq(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Null, SmeltUnknown::Null) => true, (SmeltUnknown::Undefined, SmeltUnknown::Undefined) => true, (SmeltUnknown::Bool(left), SmeltUnknown::Bool(right)) => left == right, (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right, /*@if bigint*/(SmeltUnknown::BigInt(left), SmeltUnknown::BigInt(right)) => left == right, /*@end*/(SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right, (SmeltUnknown::Symbol(left), SmeltUnknown::Symbol(right)) => left == right, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => smelt_same_erased_function(left, right), (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id, _ => false } }
}
impl SmeltJsStrictEq for String { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }
impl SmeltJsStrictEq for JsString { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }
impl SmeltJsStrictEq for bool { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }
impl SmeltJsStrictEq for i64 { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }
impl SmeltJsStrictEq for f64 { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }

// @smelt:item-end
