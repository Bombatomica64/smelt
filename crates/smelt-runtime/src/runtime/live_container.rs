// Live erased views of typed `Map`s and `Set`s.
//
// A JavaScript `Map` or `Set` is ONE object: every reference to it, typed or
// not, sees every write. The typed containers (`SmeltJsMap`, `SmeltJsSet`,
// `SmeltPrimSet`) already share their store across clones, but erasing one
// (`IntoSmeltUnknown`) builds a marker object holding a SNAPSHOT of its
// entries. A write through the erased value (`(m as any).set(k, v)`, a static
// `Map` read through an erased class value) then landed in the snapshot, and a
// typed write after the erasure was invisible through it.
//
// A live view closes both directions. Erasing a container whose element types
// can be rebuilt from the erased carrier (`into_live_smelt_unknown`, chosen by
// the emitter) stamps one extra hook on the marker object,
// `__smelt_method:__smelt_live`: a closure over the typed handle that answers
// each erased operation against the typed store. The runtime's erased `Map`
// and `Set` methods route through it (`smelt_get_unknown_field`), and every
// reader of the marker array refreshes it from the hook first
// (`smelt_live_container_refresh`), so the snapshot is never what answers.
// Narrowing the view back to the same typed container hands out the SAME
// handle (`smelt_live_container_take`), not a copy rebuilt from the snapshot.
//
// The hook speaks one small protocol: its first argument names the operation.
//
// | op        | args         | answer                                  |
// |-----------|--------------|-----------------------------------------|
// | `get`     | key          | the value, or `undefined`               |
// | `has`     | key / member | `true` when present                     |
// | `set`     | key, value   | `undefined` (Map)                       |
// | `add`     | member       | `undefined` (Set)                       |
// | `delete`  | key / member | `true` when something was removed       |
// | `clear`   |              | `undefined`                             |
// | `size`    |              | the entry count                         |
// | `entries` |              | the marker array's fresh contents       |
// | `handoff` |              | parks a typed clone for `..._take`      |
// | `id`      |              | the typed container's identity          |
//
// A key is looked up only when it converts to the container's key type and
// back to the same JavaScript key (SameValueZero): an erased `5` asked of a
// `Map<string, V>` is a miss, as it is in JavaScript, never the key `""` a
// lossy conversion would produce.

// @smelt:item SMELT_LIVE_HOOK when unknown & live_containers
/// The marker-object key of the hook an erased `Map`/`Set` answers its live operations through.
const SMELT_LIVE_HOOK: &str = "__smelt_method:__smelt_live";

thread_local! {
    //~ The typed handle a live hook parks for `smelt_live_container_take`.
    //~ Written and taken within one call, so it never holds a stale handle.
    static SMELT_LIVE_HANDOFF: ::std::cell::RefCell<Option<Box<dyn ::std::any::Any>>> = const { ::std::cell::RefCell::new(None) };
}

/// The typed key an erased one names, when it round-trips to the same JavaScript key.
fn smelt_live_key<K: SmeltFromUnknown + IntoSmeltUnknown + Clone>(key: &SmeltUnknown) -> Option<K> { let typed = K::smelt_from_unknown(key.clone()); typed.clone().into_smelt_unknown().same_js_key(key).then_some(typed) }

/// The operation name and the first two operands of one live-hook call.
fn smelt_live_args(args: Vec<SmeltUnknown>) -> (String, SmeltUnknown, SmeltUnknown) { let mut args = args.into_iter(); let op = match args.next() { Some(SmeltUnknown::String(op)) => op.to_string(), _ => String::new() }; let first = args.next().unwrap_or(SmeltUnknown::Undefined); let second = args.next().unwrap_or(SmeltUnknown::Undefined); (op, first, second) }

/// Park `handle` for the `smelt_live_container_take` that asked for it.
fn smelt_live_park<T: 'static>(handle: T) -> SmeltUnknown { SMELT_LIVE_HANDOFF.with(|slot| *slot.borrow_mut() = Some(Box::new(handle))); SmeltUnknown::Undefined }

