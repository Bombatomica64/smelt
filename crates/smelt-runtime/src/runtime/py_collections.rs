// Python's `collections` containers: `deque`, `Counter`, `defaultdict` and
// `OrderedDict`, each a concrete generic Rust type.
//
// A hand port of Python code that uses these would reach for the obvious Rust
// shapes: a `VecDeque` for a deque, an insertion-ordered map counting `int`s
// for a `Counter`, the same map plus a stored factory for a `defaultdict`. That
// is what each type here is. The element, key and value types are the Python
// ones the frontend proved (`deque[int]` is `SmeltPyDeque<SmeltPyInt>`), so no
// value crosses an erased carrier.
//
// Every container is a Python object with reference semantics: `b = a;
// b.append(x)` appends to `a`, and a container mutated inside a function is
// mutated for its caller. So the contents live in ONE store behind
// `Rc<RefCell<..>>`, and `.clone()` is another handle to it, exactly like
// `SmeltList` and `SmeltPrimSet`. A genuine copy (`d.copy()`, `Counter(c)`, the
// result of `c1 + c2`) is a NEW store.
//
// Reads hand out owned values (a clone of the element), because a borrow out of
// the `RefCell` cannot outlive its guard. A value that is itself a container
// (`defaultdict(list)`) clones as another handle, so `d[k].append(x)` appends
// to the list the map holds, as in Python.
//
// The method names and argument orders are what `smelt-codegen-rust`'s
// `py_collections` emitter writes: one Python method, one Rust method, with a
// key a lookup only reads passed by reference.

// @smelt:item SmeltPyDeque when py_collections
/// Python's `collections.deque`: a double-ended queue with an optional bound.
///
/// Appending to a full bounded deque drops an item from the opposite end, as
/// `CPython` does; every end operation is O(1) on the `VecDeque`.
#[allow(dead_code)]
struct SmeltPyDeque<T> {
    store: ::std::rc::Rc<::std::cell::RefCell<SmeltPyDequeStore<T>>>,
}

/// The shared contents of a [`SmeltPyDeque`].
#[allow(dead_code)]
struct SmeltPyDequeStore<T> {
    items: ::std::collections::VecDeque<T>,
    maxlen: Option<usize>,
}

impl<T> Clone for SmeltPyDeque<T> { fn clone(&self) -> Self { Self { store: ::std::rc::Rc::clone(&self.store) } } }
impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltPyDeque<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.store.borrow().items.fmt(formatter) } }
impl<T> Default for SmeltPyDeque<T> { fn default() -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltPyDequeStore { items: ::std::collections::VecDeque::new(), maxlen: None })) } } }
impl<T: PartialEq> PartialEq for SmeltPyDeque<T> { fn eq(&self, other: &Self) -> bool { self.store.borrow().items == other.store.borrow().items } }

/// Python's index rule for a sequence of `len` items: a negative index counts
/// from the end; `None` when the position is outside the sequence.
#[allow(dead_code)]
fn smelt_py_collection_position(index: i64, len: usize) -> Option<usize> {
    let len_i64 = i64::try_from(len).unwrap_or(i64::MAX);
    let at = if index < 0 { index.saturating_add(len_i64) } else { index };
    usize::try_from(at).ok().filter(|at| *at < len)
}

