// `SmeltArray`: the erased JavaScript array, its named properties and its list bridges.
//
// Handing an erased generic array to another erased slot without copying it.
//
// A `T[]` whose `T` is out of scope at the emitted item is rendered under the
// erased-generic ABI: `T` becomes `SmeltUnknown`, so the array is stored as
// `SmeltList<SmeltUnknown>`, exactly like a source `unknown[]`. When such an
// array flows into a slot whose elements also render as `SmeltUnknown` (an
// `unknown[]`, another erased `U[]`, or an `unknown` slot holding a
// `SmeltUnknown::Array`), the two sides already share one representation:
// `Rc<RefCell<Vec<SmeltUnknown>>>` plus the array's identity. JavaScript hands
// over the SAME array object there, so the conversion must alias the buffer
// and keep the id, not rebuild it element by element (#250).
//
// The two prelude helpers below are that conversion and nothing else. The
// emitter calls them only where `FunctionEmitter::list_items_render_as_unknown`
// holds for BOTH sides and the SOURCE element is an erased type parameter
// ([`leaves_erased_generic`]; the call sites are in `emitter/coercion.rs`).
// Their signatures pin `SmeltUnknown` elements, so a generated crate that
// reached them with a concrete `SmeltList<f64>` or an in-scope `SmeltList<T>`
// would not compile.
//
// # Why this is a `SmeltUnknown` boundary
//
// The helpers create no erasure: the erased-generic ABI erased `T` before the
// helper is reached, because no Rust generic in scope can name it. The helper
// is where a value leaves that ABI. Before #250 those conversions rebuilt the
// array through `into_smelt_unknown()`, one call per element: the outbound
// adapter for an erased `T`. The SmeltUnknown report (`smelt-transpiler`'s
// `unknown_report::classify_line`) counted those lines as a legitimate
// boundary through its `into_smelt_unknown` marker. #250 made them alias for
// speed and the marker went with the rebuild, so the same lines moved to
// avoidable with no erasure added. The report now keys on
// [`ERASED_GENERIC_LIST_ALIAS_FN`] and [`ERASED_GENERIC_ARRAY_ALIAS_FN`]
// instead, which restores that classification and no more.
//
// What is deliberately NOT routed through a helper, so the report keeps
// counting it as avoidable exactly as it did before #250:
//
// - a source `unknown[]`, into `unknown` or into an erased `T[]` slot. The
//   first aliased before #250 too, as `SmeltUnknown::Array(list.into())`; the
//   second rebuilt through an identity map with no adapter. Neither line
//   carried a boundary marker, and both keep their plain spelling, because
//   the erasure is the source's own `unknown`, which the metric exists to see
//   reduced.
// - the general list conversion `Into::<SmeltList<_>>::into(..)`, which every
//   list-typed rvalue goes through, including ones that erase a concrete
//   value.
//
// Where the value can still be carried concretely, these helpers are not
// reached: a concrete element list converts each element through its adapter
// (`concrete_list_erasure_still_converts_elements`), and an in-scope generic
// `SmeltList<T>` converts through `IntoSmeltUnknown`
// (`erased_generic_list_argument_judges_each_side_at_its_own_scope`).

