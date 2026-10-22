//! Bounded identity side tables for erased callables and reference records.
//!
//! # What the tables answer
//!
//! An `Rc<dyn Fn(..)>` is only a code pointer and its captures, but a
//! JavaScript function is an object: it has an identity that survives being
//! wrapped (`f === f` across an erasure adapter), a `length`, own properties,
//! and — when it crossed an erased ABI — a typed callable it came from or an
//! erased callable object it was narrowed out of. The generated runtime keeps
//! that metadata beside the allocation, keyed by the `Rc`'s ADDRESS
//! ([`SmeltAddressTable`]), because both representations of one function (the
//! typed `Rc<dyn Fn(A) -> R>` and the erased `SmeltUnknown::Function`) have to
//! reach it and neither can carry a field.
//!
//! # Why the tables are bounded (the leak this module closes)
//!
//! The registries this replaces (`SMELT_CALLABLE_KEY_GUARDS`,
//! `SMELT_FUNCTION_ORIGINS`, `SMELT_FUNCTION_IDENTITIES`,
//! `SMELT_FUNCTION_LENGTHS`, `SMELT_CALLABLE_OBJECTS`,
//! `SMELT_ERASED_FUNCTION_VALUES`, `SMELT_FUNCTION_PROPERTIES`,
//! `SMELT_FUNCTION_CLASSES`, `SMELT_RECEIVER_METHODS`,
//! `SMELT_REFERENCE_OBJECT_IDENTITIES`) were nine separate maps that never
//! removed an entry: there is no drop hook on an `Rc` to remove it from. A
//! request in the Hono showcase erases ~20 callables, so a server leaked
//! ~16 KB per request and reached 10 GiB. Sweeping dead keys was tried and
//! reclaimed little, because the maps kept their own keys alive: the typed
//! origin of an erased adapter, the callable object a typed callback was
//! narrowed from, and the host values in the (separate) host-origin map all
//! held strong references back into the graph the keys belong to.
//!
//! The design here makes the life of every entry equal to the life of the
//! allocation it describes:
//!
//! * **One entry per allocation, owning its guard.** Each entry stores a
//!   `Weak` to the allocation it is keyed by. A `Weak` keeps the `RcBox`
//!   allocated after the value is dropped, so the address cannot be handed to
//!   a new allocation while the entry exists — the address-reuse guarantee
//!   that fixed remeda's flaky lazy `pipe` (a fresh `map(cb)` evaluator landed
//!   on a dead callback's address and inherited its callable-object entry).
//!   Because the guard and the metadata are ONE entry, they are removed
//!   together: once removed, a recycled address finds nothing, never a stale
//!   entry.
//! * **Dead entries are swept, amortized.** An insert that finds the table at
//!   its sweep threshold first removes every entry whose `Weak` is dead, then
//!   resets the threshold to a fixed multiple of the live count. Each entry is examined
//!   O(1) times on average, and the table stays within a constant factor of
//!   the number of LIVE allocations it describes.
//! * **Identity is a shared token, not an address.** Every allocation that
//!   denotes one JavaScript function holds the same [`SmeltFunctionIdentity`]
//!   (an `Rc`). The token's address is the identity number `===` compares,
//!   and it can only be recycled once no entry holds the token — that is,
//!   once nothing that could be compared against it is alive. The token also
//!   carries the function's `length` and own-property bag, so those die with
//!   the last representation of the function instead of being keyed by a
//!   number in a map of their own.
//! * **No entry may own a path back to its key.** A value stored in an entry
//!   that strongly reaches the entry's own allocation would keep the
//!   allocation, and so the entry, alive forever. The erasure adapters are
//!   arranged so that never happens (see `smelt_register_callable_object`'s
//!   callers in `emitter/coercion.rs`). A JavaScript-level cycle — a function
//!   whose own `prototype.constructor` is itself — is still a cycle, exactly
//!   as it is for any `Rc` graph in the generated runtime.
//!
//! # Why not carry the metadata inline on the erased value
//!
//! The alternative (status report §6.1) was an erased callable as a struct
//! carrying origin/identity/length. That removes the table for ERASED
//! callables only: typed callbacks (`Rc<dyn Fn(A) -> R>` in struct fields and
//! parameters, the shape a hand-written port uses) still need identity and
//! callable-object metadata, so a side table would remain, and every one of
//! the ~200 `SmeltUnknown::Function` construction and match sites would change
//! ABI. Tying each entry's life to its allocation bounds the memory for BOTH
//! representations through one mechanism and keeps both ABIs as they are.
//! Host values do go inline: they already had a slot to go to (the erased
//! object store's `origin`, see `reference_origin`).

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the prelude emitter"
)]

