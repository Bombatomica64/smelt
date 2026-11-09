// Exception-payload ABI for Smelt's generated Rust.
//
// Every fallible or `async` Smelt function models its error channel as
// `Result<T, Box<dyn std::error::Error>>`. A JavaScript `throw`, however,
// carries an arbitrary *value*, not a message: `throw new TypeError(m)`,
// `throw 'a string'`, `throw {code: 42}` and `throw someErrorInstance` are all
// legal and are all observable in the corresponding `catch`.
//
// Before this module, the two `Terminator::Throw` emit sites and the
// `new Promise(reject)` bridge collapsed the thrown value with
// `format!("{}", value)` into a `std::io::Error`. Because an erased JavaScript
// `Error` is a marker-bearing `SmeltUnknown::Object`, `Display` rendered it as
// the literal text `[object Object]`, so the payload's class, `name`,
// `message`, `cause` and any custom fields were destroyed at the throw site and
// could never be recovered by a `catch`.
//
// This module emits a payload-carrying error type, `SmeltThrown`, and the two
// adapters that bracket the boundary: [`THROW_FN`] to enter it and
// [`THROWN_VALUE_FN`] to leave it. The representation is a general rule for
// *every* `throw`, not a per-library special case.
//
// # Why the payload is a `SmeltUnknown`
//
// This is a genuine dynamic boundary in the sense of `CLAUDE.md`, not an
// erasure of convenience:
//
// * `throw` is not typed. TypeScript deliberately gives a `catch` binding the
//   static type `any` (or `unknown` under `useUnknownInCatchVariables`); the
//   language has no throws-clause, so the set of values that can arrive at a
//   given `catch` is not statically known.
// * The error channel is *one* Rust type shared by every fallible function in
//   the crate. A concrete payload type, a generated union, or a scoped generic
//   would all have to be chosen per `catch` site, but a `catch` may receive a
//   value thrown by any transitively called function — including through an
//   erased `SmeltUnknown::Function` callback, whose signature is already
//   `Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn Error>>>` and
//   therefore cannot mention a caller-specific error type.
// * The channel must also stay open to *foreign* errors (`std::io::Error` and
//   anything else that reaches it via `?`), which is why the boundary is a
//   downcast rather than an enum discriminant.
//
// `crates/smelt-codegen-rust/src/tests/thrown_tests.rs` carries the regression
// test that pins this reasoning: it throws two structurally unrelated payloads
// (an `Error` object and a bare string) through one function's error channel and
// asserts both survive, which no single concrete type, generated union arm, or
// function-scoped type parameter could express.