#[allow(dead_code)]
impl<T: Clone + PartialEq> SmeltPyDeque<T> {
    //~ `deque(iterable, maxlen)`: the iterable's items appended in order, so a
    //~ bound keeps only the last `maxlen` of them. A negative bound is
    //~ `CPython`'s `ValueError`.
    fn new(items: SmeltList<T>, maxlen: Option<i64>) -> Result<Self, Box<dyn ::std::error::Error>> {
        let maxlen = match maxlen {
            Some(bound) => Some(usize::try_from(bound).map_err(|_negative| smelt_py_value_error("maxlen must be non-negative"))?),
            None => None,
        };
        let deque = Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltPyDequeStore { items: ::std::collections::VecDeque::new(), maxlen })) };
        deque.extend(items);
        Ok(deque)
    }
    //~ `d.append(x)`: add on the right, dropping the leftmost item when full.
    fn append(&self, item: T) {
        let mut store = self.store.borrow_mut();
        if store.maxlen == Some(0) { return; }
        if store.maxlen.is_some_and(|bound| store.items.len() >= bound) { store.items.pop_front(); }
        store.items.push_back(item);
    }
    //~ `d.appendleft(x)`: add on the left, dropping the rightmost item when full.
    fn appendleft(&self, item: T) {
        let mut store = self.store.borrow_mut();
        if store.maxlen == Some(0) { return; }
        if store.maxlen.is_some_and(|bound| store.items.len() >= bound) { store.items.pop_back(); }
        store.items.push_front(item);
    }
    //~ `d.pop()`: remove and return the rightmost item.
    fn pop(&self) -> Result<T, Box<dyn ::std::error::Error>> {
        self.store.borrow_mut().items.pop_back().ok_or_else(|| smelt_py_index_error("pop from an empty deque"))
    }
    //~ `d.popleft()`: remove and return the leftmost item.
    fn popleft(&self) -> Result<T, Box<dyn ::std::error::Error>> {
        self.store.borrow_mut().items.pop_front().ok_or_else(|| smelt_py_index_error("pop from an empty deque"))
    }
    //~ `d.extend(iterable)`: append each item in order. The items are read
    //~ before any is appended, so `d.extend(d)` doubles the deque.
    fn extend(&self, items: SmeltList<T>) {
        for item in items.to_vec() { self.append(item); }
    }
    //~ `d.extendleft(iterable)`: appendleft each item in order, which reverses
    //~ them.
    fn extendleft(&self, items: SmeltList<T>) {
        for item in items.to_vec() { self.appendleft(item); }
    }
    //~ `d.rotate(n)`: rotate `n` steps to the right (left when negative).
    fn rotate(&self, steps: i64) {
        let mut store = self.store.borrow_mut();
        let len = store.items.len();
        if len < 2 { return; }
        let len_i64 = i64::try_from(len).unwrap_or(i64::MAX);
        let right = usize::try_from(steps.rem_euclid(len_i64)).unwrap_or(0);
        store.items.rotate_right(right);
    }
    //~ `d.clear()`.
    fn clear(&self) { self.store.borrow_mut().items.clear(); }
    //~ `len(d)`.
    fn len(&self) -> i64 { i64::try_from(self.store.borrow().items.len()).unwrap_or(i64::MAX) }
    //~ `d[i]`, a negative `i` counting from the right.
    fn get(&self, index: i64) -> Result<T, Box<dyn ::std::error::Error>> {
        let store = self.store.borrow();
        smelt_py_collection_position(index, store.items.len()).and_then(|at| store.items.get(at).cloned()).ok_or_else(|| smelt_py_index_error("deque index out of range"))
    }
    //~ `d[i] = x`.
    fn set(&self, index: i64, item: T) -> Result<(), Box<dyn ::std::error::Error>> {
        let mut store = self.store.borrow_mut();
        let at = smelt_py_collection_position(index, store.items.len()).ok_or_else(|| smelt_py_index_error("deque index out of range"))?;
        store.items[at] = item;
        Ok(())
    }
    //~ `x in d`.
    fn contains(&self, item: &T) -> bool { self.store.borrow().items.contains(item) }
    //~ `d.count(x)`.
    fn count(&self, item: &T) -> SmeltPyInt { SmeltPyInt::from(self.store.borrow().items.iter().filter(|candidate| *candidate == item).count()) }
    //~ `d.index(x)`: the first position holding `x`.
    fn index(&self, item: &T) -> Result<SmeltPyInt, Box<dyn ::std::error::Error>> {
        self.store.borrow().items.iter().position(|candidate| candidate == item).map(SmeltPyInt::from).ok_or_else(|| smelt_py_value_error("deque.index(x): x not in deque"))
    }
    //~ `d.remove(x)`: delete the first `x`.
    fn remove(&self, item: &T) -> Result<(), Box<dyn ::std::error::Error>> {
        let mut store = self.store.borrow_mut();
        let at = store.items.iter().position(|candidate| candidate == item).ok_or_else(|| smelt_py_value_error("deque.remove(x): x not in deque"))?;
        store.items.remove(at);
        Ok(())
    }
    //~ `d.reverse()`, in place.
    fn reverse(&self) { self.store.borrow_mut().items.make_contiguous().reverse(); }
    //~ `d.copy()`: a new deque with the same items and bound.
    fn copy(&self) -> Self {
        let store = self.store.borrow();
        Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltPyDequeStore { items: store.items.clone(), maxlen: store.maxlen })) }
    }
    //~ `d.maxlen`: the bound, or `None`.
    fn maxlen(&self) -> Option<SmeltPyInt> { self.store.borrow().maxlen.map(SmeltPyInt::from) }
    //~ The items left to right, as the list a `for` loop walks. A snapshot, so
    //~ a body that mutates the deque cannot invalidate the walk.
    fn items(&self) -> SmeltList<T> { SmeltList::new(self.store.borrow().items.iter().cloned().collect()) }
    //~ `d1 == d2`: same items in the same order (the bound is not compared).
    fn equals(&self, other: &Self) -> bool { self.store.borrow().items == other.store.borrow().items }
}