use crate::rust::CodeWriter;

/// The generic address-keyed table with per-entry allocation guards.
///
/// Emitted whenever the erased runtime is, because both the callable
/// metadata table and the reference-record identity table are built on it.
const ADDRESS_TABLE: &str = r"/// Whether the allocation an address-table key names is still alive.
trait SmeltAllocationGuard { fn is_live(&self) -> bool; }

impl<T: ?Sized> SmeltAllocationGuard for ::std::rc::Weak<T> {
    #[inline]
    fn is_live(&self) -> bool { self.strong_count() != 0 }
}

/// The size below which an address table never sweeps.
const SMELT_ADDRESS_TABLE_MIN_SWEEP: usize = 256;

/// How far past its surviving size a table grows before the next sweep.
///
/// A sweep scans every entry once and then lets the table reach this multiple
/// of what survived, so each insert pays `GROWTH / (GROWTH - 1)` liveness
/// checks on average and the table stays within `GROWTH` times its live size.
/// Four keeps the scan near one check per insert for a small, bounded table.
const SMELT_ADDRESS_TABLE_GROWTH: usize = 4;

/// One address-table entry: the metadata, and the `Weak` reserving its key.
struct SmeltAddressSlot<V, F: ?Sized> { value: V, guard: ::std::rc::Weak<F> }

/// An address-table entry seen without its allocation's type.
///
/// One boxed slot per entry keeps the map's own entries a pointer wide, so a
/// rehash moves a few words per entry whatever the metadata holds.
trait SmeltAddressEntry<V> {
    fn value(&self) -> &V;
    fn value_mut(&mut self) -> &mut V;
    fn is_live(&self) -> bool;
}

impl<V, F: ?Sized> SmeltAddressEntry<V> for SmeltAddressSlot<V, F> {
    #[inline]
    fn value(&self) -> &V { &self.value }
    #[inline]
    fn value_mut(&mut self) -> &mut V { &mut self.value }
    #[inline]
    fn is_live(&self) -> bool { self.guard.strong_count() != 0 }
}

/// Metadata keyed by the address of a live `Rc` allocation.
///
/// Every entry owns a `Weak` to the allocation it is keyed by, which keeps
/// the address reserved for exactly as long as the entry exists, so an
/// address never names two allocations. Entries whose allocation has died
/// are removed by an amortized sweep when the table reaches `sweep_at`, which
/// is then reset to `SMELT_ADDRESS_TABLE_GROWTH` times the surviving count:
/// the table stays proportional to the number of LIVE allocations it describes.
struct SmeltAddressTable<V> {
    entries: ::std::collections::HashMap<usize, Box<dyn SmeltAddressEntry<V>>>,
    sweep_at: usize,
}

impl<V: 'static> SmeltAddressTable<V> {
    fn new() -> Self { Self { entries: ::std::collections::HashMap::new(), sweep_at: SMELT_ADDRESS_TABLE_MIN_SWEEP } }
    /// Number of entries, live or not yet swept.
    #[allow(dead_code)]
    fn len(&self) -> usize { self.entries.len() }
    /// The metadata of the allocation at `key`, when it has any.
    ///
    /// `key` must come from a live handle (`Rc::as_ptr`), so a hit is always
    /// that allocation's own entry: a dead entry's address is still reserved
    /// by its guard and cannot belong to a live allocation.
    #[inline]
    fn get(&self, key: usize) -> Option<&V> { self.entries.get(&key).map(|entry| entry.value()) }
    /// The metadata of `allocation`, created with `init` on first use.
    ///
    /// The caller sweeps first when [`Self::wants_sweep`] says so (see
    /// `smelt_sweep_address_table`); this never drops an entry itself.
    fn slot<F: ?Sized + 'static>(&mut self, allocation: &::std::rc::Rc<F>, init: impl FnOnce() -> V) -> &mut V {
        let key = ::std::rc::Rc::as_ptr(allocation) as *const () as usize;
        self.entries.entry(key).or_insert_with(|| Box::new(SmeltAddressSlot { value: init(), guard: ::std::rc::Rc::downgrade(allocation) })).value_mut()
    }
    /// Whether inserting an entry for `allocation` should sweep first.
    #[inline]
    fn wants_sweep<F: ?Sized>(&self, allocation: &::std::rc::Rc<F>) -> bool {
        self.entries.len() >= self.sweep_at && !self.entries.contains_key(&(::std::rc::Rc::as_ptr(allocation) as *const () as usize))
    }
    /// Remove every entry whose allocation has died, returning them undropped.
    fn take_dead(&mut self) -> Vec<Box<dyn SmeltAddressEntry<V>>> {
        self.entries.extract_if(|_, entry| !entry.is_live()).map(|(_, entry)| entry).collect()
    }
    /// Set the next threshold from the entries that survived a full sweep.
    fn finish_sweep(&mut self) {
        if self.entries.capacity() > 2 * SMELT_ADDRESS_TABLE_GROWTH * self.entries.len().max(SMELT_ADDRESS_TABLE_MIN_SWEEP) { self.entries.shrink_to(SMELT_ADDRESS_TABLE_GROWTH * self.entries.len()); }
        self.sweep_at = (SMELT_ADDRESS_TABLE_GROWTH * self.entries.len()).max(SMELT_ADDRESS_TABLE_MIN_SWEEP);
    }
}

