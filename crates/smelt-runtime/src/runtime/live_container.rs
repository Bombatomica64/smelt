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
    fn into_live_smelt_unknown(self) -> SmeltUnknown {
        let live = self.clone();
        smelt_live_stamp(self.into_smelt_unknown(), move |args: Vec<SmeltUnknown>| {
            let mut map = live.clone();
            let (op, first, second) = smelt_live_args(args);
            Ok(match op.as_str() {
                "get" => smelt_live_key::<K>(&first).and_then(|key| map.get(&key)).map_or(SmeltUnknown::Undefined, IntoSmeltUnknown::into_smelt_unknown),
                "has" => SmeltUnknown::Bool(smelt_live_key::<K>(&first).is_some_and(|key| map.contains_key(&key))),
                "set" => { map.insert(K::smelt_from_unknown(first), V::smelt_from_unknown(second)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<K>(&first).is_some_and(|key| map.remove(&key).is_some())),
                "clear" => { map.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(map.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(map.iter().map(|(key, value)| SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), vec![key.into_smelt_unknown(), value.into_smelt_unknown()]))).collect::<Vec<_>>().into()),
                "handoff" => smelt_live_park(map),
                "id" => SmeltUnknown::Number(f64::from(u32::try_from(map.id).unwrap_or(u32::MAX))),
                _ => SmeltUnknown::Undefined,
            })
        })
    }
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltJsSet<T> {
    //~ Erase this set to a LIVE view (see `SmeltJsMap::into_live_smelt_unknown`).
    fn into_live_smelt_unknown(self) -> SmeltUnknown {
        let live = self.clone();
        smelt_live_stamp(self.into_smelt_unknown(), move |args: Vec<SmeltUnknown>| {
            let mut set = live.clone();
            let (op, first, _) = smelt_live_args(args);
            Ok(match op.as_str() {
                "has" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.contains(&member))),
                "add" => { set.insert(T::smelt_from_unknown(first)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.remove(&member))),
                "clear" => { set.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(set.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(set.iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>().into()),
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
    fn into_live_smelt_unknown(self) -> SmeltUnknown {
        let live = self.clone();
        smelt_live_stamp(self.into_smelt_unknown(), move |args: Vec<SmeltUnknown>| {
            let mut set = live.clone();
            let (op, first, _) = smelt_live_args(args);
            Ok(match op.as_str() {
                "has" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.contains(&member))),
                "add" => { set.insert(T::smelt_from_unknown(first)); SmeltUnknown::Undefined }
                "delete" => SmeltUnknown::Bool(smelt_live_key::<T>(&first).is_some_and(|member| set.remove(&member))),
                "clear" => { set.clear(); SmeltUnknown::Undefined }
                "size" => SmeltUnknown::Number(f64::from(u32::try_from(set.len()).unwrap_or(u32::MAX))),
                "entries" => SmeltUnknown::Array(set.iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>().into()),
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
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltLiveList for SmeltList<T> {
    fn element_count(&self) -> usize { self.borrow().len() }
    fn element(&self, index: usize) -> Option<SmeltUnknown> { self.borrow().get(index).cloned().map(IntoSmeltUnknown::into_smelt_unknown) }
    fn set_element(&self, index: usize, value: SmeltUnknown) { let mut values = self.borrow_mut(); if index >= values.len() { values.resize_with(index.saturating_add(1), || T::smelt_from_unknown(SmeltUnknown::Undefined)); } values[index] = T::smelt_from_unknown(value); }
    fn elements(&self) -> Vec<SmeltUnknown> { self.borrow().iter().cloned().map(IntoSmeltUnknown::into_smelt_unknown).collect() }
    fn splice_elements(&self, start: usize, delete: usize, items: Vec<SmeltUnknown>) -> Vec<SmeltUnknown> { let mut values = self.borrow_mut(); let start = start.min(values.len()); let end = start.saturating_add(delete).min(values.len()); values.splice(start..end, items.into_iter().map(T::smelt_from_unknown)).map(IntoSmeltUnknown::into_smelt_unknown).collect() }
    fn replace_elements(&self, values: Vec<SmeltUnknown>) { *self.borrow_mut() = values.into_iter().map(T::smelt_from_unknown).collect(); }
    fn typed_list(&self) -> &dyn ::std::any::Any { self }
}

impl<T: IntoSmeltUnknown + SmeltFromUnknown + Clone + 'static> SmeltList<T> {
    //~ Erase this list to a LIVE view: the erased array whose elements are this list's.
    fn into_live_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray { id: self.id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(Vec::new())), detached: false, live: Some(::std::rc::Rc::new(self)) }) }
}

impl SmeltArray {
    //~ The typed list this array is a live view of, when it is a `SmeltList<T>`: the same handle, not a copy.
    fn live_typed<T: 'static>(&self) -> Option<SmeltList<T>> { self.live.as_ref()?.typed_list().downcast_ref::<SmeltList<T>>().cloned() }
}
// @smelt:item-end