impl<T: SmeltPyRepr> SmeltPyRepr for SmeltPyDeque<T> {
    fn py_repr(&self) -> String {
        let store = self.store.borrow();
        let items = smelt_py_repr_items(store.items.iter());
        match store.maxlen {
            Some(bound) => format!("deque([{items}], maxlen={bound})"),
            None => format!("deque([{items}])"),
        }
    }
}

// @smelt:item-end

// @smelt:item SmeltPyCounter when py_collections & dep_indexmap
/// Python's `collections.Counter`: an insertion-ordered count per key.
///
/// A missing key reads as `0` without being inserted; `most_common`, the
/// `repr` and `elements` follow `CPython`'s orders (count descending with ties
/// in insertion order, and insertion order).
#[allow(dead_code)]
struct SmeltPyCounter<K> {
    store: ::std::rc::Rc<::std::cell::RefCell<::indexmap::IndexMap<K, SmeltPyInt>>>,
}

impl<K> Clone for SmeltPyCounter<K> { fn clone(&self) -> Self { Self { store: ::std::rc::Rc::clone(&self.store) } } }
impl<K: ::std::fmt::Debug> ::std::fmt::Debug for SmeltPyCounter<K> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.store.borrow().fmt(formatter) } }
impl<K> Default for SmeltPyCounter<K> { fn default() -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(::indexmap::IndexMap::new())) } } }
impl<K: Clone + Eq + ::std::hash::Hash> PartialEq for SmeltPyCounter<K> { fn eq(&self, other: &Self) -> bool { self.equals(other) } }