// An erased JavaScript array is a REFERENCE, exactly like `SmeltObject`:
// the elements live in a shared `Rc<RefCell<Vec<_>>>` so every clone of the
// handle observes the same buffer. Codegen `.clone()`s erased values freely
// as they flow through expressions, and JS semantics require those copies to
// stay aliases.
//
// This used to be a plain `Vec`, which silently dropped writes. `obj.a[2] = 4`
// emits "read the field into a temporary, `smelt_index_assign` into the
// temporary" with no write-back — correct for an object payload only because
// `SmeltObject` already shared its `Rc`. With a copied `Vec` the array
// mutation went to the temporary and the parent never saw it. It also broke
// es-toolkit's cycle guards, which do `stack.set(a, b)` before recursing and
// then rely on the stored handle aliasing the array being filled in.
//
// A JavaScript array is an *exotic object*: it has index elements AND
// ordinary named properties (`const a = ['1']; a.x = 2` keeps `a` an
// array, with `a.length === 1` and `a.x === 2`). The named half lives in
// `SMELT_ARRAY_PROPS`, keyed by the array's IDENTITY rather than stored as
// a field here, because one JavaScript array is more than one Rust value:
// a typed `SmeltList<T>` and the `SmeltArray` an erasure wraps around its
// buffer are the same array (same `id`, same storage) but different
// structs, and `SmeltList` lives in `smelt-runtime`, which cannot name
// `SmeltUnknown` and so cannot hold the table itself. Keying by identity
// is what lets `Array.isArray(v)` narrow an erased match to a typed list
// and still read `.index` off it. Values are `SmeltUnknown` regardless of
// what the elements are: a named property only reaches an array through an
// erased receiver (TypeScript's `T[]` has no named members, so the source
// needs an `as any` first), which is a genuine dynamic boundary.
//
// Both store seams used to REPLACE the array with a fresh one-property
// object when the key was not an index, which lost the elements and made
// `Array.isArray` answer `false`. The named table is deliberately
// invisible to structural equality, `len()`, iteration and JSON —
// JavaScript compares arrays index-wise (es-toolkit `isEqualWith`'s array
// arm compares only `length` and the elements) and `JSON.stringify`
// serializes only the elements — and visible to property reads and to key
// enumeration, where it follows the index keys.
//
// Lifetime. An entry is keyed by an id, and an id has no drop hook, so the
// entry also records WHICH storages it belongs to: a `Weak` to the shared
// buffer of every erased array that wrote it. Typed `SmeltList<SmeltUnknown>`
// handles alias that buffer (`with_storage`), so a live typed handle keeps
// the entry too. Once every recorded buffer is dead nothing can name the
// array any more and an amortized sweep (the same doubling rule as
// `SmeltAddressTable`) drops the entry. This used to be never pruned, and
// every regex match a router erased (`index`/`input`/`groups`) stayed in
// the table for the life of the process.
//
// One kind of entry cannot be bounded that way and is PINNED, which is
// exactly the old never-pruned behavior: a property the program writes
// through a DETACHED array — one built by `with_id` as a copy of another
// representation of the same id (the erasure of a typed `string[]`
// copies its elements). The typed original carries the id but no buffer
// this table was told about, and it may outlive every erased copy.
// A typed COPY made from an erased array with properties
// (`SmeltFromUnknown` for `SmeltList<T>`, and the emitter's
// erased-array-to-list arm) ADOPTS the entry instead: its own buffer
// becomes an owner (`smelt_array_props_adopt`), so the properties live
// exactly as long as some representation that carries the id.
// Known gap: a typed-to-typed list conversion that changes the element
// type (`unknown[]` to `string[]`, emitter `coercion.rs`) copies the id
// without adopting, because it can be emitted in a crate without this
// table. Properties written through the `unknown[]` alias then live only
// as long as that alias's buffer.
// Properties DERIVED from a typed value at its erasure (a regex match's
// `index`/`input`/`groups`) are rewritten by every erasure of that value,
// so they are guarded by the buffer alone even though the array is a copy.
// @smelt:item SmeltArrayProps when unknown
/// The named properties of one array identity, and the buffers that keep them.
#[derive(Default)]
struct SmeltArrayProps { props: Vec<(String, SmeltUnknown)>, owners: Vec<(usize, Box<dyn SmeltAllocationGuard>)>, pinned: bool }

impl SmeltArrayProps {
    /// Whether some representation that can still name this array is alive.
    fn is_live(&self) -> bool { self.pinned || self.owners.iter().any(|(_, owner)| owner.is_live()) }
    /// Record a buffer carrying this array's id (erased or typed) as an owner of this entry.
    fn own<T: 'static>(&mut self, storage: &::std::rc::Rc<::std::cell::RefCell<Vec<T>>>) { let key = ::std::rc::Rc::as_ptr(storage) as *const () as usize; if !self.owners.iter().any(|(owner, _)| *owner == key) { self.owners.retain(|(_, owner)| owner.is_live()); self.owners.push((key, Box::new(::std::rc::Rc::downgrade(storage)))); } }
}

