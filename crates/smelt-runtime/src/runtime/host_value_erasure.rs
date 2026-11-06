// The erasure adapter pair for a generated host runtime type.
//
// # Why a shared emitter
//
// A modeled class backed by a generated runtime type cannot use the generic
// struct erasure. That path reads DECLARED FIELDS and stamps `__smelt_class`,
// and a prelude type has no declared fields — so it produced a record with no
// state and no host identity, which is why `x instanceof TextEncoder` on an
// erased codec could not be answered at all.
//
// `Headers` and `Blob` each hand-wrote their own adapter, and the shape they
// wrote is the same one every such type needs: the class's identity marker,
// the spec's readable data properties, and the value's own object id so two
// erasures of one value compare `===` equal. Nine types in, the shape is worth
// having once.
//
// # Identity, not reconstruction
//
// `Headers` and `Blob` round-trip STRUCTURALLY: their record carries the
// header pairs or the bytes, so a recovery can rebuild an equal value from it.
// The text codecs, the `node:events` emitter and the `node:http` types cannot
// — their state is closures, cells and a tokio shutdown sender, so there is
// nothing to rebuild from.
//
// What JavaScript does there is not rebuild anything. Erasing a value and
// narrowing it back yields the SAME object, so
//
// ```js
// const x = emitter;        // widened to unknown by the type system
// (x as EventEmitter).on("data", f);   // the same listener list
// ```
//
// must reach the emitter's own listeners. So the erasure RETAINS the live
// value in the erased object's own `origin` slot (`smelt_with_host_origin`),
// and the recovery hands that value back. The slot lives in the object's
// store, so the retained value lives exactly as long as some erased view of
// it does. Reconstruction never enters into it; the only
// question a type has to answer is what a record that did NOT come from an
// erasure should mean, which is [`Recovery`].

// Emit the `IntoSmeltUnknown` / `SmeltFromUnknown` pair for a host type.
//
// The record is built with the value's OWN `id`, so erasing one value twice
// yields two `===`-equal objects, and the marker is what makes the whole
// record non-enumerable in `for...in` and what `instanceof` resolves through.
// Both come from the shared host-object registry, so this emitter and
// `instance_of_text` cannot disagree about a class's identity.
// @smelt:item impl_IntoSmeltUnknown_for_SmeltTextEncoder when text_encoder & unknown
/// Erase a `SmeltTextEncoder` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltTextEncoder { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_textencoder".to_owned(), SmeltUnknown::Bool(true)), ("encoding".to_owned(), SmeltUnknown::String(self.encoding().into()))]))), smelt_host_origin) } }

/// Recover a `SmeltTextEncoder` from an erased value.
///
/// Lossless: the record carries everything the value is, so a record
/// that did not come from an erasure still recovers correctly.
impl SmeltFromUnknown for SmeltTextEncoder { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::new()) } }

// @smelt:item-end

// @smelt:item impl_IntoSmeltUnknown_for_SmeltTextDecoder when text_decoder & unknown
/// Erase a `SmeltTextDecoder` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltTextDecoder { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_textdecoder".to_owned(), SmeltUnknown::Bool(true)), ("encoding".to_owned(), SmeltUnknown::String(self.encoding().into()))]))), smelt_host_origin) } }

/// Recover a `SmeltTextDecoder` from an erased value.
///
/// Lossless: the record carries everything the value is, so a record
/// that did not come from an erasure still recovers correctly.
impl SmeltFromUnknown for SmeltTextDecoder { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::new()) } }

// @smelt:item-end

// @smelt:item impl_IntoSmeltUnknown_for_SmeltHash when node_hash
/// Erase a `SmeltHash` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltHash { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_hash".to_owned(), SmeltUnknown::Bool(true))]))), smelt_host_origin) } }

// @smelt:item-end

// @smelt:item impl_IntoSmeltUnknown_for_SmeltEventEmitter when event_emitter
/// Erase a `SmeltEventEmitter` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltEventEmitter { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_eventemitter".to_owned(), SmeltUnknown::Bool(true))]))), smelt_host_origin) } }