#[allow(dead_code)]
impl<K: Clone + Eq + ::std::hash::Hash> SmeltPyCounter<K> {
    //~ A counter over `counts` as they are.
    fn from_map(counts: ::indexmap::IndexMap<K, SmeltPyInt>) -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(counts)) } }
    //~ `Counter(iterable)`: each item counted once, keys in first-seen order.
    fn new(items: SmeltList<K>) -> Self {
        let counter = Self::from_map(::indexmap::IndexMap::new());
        counter.update(items);
        counter
    }
    //~ `Counter(mapping)`: the given count per key.
    fn from_counts(counts: SmeltList<(K, SmeltPyInt)>) -> Self { Self::from_map(counts.to_vec().into_iter().collect()) }
    //~ `c[k]`: the count, `0` for a missing key (which stays missing).
    fn get(&self, key: &K) -> SmeltPyInt { self.store.borrow().get(key).cloned().unwrap_or_default() }
    //~ `c.get(k, default)`.
    fn get_or(&self, key: &K, default: SmeltPyInt) -> SmeltPyInt { self.store.borrow().get(key).cloned().unwrap_or(default) }
    //~ `c[k] = n`.
    fn set(&self, key: K, count: SmeltPyInt) { self.store.borrow_mut().insert(key, count); }
    //~ `del c[k]`: a missing key is not an error for a `Counter`.
    fn delete(&self, key: &K) { self.store.borrow_mut().shift_remove(key); }
    //~ `c.update(iterable)`: add one per item.
    fn update(&self, items: SmeltList<K>) {
        let mut store = self.store.borrow_mut();
        for key in items.to_vec() {
            let count = store.entry(key).or_default();
            *count = &*count + SmeltPyInt::from(1_i64);
        }
    }
    //~ `c.update(other)` with a counter: add its counts.
    fn update_counts(&self, other: &Self) {
        let added: Vec<(K, SmeltPyInt)> = other.store.borrow().iter().map(|(key, count)| (key.clone(), count.clone())).collect();
        let mut store = self.store.borrow_mut();
        for (key, count) in added {
            let slot = store.entry(key).or_default();
            *slot = &*slot + count;
        }
    }
    //~ `c.subtract(iterable)`: subtract one per item, keeping zero and
    //~ negative counts.
    fn subtract(&self, items: SmeltList<K>) {
        let mut store = self.store.borrow_mut();
        for key in items.to_vec() {
            let count = store.entry(key).or_default();
            *count = &*count - SmeltPyInt::from(1_i64);
        }
    }
    //~ `c.subtract(other)` with a counter: subtract its counts.
    fn subtract_counts(&self, other: &Self) {
        let taken: Vec<(K, SmeltPyInt)> = other.store.borrow().iter().map(|(key, count)| (key.clone(), count.clone())).collect();
        let mut store = self.store.borrow_mut();
        for (key, count) in taken {
            let slot = store.entry(key).or_default();
            *slot = &*slot - count;
        }
    }
    //~ The `(key, count)` pairs, most common first; ties keep insertion order
    //~ (`sorted(..., key=count, reverse=True)` is stable).
    fn ranked(&self) -> Vec<(K, SmeltPyInt)> {
        let mut pairs: Vec<(K, SmeltPyInt)> = self.store.borrow().iter().map(|(key, count)| (key.clone(), count.clone())).collect();
        pairs.sort_by(|left, right| right.1.cmp(&left.1));
        pairs
    }
    //~ `c.most_common(n)`: the `n` most common pairs, every pair for `None`.
    fn most_common(&self, limit: Option<i64>) -> SmeltList<(K, SmeltPyInt)> {
        let mut pairs = self.ranked();
        if let Some(limit) = limit { pairs.truncate(usize::try_from(limit).unwrap_or(0)); }
        SmeltList::new(pairs)
    }
    //~ `c.elements()`: each key repeated by its positive count, keys in
    //~ insertion order.
    fn elements(&self) -> SmeltList<K> {
        let mut items = Vec::new();
        for (key, count) in self.store.borrow().iter() {
            let mut left = count.clone();
            while !left.is_negative() && !left.is_zero() {
                items.push(key.clone());
                left = &left - SmeltPyInt::from(1_i64);
            }
        }
        SmeltList::new(items)
    }
    //~ `c.total()`: the sum of the counts.
    fn total(&self) -> SmeltPyInt { self.store.borrow().values().sum() }
    //~ `len(c)`: the number of keys.
    fn len(&self) -> i64 { i64::try_from(self.store.borrow().len()).unwrap_or(i64::MAX) }
    //~ `k in c`.
    fn contains(&self, key: &K) -> bool { self.store.borrow().contains_key(key) }
    //~ The keys in insertion order: `list(c)`, `c.keys()`, a `for` loop.
    fn keys(&self) -> SmeltList<K> { SmeltList::new(self.store.borrow().keys().cloned().collect()) }
    //~ `c.values()`.
    fn values(&self) -> SmeltList<SmeltPyInt> { SmeltList::new(self.store.borrow().values().cloned().collect()) }
    //~ `c.items()`.
    fn items(&self) -> SmeltList<(K, SmeltPyInt)> { SmeltList::new(self.store.borrow().iter().map(|(key, count)| (key.clone(), count.clone())).collect()) }
    //~ `c.clear()`.
    fn clear(&self) { self.store.borrow_mut().clear(); }
    //~ `c.copy()`.
    fn copy(&self) -> Self { Self::from_map(self.store.borrow().clone()) }
    //~ `c.pop(k)`: remove `k` and return its count; `KeyError` when missing.
    fn pop(&self, key: &K) -> Result<SmeltPyInt, Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        self.store.borrow_mut().shift_remove(key).ok_or_else(|| smelt_py_key_error(key))
    }
    //~ `c1 + c2`: the summed counts, keeping only positive ones (`self`'s keys
    //~ first, then `other`'s new ones).
    fn plus(&self, other: &Self) -> Self {
        let left = self.store.borrow();
        let right = other.store.borrow();
        let mut out = ::indexmap::IndexMap::new();
        for (key, count) in left.iter() {
            let sum = count + right.get(key).cloned().unwrap_or_default();
            if !sum.is_negative() && !sum.is_zero() { out.insert(key.clone(), sum); }
        }
        for (key, count) in right.iter() {
            if !left.contains_key(key) && !count.is_negative() && !count.is_zero() { out.insert(key.clone(), count.clone()); }
        }
        Self::from_map(out)
    }
    //~ `c1 - c2`: the differences, keeping only positive ones; a key only
    //~ `other` has counts `0 - n` when that is positive.
    fn minus(&self, other: &Self) -> Self {
        let left = self.store.borrow();
        let right = other.store.borrow();
        let mut out = ::indexmap::IndexMap::new();
        for (key, count) in left.iter() {
            let difference = count - right.get(key).cloned().unwrap_or_default();
            if !difference.is_negative() && !difference.is_zero() { out.insert(key.clone(), difference); }
        }
        for (key, count) in right.iter() {
            if !left.contains_key(key) && count.is_negative() { out.insert(key.clone(), SmeltPyInt::from(0_i64) - count); }
        }
        Self::from_map(out)
    }
    //~ `c1 == c2`: equal counts for every key, a missing key counting `0`.
    fn equals(&self, other: &Self) -> bool {
        let left = self.store.borrow();
        let right = other.store.borrow();
        left.iter().all(|(key, count)| right.get(key).cloned().unwrap_or_default() == *count)
            && right.iter().all(|(key, count)| left.get(key).cloned().unwrap_or_default() == *count)
    }
}