/// Stamp the live hook `hook` on the erased marker object `erased`.
fn smelt_live_stamp(erased: SmeltUnknown, hook: impl Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>> + 'static) -> SmeltUnknown { if let SmeltUnknown::Object(object) = &erased { object.insert(SMELT_LIVE_HOOK, SmeltUnknown::Function(::std::rc::Rc::new(hook))); } erased }

/// The live hook of an erased `Map`/`Set` view, if it is one.
///
/// Only the object the hook was stamped on is the view: it carries the typed
/// container's own identity. A copy of the marker object (`new Map(erased)`
/// through an erased constructor) has a fresh identity and is an independent
/// Map, so the hook it copied along does not answer for it.
fn smelt_live_hook(object: &SmeltObject) -> Option<::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>> { match object.get(SMELT_LIVE_HOOK) { Some(SmeltUnknown::Function(hook)) => match hook(vec![SmeltUnknown::String("id".into())]) { Ok(SmeltUnknown::Number(id)) if id.to_bits() == f64::from(u32::try_from(object.id).unwrap_or(u32::MAX)).to_bits() => Some(hook), _ => None }, _ => None } }

/// Bring an erased live `Map`/`Set` marker array up to date with the typed container behind it.
///
/// The marker array is the shared `SmeltArray` every handle on the erased value reads, so one
/// refresh serves all of them. A value that is not a live view is left alone.
fn smelt_live_container_refresh(object: &SmeltObject) { let Some(hook) = smelt_live_hook(object) else { return }; for marker in ["__smelt_map", "__smelt_set"] { if let Some(SmeltUnknown::Array(store)) = object.get(marker) && let Ok(SmeltUnknown::Array(fresh)) = hook(vec![SmeltUnknown::String("entries".into())]) { store.replace_all(fresh.into_vec()); } } }

/// Ask the live hook of `object` for one operation, when it is a live view.
fn smelt_live_container_call(object: &SmeltObject, op: &str, args: Vec<SmeltUnknown>) -> Option<SmeltUnknown> { let hook = smelt_live_hook(object)?; let mut call = Vec::with_capacity(args.len() + 1); call.push(SmeltUnknown::String(op.into())); call.extend(args); hook(call).ok() }

/// The typed container `object` is a live view of, when it is a `T`: the same handle, not a copy.
fn smelt_live_container_take<T: 'static>(object: &SmeltObject) -> Option<T> { smelt_live_container_call(object, "handoff", Vec::new())?; SMELT_LIVE_HANDOFF.with(|slot| slot.borrow_mut().take()).and_then(|parked| parked.downcast::<T>().ok()).map(|parked| *parked) }

impl<K: IntoSmeltUnknown + SmeltFromUnknown + SmeltJsKeyEq + Clone + 'static, V: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltJsMap<K, V> {
    //~ Erase this map to a LIVE view: the `__smelt_map` marker object, plus the
    //~ hook through which erased reads and writes reach this very store.
    fn into_live_smelt_unknown(self) -> SmeltUnknown { self.into_live_smelt_unknown_with(K::into_smelt_unknown, V::into_smelt_unknown) }
    //~ `into_live_smelt_unknown` with the key and value erasures the static
    //~ type decides in place of `K`'s and `V`'s own (a `Map<K, V | null>`
    //~ reads an absent value as `null`).
    fn into_live_smelt_unknown_with(self, key_to: fn(K) -> SmeltUnknown, value_to: fn(V) -> SmeltUnknown) -> SmeltUnknown {
        let live = self.clone();
        smelt_live_stamp(self.into_smelt_unknown_with(key_to, value_to), move |args: Vec<SmeltUnknown>| {
            let mut map = live.clone();
            let (op, first, second) = smelt_live_args(args);
            Ok(match op.as_str() {
                "get" => smelt_live_key::<K>(&first).and_then(|key| map.get(&key)).map_or(SmeltUnknown::Undefined, value_to),
                "has" => SmeltUnknown::Bool(smelt_live_key::<K>(&first).is_some_and(|key| map.contains_key(&key))),
                "set" => { map.insert(K::smelt_from_unknown(first), V::smelt_from_unknown(second)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<K>(&first).is_some_and(|key| map.remove(&key).is_some())),
                "clear" => { map.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(map.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(map.iter().map(|(key, value)| SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), vec![key_to(key), value_to(value)]))).collect::<Vec<_>>().into()),
                "handoff" => smelt_live_park(map),
                "id" => SmeltUnknown::Number(f64::from(u32::try_from(map.id).unwrap_or(u32::MAX))),
                _ => SmeltUnknown::Undefined,
            })
        })
    }
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltJsSet<T> {
    //~ Erase this set to a LIVE view (see `SmeltJsMap::into_live_smelt_unknown`).
    fn into_live_smelt_unknown(self) -> SmeltUnknown { self.into_live_smelt_unknown_with(T::into_smelt_unknown) }
    //~ `into_live_smelt_unknown` with the member erasure the static type
    //~ decides (a `Set<T | null>` holds `null`, not `undefined`).
    fn into_live_smelt_unknown_with(self, to: fn(T) -> SmeltUnknown) -> SmeltUnknown {
        let live = self.clone();
        smelt_live_stamp(self.into_smelt_unknown_with(to), move |args: Vec<SmeltUnknown>| {
            let mut set = live.clone();
            let (op, first, _) = smelt_live_args(args);
            Ok(match op.as_str() {
                "has" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.contains(&member))),
                "add" => { set.insert(T::smelt_from_unknown(first)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.remove(&member))),
                "clear" => { set.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(set.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(set.iter().map(to).collect::<Vec<_>>().into()),
                "handoff" => smelt_live_park(set),
                "id" => SmeltUnknown::Number(f64::from(u32::try_from(set.id).unwrap_or(u32::MAX))),
                _ => SmeltUnknown::Undefined,
            })
        })
    }
}
// @smelt:if prim_set

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + ::std::cmp::Eq + ::std::hash::Hash + 'static> SmeltPrimSet<T> {
    //~ Erase this set to a LIVE view (see `SmeltJsMap::into_live_smelt_unknown`).
    fn into_live_smelt_unknown(self) -> SmeltUnknown { self.into_live_smelt_unknown_with(T::into_smelt_unknown) }
    //~ `into_live_smelt_unknown` with the member erasure the static type decides.
    fn into_live_smelt_unknown_with(self, to: fn(T) -> SmeltUnknown) -> SmeltUnknown {
        let live = self.clone();
        smelt_live_stamp(self.into_smelt_unknown_with(to), move |args: Vec<SmeltUnknown>| {
            let mut set = live.clone();
            let (op, first, _) = smelt_live_args(args);
            Ok(match op.as_str() {
                "has" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.contains(&member))),
                "add" => { set.insert(T::smelt_from_unknown(first)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.remove(&member))),
                "clear" => { set.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(set.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(set.iter().map(to).collect::<Vec<_>>().into()),
                "handoff" => smelt_live_park(set),
                "id" => SmeltUnknown::Number(f64::from(u32::try_from(set.id).unwrap_or(u32::MAX))),
                _ => SmeltUnknown::Undefined,
            })
        })
    }
}
// @smelt:end
// @smelt:item-end

