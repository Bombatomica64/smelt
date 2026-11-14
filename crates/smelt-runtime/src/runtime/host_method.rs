// Dispatch of members read off erased host values.

// The one place that decides "own member, else synthesized host
// method". A host object such as an `AbortSignal` is a marker-bearing
// record with no stored `addEventListener` field, so the member read has
// to synthesize the method -- but only when the key is genuinely absent,
// because source code (and `vi.spyOn`) may install a real member of that
// name and JavaScript's own-property lookup wins over the prototype.
// Both the member-read emitter and `smelt_vitest_spy_on` ask this, so a
// spy resolves the same member the program would have called.
// @smelt:item smelt_body_host_method when unknown
/// The synthesized host method a member read resolves to, if the object
/// carries a host marker and has no OWN member of that name.
// @smelt:if body_host_method
//~ Emits `smelt_body_host_method`: the body readers of an erased
//~ `Request`/`Response`, resolved at run time.
//~
//~ **Dynamic boundary.** A member read whose KEY is a run-time value
//~ (`raw[key]()` with `key: keyof Body`, Hono's request body cache) has no
//~ static member to dispatch to, so it reads the member off the value's
//~ erased view, and that view is a marker-bearing record with no stored
//~ `text`/`json` field. The member has to be synthesized from the marker and
//~ the key, exactly as `smelt_headers_host_method` does for `Headers`;
//~ answering `undefined` instead made every such read call nothing and
//~ resolve `null`.
//~
//~ The live value is recovered through the origin registry, so the body read
//~ is the SAME body the program holds: it is disturbed (`bodyUsed`) and a
//~ second read fails with the spec's `TypeError`, as in Node. `text` and
//~ `json` are the readers with an erased result representation; the others
//~ keep the plain read's `undefined`.
/// The `text`/`json` body readers of an erased `Request`/`Response` record.
fn smelt_body_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if !matches!(name, "text" | "json") { return None; } let body = /*@if request*/if object.contains_key("__smelt_request") { <SmeltRequest as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())).body.clone() } else /*@end*//*@if response*/if object.contains_key("__smelt_response") { <SmeltResponse as SmeltFromUnknown>::smelt_from_unknown(SmeltUnknown::Object(object.clone())).body.clone() } else /*@end*/{ return None; }; let parse_json = name == "json"; Some(SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let result = body.take_bytes().and_then(|bytes| { let text = String::from_utf8_lossy(&bytes).into_owned(); if parse_json { smelt_json_parse(&text) } else { Ok(SmeltUnknown::String(text.into())) } }); Ok(SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move { result })))) }))) }

// @smelt:end
fn smelt_host_method(object: &SmeltObject, name: &str) -> Option<SmeltUnknown> { if object.contains_key(name) { return None; } if (object.contains_key("__smelt_abortcontroller") || object.contains_key("__smelt_abortsignal")) && matches!(name, "abort" | "addEventListener" | "removeEventListener" | "dispatchEvent" | "throwIfAborted") { return Some(smelt_abort_method(object.clone(), name)); } /*@if headers*/if let Some(found) = smelt_headers_host_method(object, name) { return Some(found); } /*@end*//*@if url_search_params*/if let Some(found) = smelt_url_search_params_host_method(object, name) { return Some(found); } /*@end*//*@if form_data*/if let Some(found) = smelt_form_data_host_method(object, name) { return Some(found); } /*@end*//*@if text_encoder*/if let Some(found) = smelt_text_encoder_host_method(object, name) { return Some(found); } /*@end*//*@if text_decoder*/if let Some(found) = smelt_text_decoder_host_method(object, name) { return Some(found); } /*@end*//*@if body_host_method*/if let Some(found) = smelt_body_host_method(object, name) { return Some(found); } /*@end*/None }

//~ An erased iterator: `next()` steps `step` until it answers `None`, then
//~ stays done; `[Symbol.iterator]()` answers the iterator itself (through a
//~ weak handle, so the object is no reference cycle). `next` and the marker
//~ are hidden, as they live on the iterator prototype in JavaScript, so the
//~ iterator stringifies as `{}`. It is the erased twin of a typed collection
//~ iterator (`smelt_live_iterator`) erased through `IntoSmeltUnknown`.
/// A live, single-pass erased iterator over `step`.
fn smelt_erased_live_iterator(step: impl FnMut() -> Option<SmeltUnknown> + 'static) -> SmeltUnknown {
    let step = ::std::rc::Rc::new(::std::cell::RefCell::new(step));
    let done = ::std::rc::Rc::new(::std::cell::Cell::new(false));
    let object = SmeltObject::new(Vec::from([("__smelt_method:__smelt_generator".to_owned(), SmeltUnknown::Bool(true))]));
    object.insert("__smelt_method:next", SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| {
        let value = if done.get() { None } else { (step.borrow_mut())() };
        if value.is_none() { done.set(true); }
        Ok(SmeltUnknown::Object(SmeltObject::new(Vec::from([("value".to_owned(), value.clone().unwrap_or(SmeltUnknown::Undefined)), ("done".to_owned(), SmeltUnknown::Bool(value.is_none()))]))))
    })));
    let itself = object.downgrade();
    object.insert("__smelt_symbol_iterator", SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(itself.upgrade().map_or(SmeltUnknown::Undefined, SmeltUnknown::Object)))));
    SmeltUnknown::Object(object)
}
// @smelt:item-end
