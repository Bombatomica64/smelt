// Reassignable host globals: `globalThis.<Name> = ...` and its `instanceof` consequences.

// Bounded host-global override support: a fixed override-state enum, one
// `thread_local!` slot per host name the crate reassigns (fresh `Native`
// per test thread), and the three fixed helpers. Gated on any
// `globalThis.<Name> =` write, so crates that never reassign a host
// global emit byte-identical output.
// @smelt:item SmeltHostOverride when host_override
/// Override state of a modeled host constructor's global slot.
//~
//~ `Native` is the unmodified builtin; `Absent` is an explicit
//~ `globalThis.X = undefined`; `Ctor` holds a reassigned constructor value.
#[derive(Clone)] enum SmeltHostOverride { Native, Absent, Ctor(SmeltUnknown) }

thread_local! {
// @smelt:slot host_override_slots
}

/// Read a host override slot: native-handle marker for `Native`, the
/// stored constructor for `Ctor`, JS `undefined` for `Absent`.
fn smelt_host_override_read(slot: &::std::cell::RefCell<SmeltHostOverride>, name: &str) -> SmeltUnknown {
    match &*slot.borrow() { SmeltHostOverride::Native => { let entries = Vec::from([("__smelt_native_ctor".to_owned(), SmeltUnknown::Bool(true)), ("name".to_owned(), SmeltUnknown::String(name.into()))]); SmeltUnknown::Object(SmeltObject::new(entries)) }, SmeltHostOverride::Absent => SmeltUnknown::Undefined, SmeltHostOverride::Ctor(value) => value.clone() }
}

/// Classify and store a written host-global value; returns the stored value.
//~
//~ JS `undefined` -> `Absent`; a native-handle marker record -> `Native`
//~ (the save/restore round trip); any function/class value -> `Ctor`.
fn smelt_host_override_write(slot: &::std::cell::RefCell<SmeltHostOverride>, value: SmeltUnknown) -> SmeltUnknown {
    let state = match &value { SmeltUnknown::Undefined => SmeltHostOverride::Absent, SmeltUnknown::Object(entries) if entries.contains_key("__smelt_native_ctor") => SmeltHostOverride::Native, _ => SmeltHostOverride::Ctor(value.clone()) }; *slot.borrow_mut() = state; value
}

//~ A class erased to a plain callable loses its constructor identity: an
//~ `Rc<dyn Fn>` has no `prototype` and no name, so nothing relates the
//~ stored override to the `__smelt_class` its instances carry. The
//~ erasure site knows both, and records the class names whose instances
//~ are instances of this constructor — the class itself plus every
//~ subclass of it in this crate, computed where the hierarchy is known so
//~ the runtime needs no class graph.
//~ The names live in the callable's entry of the bounded callable table
//~ (`callable_registry`), so they die with the constructor value.
/// Record which classes a callable constructs (the class and its subclasses).
fn smelt_register_function_classes<T: ?Sized + 'static>(function: &::std::rc::Rc<T>, classes: &[&'static str]) { smelt_update_callable(function, |meta| meta.classes = Some(classes.to_vec())); }
/// The classes a stored constructor value constructs, if it is a known one.
fn smelt_function_classes(value: &SmeltUnknown) -> Option<Vec<&'static str>> { let SmeltUnknown::Function(function) = value else { return None; }; smelt_read_callable(function, |meta| meta.classes.clone()) }

/// `value instanceof <HostName>` where the name lives in an override slot.
//~
//~ `instanceof` reads the binding, and an overridable host constructor's
//~ binding is its slot, so the answer follows the slot's state: the native
//~ builtin is recognized by its identity marker(s); a reassigned constructor
//~ is recognized by the class its instances record (which is why a native
//~ record is *not* an instance of a replacement class); and a deleted global
//~ has no instances at all. A stored constructor with no registered class
//~ (an ordinary function assigned into the slot) falls back to the marker
//~ probe rather than answering `false` for the native records still around.
fn smelt_host_override_instance_of(slot: &::std::cell::RefCell<SmeltHostOverride>, value: &SmeltUnknown, markers: &[&str]) -> bool {
    let marker_probe = || matches!(value, SmeltUnknown::Object(map) if markers.iter().any(|marker| map.contains_key(*marker)));
    match &*slot.borrow() { SmeltHostOverride::Absent => false, SmeltHostOverride::Native => marker_probe(), SmeltHostOverride::Ctor(ctor) => match smelt_function_classes(ctor) { Some(classes) => matches!(value, SmeltUnknown::Object(map) if matches!(map.get("__smelt_class"), Some(SmeltUnknown::String(class)) if classes.iter().any(|entry| class.eq_str(entry)))), None => marker_probe() } }
}

/// The check `instanceof` makes of its right-hand binding before anything else.
//~
//~ JavaScript throws `TypeError` when the right-hand side of `instanceof` is
//~ not callable, so a slot left `Absent` (`globalThis.File = undefined`) or
//~ holding a value that is no function throws instead of answering `false`.
//~ The native builtin and a stored function or class value pass.
fn smelt_host_override_require_constructor(slot: &::std::cell::RefCell<SmeltHostOverride>) -> Result<(), Box<dyn ::std::error::Error>> {
    let callable = match &*slot.borrow() { SmeltHostOverride::Native => true, SmeltHostOverride::Absent => false, SmeltHostOverride::Ctor(value) => matches!(value, SmeltUnknown::Function(_)) || matches!(value, SmeltUnknown::Object(class) if class.contains_key("__smelt_class_value") || class.contains_key("__smelt_call")) };
    if callable { return Ok(()); }
    Err(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String("TypeError".into())), ("message".to_owned(), SmeltUnknown::String("Right-hand side of 'instanceof' is not callable".into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)])))))
}

/// Whether a host override slot is present (`false` only when `Absent`).
fn smelt_host_override_present(slot: &::std::cell::RefCell<SmeltHostOverride>) -> bool { !matches!(&*slot.borrow(), SmeltHostOverride::Absent) }

// @smelt:item-end
