// Runtime support for JavaScript function *objects* and `[[Construct]]`.
//
// A JavaScript function is an object. Three consequences the generated runtime
// has to answer, all emitted here:
//
// * **A function has own properties.** `f.prototype` is one of them, and source
//   code writes others (`partialed.prototype = …`, `curried.placeholder = …`).
//   An `Rc<dyn Fn(..)>` has nowhere to put them, so they live in a bag on the
//   function's *canonical identity* token — the identity
//   `smelt_same_function_identity` compares and `Function.prototype.length`
//   is stored on (see `callable_registry`). Carrying the bag on the identity
//   rather than per allocation is what makes a typed callable and every
//   erasure adapter derived from it share ONE property bag, so a write through
//   one spelling is visible through all of them.
//
// * **`new f(args)` is not `f(args)`.** `[[Construct]]` allocates an object
//   whose prototype link is `f.prototype`, runs `f` with that object installed
//   as the receiver, and answers the allocated object unless `f` returned an
//   object of its own. All three are observable, and none of them happens for a
//   plain call.
//
// * **`x instanceof f` is a prototype-chain walk** (`OrdinaryHasInstance`),
//   not a nominal test: it follows `x`'s chain link by link looking for the
//   *object* `f.prototype` by reference. That is why `Object.create(p)` has to
//   record a real link to `p` and not merely copy `p`'s members. The link lives
//   in `SMELT_OBJECT_PROTOTYPES`, keyed by the created object's identity (see
//   `smelt_register_object_prototype` in `lib.rs`); the chain walk here,
//   `v.__proto__` (`smelt_proto_accessor`) and `Object.getPrototypeOf`
//   (`smelt_prototype_sentinel`) all read that ONE table, which is what keeps
//   the three answers consistent.
//
// The `prototype` object is materialised on FIRST READ rather than at function
// creation. Nothing can observe the object before something reads it, so the
// two are indistinguishable, and closures that are never constructed through
// (the overwhelming majority) pay no allocation.
//
// Known infidelity: an arrow function has no `prototype` in JavaScript, and
// this runtime cannot tell an arrow from a function expression — MIR carries no
// arrow flag — so it answers an object for both. Reading `.prototype` off an
// arrow is the only way to see the difference.

// Emit the identity-carried own-property bag of function values.
//
// The bag lives on the function's identity token (`SmeltFunctionIdentity`,
// see `callable_registry`), so a typed callable and every erasure adapter
// derived from it share ONE bag, and the bag dies with the last of them.
// @smelt:item smelt_function_property_lookup when unknown
/// Read one own property of the function value with this canonical identity.
fn smelt_function_property_lookup(identity: &SmeltFunctionIdentity, name: &str) -> Option<SmeltUnknown> { identity.0.properties.borrow().as_ref().and_then(|bag| bag.get(name)) }

/// Store one own property on the function value with this canonical identity.
fn smelt_set_function_property_key(identity: &SmeltFunctionIdentity, name: &str, value: SmeltUnknown) { identity.0.properties.borrow_mut().get_or_insert_with(|| SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new())).insert(name.to_owned(), value); }

/// Store one own property on a function value (JS `f.name = value`).
///
/// Accepts any callable handle, typed or erased, and resolves it to the one
/// canonical identity every representation of that function shares.
fn smelt_set_function_property<F: ?Sized + 'static>(function: &::std::rc::Rc<F>, name: &str, value: SmeltUnknown) { smelt_set_function_property_key(&smelt_canonical_function_identity(function), name, value); }

/// The `prototype` object of a function value, created on first read.
///
/// JavaScript gives every non-arrow function a fresh `{ constructor: f }`
/// object when the function is created. Creating it lazily is observationally
/// the same and costs nothing for a function nothing constructs through.
/// The object references the function (`constructor`), which is the same
/// reference cycle JavaScript has: a function whose prototype was read is
/// kept alive by it, as any `Rc` cycle in the generated runtime is.
fn smelt_function_prototype(identity: &SmeltFunctionIdentity, constructor: SmeltUnknown) -> SmeltUnknown { if let Some(value) = smelt_function_property_lookup(identity, "prototype") { return value; } let prototype = SmeltUnknown::Object(SmeltObject::new(Vec::from([("constructor".to_owned(), constructor)]))); smelt_set_function_property_key(identity, "prototype", prototype.clone()); prototype }

/// Read one own property of an erased function value (JS `f.prototype`).
fn smelt_function_value_property(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, name: &str) -> SmeltUnknown { let identity = smelt_canonical_function_identity(function); if let Some(value) = smelt_function_property_lookup(&identity, name) { return value; } if name == "prototype" { return smelt_function_prototype(&identity, SmeltUnknown::Function(function.clone())); } SmeltUnknown::Undefined }

// @smelt:item-end