impl<K: SmeltPyRepr + Clone + Eq + ::std::hash::Hash> SmeltPyRepr for SmeltPyCounter<K> {
    fn py_repr(&self) -> String {
        let ranked = self.ranked();
        if ranked.is_empty() { return "Counter()".to_owned(); }
        format!("Counter({{{}}})", ranked.iter().map(|(key, count)| format!("{}: {}", key.py_repr(), count.py_repr())).collect::<Vec<_>>().join(", "))
    }
}

// @smelt:item-end

// @smelt:item SmeltPyDefaultDict when py_collections & dep_indexmap
/// Python's `collections.defaultdict`: an insertion-ordered map whose `d[k]`
/// read of a missing key inserts and returns the factory's value.
///
/// The factory is stored with the map, as `CPython` stores
/// `default_factory`, so a defaultdict handed to another function keeps
/// inserting the same defaults. `factory_repr` is the factory's `repr` for the
/// map's own `repr` (`<class 'list'>`).
#[allow(dead_code)]
struct SmeltPyDefaultDict<K, V> {
    store: ::std::rc::Rc<::std::cell::RefCell<::indexmap::IndexMap<K, V>>>,
    factory: ::std::rc::Rc<dyn Fn() -> V>,
    factory_repr: ::std::rc::Rc<str>,
}

