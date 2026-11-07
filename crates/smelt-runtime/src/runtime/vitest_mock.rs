// Vitest mocks, spies and asymmetric matchers.
//
// Runtime prelude for vitest *asymmetric matchers*.
//
// An asymmetric matcher — `expect.any(Number)`, `expect.arrayContaining([..])`,
// `expect.objectContaining({..})`, `expect.stringContaining("x")`,
// `expect.stringMatching(/x/)`, `expect.closeTo(n, p)`, `expect.anything()`,
// and every one of those under `expect.not` — is a **value**, not an
// assertion. Vitest types these factories `any`; what makes the value a
// matcher is that the deep equality behind `toEqual` / `toStrictEqual` /
// `toHaveBeenCalledWith` *asks* it whether the actual value matches, instead
// of comparing it structurally. So a matcher can be stored in a variable,
// nested inside an expected object or array literal, or handed over as one
// argument of `toHaveBeenCalledWith`.
//
// The frontend lowers one to a branded record — `{ __smelt_asymmetric:
// <kind>, sample: [<factory arguments>], inverted: <bool> }` — using the same
// convention as `__smelt_vitest_mock`, `__smelt_map` and `__smelt_set`: the
// value's kind travels with it, so a runtime helper recognizes it wherever it
// ended up. This module emits the two halves that give the record meaning:
//
// * [`MATCH_FN`] — the per-kind predicate.
// * [`EQUALS_FN`] — the deep-equality walk that consults the brand on **either**
//   side, at **every** level, which is what makes a nested matcher work.
//
// # Why this is not `PartialEq for SmeltUnknown`
//
// `smelt_unknown_structural_eq` backs `PartialEq for SmeltUnknown`, and
// transpiled library code observes that operator — es-toolkit's own `isEqual`
// among other things. Teaching it about `__smelt_asymmetric` would let a
// test-harness marker change what a transpiled predicate answers about an
// ordinary object. The walk here is therefore a separate function, and when
// neither operand holds a matcher anywhere it delegates to the structural one
// unchanged, so an ordinary `toEqual` compares exactly as it did before.

// Stateful Vitest `vi.fn()` mock runtime. A mock is a genuine dynamic
// boundary: the source spells no parameter or return shape at all, and
// its behavior is reconfigured imperatively at runtime through the
// chainable `mock*` methods, so the callable and its recorded calls
// live behind `SmeltUnknown` by design (see `SmeltUnknown boundaries`
// in CLAUDE.md). The mock erases to a callable object (`__smelt_call`)
// that flows through every existing erased-call path; its shared state
// is reachable from the erased object through the `__smelt_vitest_mock`
// marker field (a registry id), so matchers can read call counts and
// recorded arguments without a parallel value channel.
// @smelt:item SmeltVitestMockOutcome when vitest_mock
/// One configured outcome served by a Vitest mock invocation.
#[derive(Clone)]
enum SmeltVitestMockOutcome {
    /// `mockReturnValue(Once)`: return the value directly.
    Return(SmeltUnknown),
    /// `mockResolvedValue(Once)`: return a resolved promise of the value.
    Resolve(SmeltUnknown),
    /// `mockRejectedValue(Once)`: return a rejected promise of the value.
    Reject(SmeltUnknown),
    /// `vi.fn(impl)` / `mockImplementation(Once)`: delegate to the callback.
    Implementation(::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>),
}

/// Shared mutable state behind one `vi.fn()` mock instance.
struct SmeltVitestMockState {
    /// FIFO of one-shot outcomes (`mock*Once`), consumed before `default`.
    once: ::std::collections::VecDeque<SmeltVitestMockOutcome>,
    /// Sticky outcome served when `once` is empty; `None` yields `undefined`.
    default: Option<SmeltVitestMockOutcome>,
    /// Recorded argument vectors, one entry per invocation.
    calls: Vec<Vec<SmeltUnknown>>,
    /// Recorded return value, one entry per invocation. An async mock
    /// records the returned `SmeltUnknown::Promise`; the result matchers
    /// flatten it to its settled value at assertion time.
    results: Vec<SmeltUnknown>,
}