// Emit the JavaScript `[[Construct]]` helper.
// @smelt:item smelt_construct when construct
/// JavaScript `new callee(args)` through a function VALUE (`[[Construct]]`).
///
/// Allocates an object linked to the callee's `prototype`, runs the callee with
/// that object installed as the receiver, and answers the callee's own result
/// only when it is an object — a constructor that returns a primitive (or
/// nothing) yields the allocated object, which is what makes
/// `new f() instanceof g` answerable at all. A non-callable operand answers
/// `undefined`: JavaScript throws a TypeError there, and Smelt models an
/// unconstructible callee the same way an uncallable one is modeled.
///
/// The prototype link is established by `smelt_object_from_prototype`, which
/// records it in `SMELT_OBJECT_PROTOTYPES` under the allocated object's
/// identity — the same table `smelt_instance_of_value` walks below, so
/// `new F() instanceof F` holds by construction.
fn smelt_construct(callee: SmeltUnknown, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let Some(function) = smelt_callable_of(&callee) else { return SmeltUnknown::Undefined; };
    let prototype = smelt_function_value_property(&function, "prototype");
    let instance = smelt_object_from_prototype(prototype);
    let result = { let _smelt_this_guard = smelt_push_this(instance.clone()); (function)(args).unwrap_or_else(|error| smelt_panic_throw(error)) };
    match result { SmeltUnknown::Object(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => result, _ => instance }
}

/// JavaScript `new value.constructor(args)`.
///
/// A class instance constructs through its erased `constructor` entry, a host
/// object through what `.constructor` reads.
/// An array is a new array and a plain object a new plain object: their
/// constructors are `Array` and `Object`, which no read answers as a value.
fn smelt_construct_like(value: SmeltUnknown, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    //~ A class instance carries its real constructor as a prototype entry when
    //~ the program needs it, so field initialisers and the constructor body run.
    if let SmeltUnknown::Object(object) = &value && let Some(SmeltUnknown::Function(build)) = object.get("__smelt_method:constructor") {
        return (build)(args).unwrap_or_else(|error| smelt_panic_throw(error));
    }
    let constructor = smelt_get_unknown_field(&value, "constructor");
    if smelt_callable_of(&constructor).is_some() { return smelt_construct(constructor, args); }
    match (value, args.as_slice()) {
        (SmeltUnknown::Array(_), [SmeltUnknown::Number(length)]) => SmeltUnknown::Array(SmeltArray::from(vec![SmeltUnknown::Undefined; *length as usize])),
        (SmeltUnknown::Array(_), _) => SmeltUnknown::Array(SmeltArray::from(args)),
        _ => SmeltUnknown::Object(SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new())),
    }
}

// @smelt:item-end

// Emit the "callable behind an erased value" resolver both operations need.
// @smelt:item smelt_callable_of when callable_of & unknown
/// The callable behind an erased value: a function, or a callable object's
/// `__smelt_call` slot.
fn smelt_callable_of(value: &SmeltUnknown) -> Option<::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>> { match value { SmeltUnknown::Function(function) => Some(function.clone()), SmeltUnknown::Object(object) => match object.get("__smelt_call") { Some(SmeltUnknown::Function(function)) => Some(function), _ => None }, _ => None } }

// @smelt:item-end

// Emit the `OrdinaryHasInstance` prototype-chain walk.
// @smelt:item smelt_instance_of_value when instance_of_value
/// JavaScript `value instanceof target` for a runtime constructor value.
///
/// Walks `value`'s prototype chain comparing each link by REFERENCE against
/// the target's own `prototype` object, exactly as `OrdinaryHasInstance` does.
/// A non-callable target, or one whose `prototype` is not an object, answers
/// `false` (JavaScript throws for the former; Smelt reports the absence).
///
/// The walk stops when `smelt_proto_accessor` stops advancing, so it
/// terminates on the opaque `__smelt_proto:*` sentinels too.
fn smelt_instance_of_value(value: &SmeltUnknown, target: &SmeltUnknown) -> bool {
    if let SmeltUnknown::Object(class) = target && let Some(SmeltUnknown::String(covered)) = class.get("__smelt_class_value") {
        let SmeltUnknown::Object(instance) = value else { return false; };
        let Some(SmeltUnknown::String(name)) = instance.get("__smelt_class") else { return false; };
        return covered.to_string_lossy().split(',').any(|class_name| name.eq_str(class_name));
    }
    let Some(function) = smelt_callable_of(target) else { return false; };
    let prototype = smelt_function_value_property(&function, "prototype");
    let SmeltUnknown::Object(prototype) = prototype else { return false; };
    let mut current = smelt_proto_accessor(value);
    loop {
        match &current {
            SmeltUnknown::Null | SmeltUnknown::Undefined => return false,
            SmeltUnknown::Object(link) if link.id == prototype.id => return true,
            _ => {}
        }
        let next = smelt_proto_accessor(&current);
        if smelt_same_prototype_link(&next, &current) { return false; }
        current = next;
    }
}

/// Whether two prototype-chain links are the same link, so a walk that reached
/// one has made no progress and must stop.
fn smelt_same_prototype_link(left: &SmeltUnknown, right: &SmeltUnknown) -> bool { match (left, right) { (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right, _ => false } }

// @smelt:item-end

// @smelt:item smelt_class_value when class_value & unknown
/// A class as a runtime value: callable through `__smelt_call` when its
/// constructor is known, named, carrying the classes `instanceof` accepts
/// (itself and its subclasses, comma separated), and holding its static
/// methods as own properties, keyed as `smelt_property_key` keys them.
fn smelt_class_value(name: &str, covered: &str, constructor: SmeltUnknown, statics: Vec<(String, SmeltUnknown)>) -> SmeltUnknown { let mut entries = Vec::from([("name".to_owned(), SmeltUnknown::String(name.into())), ("__smelt_class_value".to_owned(), SmeltUnknown::String(covered.into())), ("__smelt_call".to_owned(), constructor)]); entries.extend(statics); SmeltUnknown::Object(SmeltObject::new(entries)) }

// @smelt:item-end