impl<K: ::std::fmt::Debug, V: ::std::fmt::Debug> ::std::fmt::Debug for SmeltPyDefaultDict<K, V> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.store.borrow().fmt(formatter) } }
impl<K: Clone + Eq + ::std::hash::Hash, V: Clone + PartialEq> PartialEq for SmeltPyDefaultDict<K, V> { fn eq(&self, other: &Self) -> bool { self.equals(other) } }
//~ The placeholder a generated struct's field holds before its constructor
//~ assigns it: an empty map whose factory is the value type's default.
impl<K, V: Default + 'static> Default for SmeltPyDefaultDict<K, V> {
    fn default() -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(::indexmap::IndexMap::new())), factory: ::std::rc::Rc::new(V::default), factory_repr: ::std::rc::Rc::from("None") } }
}
impl<K, V> Clone for SmeltPyDefaultDict<K, V> {
    fn clone(&self) -> Self { Self { store: ::std::rc::Rc::clone(&self.store), factory: ::std::rc::Rc::clone(&self.factory), factory_repr: ::std::rc::Rc::clone(&self.factory_repr) } }
}

#[allow(dead_code)]
impl<K: Clone + Eq + ::std::hash::Hash, V: Clone> SmeltPyDefaultDict<K, V> {
    //~ `defaultdict(factory, pairs)`: the given entries, and `factory` for
    //~ every missing key read later.
    fn new(factory: impl Fn() -> V + 'static, factory_repr: &str, pairs: SmeltList<(K, V)>) -> Self {
        Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(pairs.to_vec().into_iter().collect())), factory: ::std::rc::Rc::new(factory), factory_repr: ::std::rc::Rc::from(factory_repr) }
    }
    //~ `d[k]`: the value, inserting the factory's value for a missing key.
    //~ The factory runs with no borrow held, so it may read the map itself.
    fn get_or_insert(&self, key: K) -> V {
        if let Some(value) = self.store.borrow().get(&key) { return value.clone(); }
        let value = (self.factory)();
        self.store.borrow_mut().entry(key).or_insert(value).clone()
    }
    //~ `d[k] = v`.
    fn set(&self, key: K, value: V) { self.store.borrow_mut().insert(key, value); }
    //~ `d.get(k)`: the value or `None`, never inserting.
    fn get(&self, key: &K) -> Option<V> { self.store.borrow().get(key).cloned() }
    //~ `d.get(k, default)`.
    fn get_or(&self, key: &K, default: V) -> V { self.store.borrow().get(key).cloned().unwrap_or(default) }
    //~ `del d[k]`: `KeyError` when missing.
    fn delete(&self, key: &K) -> Result<(), Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        self.store.borrow_mut().shift_remove(key).map(|_removed| ()).ok_or_else(|| smelt_py_key_error(key))
    }
    //~ `d.pop(k)`: `KeyError` when missing.
    fn pop(&self, key: &K) -> Result<V, Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        self.store.borrow_mut().shift_remove(key).ok_or_else(|| smelt_py_key_error(key))
    }
    //~ `d.pop(k, default)`.
    fn pop_or(&self, key: &K, default: V) -> V { self.store.borrow_mut().shift_remove(key).unwrap_or(default) }
    //~ `d.setdefault(k, v)`.
    fn setdefault(&self, key: K, value: V) -> V { self.store.borrow_mut().entry(key).or_insert(value).clone() }
    //~ `len(d)`.
    fn len(&self) -> i64 { i64::try_from(self.store.borrow().len()).unwrap_or(i64::MAX) }
    //~ `k in d`.
    fn contains(&self, key: &K) -> bool { self.store.borrow().contains_key(key) }
    //~ The keys in insertion order: `list(d)`, `d.keys()`, a `for` loop.
    fn keys(&self) -> SmeltList<K> { SmeltList::new(self.store.borrow().keys().cloned().collect()) }
    //~ `d.values()`.
    fn values(&self) -> SmeltList<V> { SmeltList::new(self.store.borrow().values().cloned().collect()) }
    //~ `d.items()`.
    fn items(&self) -> SmeltList<(K, V)> { SmeltList::new(self.store.borrow().iter().map(|(key, value)| (key.clone(), value.clone())).collect()) }
    //~ `d.clear()`.
    fn clear(&self) { self.store.borrow_mut().clear(); }
    //~ `d.copy()`: a new map with the same entries and factory.
    fn copy(&self) -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(self.store.borrow().clone())), factory: ::std::rc::Rc::clone(&self.factory), factory_repr: ::std::rc::Rc::clone(&self.factory_repr) } }
    //~ `d1 == d2`: the same entries, in any order (dict equality).
    fn equals(&self, other: &Self) -> bool where V: PartialEq {
        let left = self.store.borrow();
        let right = other.store.borrow();
        left.len() == right.len() && left.iter().all(|(key, value)| right.get(key) == Some(value))
    }
}