// Emits the panic-route support items into the generated prelude.
//
// # Why the panic channel exists at all
//
// A generated function whose body cannot propagate an error — because its own
// type says `may_throw: false`, which is the case for every closure coerced to
// a declared non-throwing callback parameter type — still has to report a
// `throw`. It does so by panicking, and an enclosing `try` catches it with
// `std::panic::catch_unwind`. That route is why the generated `Cargo.toml` must
// never set `panic = "abort"`.
//
// # Why the panic payload is a class plus a message
//
// `std::panic::panic_any` requires `Any + Send`, and a `SmeltUnknown` holds
// `Rc` handles, so the thrown value cannot ride the unwind itself.
// `SmeltPanic` carries the two parts that *are* `Send`: the error class brand
// and the message. Before this existed the route panicked with
// `format!("{}", error)`, so every panic-routed throw arrived at its `catch` as
// a bare `Error` — `error.name` was wrong for `URIError`, `TypeError`, and
// every user error class.
//
// # And why the VALUE still arrives
//
// The value travels beside the panic rather than inside it, in a thread-local
// slot (`SMELT_PANIC_VALUE`): a routed throw parks it, and the `catch_unwind`
// that receives the panic takes it. That is sound because the two are the same
// thread by construction — the `catch_unwind` is emitted around the call in the
// same generated function — so `Send` is a static bound on the panic payload
// and not a claim about where this value goes.
//
// This is what makes a panic-routed `throw "text"` arrive at its `catch` as
// that string rather than as an `Error` whose message happens to be it, and
// what lets a thrown class instance keep its own fields. `AbortSignal`'s
// `throwIfAborted()` is the shape that needed it: the spec throws the REASON,
// which is any JavaScript value, and a string reason is the commonest kind.
//
// # The hook
//
// Routing control flow through panics otherwise prints a panic line and the
// backtrace note on ordinary caught input — per request, for a router decoding
// untrusted path segments. The installed hook suppresses output *only* for a
// `SmeltPanic` payload and delegates everything else to the previous hook, so a
// genuine panic stays as loud as it was.
//
// `needs_unknown` selects the panic-payload builder's body: the structured form
// projects the thrown payload's class, and is emitted by
// the thrown-payload items because it names `SmeltUnknown`. A crate with
// no erased values has no structured payloads either — its throw sites carry
// plain message strings — so the class is `Error` there by construction.
//
// No leading blank line: the caller has just written the crate attribute
// block, which already ends with one.
// @smelt:item SmeltPanic when panic_route
/// A Smelt `throw` crossing an unwind boundary, keeping its class.
#[derive(Debug)]
struct SmeltPanic { class: String, message: String }
impl ::std::fmt::Display for SmeltPanic { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { write!(formatter, "{}: {}", self.class, self.message) } }
static SMELT_PANIC_HOOK: ::std::sync::Once = ::std::sync::Once::new();
thread_local! { static SMELT_PANIC_CATCH_DEPTH: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) }; }
/// Arm a Smelt `catch` around the call about to unwind into it.
#[allow(dead_code)]
fn smelt_panic_catch_enter() { SMELT_PANIC_CATCH_DEPTH.with(|depth| depth.set(depth.get() + 1)); }
/// Disarm the `catch` armed by [`smelt_panic_catch_enter`].
#[allow(dead_code)]
fn smelt_panic_catch_leave() { SMELT_PANIC_CATCH_DEPTH.with(|depth| depth.set(depth.get() - 1)); }
/// Report a Smelt throw only when no `catch` will receive it: an uncaught throw
/// must say what was thrown, as Node does, instead of exiting silently.
fn smelt_install_panic_hook() { SMELT_PANIC_HOOK.call_once(|| { let previous = ::std::panic::take_hook(); ::std::panic::set_hook(Box::new(move |info| { if let Some(thrown) = info.payload().downcast_ref::<SmeltPanic>() { if SMELT_PANIC_CATCH_DEPTH.with(::std::cell::Cell::get) == 0 { eprintln!("Uncaught {thrown}"); } return; } previous(info); })); }); }
/// Report a Smelt error through the panic channel, keeping its identity.
fn smelt_panic_throw(error: Box<dyn ::std::error::Error>) -> ! { smelt_install_panic_hook(); ::std::panic::panic_any(smelt_panic_payload(&*error)) }
/// Recover the message text a `catch` observes from a caught panic.
fn smelt_panic_message(panic: &(dyn ::std::any::Any + Send)) -> String { if let Some(payload) = panic.downcast_ref::<SmeltPanic>() { return payload.message.clone(); } if let Some(message) = panic.downcast_ref::<String>() { return message.clone(); } if let Some(message) = panic.downcast_ref::<&'static str>() { return (*message).to_owned(); } "JavaScript exception".to_owned() }
/// Recover the error class a `catch` observes from a caught panic.
fn smelt_panic_class(panic: &(dyn ::std::any::Any + Send)) -> String { panic.downcast_ref::<SmeltPanic>().map_or_else(|| "Error".to_owned(), |payload| payload.class.clone()) }
// @smelt:if !unknown
//= /// Build the unwind payload for a crate with no erased values.
//= fn smelt_panic_payload(error: &(dyn ::std::error::Error + 'static)) -> SmeltPanic { SmeltPanic { class: "Error".to_owned(), message: error.to_string() } }
// @smelt:end
// @smelt:item-end

// @smelt:item SmeltThrown when unknown
/// A JavaScript `throw` payload travelling Smelt's `Box<dyn Error>` channel.
//~
//~ DYNAMIC BOUNDARY: `throw` accepts any JavaScript value and a `catch`
//~ binding has no static type, so the single error channel shared by every
//~ generated fallible function cannot be a concrete type, a generated union,
//~ or a scoped generic. The payload is kept whole here and recovered by
//~ `smelt_thrown_value`; foreign errors reaching the same channel fall back
//~ to their `Display` text.
#[derive(Debug)]
struct SmeltThrown { value: SmeltUnknown }
//~ `Display` projects the payload's `message` field when present. Erased
//~ `Error` values carry `message`, and it is what the runtime's own promise
//~ rejection bridge already surfaced, so keeping the same projection means
//~ string-typed `catch` bindings and `.toThrow("text")`-style substring
//~ checks observe exactly the text they observed before this change --
//~ whereas `format!("{}", value)` on an error object rendered the useless
//~ literal `[object Object]`.
/// Project a thrown payload to the message text a string-typed `catch` observes.
fn smelt_thrown_message(value: &SmeltUnknown) -> String { if let SmeltUnknown::Object(object) = value { if let Some(SmeltUnknown::String(message)) = object.get("message") { return message.to_string(); } } format!("{value}") }
impl ::std::fmt::Display for SmeltThrown { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(&smelt_thrown_message(&self.value)) } }
impl ::std::error::Error for SmeltThrown {}
/// Enter the error channel, keeping the thrown value's structure and identity.
fn smelt_throw(value: SmeltUnknown) -> Box<dyn ::std::error::Error> { Box::new(SmeltThrown { value }) }
/// Recover the original thrown payload from the error channel.
//~
//~ A foreign error (anything that reached the channel through `?` rather than
//~ a Smelt `throw`) has no payload, so it is presented as an erased `Error`
//~ record built from its `Display` text -- the shape a `catch` saw before the
//~ payload ABI existed.
fn smelt_thrown_value(error: &(dyn ::std::error::Error + 'static)) -> SmeltUnknown { if let Some(thrown) = error.downcast_ref::<SmeltThrown>() { return thrown.value.clone(); } SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("Error".into())), ("message".to_owned(), SmeltUnknown::String(error.to_string().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))) }
//~ Emits the payload-projecting halves of the panic route.
//~
//~ Only well-formed inside the prelude's `needs_unknown` region: both items name
//~ `SmeltUnknown`. See the panic route in `runtime/thrown.rs` for the design.

/// Project a channel error's class and message across an unwind.
//~
//~ The class brand is the one `new <ErrorClass>(message)` writes; a thrown
//~ class instance is read through its `name` property instead, which is what
//~ JavaScript reports for `error.name` on a user error class.
//~ The thrown VALUE, parked for the catch that is about to run.
//~
//~ `panic_any` needs `Any + Send` and a `SmeltUnknown` holds `Rc` handles,
//~ so the value cannot ride the unwind itself — but it does not have to.
//~ A panic-routed throw and the `catch_unwind` that receives it are the same
//~ thread by construction (the `catch_unwind` is emitted around the call, in
//~ the same generated function), so a thread-local slot hands the value
//~ across without ever crossing a thread boundary. `Send` is a static bound
//~ on the panic payload, not a description of where this value goes.
//~
//~ Nesting is safe because throw/catch is strictly nested on one thread and
//~ each catch TAKES the slot: an inner catch cannot see an outer throw's
//~ value, and a panic that is not a `SmeltPanic` finds the slot empty and
//~ falls back to the class-and-message record.
/// The thrown value a panic-routed `throw` parked for its `catch`.
thread_local! { static SMELT_PANIC_VALUE: ::std::cell::RefCell<Option<SmeltUnknown>> = const { ::std::cell::RefCell::new(None) }; }
fn smelt_panic_payload(error: &(dyn ::std::error::Error + 'static)) -> SmeltPanic { let value = smelt_thrown_value(error); let message = smelt_thrown_message(&value); let mut class = "Error".to_owned(); if let SmeltUnknown::Object(object) = &value { if let Some(SmeltUnknown::String(name)) = object.get("__smelt_error") { class = name.to_string(); } else if let Some(SmeltUnknown::String(name)) = object.get("name") { class = name.to_string(); } } SMELT_PANIC_VALUE.with(|slot| { *slot.borrow_mut() = Some(value); }); SmeltPanic { class, message } }
/// Present a caught panic as the value a `catch` binds.
//~
//~ The parked value when the throw took the panic route, so a thrown
//~ string arrives at the `catch` as that string and a thrown class
//~ instance keeps its own fields. The class-and-message record otherwise:
//~ a panic that is not a routed `throw` has no JavaScript value behind it.
fn smelt_panic_error_value(panic: &(dyn ::std::any::Any + Send)) -> SmeltUnknown { if let Some(value) = SMELT_PANIC_VALUE.with(|slot| slot.borrow_mut().take()) { if panic.downcast_ref::<SmeltPanic>().is_some() { return value; } } SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String(smelt_panic_class(panic).into())), ("message".to_owned(), SmeltUnknown::String(smelt_panic_message(panic).into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))) }
// @smelt:if json_parse
//~ Emits the fallible `JSON.parse` adapter into the generated prelude.
//~
//~ `JSON.parse` throws a `SyntaxError` on malformed text and JavaScript code
//~ catches it (`try { JSON.parse(s) } catch { return false }`). The adapter
//~ reports failure through the same error channel a source-level `throw` uses,
//~ carrying the same `SyntaxError` record `new SyntaxError(message)` builds — so
//~ a `catch` binding cannot tell a runtime-raised parse error from a
//~ hand-written one.

// @smelt:end
// @smelt:item-end

// @smelt:item smelt_json_parse when json_parse & unknown
/// `JSON.parse`: parse JSON text, throwing a catchable `SyntaxError`.
fn smelt_json_parse(text: &(impl SmeltHostText + ?Sized)) -> Result<SmeltUnknown, Box<dyn ::std::error::Error>> { let text = text.host_text(); let text: &str = &text; let parsed = serde_json::from_str::<SmeltUnknown>(text); let parsed = match parsed { Err(error) => if smelt_json_has_surrogate_escape(text) { SmeltJsonReader::read(text).ok_or(error) } else { Err(error) }, ok => ok }; match parsed { Ok(value) => Ok(value), Err(error) => Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("SyntaxError".into())), ("message".to_owned(), SmeltUnknown::String(error.to_string().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }

/// Whether JSON text holds a `\uD800`-`\uDFFF` escape: `serde_json` rejects a
/// lone one, which JavaScript's `JSON.parse` reads as a lone surrogate.
fn smelt_json_has_surrogate_escape(text: &str) -> bool {
    text.as_bytes().windows(3).any(|window| window[0] == b'\\' && window[1] == b'u' && matches!(window[2], b'd' | b'D'))
}

/// A JSON reader that keeps every `\uXXXX` escape as the code unit it names,
/// lone surrogates included (ECMA-262 `JSON.parse`).
//~ `smelt_json_parse` uses it
//~ only for text `serde_json` rejects because of such an escape.
struct SmeltJsonReader<'a> { text: &'a [u8], at: usize }

impl SmeltJsonReader<'_> {
    //~ The value of the whole of `text`, or `None` when it is not JSON.
    fn read(text: &str) -> Option<SmeltUnknown> {
        let mut reader = SmeltJsonReader { text: text.as_bytes(), at: 0 };
        let value = reader.value()?;
        reader.skip_space();
        (reader.at == reader.text.len()).then_some(value)
    }
    //~ Skips JSON white space.
    fn skip_space(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) { self.at += 1; }
    }
    //~ Consumes `word` if the text continues with it.
    fn eat(&mut self, word: &[u8]) -> bool {
        if self.text.get(self.at..self.at + word.len()) == Some(word) { self.at += word.len(); true } else { false }
    }
    //~ One JSON value.
    fn value(&mut self) -> Option<SmeltUnknown> {
        self.skip_space();
        match *self.text.get(self.at)? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(SmeltUnknown::String),
            b't' => self.eat(b"true").then_some(SmeltUnknown::Bool(true)),
            b'f' => self.eat(b"false").then_some(SmeltUnknown::Bool(false)),
            b'n' => self.eat(b"null").then_some(SmeltUnknown::Null),
            _ => self.number(),
        }
    }
    //~ A JSON number, read as an `f64` as JavaScript reads it.
    fn number(&mut self) -> Option<SmeltUnknown> {
        let start = self.at;
        while matches!(self.text.get(self.at), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) { self.at += 1; }
        let digits = ::std::str::from_utf8(self.text.get(start..self.at)?).ok()?;
        serde_json::from_str::<f64>(digits).ok().map(SmeltUnknown::Number)
    }
    //~ A JSON string, as code units.
    fn string(&mut self) -> Option<JsString> {
        self.at += 1;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let byte = *self.text.get(self.at)?;
            match byte {
                b'"' => { self.at += 1; return Some(JsString::from_units(&units)); }
                b'\\' => {
                    let escape = *self.text.get(self.at + 1)?;
                    self.at += 2;
                    match escape {
                        b'"' => units.push(u16::from(b'"')),
                        b'\\' => units.push(u16::from(b'\\')),
                        b'/' => units.push(u16::from(b'/')),
                        b'b' => units.push(0x08),
                        b'f' => units.push(0x0C),
                        b'n' => units.push(u16::from(b'\n')),
                        b'r' => units.push(u16::from(b'\r')),
                        b't' => units.push(u16::from(b'\t')),
                        b'u' => {
                            let hex = ::std::str::from_utf8(self.text.get(self.at..self.at + 4)?).ok()?;
                            units.push(u16::from_str_radix(hex, 16).ok()?);
                            self.at += 4;
                        }
                        _ => return None,
                    }
                }
                0x00..=0x1F => return None,
                _ => {
                    let rest = ::std::str::from_utf8(self.text.get(self.at..)?).ok()?;
                    let ch = rest.chars().next()?;
                    let mut buffer = [0_u16; 2];
                    units.extend_from_slice(ch.encode_utf16(&mut buffer));
                    self.at += ch.len_utf8();
                }
            }
        }
    }
    //~ A JSON array.
    fn array(&mut self) -> Option<SmeltUnknown> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.eat(b"]") { return Some(SmeltUnknown::Array(items.into())); }
        loop {
            items.push(self.value()?);
            self.skip_space();
            if self.eat(b"]") { return Some(SmeltUnknown::Array(items.into())); }
            if !self.eat(b",") { return None; }
        }
    }
    //~ A JSON object; a repeated key keeps its first position and its last value.
    fn object(&mut self) -> Option<SmeltUnknown> {
        self.at += 1;
        let object = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
        self.skip_space();
        if self.eat(b"}") { return Some(SmeltUnknown::Object(object)); }
        loop {
            self.skip_space();
            if self.text.get(self.at) != Some(&b'"') { return None; }
            let key = self.string()?;
            self.skip_space();
            if !self.eat(b":") { return None; }
            let value = self.value()?;
            object.insert(key, value);
            self.skip_space();
            if self.eat(b"}") { return Some(SmeltUnknown::Object(object)); }
            if !self.eat(b",") { return None; }
        }
    }
}
// @smelt:item-end