/// Sweep `table` until no entry dies, then reset its threshold.
///
/// Dropping a dead entry can release the last handle on allocations other
/// entries describe (a callback's entry held the erased adapter of another),
/// so one pass leaves entries that are dead the moment its garbage is dropped.
/// Setting the threshold from that inflated count let a table of such chains
/// grow without bound; sweeping again until a pass frees nothing sets it from
/// the TRUE live count. Garbage is dropped outside the table borrow, because
/// dropping it can run code that reads the table.
fn smelt_sweep_address_table<V: 'static>(table: &::std::cell::RefCell<SmeltAddressTable<V>>) {
    loop {
        let dead = table.borrow_mut().take_dead();
        if dead.is_empty() { break; }
        drop(dead);
    }
    table.borrow_mut().finish_sweep();
}";

/// The reference-record identity table (erased object id per shared cell).
const REFERENCE_IDENTITIES: &str = r"thread_local! {
    /// The erased object id of each live reference record cell.
    static SMELT_REFERENCE_OBJECT_IDENTITIES: ::std::cell::RefCell<SmeltAddressTable<usize>> = ::std::cell::RefCell::new(SmeltAddressTable::new());
}

/// Return a stable erased-object id for a reference record's shared cell.
///
/// A reference record (a class or object shape whose fields are written
/// after construction) is a handle over one `Rc<RefCell<..>>`, and every
/// handle on that cell is the SAME JavaScript object. Erasing any of them
/// must therefore produce an object that compares `===` equal to the
/// others, which means one id per cell rather than a fresh id per erasure.
/// The entry lives exactly as long as the cell (see `SmeltAddressTable`), so
/// a recycled cell address gets a fresh id rather than a dead object's.
#[allow(dead_code)]
fn smelt_reference_object_identity<T: ?Sized + 'static>(cell: &::std::rc::Rc<T>) -> usize {
    SMELT_REFERENCE_OBJECT_IDENTITIES.with(|identities| {
        if identities.borrow().wants_sweep(cell) { smelt_sweep_address_table(identities); }
        *identities.borrow_mut().slot(cell, smelt_next_object_id)
    })
}";

/// The callable metadata table and every accessor over it.
const CALLABLE_TABLE: &str = r"/// The state of one JavaScript function value, shared by every allocation
/// that denotes it (a typed callable, its erasure adapters, the typed
/// callbacks recovered from those).
#[derive(Default)]
struct SmeltFunctionIdentityState {
    /// `Function.prototype.length`, once some representation recorded it.
    length: ::std::cell::Cell<Option<f64>>,
    /// Own JavaScript properties (`f.prototype`, `f.placeholder = ..`).
    properties: ::std::cell::RefCell<Option<SmeltObject>>,
}

/// A JavaScript function identity: a shared token every linked allocation holds.
///
/// Its address is the number `===` compares (`key`). The number can only be
/// reused after the token is freed, which happens once no table entry holds
/// it — when nothing that could still be compared against it is alive.
#[derive(Clone, Default)]
struct SmeltFunctionIdentity(::std::rc::Rc<SmeltFunctionIdentityState>);

impl SmeltFunctionIdentity {
    /// The identity number two callables share when they are one JS function.
    #[inline]
    fn key(&self) -> usize { ::std::rc::Rc::as_ptr(&self.0) as usize }
}