/// Named array properties by array identity, swept like `SmeltAddressTable`.
struct SmeltArrayPropsTable { entries: SmeltAddressMap<SmeltArrayProps>, sweep_at: usize }

impl SmeltArrayPropsTable {
    /// Number of entries, live or not yet swept.
    #[allow(dead_code)]
    fn len(&self) -> usize { self.entries.len() }
    /// Whether creating an entry for `id` should sweep first.
    fn wants_sweep(&self, id: usize) -> bool { self.entries.len() >= self.sweep_at && !self.entries.contains_key(&id) }
    /// The entry for `id`, created on first use.
    fn slot(&mut self, id: usize) -> &mut SmeltArrayProps { self.entries.entry(id).or_default() }
}

/// Sweep the named-property table until no entry dies (see `smelt_sweep_address_table`).
fn smelt_sweep_array_props(table: &::std::cell::RefCell<SmeltArrayPropsTable>) {
    loop {
        let dead: Vec<SmeltArrayProps> = table.borrow_mut().entries.extract_if(|_, entry| !entry.is_live()).map(|(_, entry)| entry).collect();
        if dead.is_empty() { break; }
        drop(dead);
    }
    let mut table = table.borrow_mut();
    table.sweep_at = (SMELT_ADDRESS_TABLE_GROWTH * table.entries.len()).max(SMELT_ADDRESS_TABLE_MIN_SWEEP);
}

thread_local! {
    /// Named (non-index) properties of erased arrays, keyed by array identity.
    static SMELT_ARRAY_PROPS: ::std::cell::RefCell<SmeltArrayPropsTable> = ::std::cell::RefCell::new(SmeltArrayPropsTable { entries: SmeltAddressMap::default(), sweep_at: SMELT_ADDRESS_TABLE_MIN_SWEEP });
}

/// Read one named property of the array with this identity.
fn smelt_array_named_property(id: usize, key: &str) -> Option<SmeltUnknown> { SMELT_ARRAY_PROPS.with(|props| props.borrow().entries.get(&id).and_then(|entry| entry.props.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone()))) }

/// Write one named property of `array`, owned by its buffer (pinned through a detached copy).
///
/// `derived` marks a property the erasure of a typed value rewrites every
/// time it erases (a match's `index`): such an entry is never pinned.
fn smelt_array_store_named_property(array: &SmeltArray, key: String, property: SmeltUnknown, derived: bool) { SMELT_ARRAY_PROPS.with(|props| { if props.borrow().wants_sweep(array.id) { smelt_sweep_array_props(props); } let mut props = props.borrow_mut(); let entry = props.slot(array.id); entry.own(&array.values); if array.detached && !derived { entry.pinned = true; } match entry.props.iter_mut().find(|(name, _)| *name == key) { Some(slot) => slot.1 = property, None => entry.props.push((key, property)) } }); }

