// Structural equality and hashing of erased values.

// @smelt:item smelt_unknown_structural_eq
// @smelt:if unknown
fn smelt_unknown_structural_eq(left: &SmeltUnknown, right: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {
    match (left, right) {
        (SmeltUnknown::Null, SmeltUnknown::Null) => true,
        (SmeltUnknown::Undefined, SmeltUnknown::Undefined) => true,
        (SmeltUnknown::Bool(left), SmeltUnknown::Bool(right)) => left == right,
        (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right || (left.is_nan() && right.is_nan()),
// @smelt:end
// @smelt:if bigint
        (SmeltUnknown::BigInt(left), SmeltUnknown::BigInt(right)) => left == right,
// @smelt:end
// @smelt:if unknown
        (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right,
        (SmeltUnknown::Symbol(left), SmeltUnknown::Symbol(right)) => left == right,
        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| smelt_unknown_structural_eq(&left, &right, seen)),
        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => smelt_object_structural_eq(left, right, seen),
        (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => smelt_same_erased_function(left, right),
        (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id,
        _ => false,
    }
}

// @smelt:end
// @smelt:item-end

// The one predicate behind own-property structural equality AND its `Hash`
// mirror: they must agree key for key, so both read this list.
// @smelt:item smelt_is_own_structural_key when unknown
/// Whether an erased object key is an OWN property for structural comparison.
//~
//~ `__smelt_proto:<name>` / `__smelt_method:<name>` carry inherited members and
//~ `__smelt_class` records which prototype an instance came from;
//~ JavaScript exposes none of the three as an own property, so deep equality
//~ and the matching hash both skip them.
fn smelt_is_own_structural_key(key: &JsStr) -> bool { !key.eq_str_prefix("__smelt_proto:") && !key.eq_str_prefix("__smelt_method:") && !key.eq_str("__smelt_class") }

fn smelt_object_structural_eq(left: &SmeltObject, right: &SmeltObject, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {
    if left.contains_key("__smelt_date") || right.contains_key("__smelt_date") { let left_date = smelt_unknown_date_value(&SmeltUnknown::Object(left.clone())); let right_date = smelt_unknown_date_value(&SmeltUnknown::Object(right.clone())); return left_date == right_date || (left_date.is_nan() && right_date.is_nan()); }
    if left.id == right.id { return true; }
    let key = (left.id, right.id);
    if !seen.insert(key) { return true; }
    //~ Prototype-carried members (`__smelt_proto:<name>`) are NOT own
    //~ properties in JavaScript, so structural equality must not see them.
    //~ Erasing a class instance pushes one bound `SmeltUnknown::Function`
    //~ per method under that prefix, and function values compare by `Rc`
    //~ pointer — without this filter two structurally equal instances of the
    //~ same class would each carry distinct closures and compare unequal.
    //~ The same rule already governs `Object.create(proto)` results, whose
    //~ inherited keys live under the same prefix.
    //~
    //~ `__smelt_class` belongs to that group: it records which prototype an
    //~ erased instance came from, and a JavaScript class instance has no such
    //~ own property. Own-property deep equality — what `toEqual`, `isEqual`
    //~ and `PartialEq for SmeltUnknown` all mean — therefore ignores it, so a
    //~ clone that keeps its prototype still equals a plain object of the same
    //~ own properties. A prototype-SENSITIVE comparison (`toStrictEqual`) has
    //~ to ask for the prototype explicitly rather than lean on entry counts.
    let left_entries = left.iter().filter(|(key, _)| smelt_is_own_structural_key(key)).collect::<Vec<_>>();
    let right_own = right.iter().filter(|(key, _)| smelt_is_own_structural_key(key)).count();
    if left_entries.len() != right_own { return false; }
    left_entries.into_iter().all(|(key, left_value)| right.get(&key).is_some_and(|right_value| smelt_unknown_structural_eq(&left_value, &right_value, seen)))
}

// @smelt:item-end

// @smelt:item smelt_unknown_structural_hash
// @smelt:if unknown
fn smelt_unknown_structural_hash<H: ::std::hash::Hasher>(value: &SmeltUnknown, state: &mut H, seen: &mut ::std::collections::HashSet<usize>) {
    match value {
        SmeltUnknown::Null => 0_u8.hash(state),
        SmeltUnknown::Undefined => 8_u8.hash(state),
        SmeltUnknown::Bool(value) => { 1_u8.hash(state); value.hash(state); }
        SmeltUnknown::Number(value) => { 2_u8.hash(state); if value.is_nan() { f64::NAN.to_bits().hash(state); } else { value.to_bits().hash(state); } }
// @smelt:end
// @smelt:if bigint
        SmeltUnknown::BigInt(value) => { 10_u8.hash(state); value.hash(state); }
// @smelt:end
// @smelt:if unknown
        SmeltUnknown::String(value) => { 3_u8.hash(state); value.hash(state); }
        SmeltUnknown::Symbol(value) => { 4_u8.hash(state); value.hash(state); }
        SmeltUnknown::Array(values) => { 5_u8.hash(state); values.len().hash(state); for value in values.iter() { smelt_unknown_structural_hash(&value, state, seen); } }
        SmeltUnknown::Object(values) => { 6_u8.hash(state); smelt_object_structural_hash(values, state, seen); }
        SmeltUnknown::Function(function) => { 7_u8.hash(state); ::std::rc::Rc::as_ptr(function).hash(state); }
        SmeltUnknown::Promise(promise) => { 9_u8.hash(state); promise.id.hash(state); }
    }
}

// @smelt:end
// @smelt:item-end

// @smelt:item smelt_unknown_stable_hash_key when unknown
fn smelt_unknown_stable_hash_key(value: &SmeltUnknown) -> u64 {
    let mut hasher = SmeltFieldHasher::default();
    let mut seen = ::std::collections::HashSet::new();
    smelt_unknown_structural_hash(value, &mut hasher, &mut seen);
    ::std::hash::Hasher::finish(&hasher)
}

fn smelt_object_structural_hash<H: ::std::hash::Hasher>(object: &SmeltObject, state: &mut H, seen: &mut ::std::collections::HashSet<usize>) {
    if !seen.insert(object.id) { 255_u8.hash(state); return; }
    //~ Mirror the `__smelt_proto:` filter in `smelt_object_structural_eq`:
    //~ `Hash` and `PartialEq` must agree, and prototype-carried members are
    //~ not part of an object's own structural identity.
    let mut entries = object.iter().filter(|(key, _)| smelt_is_own_structural_key(key)).collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.len().hash(state);
    for (key, value) in entries { key.hash(state); smelt_unknown_structural_hash(&value, state, seen); }
}

// @smelt:item-end