impl<K: SmeltPyRepr, V: SmeltPyRepr> SmeltPyRepr for SmeltPyDefaultDict<K, V> {
    fn py_repr(&self) -> String {
        let entries = self.store.borrow().iter().map(|(key, value)| format!("{}: {}", key.py_repr(), value.py_repr())).collect::<Vec<_>>().join(", ");
        format!("defaultdict({}, {{{entries}}})", self.factory_repr)
    }
}

// @smelt:item-end

// @smelt:item SmeltPyOrderedDict when py_collections & dep_indexmap
/// Python's `collections.OrderedDict`: an insertion-ordered map that can move
/// a key to either end and pop from either end.
#[allow(dead_code)]
struct SmeltPyOrderedDict<K, V> {
    store: ::std::rc::Rc<::std::cell::RefCell<::indexmap::IndexMap<K, V>>>,
}

impl<K, V> Clone for SmeltPyOrderedDict<K, V> { fn clone(&self) -> Self { Self { store: ::std::rc::Rc::clone(&self.store) } } }
impl<K: ::std::fmt::Debug, V: ::std::fmt::Debug> ::std::fmt::Debug for SmeltPyOrderedDict<K, V> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { self.store.borrow().fmt(formatter) } }
impl<K, V> Default for SmeltPyOrderedDict<K, V> { fn default() -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(::indexmap::IndexMap::new())) } } }
impl<K: Clone + Eq + ::std::hash::Hash, V: Clone + PartialEq> PartialEq for SmeltPyOrderedDict<K, V> { fn eq(&self, other: &Self) -> bool { self.equals(other) } }