// Live erased views of typed arrays.
//
// The same contract as a live `Map`/`Set`, for `Array`: a JavaScript array is
// ONE object, so erasing a typed `SmeltList<T>` (`into_live_smelt_unknown`,
// chosen by the emitter when `T` can be rebuilt from the erased carrier) gives
// a `SmeltArray` whose elements ARE the typed list's. The array keeps no buffer
// of its own: every access (`len`, an index read or write, `push`, `splice`,
// a `length` write, `sort`) goes through `SmeltLiveList` to the typed storage,
// converting the elements it touches, so each costs what the typed operation
// costs plus one conversion per element, and nothing is ever stale. Narrowing
// the view back to `SmeltList<T>` hands out the SAME typed handle
// (`SmeltArray::live_typed`). A copy (`[...view]`, `Array.from(view)`,
// `slice`) snapshots the elements into an ordinary erased array, independent
// as in JavaScript.
//
// A value written through the view is converted to `T` (`SmeltFromUnknown`):
// the static type says every element is a `T`, so a write of another kind
// (only possible through `any`) stores `T`'s conversion of it.

// @smelt:item SmeltLiveList when unknown & live_arrays
/// The typed list behind a live erased array, seen element by element as erased values.
trait SmeltLiveList {
    //~ Element count.
    fn element_count(&self) -> usize;
    //~ The element at `index`, erased, or `None` past the end.
    fn element(&self, index: usize) -> Option<SmeltUnknown>;
    //~ Store `value` at `index`, growing the list with `undefined`'s conversion as JS leaves holes.
    fn set_element(&self, index: usize, value: SmeltUnknown);
    //~ Every element, erased: a copy.
    fn elements(&self) -> Vec<SmeltUnknown>;
    //~ Remove `delete` elements at `start`, insert `items` there, answer the removed ones erased.
    fn splice_elements(&self, start: usize, delete: usize, items: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>;
    //~ Replace every element in place.
    fn replace_elements(&self, values: Vec<SmeltUnknown>);
    //~ The typed list itself, for narrowing back to it.
    fn typed_list(&self) -> &dyn ::std::any::Any;
    //~ Whether `value` survives the round trip through the element type, so
    //~ storing it in the typed list loses nothing.
    fn holds(&self, value: &SmeltUnknown) -> bool;
    //~ Grow to `len` elements, the new slots holes (JS `arr.length = n`).
    fn grow_with_holes(&self, len: usize);
    //~ Whether the slot at `index` is a hole.
    fn is_hole(&self, index: usize) -> bool;
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltLiveList for SmeltList<T> {
    fn element_count(&self) -> usize { self.borrow().len() }
    fn element(&self, index: usize) -> Option<SmeltUnknown> { self.borrow().get(index).cloned().map(smelt_list_slot_unknown) }
    fn set_element(&self, index: usize, value: SmeltUnknown) { let mut values = self.borrow_mut(); if index >= values.len() { values.resize_with(index.saturating_add(1), smelt_list_hole::<T>); } values[index] = T::smelt_from_unknown(value); }
    fn elements(&self) -> Vec<SmeltUnknown> { self.borrow().iter().cloned().map(smelt_list_slot_unknown).collect() }
    fn splice_elements(&self, start: usize, delete: usize, items: Vec<SmeltUnknown>) -> Vec<SmeltUnknown> { let mut values = self.borrow_mut(); let start = start.min(values.len()); let end = start.saturating_add(delete).min(values.len()); values.splice(start..end, items.into_iter().map(T::smelt_from_unknown)).map(IntoSmeltUnknown::into_smelt_unknown).collect() }
    fn replace_elements(&self, values: Vec<SmeltUnknown>) { *self.borrow_mut() = values.into_iter().map(T::smelt_from_unknown).collect(); }
    fn typed_list(&self) -> &dyn ::std::any::Any { self }
    fn holds(&self, value: &SmeltUnknown) -> bool { smelt_live_same_content(&T::smelt_from_unknown(value.clone()).into_smelt_unknown(), value, 0) }
    fn grow_with_holes(&self, len: usize) { let mut values = self.borrow_mut(); if len > values.len() { values.resize_with(len, smelt_list_hole::<T>); } }
    fn is_hole(&self, index: usize) -> bool { self.borrow().get(index).is_some_and(smelt_list_value_is_hole) }
}

//~ The value a hole of a `SmeltList<T>` is stored as.
//~
//~ JavaScript leaves a hole where a write past the end, or a `length` grown
//~ through an erased handle, skips slots. A `number` element has an
//~ out-of-band value to spare (`smelt_number_hole`, a reserved NaN payload that
//~ any arithmetic turns into an ordinary NaN, as `undefined` becomes in JS), so
//~ a `number[]` keeps its holes. Any other element type has no such value and
//~ takes the conversion of `undefined`, as before.
fn smelt_list_hole<T: SmeltFromUnknown + 'static>() -> T {
    let hole: Box<dyn ::std::any::Any> = Box::new(smelt_number_hole());
    hole.downcast::<T>().map_or_else(|_| T::smelt_from_unknown(SmeltUnknown::Undefined), |hole| *hole)
}

//~ Whether a typed list's slot holds a hole (`smelt_list_hole`).
fn smelt_list_value_is_hole<T: 'static>(value: &T) -> bool { (value as &dyn ::std::any::Any).downcast_ref::<f64>().is_some_and(|value| smelt_is_number_hole(*value)) }

//~ A typed list's slot, erased: a hole reads as `undefined`.
fn smelt_list_slot_unknown<T: IntoSmeltUnknown + 'static>(value: T) -> SmeltUnknown { if smelt_list_value_is_hole(&value) { SmeltUnknown::Undefined } else { value.into_smelt_unknown() } }

/// Whether two erased values hold the same content, ignoring object identity.
//~
//~ The question `SmeltLiveList::holds` asks: a value's round trip through
//~ the element type is a NEW object carrying the source's id (a record's
//~ erasure keeps its id), so identity would answer "same" for exactly the
//~ conversions that lost something. Content is compared instead: primitives
//~ by value (`NaN` matching `NaN`), arrays element-wise, objects key by key in
//~ both directions. Past a fixed depth (a cycle, or a structure too deep to be
//~ worth walking) the answer is "different", which only detaches the view
//~ early: the safe direction.
fn smelt_live_same_content(left: &SmeltUnknown, right: &SmeltUnknown, depth: usize) -> bool {
    if depth > 32 { return false; }
    match (left, right) {
        (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left.to_bits() == right.to_bits() || (left.is_nan() && right.is_nan()),
        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => { let (left, right) = (left.clone().into_vec(), right.clone().into_vec()); left.len() == right.len() && left.iter().zip(&right).all(|(left, right)| smelt_live_same_content(left, right, depth + 1)) }
        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => { let left_entries = left.iter().collect::<Vec<_>>(); let right_count = right.iter().count(); left_entries.len() == right_count && left_entries.iter().all(|(key, value)| right.get(&*key.to_string_lossy()).is_some_and(|other| smelt_live_same_content(value, &other, depth + 1))) }
        (left, right) => left == right,
    }
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltList<T> {
    //~ Erase this list to a LIVE view: the erased array whose elements are this list's.
    fn into_live_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray { id: self.id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), detached: false, live: Some(::std::rc::Rc::new(::std::cell::RefCell::new(Some(::std::rc::Rc::new(self))))) }) }
}