impl PartialEq for SmeltFunctionIdentity {
    fn eq(&self, other: &Self) -> bool { ::std::rc::Rc::ptr_eq(&self.0, &other.0) }
}

/// Everything the runtime knows about one callable allocation.
#[derive(Default)]
struct SmeltCallableMeta {
    /// The JavaScript function this allocation denotes; minted on first need.
    identity: Option<SmeltFunctionIdentity>,
    /// The typed callable an erased adapter wraps, held WEAKLY (`Box<Weak<F>>`
    /// for its `Rc<F>` type; see `SmeltFunctionOrigin`).
    ///
    /// The adapter captures this callable, so while the adapter is alive (the
    /// only time it can be looked up) the upgrade succeeds. A strong handle
    /// here would outlive the adapter until the next sweep, and that stale
    /// count is what `SmeltClosureShare` reads: a class-field arrow erased
    /// through an object view looked shared when its instance's last handle
    /// went, so the instance was pinned for the rest of the program — every
    /// Hono 4 `Context`, one per request.
    wrapped: Option<Box<dyn ::std::any::Any>>,
    /// The erased value a typed callable was narrowed from (a callable object,
    /// or the full-arity function an arity-narrowing adapter forwards to).
    callable_object: Option<SmeltUnknown>,
    /// The outer erased function built for this inner `SmeltErasedFunction`
    /// callback, weak so it does not pin a transient erasure.
    erased_value: Option<::std::rc::Weak<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>>,
    /// The classes a stored constructor value constructs.
    classes: Option<Vec<&'static str>>,
    /// Whether the callable takes its receiver as the leading argument.
    receiver_method: bool,
}

thread_local! {
    /// Metadata of every callable allocation that crossed an erased ABI.
    static SMELT_CALLABLES: ::std::cell::RefCell<SmeltAddressTable<SmeltCallableMeta>> = ::std::cell::RefCell::new(SmeltAddressTable::new());
}

/// Stable key for a callable allocation (address of its `Rc`).
fn smelt_callable_object_key<F: ?Sized>(function: &::std::rc::Rc<F>) -> usize {
    ::std::rc::Rc::as_ptr(function) as *const () as usize
}

/// Update `function`'s metadata, creating its entry on first use.
///
/// `update` must not call back into the callable table.
fn smelt_update_callable<F: ?Sized + 'static, R>(function: &::std::rc::Rc<F>, update: impl FnOnce(&mut SmeltCallableMeta) -> R) -> R {
    SMELT_CALLABLES.with(|table| {
        if table.borrow().wants_sweep(function) { smelt_sweep_address_table(table); }
        update(table.borrow_mut().slot(function, SmeltCallableMeta::default))
    })
}

/// Read `function`'s metadata without creating an entry.
fn smelt_read_callable<F: ?Sized, R>(function: &::std::rc::Rc<F>, read: impl FnOnce(&SmeltCallableMeta) -> Option<R>) -> Option<R> {
    let key = smelt_callable_object_key(function);
    SMELT_CALLABLES.with(|table| table.borrow().get(key).and_then(read))
}

/// The canonical JavaScript identity of a callable, minted on first use.
///
/// Holding the result is what keeps its number from being reused, so a
/// caller that stores the number must also keep `function` alive (the
/// event emitter keeps the listener beside its identity).
fn smelt_canonical_function_identity<F: ?Sized + 'static>(function: &::std::rc::Rc<F>) -> SmeltFunctionIdentity {
    smelt_update_callable(function, |meta| meta.identity.get_or_insert_with(SmeltFunctionIdentity::default).clone())
}

/// The JavaScript function identity number of a callable allocation.
///
/// Defaults to the allocation's own address, so two callables that were never
/// linked stay distinct. An address and a token address cannot coincide: both
/// name live allocations.
fn smelt_function_identity_of(key: usize) -> usize { SMELT_CALLABLES.with(|table| table.borrow().get(key).and_then(|meta| meta.identity.as_ref().map(SmeltFunctionIdentity::key))).unwrap_or(key) }

/// Record that `derived` wraps `origin`, so both denote one JavaScript function.
fn smelt_link_function_identity<D: ?Sized + 'static, O: ?Sized + 'static>(derived: &::std::rc::Rc<D>, origin: &::std::rc::Rc<O>) { smelt_link_function_identity_key(derived, smelt_canonical_function_identity(origin)); }

