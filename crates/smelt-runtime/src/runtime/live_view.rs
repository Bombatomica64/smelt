// Live views: one typed record seen through its map-shaped handles.
//
// A generated record is one JavaScript object held by one cell
// (`Rc<RefCell<Inner>>`). A slot of another type that receives it -- `any`,
// `unknown`, an erased type parameter, `Record<string, V>` -- cannot hold that
// cell, so it holds a `SmeltObject`/`SmeltRecord` instead. That handle used to
// be a snapshot of the fields: a write through it never reached the record, a
// later write to the record never reached it, and a property added through it
// was lost with it.
//
// Here the handle is a VIEW. Its store carries a `SmeltLiveLink` to the record
// (`field_store.rs`), and:
//
// * reading or writing one data key goes to the record's field;
// * a whole-store read (keys, iteration, equality, JSON) first refreshes the
//   store from the record;
// * what a typed field cannot hold -- a property the record type does not
//   declare, a declared property that was deleted, an own-key order other than
//   the declaration's -- is kept beside the record's cell in
//   `SMELT_OPEN_SHAPES`, so it belongs to the object and not to any one view.
//
// The record stays the single source of truth and keeps its concrete type;
// nothing typed is routed through the erased carrier.

// @smelt:item SmeltLiveObject when live_views
/// A typed record as its map-shaped views read and write it.
//~
//~ Generated per reference record. Keys are JavaScript property names and
//~ values the erased carrier, because a view may be of any map type; the
//~ record converts at its own fields.
pub trait SmeltLiveObject {
    //~ The source spelling of every property the record type declares.
    fn smelt_field_names(&self) -> &'static [&'static str];
    //~ The current value of the declared property `key`, or `None` when the
    //~ record type declares no such property or an optional one is absent.
    fn smelt_field_get(&self, key: &JsStr) -> Option<SmeltUnknown>;
    //~ Assign the declared property `key`. The value is handed back when the
    //~ record type does not declare `key`, and when it declares it at a type
    //~ this value is not of (JavaScript lets `p.name = {..}` through an `any`
    //~ view of a record whose `name` is a string; a typed field cannot hold
    //~ that, and coercing it would invent a value nobody wrote).
    fn smelt_field_set(&self, key: &JsStr, value: SmeltUnknown) -> Result<(), SmeltUnknown>;
    //~ Every declared property that is present, in declaration order.
    fn smelt_field_entries(&self) -> Vec<(&'static str, SmeltUnknown)>;
    //~ The record's cell: the allocation its identity and open shape hang on.
    fn smelt_live_cell(&self) -> ::std::rc::Rc<dyn ::std::any::Any>;
    //~ The address of that cell, which is what the open-shape table is keyed by.
    fn smelt_live_address(&self) -> usize;
    //~ The typed handle itself, so a view narrowed back to the record type
    //~ hands out this record rather than a copy.
    fn smelt_live_handle(&self) -> &dyn ::std::any::Any;
}