impl SmeltArray {
    //~ The typed list this array is a live view of, when it is a `SmeltList<T>`: the same handle, not a copy.
    fn live_typed<T: 'static>(&self) -> Option<SmeltList<T>> { self.live_view()?.typed_list().downcast_ref::<SmeltList<T>>().cloned() }
    //~ The typed list this array is still a live view of.
    fn live_view(&self) -> Option<::std::rc::Rc<dyn SmeltLiveList>> { self.live.as_ref()?.borrow().clone() }
    //~ The live view, for a write of `values`. A value the typed list cannot
    //~ hold faithfully (a `number` written into a `string[]` through `any`, an
    //~ object with more fields than the element type) DETACHES the view first:
    //~ its elements move into the array's own erased buffer and the shared
    //~ cell lets go of the typed list, so every handle on this array goes on
    //~ as an ordinary erased array, holding exactly what JavaScript wrote, and
    //~ the typed list keeps what it had (the copy semantics erasure had before
    //~ live views). `None` once detached, or when the write detaches.
    fn live_view_for(&self, values: &[SmeltUnknown]) -> Option<::std::rc::Rc<dyn SmeltLiveList>> { let live = self.live_view()?; if values.iter().all(|value| live.holds(value)) { return Some(live); } *self.values.borrow_mut() = live.elements(); if let Some(cell) = &self.live { *cell.borrow_mut() = None; } None }
}
// @smelt:item-end