/// Write several named properties of `array` at once: one table lookup for all of them.
///
/// The same as calling `smelt_array_store_named_property` per pair, which a
/// regex match's erasure did three times per match (`index`, `input`,
/// `groups`), each with its own sweep check, slot lookup and owner check.
#[allow(dead_code)]
fn smelt_array_store_named_properties<const N: usize>(array: &SmeltArray, properties: [(&'static str, SmeltUnknown); N], derived: bool) { SMELT_ARRAY_PROPS.with(|props| { if props.borrow().wants_sweep(array.id) { smelt_sweep_array_props(props); } let mut props = props.borrow_mut(); let entry = props.slot(array.id); entry.own(&array.values); if array.detached && !derived { entry.pinned = true; } for (key, property) in properties { match entry.props.iter_mut().find(|(name, _)| name == key) { Some(slot) => slot.1 = property, None => entry.props.push((key.to_owned(), property)) } } }); }

/// Make a typed copy's buffer an owner of `id`'s named properties, if it has any.
///
/// A copy carries the id onward (`as string[]` of a regex match), so the
/// properties must live as long as it does, not only as long as the erased
/// array it was copied from.
fn smelt_array_props_adopt<T: 'static>(id: usize, storage: &::std::rc::Rc<::std::cell::RefCell<Vec<T>>>) { SMELT_ARRAY_PROPS.with(|props| { if let Ok(mut props) = props.try_borrow_mut() { if let Some(entry) = props.entries.get_mut(&id) { entry.own(storage); } } }); }

/// The named property keys of the array with this identity, in insertion order.
fn smelt_array_named_keys(id: usize) -> Vec<String> { SMELT_ARRAY_PROPS.with(|props| props.borrow().entries.get(&id).map_or_else(Vec::new, |entry| entry.props.iter().map(|(name, _)| name.clone()).collect())) }

/// The named properties of the array with this identity, in insertion order.
fn smelt_array_named_entries(id: usize) -> Vec<(String, SmeltUnknown)> { SMELT_ARRAY_PROPS.with(|props| props.borrow().entries.get(&id).map(|entry| entry.props.clone()).unwrap_or_default()) }

pub struct SmeltArray {
    id: usize,
    values: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltUnknown>>>,
    /// Whether the buffer is a COPY of another representation of `id` (`with_id`).
    detached: bool,
}

//~ Hand-written `Debug` rather than a derive, so the shared cell does not
//~ show up as `RefCell { value: [..] }` in output that used to read
//~ `SmeltArray { id: 1, values: [..] }`. Keeping the rendering byte-identical
//~ means the storage change is invisible to anything that formats a value.
impl ::std::fmt::Debug for SmeltArray { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct("SmeltArray").field("id", &self.id).field("values", &*self.values.borrow()).finish() } }
impl Clone for SmeltArray { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone(), detached: self.detached } } }
impl SmeltArray {
    /// Create an identity-bearing erased JavaScript array.
    fn new(values: Vec<SmeltUnknown>) -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), detached: false } }
    /// Reuse a caller-supplied identity so repeated erasures of one source list compare `===` equal.
    fn with_id(id: usize, values: Vec<SmeltUnknown>) -> Self { Self { id, values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), detached: true } }
    /// Reuse an existing shared buffer, so a re-wrap keeps aliasing the same array.
    fn with_storage(id: usize, values: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltUnknown>>>) -> Self { Self { id, values, detached: false } }
    //~ The twin of `SmeltList::storage()`. An erased array re-wrapped as a
    //~ typed `SmeltList<SmeltUnknown>` must keep aliasing the SAME buffer, or
    //~ the round-trip produces a stale snapshot wearing the live array's id
    //~ (see the `From<SmeltList<SmeltUnknown>>` comment below for the same
    //~ argument in the other direction). Without this accessor the only way
    //~ back was `into_vec()`, which copies.
    /// Another handle on this array's shared buffer.
    fn storage(&self) -> ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltUnknown>>> { ::std::rc::Rc::clone(&self.values) }
    /// Snapshot the elements. This COPIES: mutating the result does not write back.
    fn into_vec(self) -> Vec<SmeltUnknown> { self.values.borrow().clone() }
    /// Element count.
    fn len(&self) -> usize { self.values.borrow().len() }
    /// Whether the array holds no elements.
    fn is_empty(&self) -> bool { self.values.borrow().is_empty() }
    /// Read one element by index, cloned out of the shared buffer (JS `arr[i]`).
    fn get(&self, index: usize) -> Option<SmeltUnknown> { self.values.borrow().get(index).cloned() }
    /// Iterate a snapshot of the elements, so the buffer is not borrowed across the loop body.
    fn iter(&self) -> ::std::vec::IntoIter<SmeltUnknown> { self.values.borrow().clone().into_iter() }
    /// Set the element at a numeric index, extending with `undefined` holes to match JS `arr[i] = v`.
    fn set_index(&self, index: usize, value: SmeltUnknown) { let mut values = self.values.borrow_mut(); if index >= values.len() { values.resize(index.saturating_add(1), SmeltUnknown::Undefined); } values[index] = value; }
    /// Append one element (JS `arr.push(v)`), through the shared buffer.
    fn push(&self, value: SmeltUnknown) { self.values.borrow_mut().push(value); }
    /// Replace every element in place, so aliases observe the new contents.
    fn replace_all(&self, values: Vec<SmeltUnknown>) { *self.values.borrow_mut() = values; }
    //~ The named half of the array's property model. Kept as an insertion-ordered
    //~ `Vec` rather than a map because JavaScript enumerates an object's string
    //~ keys in insertion order and there are only ever a handful of them. These
    //~ are thin wrappers over the identity-keyed registry, so a typed
    //~ `SmeltList` handle reaches the same table through the same `id`.
    /// Read a NON-INDEX named property (JS `arr.x`), or `None` when absent.
    fn named_property(&self, key: &str) -> Option<SmeltUnknown> { smelt_array_named_property(self.id, key) }
    /// Write a NON-INDEX named property (JS `arr.x = v`), allocating the side table on first use.
    fn set_named_property(&self, key: String, value: SmeltUnknown) { smelt_array_store_named_property(self, key, value, false); }
    /// The named property keys, in insertion order (empty in the common case).
    fn named_keys(&self) -> Vec<String> { smelt_array_named_keys(self.id) }
    /// Own enumerable keys: the element indices, then the named properties, exactly the order `Object.keys` reports.
    fn own_keys(&self) -> Vec<String> { let mut keys = (0..self.len()).map(|index| index.to_string()).collect::<Vec<_>>(); keys.extend(self.named_keys()); keys }
    /// Own enumerable entries, paired with the keys `own_keys` reports.
    fn own_entries(&self) -> Vec<(String, SmeltUnknown)> { let mut entries = self.values.borrow().iter().enumerate().map(|(index, value)| (index.to_string(), value.clone())).collect::<Vec<_>>(); entries.extend(smelt_array_named_entries(self.id)); entries }
}
impl From<Vec<SmeltUnknown>> for SmeltArray { fn from(values: Vec<SmeltUnknown>) -> Self { Self::new(values) } }
impl ::std::iter::FromIterator<SmeltUnknown> for SmeltArray { fn from_iter<T: IntoIterator<Item = SmeltUnknown>>(iter: T) -> Self { Self::new(iter.into_iter().collect()) } }
//~ No `Deref` to `[SmeltUnknown]`: the elements live behind a `RefCell`, so
//~ there is no `&[SmeltUnknown]` to hand out that could outlive the borrow.
//~ `len`/`is_empty`/`get`/`iter` above cover what the old deref was used for,
//~ each taking its own short-lived borrow.
impl IntoIterator for SmeltArray { type Item = SmeltUnknown; type IntoIter = ::std::vec::IntoIter<SmeltUnknown>; fn into_iter(self) -> Self::IntoIter { self.values.borrow().clone().into_iter() } }
impl<'smelt_array> IntoIterator for &'smelt_array SmeltArray { type Item = SmeltUnknown; type IntoIter = ::std::vec::IntoIter<SmeltUnknown>; fn into_iter(self) -> Self::IntoIter { self.values.borrow().clone().into_iter() } }