/// Link `derived` to an already-resolved canonical identity.
///
/// Needed where the origin callable is moved into the wrapper being built, so
/// its identity has to be read before the move.
fn smelt_link_function_identity_key<D: ?Sized + 'static>(derived: &::std::rc::Rc<D>, canonical: SmeltFunctionIdentity) { smelt_update_callable(derived, |meta| meta.identity = Some(canonical)); }

/// Return the stable key for an erased callback wrapper.
#[allow(dead_code)]
fn smelt_erased_function_key(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> usize {
    smelt_callable_object_key(function)
}

/// A typed callable that can be recorded as an erased adapter's origin.
///
/// The record is a `Weak` (see `SmeltCallableMeta::wrapped`); typed callables
/// are `Rc<dyn Fn(..)>`, so this is implemented for `Rc` alone.
trait SmeltFunctionOrigin: Sized + 'static {
    /// The weak record kept in the callable table.
    fn smelt_origin_record(&self) -> Box<dyn ::std::any::Any>;
    /// The callable back out of a record, while it is alive and of this type.
    fn smelt_from_origin_record(record: &dyn ::std::any::Any) -> Option<Self>;
}

impl<F: ?Sized + 'static> SmeltFunctionOrigin for ::std::rc::Rc<F> {
    fn smelt_origin_record(&self) -> Box<dyn ::std::any::Any> { Box::new(::std::rc::Rc::downgrade(self)) }
    fn smelt_from_origin_record(record: &dyn ::std::any::Any) -> Option<Self> { record.downcast_ref::<::std::rc::Weak<F>>().and_then(::std::rc::Weak::upgrade) }
}

/// Retain typed callback identity while it crosses an erased ABI.
fn smelt_register_function_origin<T: SmeltFunctionOrigin>(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, origin: T) {
    let record = origin.smelt_origin_record();
    smelt_update_callable(function, |meta| meta.wrapped = Some(record));
}

/// Recover a typed callback previously passed through an erased ABI.
fn smelt_restore_function_origin<T: SmeltFunctionOrigin>(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> Option<T> {
    smelt_read_callable(function, |meta| meta.wrapped.as_deref().and_then(T::smelt_from_origin_record))
}

/// Remember the callable object a typed callback was narrowed from.
///
/// The object must not strongly reach `function` itself, or the entry would
/// keep its own key alive; the erasure adapters register a fresh forwarding
/// callable when the narrowed callback IS the object's own origin.
fn smelt_register_callable_object<F: ?Sized + 'static>(function: &::std::rc::Rc<F>, object: SmeltUnknown) {
    if let SmeltUnknown::Object(_) = &object {
        smelt_update_callable(function, |meta| meta.callable_object = Some(object));
    }
}

/// Remember the full-arity erased function a narrowing adapter forwards to.
///
/// An adapter that calls a wider callback through a signature with FEWER
/// parameters (`((ctx, next) => ..) as Next`) is still that wider JavaScript
/// function: a type assertion changes nothing at runtime, so a call reaching
/// it through an erased `Function` must pass every argument it has. The
/// adapter's `Rc<dyn Fn(..)>` cannot accept them, so it records the
/// full-arity erased function here and shares its identity; erasing the
/// adapter then answers that function. It preserves the source callable, it
/// erases nothing.
fn smelt_register_narrowed_callable<F: ?Sized + 'static>(function: &::std::rc::Rc<F>, wide: SmeltUnknown) {
    if let SmeltUnknown::Function(wide_function) = &wide {
        let identity = smelt_canonical_function_identity(wide_function);
        smelt_update_callable(function, |meta| { meta.identity = Some(identity); meta.callable_object = Some(wide); });
    }
}

/// Recover the callable object a typed callback was narrowed from.
fn smelt_lookup_callable_object<F: ?Sized>(function: &::std::rc::Rc<F>) -> Option<SmeltUnknown> {
    smelt_read_callable(function, |meta| meta.callable_object.clone())
}

/// Whether two callables denote the same JavaScript function value.
///
/// Two allocations can denote one source function once a wrapper sits between
/// them (an erasure adapter, or the typed callback recovered back out of one),
/// so a bare `Rc::ptr_eq` is not sufficient. Accepts differently-typed handles
/// because `===` can compare a typed callback against an erased one.
fn smelt_same_function_identity<L: ?Sized, R: ?Sized>(left: &::std::rc::Rc<L>, right: &::std::rc::Rc<R>) -> bool { let left_key = smelt_callable_object_key(left); let right_key = smelt_callable_object_key(right); left_key == right_key || smelt_function_identity_of(left_key) == smelt_function_identity_of(right_key) }