thread_local! {
    /// Registry mapping the `__smelt_vitest_mock` marker id stored on each
    /// erased mock object to its shared state handle.
    static SMELT_VITEST_MOCKS: ::std::cell::RefCell<::std::collections::HashMap<usize, ::std::rc::Rc<::std::cell::RefCell<SmeltVitestMockState>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());
}

/// Resolve an erased value to a plain callable (function or callable object).
fn smelt_vitest_mock_callable(value: &SmeltUnknown) -> Option<::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>> { match value { SmeltUnknown::Function(function) => Some(function.clone()), SmeltUnknown::Object(object) => match object.get("__smelt_call") { Some(SmeltUnknown::Function(function)) => Some(function), _ => None }, _ => None } }

/// Build a stateful Vitest `vi.fn([impl])` mock as an erased callable object.
///
/// Invoking the object's `__smelt_call` records the argument vector, then
/// serves the next one-shot outcome (FIFO) or the sticky default; with no
/// configuration it returns `undefined` (or delegates to `impl` when given).
/// The chainable `mock*` configuration methods are real fields on the object,
/// so they flow through the ordinary erased method-call path and each returns
/// the same mock instance for chaining, matching Vitest.
fn smelt_vitest_mock_new(implementation: Option<SmeltUnknown>) -> SmeltUnknown {
    let id = smelt_next_object_id();
    let state = ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltVitestMockState { once: ::std::collections::VecDeque::new(), default: implementation.as_ref().and_then(smelt_vitest_mock_callable).map(SmeltVitestMockOutcome::Implementation), calls: Vec::new(), results: Vec::new() }));
    SMELT_VITEST_MOCKS.with(|mocks| { mocks.borrow_mut().insert(id, state.clone()); });
    let object = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
    object.insert("__smelt_vitest_mock".to_owned(), SmeltUnknown::Number(id as f64));
    let call_state = state.clone();
    object.insert("__smelt_call".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let outcome = { let mut state = call_state.borrow_mut(); state.calls.push(args.clone()); state.once.pop_front().or_else(|| state.default.clone()) }; let result = match outcome { None => Ok(SmeltUnknown::Undefined), Some(SmeltVitestMockOutcome::Return(value)) => Ok(value), Some(SmeltVitestMockOutcome::Resolve(value)) => Ok(SmeltUnknown::Promise(SmeltPromise::resolved(value))), Some(SmeltVitestMockOutcome::Reject(value)) => Ok(SmeltUnknown::Promise(SmeltPromise::rejected(value))), Some(SmeltVitestMockOutcome::Implementation(callback)) => (callback)(args) }; if let Ok(value) = &result { call_state.borrow_mut().results.push(value.clone()); } result })));
    for (method, once) in [("mockReturnValue", false), ("mockReturnValueOnce", true), ("mockResolvedValue", false), ("mockResolvedValueOnce", true), ("mockRejectedValue", false), ("mockRejectedValueOnce", true), ("mockImplementation", false), ("mockImplementationOnce", true)] {
        let method_state = state.clone();
        let method_object = object.clone();
        object.insert(method.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let value = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); let outcome = match method { "mockReturnValue" | "mockReturnValueOnce" => Some(SmeltVitestMockOutcome::Return(value)), "mockResolvedValue" | "mockResolvedValueOnce" => Some(SmeltVitestMockOutcome::Resolve(value)), "mockRejectedValue" | "mockRejectedValueOnce" => Some(SmeltVitestMockOutcome::Reject(value)), _ => smelt_vitest_mock_callable(&value).map(SmeltVitestMockOutcome::Implementation) }; if let Some(outcome) = outcome { let mut state = method_state.borrow_mut(); if once { state.once.push_back(outcome); } else { state.default = Some(outcome); } } Ok(SmeltUnknown::Object(method_object.clone())) })));
    }
    for method in ["mockClear", "mockReset", "mockRestore"] {
        let method_state = state.clone();
        let method_object = object.clone();
        let reset = method != "mockClear";
        object.insert(method.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { { let mut state = method_state.borrow_mut(); state.calls.clear(); state.results.clear(); if reset { state.once.clear(); state.default = None; } } Ok(SmeltUnknown::Object(method_object.clone())) })));
    }
    SmeltUnknown::Object(object)
}

