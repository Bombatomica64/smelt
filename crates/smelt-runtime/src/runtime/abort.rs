// `AbortController` and `AbortSignal` records.

// `AbortController`/`AbortSignal` cancellation model. Both erase to
// marker-bearing `SmeltObject`s whose shared `Rc<RefCell<..>>` storage
// makes `controller.abort()` observable through any binding that read
// `controller.signal`. These helpers resolve the signal record behind a
// controller or signal, flip the shared `aborted` flag, and fire (then
// clear) registered `'abort'` listeners.
// @smelt:item smelt_abort_signal_object when unknown
/// Resolve the AbortSignal record behind an abort controller or signal object.
fn smelt_abort_signal_object(object: &SmeltObject) -> Option<SmeltObject> { if object.contains_key("__smelt_abortsignal") { return Some(object.clone()); } match object.get("signal") { Some(SmeltUnknown::Object(signal)) if signal.contains_key("__smelt_abortsignal") => Some(signal), _ => None } }
//~ The spec's two default reasons. Both are `DOMException`s, so the
//~ record carries the DOMException marker as well as the error brand:
//~ `error.name` and `error.message` read off the error fields, and
//~ `reason instanceof DOMException` answers through the marker, which is
//~ how Node reports them.
/// The `AbortError` reason `abort()` uses when the caller names none.
fn smelt_abort_default_reason() -> SmeltUnknown { SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("AbortError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String("This operation was aborted".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))) }
/// The `TimeoutError` reason `AbortSignal.timeout(ms)` aborts with.
fn smelt_abort_timeout_reason() -> SmeltUnknown { SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TimeoutError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String("The operation was aborted due to timeout".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))) }
//~ `abort(undefined)` is `abort()`: Node answers the default `AbortError`
//~ for both, so an absent argument and an explicit `undefined` are the
//~ same request. Every other value -- a string, a number, a plain object
//~ -- is carried VERBATIM, which is why the reason is an erased value and
//~ not an error type.
//~
//~ `Null` counts as absent here too, and that is a KNOWN approximation
//~ rather than the spec: Node gives `abort(null)` the reason `null`. HIR
//~ lowers `null` and `undefined` to the same `Type::None`, so the two
//~ spellings are indistinguishable by the time this helper sees them
//~ (the deferred `Type::Null` family -- see the round-9 note in
//~ `blocker-logs/standards-round9-progress.md` section 8, which records
//~ the same conflation costing the `Optional` string coercion). Given one
//~ answer for both, the default is the right one: `abort(undefined)` is a
//~ spelling real code uses and `abort(null)` is not.
/// The reason an `abort`/`AbortSignal.abort` argument list asks for.
fn smelt_abort_reason_argument(args: &[SmeltUnknown]) -> SmeltUnknown { match args.first() { None | Some(SmeltUnknown::Undefined | SmeltUnknown::Null) => smelt_abort_default_reason(), Some(reason) => reason.clone() } }
//~ Only the slots a signal always has. An un-aborted signal's `reason`
//~ is `undefined`, which is what reading an absent key answers, and the
//~ follow list is created by `smelt_abort_signal_follow` when something
//~ actually follows -- so neither is stored up front, and a program that
//~ never asks about either pays nothing for them. See the matching note
//~ in `abort_controller_constructor_expression`.
/// Build a fresh AbortSignal record, optionally already aborted.
fn smelt_abort_signal_record(reason: Option<SmeltUnknown>) -> SmeltObject { let aborted = reason.is_some(); let mut fields = Vec::from([("__smelt_abortsignal".to_owned(), SmeltUnknown::Bool(true)), ("aborted".to_owned(), SmeltUnknown::Bool(aborted)), ("__smelt_abort_listeners".to_owned(), SmeltUnknown::Array(Vec::new().into()))]); if let Some(reason) = reason { fields.push(("reason".to_owned(), reason)); } SmeltObject::new(fields) }
//~ Firing is idempotent (a second `abort()` is a no-op in the spec, and
//~ the first reason stands), sets the reason BEFORE running listeners --
//~ Node's listeners already see `signal.reason` -- and clears the list so
//~ a listener registered after the abort never runs.
//~
//~ Then it propagates to the signals FOLLOWING this one. A dependent
//~ signal is what `new Request(url, { signal })` holds: a distinct object
//~ (`request.signal !== init.signal` in Node) that aborts with the same
//~ reason when its source does. Propagation is depth-first through the
//~ same function, so a chain of dependents settles in one call, and the
//~ idempotence check above is what stops a cycle.
/// Mark an AbortSignal aborted with a reason, fire its listeners, and propagate to its dependents.
fn smelt_abort_signal_fire(signal: &SmeltObject, reason: SmeltUnknown) { if matches!(signal.get("aborted"), Some(SmeltUnknown::Bool(true))) { return; } signal.insert("aborted".to_owned(), SmeltUnknown::Bool(true)); signal.insert("reason".to_owned(), reason.clone()); let listeners = match signal.get("__smelt_abort_listeners") { Some(SmeltUnknown::Array(values)) => values.clone().into_vec(), _ => Vec::new() }; signal.insert("__smelt_abort_listeners".to_owned(), SmeltUnknown::Array(Vec::new().into())); for listener in listeners { if let SmeltUnknown::Function(callback) = listener { let event = SmeltObject::new(Vec::from([("type".to_owned(), SmeltUnknown::String("abort".into()))])); let _ = callback(vec![SmeltUnknown::Object(event)]); } } let follows = match signal.get("__smelt_abort_follows") { Some(SmeltUnknown::Array(values)) => values.clone().into_vec(), _ => Vec::new() }; for follower in follows { if let SmeltUnknown::Object(dependent) = follower { smelt_abort_signal_fire(&dependent, reason.clone()); } } }
/// Register `dependent` to abort whenever `source` does, settling it now when `source` already has.
fn smelt_abort_signal_follow(source: &SmeltObject, dependent: &SmeltObject) { if matches!(source.get("aborted"), Some(SmeltUnknown::Bool(true))) { let reason = source.get("reason").unwrap_or(SmeltUnknown::Undefined); smelt_abort_signal_fire(dependent, reason); return; } let mut follows = match source.get("__smelt_abort_follows") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; follows.push(SmeltUnknown::Object(dependent.clone())); source.insert("__smelt_abort_follows".to_owned(), SmeltUnknown::Array(follows.into())); }
//~ `throwIfAborted` throws the REASON ITSELF, not a wrapper: a string
//~ reason arrives at a `catch` as that string. `smelt_throw` is the same
//~ channel a source `throw` uses, so the two cannot be told apart.
//~ A request's signal, kept BESIDE the request rather than in it.
//~
//~ `SmeltRequest` is emitted for any crate that holds a request, and a
//~ signal is an erased record — so a `signal` field would make the
//~ request type depend on the `SmeltUnknown` carrier that a crate
//~ touching no dynamic value does not emit. Keying the signal by the
//~ request's JS reference id keeps the request type unchanged and puts
//~ this table, like the helpers it sits with, behind the carrier's own
//~ gate. It is the same id-keyed side-table shape
//~ `smelt_recorded_object_prototype` already uses.
//~
//~ Created on FIRST READ and remembered, which is what makes
//~ `request.signal === request.signal` true, and what lets a request
//~ built with no `init.signal` still answer a signal — every request has
//~ one in the spec, so the member is never null.
/// Every request's `AbortSignal`, keyed by the request's JS reference id.
thread_local! { static SMELT_REQUEST_SIGNALS: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltObject>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }
/// The request's signal, minting and remembering one on first read.
fn smelt_request_signal_record(id: usize) -> SmeltObject { SMELT_REQUEST_SIGNALS.with(|table| table.borrow_mut().entry(id).or_insert_with(|| smelt_abort_signal_record(None)).clone()) }
/// `request.signal`.
fn smelt_request_signal(id: usize) -> SmeltUnknown { SmeltUnknown::Object(smelt_request_signal_record(id)) }
//~ The spec's dependent signal: `new Request(url, { signal })` does not
//~ HOLD the given signal, it holds a new one that follows it. Node
//~ answers `false` for `request.signal === init.signal` and still aborts
//~ the request's signal when the source aborts, reason included -- and a
//~ source that is ALREADY aborted settles the dependent at construction,
//~ which `smelt_abort_signal_follow` does.
/// Make a request's signal follow the `init.signal` it was constructed with.
fn smelt_request_follow_signal(id: usize, source: SmeltUnknown) { let dependent = smelt_request_signal_record(id); if let SmeltUnknown::Object(source) = source { if let Some(source) = smelt_abort_signal_object(&source) { smelt_abort_signal_follow(&source, &dependent); } } }
/// Return an erased AbortController/AbortSignal method bound to its shared record.
fn smelt_abort_method(object: SmeltObject, method: &str) -> SmeltUnknown { let method = method.to_owned(); SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let signal = smelt_abort_signal_object(&object); match method.as_str() { "abort" => { if let Some(signal) = signal { smelt_abort_signal_fire(&signal, smelt_abort_reason_argument(&args)); } Ok(SmeltUnknown::Undefined) } "dispatchEvent" => { if let Some(signal) = signal { smelt_abort_signal_fire(&signal, smelt_abort_default_reason()); } Ok(SmeltUnknown::Bool(true)) } "throwIfAborted" => { if let Some(signal) = signal { if matches!(signal.get("aborted"), Some(SmeltUnknown::Bool(true))) { return Err(smelt_throw(signal.get("reason").unwrap_or(SmeltUnknown::Undefined))); } } Ok(SmeltUnknown::Undefined) } "addEventListener" => { if let Some(signal) = signal { let event_type = match args.first() { Some(SmeltUnknown::String(value)) => value.to_string(), _ => String::new() }; if event_type == "abort" { if let Some(listener @ SmeltUnknown::Function(_)) = args.get(1).cloned() { let mut listeners = match signal.get("__smelt_abort_listeners") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; listeners.push(listener); signal.insert("__smelt_abort_listeners".to_owned(), SmeltUnknown::Array(listeners.into())); } } } Ok(SmeltUnknown::Undefined) } "removeEventListener" => { if let Some(signal) = signal { if let Some(target @ SmeltUnknown::Function(_)) = args.get(1).cloned() { let listeners: Vec<SmeltUnknown> = match signal.get("__smelt_abort_listeners") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }.into_iter().filter(|listener| !listener.js_strict_eq(&target)).collect(); signal.insert("__smelt_abort_listeners".to_owned(), SmeltUnknown::Array(listeners.into())); } } Ok(SmeltUnknown::Undefined) } _ => Ok(SmeltUnknown::Undefined) } })) }

// @smelt:item-end