#[allow(dead_code)]
impl<K: Clone + Eq + ::std::hash::Hash, V: Clone> SmeltPyOrderedDict<K, V> {
    //~ `OrderedDict(pairs)`.
    fn new(pairs: SmeltList<(K, V)>) -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(pairs.to_vec().into_iter().collect())) } }
    //~ `od[k]`: `KeyError` when missing.
    fn get_item(&self, key: &K) -> Result<V, Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        self.store.borrow().get(key).cloned().ok_or_else(|| smelt_py_key_error(key))
    }
    //~ `od[k] = v`: a new key goes to the end, an existing one keeps its place.
    fn set(&self, key: K, value: V) { self.store.borrow_mut().insert(key, value); }
    //~ `od.get(k)`.
    fn get(&self, key: &K) -> Option<V> { self.store.borrow().get(key).cloned() }
    //~ `od.get(k, default)`.
    fn get_or(&self, key: &K, default: V) -> V { self.store.borrow().get(key).cloned().unwrap_or(default) }
    //~ `del od[k]`: `KeyError` when missing.
    fn delete(&self, key: &K) -> Result<(), Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        self.store.borrow_mut().shift_remove(key).map(|_removed| ()).ok_or_else(|| smelt_py_key_error(key))
    }
    //~ `od.pop(k)`: `KeyError` when missing.
    fn pop(&self, key: &K) -> Result<V, Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        self.store.borrow_mut().shift_remove(key).ok_or_else(|| smelt_py_key_error(key))
    }
    //~ `od.pop(k, default)`.
    fn pop_or(&self, key: &K, default: V) -> V { self.store.borrow_mut().shift_remove(key).unwrap_or(default) }
    //~ `od.popitem(last)`: the last (or, for `last=False`, first) entry.
    fn popitem(&self, last: bool) -> Result<(K, V), Box<dyn ::std::error::Error>> {
        let mut store = self.store.borrow_mut();
        let entry = if last { store.pop() } else { store.shift_remove_index(0) };
        entry.ok_or_else(|| SmeltPyException::new(&["KeyError", "LookupError", "Exception", "BaseException"], vec![SmeltPyException::arg(&"dictionary is empty".to_owned())]).into_error())
    }
    //~ `od.move_to_end(k, last)`: move `k` to the end (or, for `last=False`,
    //~ the front); `KeyError` when missing.
    fn move_to_end(&self, key: &K, last: bool) -> Result<(), Box<dyn ::std::error::Error>> where K: SmeltPyRepr {
        let mut store = self.store.borrow_mut();
        let from = store.get_index_of(key).ok_or_else(|| smelt_py_key_error(key))?;
        let to = if last { store.len() - 1 } else { 0 };
        store.move_index(from, to);
        Ok(())
    }
    //~ `od.setdefault(k, v)`.
    fn setdefault(&self, key: K, value: V) -> V { self.store.borrow_mut().entry(key).or_insert(value).clone() }
    //~ `len(od)`.
    fn len(&self) -> i64 { i64::try_from(self.store.borrow().len()).unwrap_or(i64::MAX) }
    //~ `k in od`.
    fn contains(&self, key: &K) -> bool { self.store.borrow().contains_key(key) }
    //~ The keys in order.
    fn keys(&self) -> SmeltList<K> { SmeltList::new(self.store.borrow().keys().cloned().collect()) }
    //~ `od.values()`.
    fn values(&self) -> SmeltList<V> { SmeltList::new(self.store.borrow().values().cloned().collect()) }
    //~ `od.items()`.
    fn items(&self) -> SmeltList<(K, V)> { SmeltList::new(self.store.borrow().iter().map(|(key, value)| (key.clone(), value.clone())).collect()) }
    //~ `od.clear()`.
    fn clear(&self) { self.store.borrow_mut().clear(); }
    //~ `od.copy()`.
    fn copy(&self) -> Self { Self { store: ::std::rc::Rc::new(::std::cell::RefCell::new(self.store.borrow().clone())) } }
    //~ `od1 == od2`: the same entries in the same order.
    fn equals(&self, other: &Self) -> bool where V: PartialEq {
        let left = self.store.borrow();
        let right = other.store.borrow();
        left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| left == right)
    }
}

impl<K: SmeltPyRepr, V: SmeltPyRepr> SmeltPyRepr for SmeltPyOrderedDict<K, V> {
    fn py_repr(&self) -> String {
        let store = self.store.borrow();
        if store.is_empty() { return "OrderedDict()".to_owned(); }
        format!("OrderedDict({{{}}})", store.iter().map(|(key, value)| format!("{}: {}", key.py_repr(), value.py_repr())).collect::<Vec<_>>().join(", "))
    }
}

// @smelt:item-end