/// Resolve the shared mock state behind an erased value, if it is a mock.
fn smelt_vitest_mock_state(value: &SmeltUnknown) -> Option<::std::rc::Rc<::std::cell::RefCell<SmeltVitestMockState>>> { let SmeltUnknown::Object(object) = value else { return None }; match object.get("__smelt_vitest_mock") { Some(SmeltUnknown::Number(id)) => SMELT_VITEST_MOCKS.with(|mocks| mocks.borrow().get(&(id as usize)).cloned()), _ => None } }

/// `expect(mock).toHaveBeenCalledTimes(expected)`: true when the assertion
/// holds.
/// A value that is not a mock has recorded no calls, so every call
/// assertion about it is FALSE. This arm used to answer `true`, back when
/// `vi.spyOn` lowered to a plain placeholder and failing it would have
/// failed suites Smelt could not yet model; a spy is a real mock now, so a
/// vacuous pass would only hide a broken assertion.
fn smelt_vitest_mock_called_times(value: &SmeltUnknown, expected: f64) -> bool { match smelt_vitest_mock_state(value) { Some(state) => state.borrow().calls.len() as f64 == expected, None => expected == 0.0 } }

/// `expect(mock).toHaveBeenCalledWith(...)`: true when any recorded call's
/// arguments deep-equal the expected arguments (same `toEqual` structural
/// equality). When `last` is set, only the most recent recorded call is
/// compared (`toHaveBeenLastCalledWith`). A non-mock actual is false, as in
/// `toHaveBeenCalledTimes`.
/// Drop trailing `undefined`/`null` arguments. Generated Rust closures are
/// fixed-arity, so a source `callback()` that omits a declared parameter is
/// emitted as `callback(undefined)` and the mock records a trailing nullish
/// slot, whereas JavaScript records only `arguments.length`. Normalizing both
/// the recorded and expected argument vectors this way reconciles the two so
/// `toHaveBeenLastCalledWith()` matches an omitted-argument call.
fn smelt_vitest_mock_trim_trailing_nullish(args: &[SmeltUnknown]) -> &[SmeltUnknown] { let mut end = args.len(); while end > 0 && matches!(args[end - 1], SmeltUnknown::Undefined | SmeltUnknown::Null) { end -= 1; } &args[..end] }
fn smelt_vitest_mock_called_with(value: &SmeltUnknown, expected: Vec<SmeltUnknown>, last: bool) -> bool { let expected = smelt_vitest_mock_trim_trailing_nullish(&expected); let call_matches = |call: &Vec<SmeltUnknown>| { let call = smelt_vitest_mock_trim_trailing_nullish(call); call.len() == expected.len() && call.iter().zip(expected.iter()).all(|(left, right)| smelt_vitest_asymmetric_equals(left, right, &mut ::std::collections::HashSet::new())) }; match smelt_vitest_mock_state(value) { Some(state) => { let state = state.borrow(); if last { state.calls.last().is_some_and(call_matches) } else { state.calls.iter().any(call_matches) } }, None => false } }