/// Where a live record's `smelt_field_get` leaves the one value it reads.
//~
//~ The statements that erase a record's fields push `(key, value)` entries
//~ (`record_view_field_statements` in the code generator); a single-key read
//~ runs one of them into this slot instead of a vector.
pub struct SmeltLiveSlot(Option<SmeltUnknown>);
impl SmeltLiveSlot {
    #[inline]
    fn push(&mut self, entry: (&'static str, SmeltUnknown)) { self.0 = Some(entry.1); }
}

/// What one record's views changed that its typed fields cannot hold.
//~
//~ `order` is the object's own-key order from the moment it stopped being the
//~ declaration's; a declared key missing from it (set again after the shape
//~ was made, or an optional one filled in later) follows the listed ones.
//~ `extras` holds the properties the record type does not declare, and the
//~ declared ones a view overwrote with a value of another type: such a value
//~ shadows the field for every view, until a view writes a value the field
//~ can hold.
struct SmeltOpenShape {
    order: Vec<JsString>,
    extras: Vec<(JsString, SmeltUnknown)>,
    deleted: Vec<JsString>,
}

thread_local! {
    //~ The JavaScript string of each declared property name, by the address
    //~ AND length of its literal (literals share storage: `"p"` may be the
    //~ tail of `"tag-p"`, so neither alone names one). A view refresh names
    //~ every field of its record, and a `JsString` built per field per
    //~ refresh was most of its cost.
    static SMELT_LIVE_KEYS: ::std::cell::RefCell<::std::collections::HashMap<(usize, usize), JsString>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// The JavaScript string of the declared property name `name`.
fn smelt_live_key(name: &'static str) -> JsString {
    SMELT_LIVE_KEYS.with(|keys| keys.borrow_mut().entry((name.as_ptr() as usize, name.len())).or_insert_with(|| JsString::from(name)).clone())
}

thread_local! {
    //~ The open shape of each record cell that has one. An entry lives as
    //~ long as its cell (see `SmeltAddressTable`).
    static SMELT_OPEN_SHAPES: ::std::cell::RefCell<SmeltAddressTable<SmeltOpenShape>> = ::std::cell::RefCell::new(SmeltAddressTable::new());
}

//~ Read the open shape of the cell at `address`, if it has one. The common
//~ record has none, and an empty table answers without hashing.
#[inline]
fn smelt_open_shape_read<R>(address: usize, read: impl FnOnce(&SmeltOpenShape) -> R) -> Option<R> {
    SMELT_OPEN_SHAPES.with(|shapes| {
        let shapes = shapes.borrow();
        if shapes.len() == 0 { return None; }
        shapes.get(address).map(read)
    })
}

/// Whether the record cell `cell` has an open shape: its views added or
/// deleted a property, or its own-key order is not the declaration's. A typed
/// whole-object observer (`console.log` of the record) asks before trusting
/// the field list.
#[allow(dead_code)]
fn smelt_live_is_open<T: ?Sized>(cell: &::std::rc::Rc<T>) -> bool {
    SMELT_OPEN_SHAPES.with(|shapes| {
        let shapes = shapes.borrow();
        shapes.len() != 0 && shapes.get(::std::rc::Rc::as_ptr(cell).cast::<()>() as usize).is_some()
    })
}

/// Deep equality of two records of one type: what `toEqual` and a
/// structural `==` ask of a record held inside a typed list, dictionary,
/// tuple or optional.
//~
//~ A record handle's own `PartialEq` is identity (`===`). Deep equality
//~ compares own properties instead, and a record's own properties are what
//~ its views enumerate: the declared fields that are present plus whatever a
//~ view added, less what one deleted. So the comparison is the views', which
//~ also follows a cycle once. One object equals itself without being read.
#[allow(dead_code)]
fn smelt_live_deep_eq<T: SmeltLiveObject + IntoSmeltUnknown>(left: T, right: T) -> bool {
    left.smelt_live_address() == right.smelt_live_address() || left.into_smelt_unknown() == right.into_smelt_unknown()
}

//~ Change the open shape of `object`'s cell, creating it on first use with
//~ the record's present declared keys as the order so far.
fn smelt_open_shape_write<R>(object: &dyn SmeltLiveObject, write: impl FnOnce(&mut SmeltOpenShape) -> R) -> R {
    let cell = object.smelt_live_cell();
    let exists = smelt_open_shape_read(object.smelt_live_address(), |_| ()).is_some();
    let order = if exists { Vec::new() } else { object.smelt_field_entries().into_iter().map(|(key, _)| smelt_live_key(key)).collect() };
    SMELT_OPEN_SHAPES.with(|shapes| {
        if shapes.borrow().wants_sweep(&cell) { smelt_sweep_address_table(shapes); }
        write(shapes.borrow_mut().slot(&cell, || SmeltOpenShape { order, extras: Vec::new(), deleted: Vec::new() }))
    })
}

/// The value of the data key `key` of the record behind a view, if it has one.
fn smelt_live_get(object: &::std::rc::Rc<dyn SmeltLiveObject>, key: &JsStr) -> Option<SmeltUnknown> {
    let open = smelt_open_shape_read(object.smelt_live_address(), |shape| {
        if let Some((_, value)) = shape.extras.iter().find(|(name, _)| **name == *key) { return Some(Some(value.clone())); }
        if shape.deleted.iter().any(|name| **name == *key) { return Some(None); }
        None
    });
    match open { Some(Some(found)) => found, _ => object.smelt_field_get(key) }
}

/// Write the data key `key` of the record behind a view: into its field when
/// the record type declares it, else as an undeclared property of the object.
fn smelt_live_set(object: &::std::rc::Rc<dyn SmeltLiveObject>, key: &JsStr, value: SmeltUnknown) {
    match object.smelt_field_set(key, value) {
        Ok(()) => {
            //~ A deleted property written again is a NEW property: last in key
            //~ order. A field that holds its value again is no longer shadowed.
            let (revived, shadowed) = smelt_open_shape_read(object.smelt_live_address(), |shape| (shape.deleted.iter().any(|name| **name == *key), shape.extras.iter().any(|(name, _)| **name == *key))).unwrap_or((false, false));
            if revived || shadowed {
                let shadow = smelt_open_shape_write(&**object, |shape| {
                    if revived { shape.deleted.retain(|name| **name != *key); shape.order.retain(|name| **name != *key); shape.order.push(key.to_owned()); }
                    shape.extras.iter().position(|(name, _)| **name == *key).map(|position| shape.extras.remove(position))
                });
                drop(shadow);
            }
        }
        Err(value) => {
            //~ The value this replaces is dropped after the table is released:
            //~ dropping it can run code that reads the table.
            let replaced = smelt_open_shape_write(&**object, |shape| {
                if let Some(slot) = shape.extras.iter_mut().find(|(name, _)| **name == *key) { return Some(::std::mem::replace(&mut slot.1, value)); }
                shape.extras.push((key.to_owned(), value));
                //~ A declared key keeps its place; a deleted one comes back last.
                shape.deleted.retain(|name| **name != *key);
                if !shape.order.iter().any(|name| **name == *key) { shape.order.push(key.to_owned()); }
                None
            });
            drop(replaced);
        }
    }
}

/// Delete the data key `key` of the record behind a view.
//~
//~ A typed field cannot be absent, so a deleted declared property keeps its
//~ last value in the record and is hidden from every view instead.
fn smelt_live_remove(object: &::std::rc::Rc<dyn SmeltLiveObject>, key: &JsStr) {
    if smelt_live_get(object, key).is_none() { return; }
    let declared = object.smelt_field_names().iter().any(|name| *key == **name);
    //~ As in `smelt_live_set`, the removed value outlives the table borrow.
    let removed = smelt_open_shape_write(&**object, |shape| {
        let removed = shape.extras.iter().position(|(name, _)| **name == *key).map(|position| shape.extras.remove(position));
        shape.order.retain(|name| **name != *key);
        if declared && !shape.deleted.iter().any(|name| **name == *key) { shape.deleted.push(key.to_owned()); }
        removed
    });
    drop(removed);
}

/// Every data property of the record behind a view, in the object's own-key order.
fn smelt_live_entries(object: &::std::rc::Rc<dyn SmeltLiveObject>) -> Vec<(JsString, SmeltUnknown)> {
    let fields = object.smelt_field_entries().into_iter().map(|(key, value)| (smelt_live_key(key), value)).collect::<Vec<_>>();
    let Some((order, extras, deleted)) = smelt_open_shape_read(object.smelt_live_address(), |shape| (shape.order.clone(), shape.extras.clone(), shape.deleted.clone())) else { return fields };
    //~ A field shadowed by a value of another type is listed as that value.
    let mut fields = fields.into_iter().filter(|(key, _)| !deleted.contains(key) && !extras.iter().any(|(name, _)| name == key)).map(Some).collect::<Vec<_>>();
    let mut extras = extras.into_iter().map(Some).collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(fields.len() + extras.len());
    for key in &order {
        if let Some(entry) = extras.iter_mut().chain(fields.iter_mut()).find(|slot| slot.as_ref().is_some_and(|(name, _)| name == key)).and_then(Option::take) { entries.push(entry); }
    }
    entries.extend(fields.into_iter().flatten());
    entries
}

/// Give `record`, just built from the erased object `id` with `entries`, that
/// object's identity, own-key order and undeclared properties.
fn smelt_live_adopt(record: &dyn SmeltLiveObject, id: usize, entries: Vec<(JsString, SmeltUnknown)>) {
    let cell = record.smelt_live_cell();
    SMELT_REFERENCE_OBJECT_IDENTITIES.with(|identities| {
        if identities.borrow().wants_sweep(&cell) { smelt_sweep_address_table(identities); }
        *identities.borrow_mut().slot(&cell, || id) = id;
    });
    let names = record.smelt_field_names();
    let present = record.smelt_field_entries().into_iter().map(|(key, _)| smelt_live_key(key)).collect::<Vec<_>>();
    let order = entries.iter().map(|(key, _)| key.clone()).filter(|key| present.contains(key) || !names.iter().any(|name| *key == **name)).collect::<Vec<_>>();
    let extras = entries.into_iter().filter(|(key, _)| !names.iter().any(|name| *key == **name)).collect::<Vec<_>>();
    if extras.is_empty() && order == present { return; }
    smelt_open_shape_write(record, |shape| { shape.order = order; shape.extras = extras; });
}

thread_local! {
    //~ The conversions of an erased view: its values ARE the record's vocabulary.
    static SMELT_LIVE_ERASED: (::std::rc::Rc<dyn Fn(SmeltUnknown) -> SmeltUnknown>, ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>) = (::std::rc::Rc::new(|value| value), ::std::rc::Rc::new(SmeltUnknown::clone));
}

impl SmeltLiveLink<JsString, SmeltUnknown> {
    //~ The link of an erased view of `object`.
    fn erased(object: ::std::rc::Rc<dyn SmeltLiveObject>) -> Self {
        let (wrap, unwrap) = SMELT_LIVE_ERASED.with(|(wrap, unwrap)| (::std::rc::Rc::clone(wrap), ::std::rc::Rc::clone(unwrap)));
        Self { object, key: |name| name, wrap, unwrap, same: |left, right| left.js_strict_eq(right), apply: |store, entries, same| store.resync(entries, same) }
    }
}

impl SmeltObject {
    //~ A typed dictionary view of the record this erased object is a view of:
    //~ `Record<string, V>` over a record reads and writes the record's own
    //~ properties, converted by `wrap`/`unwrap` at each access. `None` when
    //~ this object is not a view of a typed record (it is then its own store,
    //~ and the caller builds the dictionary from its entries as before).
    #[allow(dead_code)]
    fn smelt_typed_view<V: 'static>(&self, wrap: impl Fn(SmeltUnknown) -> V + 'static, unwrap: impl Fn(&V) -> SmeltUnknown + 'static) -> Option<SmeltRecord<JsString, V>> {
        let object = self.store.live.borrow().as_ref().map(|link| ::std::rc::Rc::clone(&link.object))?;
        let record = SmeltRecord { id: self.id, store: ::std::rc::Rc::new(SmeltStoreCell::new(SmeltFieldStore::default())) };
        *record.store.live.borrow_mut() = Some(SmeltLiveLink { object, key: |name| name, wrap: ::std::rc::Rc::new(wrap), unwrap: ::std::rc::Rc::new(unwrap), same: |_, _| false, apply: |store, entries, same| store.resync(entries, same) });
        Some(record)
    }
}

// @smelt:item-end
