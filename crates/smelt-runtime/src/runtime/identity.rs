// Reference identity: the address tables that map storage to a stable erased identity.

// `smelt_next_object_id` is emitted in the `needs_smelt_list` block above
// (which `needs_unknown` always implies), so it is in scope here.
// @smelt:item SMELT_PROMISE_IDENTITIES when unknown
thread_local! {
    static SMELT_PROMISE_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

// @smelt:item-end

// @smelt:item smelt_promise_identity when unknown
/// Return a stable erased promise id for a source future local.
fn smelt_promise_identity(source_key: usize) -> usize {
    SMELT_PROMISE_IDENTITIES.with(|identities| {
        let mut identities = identities.borrow_mut();
        if let Some(id) = identities.get(&source_key) { return *id; }
        let id = smelt_next_object_id();
        identities.insert(source_key, id);
        id
    })
}

thread_local! {
    /// Map a source list's storage address to a stable erased-array identity.
    static SMELT_LIST_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// Return a stable erased-array id for a source list keyed on its `Vec` address.
///
/// Erasing the SAME source list local twice must yield arrays that compare
/// `===` equal (arrays compare by id). Keying on the live `Vec`'s storage
/// address lets every erasure of one binding reuse a single id, while a
/// fresh list (literal or transform result) still gets a fresh id from
/// `SmeltArray::new`. KNOWN LIMITATION: `Vec::as_ptr` on an EMPTY `Vec`
/// returns a shared dangling sentinel, so distinct empty list bindings can
/// collide on one id; acceptable here because the targeted cases are
/// non-empty.
fn smelt_list_identity(source_key: usize) -> usize {
    SMELT_LIST_IDENTITIES.with(|identities| {
        let mut identities = identities.borrow_mut();
        if let Some(id) = identities.get(&source_key) { return *id; }
        let id = smelt_next_object_id();
        identities.insert(source_key, id);
        id
    })
}

// @smelt:item-end

// Host values whose state is NOT representable as a record.
//
// `Headers` and `Blob` round-trip structurally: their erased record
// carries the header pairs or the bytes, so `SmeltFromUnknown` can
// rebuild an equal value from it. The text codecs, the `node:events`
// emitter and the three `node:http` types cannot — their state is
// closures, cells and a tokio shutdown sender — so there is nothing to
// rebuild FROM.
//
// What JavaScript does there is not rebuild anything: erasing a value
// and narrowing it back yields the SAME object, so
// `const x: unknown = emitter; (x as EventEmitter).on(..)` reaches the
// same listener list. The erasure therefore stamps the live value into
// the erased object's own `origin` slot — the slot reference records
// already use (`reference_origin`) — and `SmeltFromUnknown` hands that
// value back.
//
// This used to be a thread-local map from object id to value that
// never removed an entry (there is no drop hook on an id), so every
// `Request`/`Response`/`Headers` that crossed the boundary stayed
// alive for the life of the process: six per request in the Hono
// showcase, and the values it retained held most of the request's
// graph. In the slot the value lives exactly as long as some erased
// view of it does, which is also the only time a recovery can ask.
// @smelt:item smelt_with_host_origin when host_origins & unknown
/// Stamp the live host value an erased record was made from into its origin slot.
///
/// Call this from the value's `IntoSmeltUnknown` on the record it builds,
/// so narrowing the record back hands out the SAME object.
#[allow(dead_code)]
fn smelt_with_host_origin<T: 'static>(erased: SmeltUnknown, origin: T) -> SmeltUnknown {
    if let SmeltUnknown::Object(object) = &erased { *object.store.origin_mut() = Some(Box::new(origin)); }
    erased
}

/// Recover the host value an erased record was made from.
///
/// `None` for a record that did not come from an erasure — a hand-built
/// object carrying the marker, or one that crossed a process boundary. Each
/// caller decides what that means for its own type rather than being given
/// a fabricated value here.
#[allow(dead_code)]
fn smelt_restore_host_origin<T: Clone + 'static>(value: &SmeltUnknown) -> Option<T> {
    let SmeltUnknown::Object(map) = value else { return None };
    map.smelt_origin::<T>()
}

// @smelt:item-end