//~ `SmeltList<T>` itself is defined in the `needs_smelt_list` block above.
//~ These impls depend on `SmeltArray`/`SmeltUnknown`, so they live here.
//~ Erasing a typed list to a `SmeltUnknown::Array` ALIASES its buffer.
//~
//~ `SmeltList<SmeltUnknown>` and `SmeltArray` have the identical
//~ `Rc<RefCell<Vec<SmeltUnknown>>>` representation, and in JavaScript passing
//~ an array where `unknown` is expected hands over THE SAME object: the erased
//~ value must observe every later write through the typed handle, and a write
//~ through the erased value must be visible to the typed one. So the erasure
//~ is a `with_storage` refcount bump that carries both halves of the array's
//~ reference semantics — the stable `id` AND the shared buffer.
//~
//~ This used to copy the elements (`with_id(list.id(), list.into_vec())`),
//~ which kept the identity but detached the storage, producing a half
//~ reference: a stale snapshot wearing the live array's `id`. That is wrong
//~ for any array mutated after it is erased, and it is what made
//~ `isEqualWith`'s cycle guard unable to see a circular array — the erased
//~ element could never BE the array it was taken from, so `Object.is(a, b)`
//~ compared a live array against a frozen copy of itself.
//~
//~ Sharing was tried and reverted once, in #219, after remeda's
//~ `uniqueBy > pipe get executed 3 times when take before uniqueBy` panicked
//~ with `unknown is not array` INTERMITTENTLY. That intermittency was later
//~ root-caused and it was NOT this copy: identity-registry keys are `Rc`
//~ addresses, and a freed address handed to a later callback inherited the
//~ dead callback's callable-object entry (see the address-reuse guarantee
//~ in `callable_registry`, and `callable_object_identity_runtime`). Sharing the buffer only perturbed
//~ allocation enough to change how often the aliasing was observed. With that
//~ fix in place the remeda suite was re-measured over twenty consecutive runs
//~ with the sharing below and stayed 1789/1789.
impl From<SmeltList<SmeltUnknown>> for SmeltArray { fn from(list: SmeltList<SmeltUnknown>) -> Self { SmeltArray::with_storage(list.id(), list.storage()) } }
//~ A callback that declares a list parameter receives it by shared reference
//~ (see `callback_param_is_shared_reference`), so the erasure adapters need to
//~ build an erased array from `&SmeltList` as well as from an owned one. The
//~ reference form aliases the same buffer for the same reason the owned form
//~ does; borrowing rather than owning the handle changes nothing about the
//~ array's identity.
impl From<&SmeltList<SmeltUnknown>> for SmeltArray { fn from(list: &SmeltList<SmeltUnknown>) -> Self { SmeltArray::with_storage(list.id(), list.storage()) } }
// @smelt:item-end