/// Recover a `SmeltEventEmitter` from an erased value.
///
/// A record with no retained origin did not come from an erasure in
/// this thread, and this type's state cannot be rebuilt from a record,
/// so it answers a fresh EMPTY value — which is a real state of the
/// type, unlike any state that could be fabricated for it.
impl SmeltFromUnknown for SmeltEventEmitter { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::new()) } }

// @smelt:item-end

// @smelt:item impl_IntoSmeltUnknown_for_SmeltIncomingMessage when http_server
/// Erase a `SmeltIncomingMessage` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltIncomingMessage { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_incomingmessage".to_owned(), SmeltUnknown::Bool(true)), ("method".to_owned(), SmeltUnknown::String(self.method().into())), ("url".to_owned(), SmeltUnknown::String(self.url().into()))]))), smelt_host_origin) } }

/// Recover a `SmeltIncomingMessage` from an erased value.
///
/// A record with no retained origin did not come from an erasure in
/// this thread, and this type's state cannot be rebuilt from a record,
/// so it answers a fresh EMPTY value — which is a real state of the
/// type, unlike any state that could be fabricated for it.
impl SmeltFromUnknown for SmeltIncomingMessage { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::from_parts(String::new(), String::new(), Vec::new(), Vec::new())) } }

/// Erase a `SmeltServerResponse` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltServerResponse { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_serverresponse".to_owned(), SmeltUnknown::Bool(true)), ("statusCode".to_owned(), SmeltUnknown::Number(self.status_code()))]))), smelt_host_origin) } }

/// Recover a `SmeltServerResponse` from an erased value.
///
/// A record with no retained origin did not come from an erasure in
/// this thread, and this type's state cannot be rebuilt from a record,
/// so it answers a fresh EMPTY value — which is a real state of the
/// type, unlike any state that could be fabricated for it.
impl SmeltFromUnknown for SmeltServerResponse { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::new()) } }

/// Erase a `SmeltHttpServer` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltHttpServer { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_httpserver".to_owned(), SmeltUnknown::Bool(true))]))), smelt_host_origin) } }

// @smelt:item-end

// @smelt:item impl_IntoSmeltUnknown_for_SmeltSqliteDatabase when sqlite
/// Erase a `SmeltSqliteDatabase` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltSqliteDatabase { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_databasesync".to_owned(), SmeltUnknown::Bool(true))]))), smelt_host_origin) } }

/// Recover a `SmeltSqliteDatabase` from an erased value.
///
/// A record with no retained origin did not come from an erasure in
/// this thread, and this type's state cannot be rebuilt from a record,
/// so it answers a fresh EMPTY value — which is a real state of the
/// type, unlike any state that could be fabricated for it.
impl SmeltFromUnknown for SmeltSqliteDatabase { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::default()) } }

/// Erase a `SmeltSqliteStatement` for a dynamic boundary.
///
/// Retains the live value in the erased record's origin slot, so narrowing the
/// erased value back hands out the SAME object rather than a copy.
impl IntoSmeltUnknown for SmeltSqliteStatement { fn into_smelt_unknown(self) -> SmeltUnknown { let smelt_id = self.id; let smelt_host_origin = self.clone(); smelt_with_host_origin(SmeltUnknown::Object(SmeltObject::with_id(smelt_id, Vec::from([("__smelt_statementsync".to_owned(), SmeltUnknown::Bool(true))]))), smelt_host_origin) } }

/// Recover a `SmeltSqliteStatement` from an erased value.
///
/// A record with no retained origin did not come from an erasure in
/// this thread, and this type's state cannot be rebuilt from a record,
/// so it answers a fresh EMPTY value — which is a real state of the
/// type, unlike any state that could be fabricated for it.
impl SmeltFromUnknown for SmeltSqliteStatement { fn smelt_from_unknown(value: SmeltUnknown) -> Self { smelt_restore_host_origin::<Self>(&value).unwrap_or_else(|| Self::default()) } }

// @smelt:item-end