/// Whether two erased callables are the same JavaScript function value.
fn smelt_same_erased_function(left: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, right: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> bool { smelt_same_function_identity(left, right) }";

/// One canonical identity per (defining class, method).
///
/// In JavaScript a method lives ONCE, on the prototype, so every read of it
/// denotes the same function value: `a.m === b.m === C.prototype.m`. A
/// generated method reference has to capture its receiver, so each read
/// builds a fresh `Rc`; linking each to one token per `Class::method` key,
/// held for the life of the thread, makes them compare equal.
const METHOD_IDENTITIES: &str = r"thread_local! {
    /// Canonical identity of each class method, keyed by `Class::method`.
    static SMELT_METHOD_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<&'static str, SmeltFunctionIdentity>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// The canonical JavaScript identity of one class method.
///
/// Minted once per key and never freed, so it is stable across every read of
/// the method and distinct from every other function's identity.
fn smelt_method_identity(key: &'static str) -> SmeltFunctionIdentity {
    SMELT_METHOD_IDENTITIES.with(|identities| identities.borrow_mut().entry(key).or_default().clone())
}";

/// `Function.prototype.length` across the erasure boundary.
///
/// A typed callable knows its own arity, and `SmeltErasedFunction` carries it
/// in a `length` field; erasing it to `SmeltUnknown::Function(Rc<..>)` throws
/// the field away, so the arity is recorded on the function's identity
/// token. Real code branches on it: es-toolkit `rest(func)` defaults its split
/// point to `func.length - 1`. Recorded and read through the CANONICAL
/// identity, so a chain of erasure wrappers resolves to the arity of the
/// function the chain started from.
fn function_length_text() -> String {
    format!(
        r#"/// Record an erased callable's `Function.prototype.length`.
fn {register}<T: ?Sized + 'static>(function: &::std::rc::Rc<T>, length: f64) {{ smelt_canonical_function_identity(function).0.length.set(Some(length)); }}

/// Record an erased callable's `Function.prototype.length` if none is known.
///
/// An erasure adapter and the callable it wraps are ONE JavaScript function
/// and share one identity, so a second registration would overwrite the
/// arity already recorded; this keeps the first (the source-declared) one.
fn smelt_register_function_length_once<T: ?Sized + 'static>(function: &::std::rc::Rc<T>, length: f64) {{ let identity = smelt_canonical_function_identity(function); if identity.0.length.get().is_none() {{ identity.0.length.set(Some(length)); }} }}

/// Read `Function.prototype.length` off an erased value.
///
/// A callable object (`{{ __smelt_call }}`) reports the arity of the callable it
/// carries; anything else reports `0`, which is what JavaScript answers for a
/// value with no `length` property.
fn {read}(value: &SmeltUnknown) -> f64 {{ let function = match value {{ SmeltUnknown::Function(function) => Some(function.clone()), SmeltUnknown::Object(object) => match object.get("__smelt_call") {{ Some(SmeltUnknown::Function(function)) => Some(function), _ => None }}, _ => None }}; let Some(function) = function else {{ return 0.0; }}; smelt_read_callable(&function, |meta| meta.identity.as_ref().and_then(|identity| identity.0.length.get())).unwrap_or(0.0) }}"#,
        register = smelt_stdlib::runtime_symbols::function_length::REGISTER,
        read = smelt_stdlib::runtime_symbols::function_length::READ,
    )
}

/// Emit `text` line by line at the writer's current indentation.
fn emit_lines(writer: &mut CodeWriter, text: &str) {
    for line in text.lines() {
        writer.line(line);
    }
    writer.blank_line();
}

/// Emit the address table and the reference-record identity table built on it.
///
/// Must precede every other user of `SmeltAddressTable`; `smelt_next_object_id`
/// is emitted by the list runtime, which the erased runtime always implies.
pub(crate) fn emit_address_tables(writer: &mut CodeWriter) {
    emit_lines(writer, ADDRESS_TABLE);
    emit_lines(writer, REFERENCE_IDENTITIES);
}

/// Emit the callable metadata table, function identities and `length`.
pub(crate) fn emit_callable_registry(writer: &mut CodeWriter) {
    emit_lines(writer, CALLABLE_TABLE);
    emit_lines(writer, METHOD_IDENTITIES);
    emit_lines(writer, &function_length_text());
}