// Emits both alias helpers into the `SmeltUnknown` prelude.
//
// Must follow the `From<SmeltList<SmeltUnknown>> for SmeltArray` impls, which
// the array helper's conversion goes through. Each helper is one `.into()`
// call marked `#[inline]`, so it compiles to the conversion #250 emitted
// inline and keeps that change's speedup.
// @smelt:item smelt_alias_erased_generic_list when erased_list_alias & unknown
/// Hand an erased generic list to an erased-list slot: the same array, never a copy.
#[inline]
fn smelt_alias_erased_generic_list(list: impl Into<SmeltList<SmeltUnknown>>) -> SmeltList<SmeltUnknown> { list.into() }
/// Hand an erased generic list to an `unknown` slot: the same array, never a copy.
#[inline]
fn smelt_alias_erased_generic_array(list: impl Into<SmeltArray>) -> SmeltArray { list.into() }
// @smelt:item-end

// @smelt:item smelt_optional_unknown_or_undefined when optional_unknown_read & unknown
/// An optional erased value read at the erased carrier: absence is `undefined`.
#[inline]
#[allow(dead_code)]
fn smelt_optional_unknown_or_undefined(value: Option<SmeltUnknown>) -> SmeltUnknown { value.unwrap_or(SmeltUnknown::Undefined) }
// @smelt:item-end

// @smelt:item impl_From_SmeltList_T_for_Vec_T when unknown
impl<T: Clone> From<&SmeltList<T>> for Vec<T> { fn from(list: &SmeltList<T>) -> Self { list.to_vec() } }
// @smelt:item-end

// serde impls only when the crate actually links serde (JSON contexts).
// @smelt:item impl_serde_Serialize_for_SmeltList_T when dep_serde_json & unknown
impl<T: serde::Serialize> serde::Serialize for SmeltList<T> { fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serde::Serialize::serialize(&*self.borrow(), serializer) } }
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for SmeltList<T> { fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> { <Vec<T> as serde::Deserialize>::deserialize(deserializer).map(SmeltList::new) } }
// @smelt:item-end
