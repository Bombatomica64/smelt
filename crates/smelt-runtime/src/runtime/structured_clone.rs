// `structuredClone` of a TYPED value: the HTML structured clone algorithm
// (`StructuredSerializeInternal` + `StructuredDeserialize`), as one trait
// with an impl per Rust storage type a TypeScript value can have.
//
// The copy keeps the value's Rust type, so a `SmeltList<Item>` clones to a
// `SmeltList<Item>`, never through the erased carrier. Every container gets
// a fresh identity, and the memo makes a reference that occurs twice in the
// graph (or a cycle) occur twice in the copy as ONE new object, as the
// spec's memory map does. The emitter adds an impl for each generated record
// struct and concrete union enum (`emitter::structured_clone`).
//
// Known differences from the platform, documented where they are decided:
// a class instance clones to an instance of its class (the platform answers a
// plain object without the prototype, whose methods are then gone), and a
// `DataCloneError` message names the value as `function`, where Node prints
// the function's source text.
// @smelt:item SmeltStructuredClone when structured_clone_typed
/// The clones made so far by one `structuredClone` call, by the identity of
/// the source object (the address of its shared storage).
#[derive(Default)]
struct SmeltCloneMemo { done: ::std::collections::HashMap<usize, Box<dyn ::std::any::Any>> }
impl SmeltCloneMemo {
    //~ The copy already made of the object at `key`, when there is one.
    fn get<T: Clone + 'static>(&self, key: usize) -> Option<T> { self.done.get(&key).and_then(|copy| copy.downcast_ref::<T>()).cloned() }
    //~ Record `copy` as the clone of the object at `key`, before its contents
    //~ are copied, so a cycle back to it finds the copy.
    fn insert<T: Clone + 'static>(&mut self, key: usize, copy: &T) { self.done.insert(key, Box::new(copy.clone())); }
}
/// A value the structured clone algorithm cannot copy (a function), named
/// for the `DataCloneError` it throws.
struct SmeltDataCloneError(String);
/// A deep copy of `self` per the structured clone algorithm.
trait SmeltStructuredClone: Sized {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError>;
}
/// `structuredClone(value)` for a typed value.
fn smelt_structured_clone_value<T: SmeltStructuredClone>(value: &T) -> Result<T, SmeltDataCloneError> { value.smelt_clone_in(&mut SmeltCloneMemo::default()) }
macro_rules! smelt_clone_by_value { ($($ty:ty),*) => { $(impl SmeltStructuredClone for $ty { fn smelt_clone_in(&self, _memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> { Ok(self.clone()) } })* }; }
smelt_clone_by_value!(f64, bool, i64, (), JsString, String);
impl<T: SmeltStructuredClone> SmeltStructuredClone for Option<T> {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> { self.as_ref().map(|value| value.smelt_clone_in(memo)).transpose() }
}
//~ A function is not serializable: `DataCloneError`.
impl<F: ?Sized> SmeltStructuredClone for ::std::rc::Rc<F> {
    fn smelt_clone_in(&self, _memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> { Err(SmeltDataCloneError("function could not be cloned.".to_owned())) }
}
macro_rules! smelt_clone_tuple { ($($name:ident : $index:tt),+) => { impl<$($name: SmeltStructuredClone),+> SmeltStructuredClone for ($($name,)+) { fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> { Ok(($(self.$index.smelt_clone_in(memo)?,)+)) } } }; }
smelt_clone_tuple!(A: 0);
smelt_clone_tuple!(A: 0, B: 1);
smelt_clone_tuple!(A: 0, B: 1, C: 2);
smelt_clone_tuple!(A: 0, B: 1, C: 2, D: 3);
smelt_clone_tuple!(A: 0, B: 1, C: 2, D: 3, E: 4);
smelt_clone_tuple!(A: 0, B: 1, C: 2, D: 3, E: 4, F: 5);
smelt_clone_tuple!(A: 0, B: 1, C: 2, D: 3, E: 4, F: 5, G: 6);
smelt_clone_tuple!(A: 0, B: 1, C: 2, D: 3, E: 4, F: 5, G: 6, H: 7);
// @smelt:item-end

// @smelt:item smelt_data_clone_error when structured_clone_throws & unknown
/// The `DataCloneError` `DOMException` a failed clone throws.
fn smelt_data_clone_error(error: SmeltDataCloneError) -> Box<dyn ::std::error::Error> { smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("DataCloneError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String(error.0.into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))) }
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltUnknown when structured_clone_typed & structured_clone
//~ An erased value inside a typed graph is copied by the erased helper.
impl SmeltStructuredClone for SmeltUnknown {
    fn smelt_clone_in(&self, _memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> { if matches!(self, SmeltUnknown::Function(_)) { return Err(SmeltDataCloneError("function could not be cloned.".to_owned())); } Ok(smelt_structured_clone(self.clone())) }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltList when structured_clone_typed & smelt_list
impl<T: SmeltStructuredClone + Clone + 'static> SmeltStructuredClone for SmeltList<T> {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.values) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let copy = SmeltList::new(Vec::new());
        memo.insert(key, &copy);
        let items = self.values.borrow().clone();
        for item in &items { let item = item.smelt_clone_in(memo)?; copy.values.borrow_mut().push(item); }
        Ok(copy)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltJsMap when structured_clone_typed & unknown
impl<K: SmeltStructuredClone + SmeltJsKeyEq + Clone + 'static, V: SmeltStructuredClone + Clone + 'static> SmeltStructuredClone for SmeltJsMap<K, V> {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.store) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let mut copy = SmeltJsMap::new();
        memo.insert(key, &copy);
        for (entry_key, value) in self.iter() { let entry_key = entry_key.smelt_clone_in(memo)?; let value = value.smelt_clone_in(memo)?; copy.insert(entry_key, value); }
        Ok(copy)
    }
}
impl<T: SmeltStructuredClone + Clone + IntoSmeltUnknown + 'static> SmeltStructuredClone for SmeltJsSet<T> {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.store) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let mut copy = SmeltJsSet::new();
        memo.insert(key, &copy);
        for member in self.iter() { let member = member.smelt_clone_in(memo)?; copy.insert(member); }
        Ok(copy)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltPrimSet when structured_clone_typed & prim_set
impl<T: SmeltStructuredClone + Clone + ::std::cmp::Eq + ::std::hash::Hash + 'static> SmeltStructuredClone for SmeltPrimSet<T> {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.store) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let mut copy = SmeltPrimSet::new();
        memo.insert(key, &copy);
        for member in self.iter() { let member = member.smelt_clone_in(memo)?; copy.insert(member); }
        Ok(copy)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltRecord when structured_clone_typed & record
impl<K: SmeltStructuredClone + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey + 'static, V: SmeltStructuredClone + Clone + 'static> SmeltStructuredClone for SmeltRecord<K, V> {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.store) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let copy = SmeltRecord::new();
        memo.insert(key, &copy);
        for (entry_key, value) in self.iter() { let entry_key = entry_key.smelt_clone_in(memo)?; let value = value.smelt_clone_in(memo)?; copy.insert(entry_key, value); }
        Ok(copy)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltDate when structured_clone_typed & date
//~ A `Date` clones to a new `Date` at the same time value.
impl SmeltStructuredClone for SmeltDate {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.time) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let mut copy = self.clone();
        copy.id = smelt_next_object_id();
        copy.time = ::std::rc::Rc::new(::std::cell::Cell::new(self.get_time()));
        memo.insert(key, &copy);
        Ok(copy)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltRegExp when structured_clone_typed & dep_regress
//~ A `RegExp` clones to a new one with the same source and flags; its
//~ `lastIndex` is not serialized, so the copy starts at 0.
impl SmeltStructuredClone for SmeltRegExp {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = ::std::rc::Rc::as_ptr(&self.last_index) as *const () as usize;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let mut copy = self.clone();
        copy.id = smelt_next_object_id();
        copy.last_index = ::std::rc::Rc::new(::std::cell::RefCell::new(0));
        memo.insert(key, &copy);
        Ok(copy)
    }
}
// @smelt:item-end

// @smelt:item impl_SmeltStructuredClone_for_SmeltTypedArray when structured_clone_typed & byte_array
//~ A typed array clones to a view of its kind over a copy of the bytes it
//~ sees, in a fresh buffer.
impl SmeltStructuredClone for SmeltTypedArray {
    fn smelt_clone_in(&self, memo: &mut SmeltCloneMemo) -> Result<Self, SmeltDataCloneError> {
        let key = self.id;
        if let Some(copy) = memo.get::<Self>(key) { return Ok(copy); }
        let width = self.kind.byte_width();
        let bytes = self.bytes.borrow()[self.byte_offset..self.byte_offset + self.length * width].to_vec();
        let copy = SmeltTypedArray::with_bytes(self.kind, bytes);
        memo.insert(key, &copy);
        Ok(copy)
    }
}
// @smelt:item-end
