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
    //~ The live view, its keys and values erased by `erase_key`/`erase_value`
    //~ (see `SmeltJsMap::into_smelt_unknown_with`). Lookups still compare keys
    //~ through `IntoSmeltUnknown`: an absent key is a miss whichever word erases it.
    fn into_live_smelt_unknown_with(self, erase_key: impl Fn(K) -> SmeltUnknown + 'static, erase_value: impl Fn(V) -> SmeltUnknown + 'static) -> SmeltUnknown {
        let live = self.clone();
        let snapshot = self.into_smelt_unknown_with(&erase_key, &erase_value);
        smelt_live_stamp(snapshot, move |args: Vec<SmeltUnknown>| {
            let mut map = live.clone();
            let (op, first, second) = smelt_live_args(args);
            Ok(match op.as_str() {
                "get" => smelt_live_key::<K>(&first).and_then(|key| map.get(&key)).map_or(SmeltUnknown::Undefined, &erase_value),
                "has" => SmeltUnknown::Bool(smelt_live_key::<K>(&first).is_some_and(|key| map.contains_key(&key))),
                "set" => { map.insert(K::smelt_from_unknown(first), V::smelt_from_unknown(second)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<K>(&first).is_some_and(|key| map.remove(&key).is_some())),
                "clear" => { map.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(map.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(map.iter().map(|(key, value)| SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), vec![erase_key(key), erase_value(value)]))).collect::<Vec<_>>().into()),
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
    //~ The live view, its members erased by `erase` (see `SmeltJsMap::into_live_smelt_unknown_with`).
    fn into_live_smelt_unknown_with(self, erase: impl Fn(T) -> SmeltUnknown + 'static) -> SmeltUnknown {
        let live = self.clone();
        let snapshot = self.into_smelt_unknown_with(&erase);
        smelt_live_stamp(snapshot, move |args: Vec<SmeltUnknown>| {
            let mut set = live.clone();
            let (op, first, _) = smelt_live_args(args);
            Ok(match op.as_str() {
                "has" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.contains(&member))),
                "add" => { set.insert(T::smelt_from_unknown(first)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.remove(&member))),
                "clear" => { set.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(set.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(set.iter().map(&erase).collect::<Vec<_>>().into()),
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
    //~ The live view, its members erased by `erase` (see `SmeltJsMap::into_live_smelt_unknown_with`).
    fn into_live_smelt_unknown_with(self, erase: impl Fn(T) -> SmeltUnknown + 'static) -> SmeltUnknown {
        let live = self.clone();
        let snapshot = self.into_smelt_unknown_with(&erase);
        smelt_live_stamp(snapshot, move |args: Vec<SmeltUnknown>| {
            let mut set = live.clone();
            let (op, first, _) = smelt_live_args(args);
            Ok(match op.as_str() {
                "has" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.contains(&member))),
                "add" => { set.insert(T::smelt_from_unknown(first)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.remove(&member))),
                "clear" => { set.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(set.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(set.iter().map(&erase).collect::<Vec<_>>().into()),
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

//~ A typed list seen through a live erased array, with the conversion that erases each element.
//~
//~ `erase` is the element type's `IntoSmeltUnknown` for most lists. Where the
//~ element type erases differently from its Rust storage type's impl, the
//~ emitter hands its own typed erasure of that element type: a `(number |
//~ null)[]` stores `Option<f64>`, whose impl answers `undefined` for `None`,
//~ but the source's absent value is `null`.
struct SmeltLiveListWith<T, E> {
    //~ The typed list: the view's storage, handed back by `typed_list`.
    list: SmeltList<T>,
    //~ The erasure of one element.
    erase: E,
}

impl<T: SmeltFromUnknown + Clone + 'static, E: Fn(T) -> SmeltUnknown + 'static> SmeltLiveList for SmeltLiveListWith<T, E> {
    fn element_count(&self) -> usize { self.list.borrow().len() }
    fn element(&self, index: usize) -> Option<SmeltUnknown> { self.list.borrow().get(index).cloned().map(|value| smelt_list_slot_unknown(value, &self.erase)) }
    fn set_element(&self, index: usize, value: SmeltUnknown) { let mut values = self.list.borrow_mut(); if index >= values.len() { values.resize_with(index.saturating_add(1), smelt_list_hole::<T>); } values[index] = T::smelt_from_unknown(value); }
    fn elements(&self) -> Vec<SmeltUnknown> { self.list.borrow().iter().cloned().map(|value| smelt_list_slot_unknown(value, &self.erase)).collect() }
    fn splice_elements(&self, start: usize, delete: usize, items: Vec<SmeltUnknown>) -> Vec<SmeltUnknown> { let mut values = self.list.borrow_mut(); let start = start.min(values.len()); let end = start.saturating_add(delete).min(values.len()); values.splice(start..end, items.into_iter().map(T::smelt_from_unknown)).map(&self.erase).collect() }
    fn replace_elements(&self, values: Vec<SmeltUnknown>) { *self.list.borrow_mut() = values.into_iter().map(T::smelt_from_unknown).collect(); }
    fn typed_list(&self) -> &dyn ::std::any::Any { &self.list }
    fn holds(&self, value: &SmeltUnknown) -> bool { smelt_live_same_content(&(self.erase)(T::smelt_from_unknown(value.clone())), value, 0) }
    fn grow_with_holes(&self, len: usize) { let mut values = self.list.borrow_mut(); if len > values.len() { values.resize_with(len, smelt_list_hole::<T>); } }
    fn is_hole(&self, index: usize) -> bool { self.list.borrow().get(index).is_some_and(smelt_list_value_is_hole) }
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

//~ A typed list's slot, erased by `erase`: a hole reads as `undefined`.
fn smelt_list_slot_unknown<T: 'static>(value: T, erase: &impl Fn(T) -> SmeltUnknown) -> SmeltUnknown { if smelt_list_value_is_hole(&value) { SmeltUnknown::Undefined } else { erase(value) } }

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

impl<T: SmeltFromUnknown + Clone + 'static> SmeltList<T> {
    //~ Erase this list to a LIVE view: the erased array whose elements are this list's.
    fn into_live_smelt_unknown(self) -> SmeltUnknown where T: IntoSmeltUnknown { self.into_live_smelt_unknown_with(IntoSmeltUnknown::into_smelt_unknown) }
    //~ The live view, its elements erased by `erase` (see `SmeltLiveListWith`).
    fn into_live_smelt_unknown_with(self, erase: impl Fn(T) -> SmeltUnknown + 'static) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray { id: self.id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), detached: false, live: Some(::std::rc::Rc::new(::std::cell::RefCell::new(Some(::std::rc::Rc::new(SmeltLiveListWith { list: self, erase }))))) }) }
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

// @smelt:item smelt_list_link when unknown & list_links
//~ A typed list at an `unknown[]` slot (`fill(nums as unknown[])`,
//~ `const all: unknown[] = nums`) is the SAME array there.
//~
//~ `unknown[]` is `SmeltList<SmeltUnknown>`, whose buffer cannot be the typed
//~ list's `Vec<E>`. The conversion links the two buffers instead: the erased
//~ list takes the typed list's identity and a buffer of its elements erased,
//~ and a link table entry per buffer records which side wrote last. A side
//~ that is read or written after the OTHER side was mutably borrowed first
//~ pulls the other's elements across (erasing them, or extracting each with
//~ `SmeltFromUnknown`), so a write through either handle is read through the
//~ other however long both live. Reads after reads cost nothing; only the
//~ first access after a write on the other side converts the elements.
//~
//~ A value the typed list cannot hold (a string written into a `number[]`
//~ through `unknown[]`, which TypeScript accepts) DETACHES the link, as a live
//~ erased array detaches (`SmeltArray::live_view_for`): each side keeps what it
//~ holds from then on. A slot left empty past the end reads back as the typed
//~ list's hole (`smelt_list_hole`).
//~
//~ The table keys are buffer addresses. Each entry holds `Weak` handles on
//~ both buffers, which keeps their allocations (so the addresses) from being
//~ reused while the entry exists; an entry whose buffers are gone is swept
//~ when a new link is made.
struct SmeltListLinkEnd {
    //~ The side that wrote last since the two were in step (0: neither).
    state: ::std::rc::Rc<::std::cell::Cell<u8>>,
    //~ This end's side: 1 typed, 2 erased.
    side: u8,
    //~ The other end's buffer address.
    other: usize,
    //~ Whether either buffer is still owned by some list.
    alive: ::std::rc::Rc<dyn Fn() -> bool>,
    //~ The erased side's buffer, which a second conversion of the same typed
    //~ list hands out again: one array has one `unknown[]` view.
    erased: ::std::rc::Weak<::std::cell::RefCell<Vec<SmeltUnknown>>>,
    //~ Copy this side's element `index` (`usize::MAX`: none) and any change of
    //~ length to the other side: 0 done, 1 busy (fall back to a whole pull),
    //~ 2 detach. The O(1) path of an index store or a push, so a loop
    //~ alternating between the two handles never converts the whole list.
    mirror: ::std::rc::Rc<dyn Fn(usize) -> u8>,
    //~ Refresh this side's buffer from the other's: 0 done, 1 busy (a borrow
    //~ is outstanding; stay out of step), 2 detach.
    pull: ::std::rc::Rc<dyn Fn() -> u8>,
}

thread_local! {
    //~ Every linked buffer, by address.
    static SMELT_LIST_LINKS: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltListLinkEnd>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
    //~ Whether any link exists: the common case returns before the table lookup.
    static SMELT_LIST_LINKED: ::std::cell::Cell<bool> = const { ::std::cell::Cell::new(false) };
}

/// Bring the buffer `values` up to date with the list it is linked to, and
/// record a write when `write` (see `smelt_list_link`).
#[inline]
fn smelt_list_sync<T>(values: &::std::rc::Rc<::std::cell::RefCell<Vec<T>>>, write: bool) {
    if !SMELT_LIST_LINKED.with(::std::cell::Cell::get) { return; }
    let address = ::std::rc::Rc::as_ptr(values) as *const () as usize;
    let Some((state, side, other, pull)) = SMELT_LIST_LINKS.with(|links| links.borrow().get(&address).map(|end| (::std::rc::Rc::clone(&end.state), end.side, end.other, ::std::rc::Rc::clone(&end.pull)))) else { return };
    let last = state.get();
    if last != 0 && last != side {
        match pull() {
            0 => state.set(0),
            2 => { SMELT_LIST_LINKS.with(|links| { let mut links = links.borrow_mut(); links.remove(&address); links.remove(&other); }); return; }
            _ => {}
        }
    }
    if write { state.set(side); }
}

/// Copy element `index` of the buffer `values` (and any change of length) to
/// the buffer it is linked to, after an index store or a push through it.
fn smelt_list_mirror<T>(values: &::std::rc::Rc<::std::cell::RefCell<Vec<T>>>, index: usize) {
    if !SMELT_LIST_LINKED.with(::std::cell::Cell::get) { return; }
    let address = ::std::rc::Rc::as_ptr(values) as *const () as usize;
    let Some((state, side, other, mirror)) = SMELT_LIST_LINKS.with(|links| links.borrow().get(&address).map(|end| (::std::rc::Rc::clone(&end.state), end.side, end.other, ::std::rc::Rc::clone(&end.mirror)))) else { return };
    match mirror(index) {
        0 => {}
        2 => SMELT_LIST_LINKS.with(|links| { let mut links = links.borrow_mut(); links.remove(&address); links.remove(&other); }),
        _ => state.set(side),
    }
}

/// Bring `target` to `source`'s length and copy `source[index]` across, each
/// element converted by `convert` (`None`: it cannot cross, detach): 0 done,
/// 2 detach.
fn smelt_list_mirror_into<S: Clone, D>(source: &[S], target: &mut Vec<D>, index: usize, convert: &dyn Fn(S) -> Option<D>) -> u8 {
    if source.len() < target.len() { target.truncate(source.len()); }
    let start = target.len();
    for value in source.iter().skip(start) { match convert(value.clone()) { Some(converted) => target.push(converted), None => return 2 } }
    if index < start && let (Some(value), Some(slot)) = (source.get(index), target.get_mut(index)) { match convert(value.clone()) { Some(converted) => *slot = converted, None => return 2 } }
    0
}

impl<T: SmeltFromUnknown + Clone + 'static> SmeltList<T> {
    //~ This list at an `unknown[]` slot: an erased-element list with this
    //~ list's identity, linked to it (see `smelt_list_link`). `erase` erases
    //~ one element.
    fn into_linked_unknown_list(self, erase: impl Fn(T) -> SmeltUnknown + 'static) -> SmeltList<SmeltUnknown> {
        let typed = self.storage();
        let typed_address = ::std::rc::Rc::as_ptr(&typed) as *const () as usize;
        if let Some(existing) = SMELT_LIST_LINKS.with(|links| links.borrow().get(&typed_address).and_then(|end| end.erased.upgrade())) { return SmeltList::with_storage(self.id(), existing); }
        let erase = ::std::rc::Rc::new(erase);
        let erased = ::std::rc::Rc::new(::std::cell::RefCell::new(typed.borrow().iter().cloned().map(|value| smelt_list_slot_unknown(value, &*erase)).collect::<Vec<_>>()));
        let erased_address = ::std::rc::Rc::as_ptr(&erased) as *const () as usize;
        let (typed_weak, erased_weak) = (::std::rc::Rc::downgrade(&typed), ::std::rc::Rc::downgrade(&erased));
        let alive: ::std::rc::Rc<dyn Fn() -> bool> = { let (typed_weak, erased_weak) = (typed_weak.clone(), erased_weak.clone()); ::std::rc::Rc::new(move || typed_weak.strong_count() > 0 || erased_weak.strong_count() > 0) };
        //~ One erased element at the typed element type, `None` when it does not round-trip.
        let narrow: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> Option<T>> = { let erase = ::std::rc::Rc::clone(&erase); ::std::rc::Rc::new(move |value: SmeltUnknown| { if matches!(value, SmeltUnknown::Undefined) { return Some(smelt_list_hole::<T>()); } let held = T::smelt_from_unknown(value.clone()); smelt_live_same_content(&erase(held.clone()), &value, 0).then_some(held) }) };
        let pull_typed: ::std::rc::Rc<dyn Fn() -> u8> = { let (typed_weak, erased_weak, narrow) = (typed_weak.clone(), erased_weak.clone(), ::std::rc::Rc::clone(&narrow)); ::std::rc::Rc::new(move || {
            let (Some(typed), Some(erased)) = (typed_weak.upgrade(), erased_weak.upgrade()) else { return 2 };
            let Ok(source) = erased.try_borrow() else { return 1 };
            let mut values = Vec::with_capacity(source.len());
            for value in source.iter() { match narrow(value.clone()) { Some(held) => values.push(held), None => return 2 } }
            drop(source);
            match typed.try_borrow_mut() { Ok(mut target) => { *target = values; 0 } Err(_) => 1 }
        }) };
        let pull_erased: ::std::rc::Rc<dyn Fn() -> u8> = { let erase = ::std::rc::Rc::clone(&erase); ::std::rc::Rc::new(move || {
            let (Some(typed), Some(erased)) = (typed_weak.upgrade(), erased_weak.upgrade()) else { return 2 };
            let Ok(source) = typed.try_borrow() else { return 1 };
            let values = source.iter().cloned().map(|value| smelt_list_slot_unknown(value, &*erase)).collect::<Vec<_>>();
            drop(source);
            match erased.try_borrow_mut() { Ok(mut target) => { *target = values; 0 } Err(_) => 1 }
        }) };
        let mirror_typed: ::std::rc::Rc<dyn Fn(usize) -> u8> = { let (typed_weak, erased_weak, erase) = (::std::rc::Rc::downgrade(&typed), ::std::rc::Rc::downgrade(&erased), ::std::rc::Rc::clone(&erase)); ::std::rc::Rc::new(move |index| {
            let (Some(typed), Some(erased)) = (typed_weak.upgrade(), erased_weak.upgrade()) else { return 2 };
            let (Ok(source), Ok(mut target)) = (typed.try_borrow(), erased.try_borrow_mut()) else { return 1 };
            smelt_list_mirror_into(&source, &mut target, index, &|value| Some(smelt_list_slot_unknown(value, &*erase)))
        }) };
        let mirror_erased: ::std::rc::Rc<dyn Fn(usize) -> u8> = { let (typed_weak, erased_weak, narrow) = (::std::rc::Rc::downgrade(&typed), ::std::rc::Rc::downgrade(&erased), ::std::rc::Rc::clone(&narrow)); ::std::rc::Rc::new(move |index| {
            let (Some(typed), Some(erased)) = (typed_weak.upgrade(), erased_weak.upgrade()) else { return 2 };
            let (Ok(source), Ok(mut target)) = (erased.try_borrow(), typed.try_borrow_mut()) else { return 1 };
            smelt_list_mirror_into(&source, &mut target, index, &*narrow)
        }) };
        let state = ::std::rc::Rc::new(::std::cell::Cell::new(0));
        SMELT_LIST_LINKS.with(|links| {
            let mut links = links.borrow_mut();
            links.retain(|_, end| (end.alive)());
            links.insert(typed_address, SmeltListLinkEnd { state: ::std::rc::Rc::clone(&state), side: 1, other: erased_address, alive: ::std::rc::Rc::clone(&alive), erased: ::std::rc::Rc::downgrade(&erased), mirror: mirror_typed, pull: pull_typed });
            links.insert(erased_address, SmeltListLinkEnd { state, side: 2, other: typed_address, alive, erased: ::std::rc::Rc::downgrade(&erased), mirror: mirror_erased, pull: pull_erased });
        });
        SMELT_LIST_LINKED.with(|linked| linked.set(true));
        SmeltList::with_storage(self.id(), erased)
    }
}
// @smelt:item-end