// Emits the fallible URI-decoder adapters into the generated prelude.
//
// The runtime decoders answer `Option<String>` — `None` for malformed
// percent-encoding — and JavaScript answers that same input with a *catchable*
// `URIError`. These adapters convert one into the other through the ABI a
// source-level `throw` uses, so a `catch` binding cannot tell a
// runtime-raised `URIError` from a hand-written one, exactly as
// the `JSON.parse` support arranges for `SyntaxError`.
//
// Before this existed the emitter wrote
// `smelt_decode_uri(..).expect("URIError: URI malformed")`, which does not
// merely fail to be catchable: the handler block ends up with no predecessor,
// MIR drops it, and a `try`/`catch` the source wrote is *absent* from the
// generated crate. Hono's `tryDecode` is that shape.
// @smelt:item smelt_decode_uri_throwing when uri_decode
/// `smelt_decode_uri`: decode percent-encoding, throwing a catchable `URIError`.
fn smelt_decode_uri_throwing(value: &(impl SmeltHostText + ?Sized)) -> Result<String, Box<dyn ::std::error::Error>> { let value = value.host_text(); let value: &str = &value; match smelt_decode_uri(value) { Some(decoded) => Ok(decoded), None => Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("URIError".into())), ("message".to_owned(), SmeltUnknown::String("URI malformed".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }

/// `smelt_decode_uri_component`: decode percent-encoding, throwing a catchable `URIError`.
fn smelt_decode_uri_component_throwing(value: &(impl SmeltHostText + ?Sized)) -> Result<String, Box<dyn ::std::error::Error>> { let value = value.host_text(); let value: &str = &value; match smelt_decode_uri_component(value) { Some(decoded) => Ok(decoded), None => Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("URIError".into())), ("message".to_owned(), SmeltUnknown::String("URI malformed".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }

/// The `URIError: URI malformed` a URI transcoder throws.
fn smelt_uri_malformed() -> Box<dyn ::std::error::Error> { smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("URIError".into())), ("message".to_owned(), SmeltUnknown::String("URI malformed".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))) }

/// `encodeURI`: percent-encode, throwing a catchable `URIError` on a lone surrogate.
fn smelt_encode_uri_throwing(value: &(impl SmeltHostText + ?Sized)) -> Result<String, Box<dyn ::std::error::Error>> { if value.has_lone_surrogate() { return Err(smelt_uri_malformed()); } Ok(smelt_encode_uri(value)) }

/// `encodeURIComponent`: percent-encode, throwing a catchable `URIError` on a lone surrogate.
fn smelt_encode_uri_component_throwing(value: &(impl SmeltHostText + ?Sized)) -> Result<String, Box<dyn ::std::error::Error>> { if value.has_lone_surrogate() { return Err(smelt_uri_malformed()); } Ok(smelt_encode_uri_component(value)) }
// @smelt:item-end

// Emits the fallible `DataView` accessor adapters into the generated prelude.
//
// An offset outside the view is a `RangeError` in JavaScript, and it is the
// first stdlib MEMBER to reach a `catch` — `JSON.parse`, the URI decoders and
// the base64 pair were all free functions. The route is the same one and for
// the same reason: only a call carries an `unwind` edge, so a fallible
// operation has to be a call (see
// `blocker-logs/standards-throwing-rvalue.md`).
//
// The BOUND is checked here rather than in the infallible members, which stay
// as they are: a read or write inside the window is not fallible, and paying
// for a `Result` on every element access would put a `?` in every generated
// signature that touches one.
//
// The OFFSET arrives as the `f64` the source computed, not as a `usize`, so
// that the spec's `ToIndex` runs here too. `ToIndex` is not a cast: it
// truncates (`getInt16(1.7)` reads byte 1), maps `NaN` and `undefined` to 0,
// and REJECTS a negative or non-representable index with the same
// `RangeError` an out-of-window offset gets — `view.getInt16(-1)` throws in
// Node. Clamping the offset at the emit site instead would have turned that
// throw into a silent read of byte 0, and casting a huge `f64` to `usize`
// would have made the window test overflow rather than answer.
// @smelt:item smelt_data_view_byte_index when data_view_access
/// ECMA-262 `ToIndex` on a `DataView` byte offset: `None` is a `RangeError`.
fn smelt_data_view_byte_index(offset: f64) -> Option<usize> { let integer = if offset.is_nan() { 0.0 } else { offset.trunc() }; if integer < 0.0 || integer > 9007199254740991.0 { return None; } Some(integer as usize) }

/// `DataView.prototype.getX`: read one element, throwing a catchable `RangeError`.
fn smelt_data_view_get_throwing(view: &SmeltDataView, kind: SmeltTypedArrayKind, offset: f64, little_endian: bool) -> Result<f64, Box<dyn ::std::error::Error>> { match smelt_data_view_byte_index(offset).and_then(|index| view.get_checked(kind, index, little_endian)) { Some(value) => Ok(value), None => Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".into())), ("message".to_owned(), SmeltUnknown::String("Offset is outside the bounds of the DataView".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }

/// `DataView.prototype.setX`: write one element, throwing a catchable `RangeError`.
fn smelt_data_view_set_throwing(view: &SmeltDataView, kind: SmeltTypedArrayKind, offset: f64, value: f64, little_endian: bool) -> Result<(), Box<dyn ::std::error::Error>> { if smelt_data_view_byte_index(offset).is_some_and(|index| view.set_checked(kind, index, value, little_endian)) { Ok(()) } else { Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".into())), ("message".to_owned(), SmeltUnknown::String("Offset is outside the bounds of the DataView".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }
// @smelt:item-end

// The bigint accessors (`getBigInt64`, `setBigUint64`, ...) answer and take
// a `bigint`, exact at all 64 bits, through the same bound and the same
// `RangeError`.
// @smelt:item smelt_data_view_get_bigint_throwing when bigint & data_view_access
/// `DataView.prototype.getBigInt64`/`getBigUint64`, throwing a catchable `RangeError`.
fn smelt_data_view_get_bigint_throwing(view: &SmeltDataView, kind: SmeltTypedArrayKind, offset: f64, little_endian: bool) -> Result<::num_bigint::BigInt, Box<dyn ::std::error::Error>> { match smelt_data_view_byte_index(offset).and_then(|index| view.get_bigint_checked(kind, index, little_endian)) { Some(value) => Ok(value), None => Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".into())), ("message".to_owned(), SmeltUnknown::String("Offset is outside the bounds of the DataView".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }
/// `DataView.prototype.setBigInt64`/`setBigUint64`, throwing a catchable `RangeError`.
fn smelt_data_view_set_bigint_throwing(view: &SmeltDataView, kind: SmeltTypedArrayKind, offset: f64, value: &::num_bigint::BigInt, little_endian: bool) -> Result<(), Box<dyn ::std::error::Error>> { if smelt_data_view_byte_index(offset).is_some_and(|index| view.set_bigint_checked(kind, index, value, little_endian)) { Ok(()) } else { Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("RangeError".into())), ("message".to_owned(), SmeltUnknown::String("Offset is outside the bounds of the DataView".to_owned().into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }
// @smelt:item-end

// Emits the fallible base64 adapters into the generated prelude.
//
// Both directions throw the spec's branded `InvalidCharacterError`
// `DOMException`, and both reach a `catch` through the same ABI a source-level
// `throw` uses, so a `catch` binding cannot tell a runtime-raised one from a
// hand-written `new DOMException(msg, 'InvalidCharacterError')`.
//
// `btoa` maps each code point to one byte, which is only possible up to
// U+00FF; anything above has no byte and is the spec's
// `InvalidCharacterError`. The output is padded, which is what
// `btoa("\u{00ff} ")` being `"/yA="` means.
//
// `atob` implements WHATWG **forgiving-base64** rather than canonical base64,
// and the difference is observable in three places a stricter decoder gets
// wrong: `atob("aGVsbG8")` (no padding) decodes, `atob("YR==")`
// (non-canonical trailing bits) decodes, and `atob("aGVs bG8=")` (embedded
// whitespace) decodes — while `atob("YR=")` and `atob("a")` throw. The
// algorithm is the spec's, in its order: strip the five ASCII whitespace code
// points, drop up to two `=` only when the length is a multiple of four,
// refuse a length of `4n + 1`, refuse any character outside the standard
// alphabet, then decode.
//
// The decoded bytes come back as a BYTE STRING — one code point per byte, the
// inverse of `btoa` — not as UTF-8 text, because that is what `atob` answers:
// `atob(btoa(s)) === s` has to hold for every `s` the encoder accepted, and a
// UTF-8 reinterpretation would break it for every byte above 0x7F.
// @smelt:item SMELT_BASE64_FORGIVING when base64
/// The forgiving-base64 decode engine `atob` is specified against.
const SMELT_BASE64_FORGIVING: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(&base64::alphabet::STANDARD, base64::engine::GeneralPurposeConfig::new().with_encode_padding(false).with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireNone).with_decode_allow_trailing_bits(true));

/// `btoa`: base64 of a byte string, throwing a catchable `InvalidCharacterError`.
fn smelt_btoa_throwing(value: &(impl SmeltHostText + ?Sized)) -> Result<String, Box<dyn ::std::error::Error>> { use base64::Engine as _; let value = value.host_text(); let value: &str = &value;          let mut bytes = Vec::with_capacity(value.len());          for ch in value.chars() {          let point = ch as u32;          if point > 0xFF { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("InvalidCharacterError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String("Invalid character".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); }          bytes.push(point as u8);          }          Ok(base64::engine::general_purpose::STANDARD.encode(bytes)) }

/// `atob`: WHATWG forgiving-base64, throwing a catchable `InvalidCharacterError`.
fn smelt_atob_throwing(value: &(impl SmeltHostText + ?Sized)) -> Result<String, Box<dyn ::std::error::Error>> { use base64::Engine as _; let value = value.host_text(); let value: &str = &value;          let mut chars: Vec<char> = value.chars().filter(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000c}')).collect();          if chars.len() % 4 == 0 {          if chars.ends_with(&['=', '=']) { chars.truncate(chars.len() - 2); }          else if chars.ends_with(&['=']) { chars.truncate(chars.len() - 1); }          }          if chars.len() % 4 == 1 { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("InvalidCharacterError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String("The string to be decoded is not correctly encoded.".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); }          if !chars.iter().all(|ch| ch.is_ascii_alphanumeric() || *ch == '+' || *ch == '/') { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("InvalidCharacterError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String("Invalid character".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); }          let encoded: String = chars.into_iter().collect();          match SMELT_BASE64_FORGIVING.decode(encoded.as_bytes()) {          Ok(bytes) => Ok(bytes.into_iter().map(char::from).collect()),          Err(_) => Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("InvalidCharacterError".into())), ("__smelt_domexception".to_owned(), SmeltUnknown::Bool(true)), ("message".to_owned(), SmeltUnknown::String("Invalid character".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) } }
// @smelt:item-end

// Emits the shared global object and the erased `Object.assign` helper.
//
// **Dynamic boundary.** The global object is JavaScript's open property bag:
// the program writes to it (`Object.assign(globalThis, {..})`,
// `globalThis.x = v`) and a host-injected `declare const X` is read from it,
// so which members exist is a run-time fact no concrete type, generated union
// or scoped generic can state. It is ONE record per thread (tests run one per
// thread, and each gets its own global, as each `vitest` worker does), shared
// by every reference through `SmeltObject`'s shared storage, so a write
// through one reference is visible through all of them. It carries the
// `__smelt_global_object` marker the rest of the runtime already recognizes
// (the `[object global]` tag, the modeled-constructor lookup of
// `globalThis[name]`, and its exclusion from `for...in`). The regression test
// `global_object_members_are_run_time_facts` in
// `crates/smelt-transpiler/src/unknown_report.rs` records why this is a
// legitimate boundary rather than avoidable erasure.
// @smelt:item smelt_global_object when global_object & unknown
/// The ambient global object (`globalThis`): one shared record per thread.
fn smelt_global_object() -> SmeltUnknown { thread_local! { static SMELT_GLOBAL_OBJECT: SmeltObject = SmeltObject::new(Vec::from([("__smelt_global_object".to_owned(), SmeltUnknown::Bool(true))])); } SmeltUnknown::Object(SMELT_GLOBAL_OBJECT.with(Clone::clone)) }
// @smelt:item-end

// Emits `Object.assign(target, ...sources)` for an ERASED target.
//
// **Dynamic boundary.** The target's static type is `unknown`, so its own
// properties are only known at run time; the helper copies each source's own
// enumerable string-keyed entries onto the SAME object (JavaScript mutates the
// target in place and returns it), writing through `smelt_index_assign` so a
// function, array or byte-backed target keeps its own write rules.
// @smelt:item smelt_unknown_assign when unknown & unknown_assign
/// `Object.assign(target, source)` onto an erased target, in place.
fn smelt_unknown_assign(target: &mut SmeltUnknown, source: &SmeltUnknown) { let entries: Vec<(String, SmeltUnknown)> = match source { SmeltUnknown::Object(object) => { let shape = smelt_for_in_object_shape(object); object.iter().filter(|(key, _)| smelt_is_for_in_shape_key(shape, key)).map(|(key, value)| (key.to_string_lossy(), value)).collect() }, SmeltUnknown::Array(values) => values.clone().into_vec().into_iter().enumerate().map(|(index, value)| (index.to_string(), value)).collect(), SmeltUnknown::String(text) => text.units().iter().enumerate().map(|(index, unit)| (index.to_string(), SmeltUnknown::String(JsString::from_units(&[*unit])))).collect(), _ => Vec::new() }; for (key, value) in entries { smelt_index_assign(target, key, value); } }
// @smelt:item-end

// Emits the fallible `new RegExp(pattern, flags)` adapter into the prelude.
//
// ECMA-262 `RegExpInitialize` validates the flags and parses the pattern at
// CONSTRUCTION, throwing a `SyntaxError` for either — a program can catch it
// around `new RegExp(p)` (Hono's `PatternRouter` turns it into its own
// `UnsupportedPathError`). The adapter runs the same two checks:
//
// * the flags are each one of `dgimsuvy`, none repeated, and not both `u`
//   and `v` — the spec's list, checked here because the engine never sees
//   most of them;
// * the pattern compiles through `SmeltRegExp::try_compiled`, the exact path
//   every later match takes, so "constructed" and "usable" cannot disagree.
//   A successful compile is memoized there, so the first match pays nothing
//   extra.
//
// The messages follow V8's spelling (`Invalid regular expression: /src/flags:
// ...`, `Invalid flags supplied to RegExp constructor '..'`); the engine's
// own reason for rejecting a pattern is not V8's, so the reason is the
// generic `Invalid pattern` when the engine rejects it.
// @smelt:item smelt_regexp_has_duplicate_group_name when reg_exp_compile
/// Whether a JavaScript pattern names two capture groups alike (a `SyntaxError` in V8).
fn smelt_regexp_has_duplicate_group_name(source: &str) -> bool { let chars: Vec<char> = source.chars().collect(); let mut names = ::std::collections::HashSet::new(); let (mut index, mut in_class) = (0, false); while index < chars.len() { match chars[index] { '\\' => { index += 2; continue; } '[' => in_class = true, ']' => in_class = false, '(' if !in_class && chars.get(index + 1) == Some(&'?') && chars.get(index + 2) == Some(&'<') && !matches!(chars.get(index + 3), Some('=' | '!')) => { let name: String = chars[index + 3..].iter().take_while(|ch| **ch != '>').collect(); if !names.insert(name) { return true; } } _ => {} } index += 1; } false }

/// `new RegExp(pattern, flags)` over a run-time pattern, throwing a catchable `SyntaxError`.
fn smelt_regexp_compile(source: impl ::std::fmt::Display, flags: impl ::std::fmt::Display) -> Result<SmeltRegExp, Box<dyn ::std::error::Error>> { let (source, flags) = (source.to_string(), flags.to_string()); let mut seen = ::std::collections::HashSet::new(); if !flags.chars().all(|flag| "dgimsuvy".contains(flag) && seen.insert(flag)) || (seen.contains(&'u') && seen.contains(&'v')) { return Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("SyntaxError".into())), ("message".to_owned(), SmeltUnknown::String(format!("Invalid flags supplied to RegExp constructor '{flags}'").into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))); } let regexp = SmeltRegExp::new(source, flags); let reason = if smelt_regexp_has_duplicate_group_name(&regexp.source) { "Duplicate capture group name" } else if regexp.try_compiled().is_none() { "Invalid pattern" } else { return Ok(regexp) }; Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("SyntaxError".into())), ("message".to_owned(), SmeltUnknown::String(format!("Invalid regular expression: /{}/{}: {reason}", regexp.source, regexp.flags).into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) }
// @smelt:item-end