/// `expect(mock).toHaveLastResolvedWith(...)`: true when the mock's most
/// recent recorded result deep-equals the expected value. An async mock
/// records a `SmeltUnknown::Promise`, so a promise result is flattened to
/// its settled `Ok` value before comparison (the caller has already awaited
/// it, so the shared state cell is populated). A non-mock actual is false,
/// as in the other mock matchers.
fn smelt_vitest_mock_last_resolved_with(value: &SmeltUnknown, expected: SmeltUnknown) -> bool { match smelt_vitest_mock_state(value) { Some(state) => { let last = state.borrow().results.last().cloned(); match last { Some(result) => { let resolved = match &result { SmeltUnknown::Promise(promise) => match &*promise.state.borrow() { Some(Ok(value)) => value.clone(), _ => return false }, other => other.clone() }; smelt_vitest_asymmetric_equals(&resolved, &expected, &mut ::std::collections::HashSet::new()) }, None => false } }, None => false } }
//~ Emits the whole asymmetric-matcher runtime into the generated prelude.
//~
//~ Only called from the vitest-mock region of the prelude: every helper here is
//~ written in terms of `SmeltUnknown`, `SmeltRegExp` and the own-key view, all
//~ of which a program carrying an erased test value already emits.

/// Read the brand, arguments and inversion off an asymmetric-matcher record.
fn smelt_asymmetric_marker(value: &SmeltUnknown) -> Option<(String, Vec<SmeltUnknown>, bool)> { let SmeltUnknown::Object(object) = value else { return None; }; let Some(SmeltUnknown::String(kind)) = object.get("__smelt_asymmetric") else { return None; }; let sample = match object.get("sample") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; let inverted = matches!(object.get("inverted"), Some(SmeltUnknown::Bool(true))); Some((kind.to_string(), sample, inverted)) }

/// Whether a value, or anything nested in it, is an asymmetric matcher.
///
/// The depth bound both terminates on a cyclic value and keeps the check
/// cheap: a matcher is written literally in the assertion, so it is never
/// nested deeper than the expected literal it sits in.
fn smelt_asymmetric_present(value: &SmeltUnknown, depth: usize) -> bool { if depth == 0 { return false; } if smelt_asymmetric_marker(value).is_some() { return true; } match value { SmeltUnknown::Array(values) => values.iter().any(|item| smelt_asymmetric_present(&item, depth.saturating_sub(1))), SmeltUnknown::Object(map) => map.iter().any(|(_, item)| smelt_asymmetric_present(&item, depth.saturating_sub(1))), _ => false } }

/// The constructor NAME an `expect.any(..)` argument denotes.
fn smelt_asymmetric_constructor_name(value: &SmeltUnknown) -> Option<String> { match value { SmeltUnknown::String(name) => Some(name.to_string()), SmeltUnknown::Object(map) => match map.get("name") { Some(SmeltUnknown::String(name)) => Some(name.to_string()), _ => match map.get("__smelt_class") { Some(SmeltUnknown::String(name)) => Some(name.to_string()), _ => None } }, _ => None } }

/// `expect.any(Ctor)`: whether a value is of the constructor's runtime kind.
///
/// The primitive wrappers name a tag; every other constructor is matched by
/// the value's own class identity, through the same view
/// `Object.prototype.toString` reports.
fn smelt_asymmetric_any(actual: &SmeltUnknown, constructor: Option<&SmeltUnknown>) -> bool { let Some(name) = constructor.and_then(smelt_asymmetric_constructor_name) else { return false; }; match name.as_str() { "Number" => matches!(actual, SmeltUnknown::Number(_)), "String" => matches!(actual, SmeltUnknown::String(_)), "Boolean" => matches!(actual, SmeltUnknown::Bool(_)), "Symbol" => matches!(actual, SmeltUnknown::Symbol(_)), "Function" => matches!(actual, SmeltUnknown::Function(_)), "Array" => matches!(actual, SmeltUnknown::Array(_)), "Object" => matches!(actual, SmeltUnknown::Object(_) | SmeltUnknown::Array(_)), "Promise" => matches!(actual, SmeltUnknown::Promise(_)), other => smelt_object_to_string_tag(actual) == format!("[object {other}]") || matches!(actual, SmeltUnknown::Object(map) if matches!(map.get("__smelt_class"), Some(SmeltUnknown::String(class)) if class.eq_str(other)) || matches!(map.get("__smelt_error"), Some(SmeltUnknown::String(class)) if class.eq_str(other))) } }