// @smelt:item SmeltCodecList when unknown & live_arrays & null_codec
//~ A typed list seen through a live erased array whose elements erase with
//~ `to` (see `SmeltList::into_live_smelt_unknown_with`). Writes convert with
//~ `T`'s own `SmeltFromUnknown`, which already maps `null` to an absent
//~ `Option`; narrowing back hands out `list` itself.
struct SmeltCodecList<T> { list: SmeltList<T>, to: fn(T) -> SmeltUnknown }

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltLiveList for SmeltCodecList<T> {
    fn element_count(&self) -> usize { self.list.element_count() }
    fn element(&self, index: usize) -> Option<SmeltUnknown> { if self.list.is_hole(index) { return Some(SmeltUnknown::Undefined); } self.list.borrow().get(index).cloned().map(self.to) }
    fn set_element(&self, index: usize, value: SmeltUnknown) { self.list.set_element(index, value); }
    fn elements(&self) -> Vec<SmeltUnknown> { (0..self.element_count()).filter_map(|index| self.element(index)).collect() }
    fn splice_elements(&self, start: usize, delete: usize, items: Vec<SmeltUnknown>) -> Vec<SmeltUnknown> { let mut values = self.list.borrow_mut(); let start = start.min(values.len()); let end = start.saturating_add(delete).min(values.len()); values.splice(start..end, items.into_iter().map(T::smelt_from_unknown)).map(self.to).collect() }
    fn replace_elements(&self, values: Vec<SmeltUnknown>) { self.list.replace_elements(values); }
    fn typed_list(&self) -> &dyn ::std::any::Any { &self.list }
    fn holds(&self, value: &SmeltUnknown) -> bool { smelt_live_same_content(&(self.to)(T::smelt_from_unknown(value.clone())), value, 0) }
    fn grow_with_holes(&self, len: usize) { self.list.grow_with_holes(len); }
    fn is_hole(&self, index: usize) -> bool { self.list.is_hole(index) }
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltList<T> {
    //~ `into_live_smelt_unknown` with the element erasure the static type
    //~ decides (`to`) in place of `T`'s own: a `(T | null)[]` reads its empty
    //~ slots as `null`, which `Option<T>`'s `IntoSmeltUnknown` cannot know.
    fn into_live_smelt_unknown_with(self, to: fn(T) -> SmeltUnknown) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray { id: self.id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), detached: false, live: Some(::std::rc::Rc::new(::std::cell::RefCell::new(Some(::std::rc::Rc::new(SmeltCodecList { list: self, to }))))) }) }
}
// @smelt:item-end