/// `toMatchObject`: the expected value's properties are a RECURSIVE SUBSET
/// of the actual value's, at every level; an array still has to have the
/// same length, and any other value compares by ordinary vitest equality.
fn smelt_vitest_match_object(actual: &SmeltUnknown, expected: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool { if smelt_asymmetric_marker(expected).is_some() { return smelt_vitest_asymmetric_equals(actual, expected, seen); } match (actual, expected) { (SmeltUnknown::Object(actual), SmeltUnknown::Object(expected)) => expected.iter().filter(|(key, _)| smelt_is_for_in_object_key(expected, &key.to_string_lossy())).all(|(key, want)| actual.get(&key).is_some_and(|have| smelt_vitest_match_object(&have, &want, seen))), (SmeltUnknown::Array(actual), SmeltUnknown::Array(expected)) => actual.len() == expected.len() && actual.iter().zip(expected.iter()).all(|(have, want)| smelt_vitest_match_object(&have, &want, seen)), _ => smelt_vitest_asymmetric_equals(actual, expected, seen) } }

/// Whether `actual` satisfies one asymmetric matcher.
fn smelt_asymmetric_match(actual: &SmeltUnknown, kind: &str, sample: &[SmeltUnknown], seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {
    match kind {
        "anything" => !matches!(actual, SmeltUnknown::Null | SmeltUnknown::Undefined),
        "any" => smelt_asymmetric_any(actual, sample.first()),
        "arrayContaining" => { let SmeltUnknown::Array(values) = actual else { return false; }; let items = values.clone().into_vec(); let wanted = match sample.first() { Some(SmeltUnknown::Array(expected)) => expected.clone().into_vec(), _ => Vec::new() }; wanted.iter().all(|want| items.iter().any(|have| smelt_vitest_asymmetric_equals(have, want, seen))) }
        "objectContaining" => { let SmeltUnknown::Object(map) = actual else { return false; }; let Some(SmeltUnknown::Object(expected)) = sample.first() else { return false; }; expected.iter().all(|(key, want)| map.get(&key).is_some_and(|have| smelt_vitest_asymmetric_equals(&have, &want, seen))) }
        "stringContaining" => { let (SmeltUnknown::String(text), Some(SmeltUnknown::String(part))) = (actual, sample.first()) else { return false; }; text.contains(&**part) }
        "stringMatching" => { let SmeltUnknown::String(text) = actual else { return false; }; match sample.first() { Some(SmeltUnknown::String(pattern)) => SmeltRegExp::new(pattern.to_string(), String::new()).test(&text), Some(SmeltUnknown::Object(map)) if map.contains_key("__smelt_regexp") => { let source = match map.get("source") { Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }; let flags = match map.get("flags") { Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }; SmeltRegExp::new(source, flags).test(&text) } _ => false } }
        "closeTo" => { let (SmeltUnknown::Number(value), Some(SmeltUnknown::Number(target))) = (actual, sample.first()) else { return false; }; let precision = match sample.get(1) { Some(SmeltUnknown::Number(precision)) => *precision, _ => 2.0 }; (value - target).abs() < 10f64.powf(-precision) / 2.0 }
        "matchObject" => { let Some(expected) = sample.first() else { return false; }; smelt_vitest_match_object(actual, expected, seen) }
        _ => false,
    }
}

/// Vitest deep equality, consulting an asymmetric matcher on either side.
///
/// With no matcher anywhere in either operand this is exactly
/// `smelt_unknown_structural_eq`, which backs `PartialEq for SmeltUnknown`
/// and must stay blind to the harness's markers. With one, the walk recurses
/// itself, so a matcher NESTED in an expected array or object is asked about
/// the corresponding part of the actual value.
fn smelt_vitest_asymmetric_equals(left: &SmeltUnknown, right: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {
    if let Some((kind, sample, inverted)) = smelt_asymmetric_marker(right) { return smelt_asymmetric_match(left, &kind, &sample, seen) != inverted; }
    if let Some((kind, sample, inverted)) = smelt_asymmetric_marker(left) { return smelt_asymmetric_match(right, &kind, &sample, seen) != inverted; }
    if !smelt_asymmetric_present(left, 32) && !smelt_asymmetric_present(right, 32) { return smelt_unknown_structural_eq(left, right, seen); }
    match (left, right) {
        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| smelt_vitest_asymmetric_equals(&left, &right, seen)),
        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => { let left_keys = smelt_own_object_keys(left).len(); let right_keys = smelt_own_object_keys(right).len(); left_keys == right_keys && left.iter().filter(|(key, _)| smelt_is_for_in_object_key(left, &key.to_string_lossy())).all(|(key, value)| right.get(&key).is_some_and(|other| smelt_vitest_asymmetric_equals(&value, &other, seen))) }
        _ => smelt_unknown_structural_eq(left, right, seen),
    }
}

//~ `vi.spyOn(target, name)` is a real boundary adapter, not a
//~ placeholder: it resolves the member's CURRENT value through the
//~ shared `smelt_host_method` decision the member-read emitter uses,
//~ wraps it in a mock whose default outcome FORWARDS to it, and writes
//~ the mock back onto the target. Library code that later reads the
//~ member therefore calls the mock and the mock calls the original, so
//~ the recorded calls are the ones the program actually made and the
//~ original side effect still happens.
thread_local! {
    /// Installed spies, newest last: the target object, the member name,
    /// and the member's value before the spy replaced it (`None` when the
    /// member had no own value, so restoring removes the key again).
    static SMELT_VITEST_SPIES: ::std::cell::RefCell<Vec<(SmeltObject, String, Option<SmeltUnknown>)>> = const { ::std::cell::RefCell::new(Vec::new()) };
}

/// Restore one recorded spy, putting the target member back as it was.
fn smelt_vitest_restore_spy(target: &SmeltObject, name: &str, original: Option<SmeltUnknown>) { match original { Some(value) => { target.insert(name.to_owned(), value); } None => { target.remove(name); } } }

/// `vi.restoreAllMocks()`: undo every installed spy, newest first.
fn smelt_vitest_restore_all_mocks() { let spies = SMELT_VITEST_SPIES.with(|spies| ::std::mem::take(&mut *spies.borrow_mut())); for (target, name, original) in spies.into_iter().rev() { smelt_vitest_restore_spy(&target, &name, original); } }

/// `vi.spyOn(target, name)`: install a recording mock over `target[name]`.
///
/// The mock forwards to the member's current value, so the original
/// behaviour still runs; its `mockRestore` puts the member back. A target
/// that is not an object has no member to replace, so it yields a bare
/// recording mock rather than failing the program.
fn smelt_vitest_spy_on(target: &SmeltUnknown, name: &(impl SmeltHostText + ?Sized)) -> SmeltUnknown {
    let name = name.host_text(); let name: &str = &name;
    let SmeltUnknown::Object(object) = target else { return smelt_vitest_mock_new(None); };
    let own = object.get(name);
    let original = smelt_host_method(object, name).or_else(|| own.clone());
    let mock = smelt_vitest_mock_new(original);
    object.insert(name.to_owned(), mock.clone());
    SMELT_VITEST_SPIES.with(|spies| spies.borrow_mut().push((object.clone(), name.to_owned(), own.clone())));
    // `mockRestore` both clears the recorded activity (the shared
    // implementation already installed by `smelt_vitest_mock_new`) and puts
    // the target member back, which a bare `vi.fn()` mock has no target to
    // do. Replacing the field keeps that difference in ONE place.
    if let SmeltUnknown::Object(handle) = &mock {
        let clear = handle.get("mockRestore");
        let restore_target = object.clone();
        let restore_name = name.to_owned();
        let restore_original = own;
        let handle_value = mock.clone();
        handle.insert("mockRestore".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { if let Some(SmeltUnknown::Function(clear)) = &clear { let _ = clear(args); } smelt_vitest_restore_spy(&restore_target, &restore_name, restore_original.clone()); Ok(handle_value.clone()) })));
    }
    mock
}
// @smelt:item-end
