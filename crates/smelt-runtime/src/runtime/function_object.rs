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
//~
//~ Accepts any callable handle, typed or erased, and resolves it to the one
//~ canonical identity every representation of that function shares.
fn smelt_set_function_property<F: ?Sized + 'static>(function: &::std::rc::Rc<F>, name: &str, value: SmeltUnknown) { smelt_set_function_property_key(&smelt_canonical_function_identity(function), name, value); }

/// The `prototype` object of a function value, created on first read.
//~
//~ JavaScript gives every non-arrow function a fresh `{ constructor: f }`
//~ object when the function is created. Creating it lazily is observationally
//~ the same and costs nothing for a function nothing constructs through.
//~ The object references the function (`constructor`), which is the same
//~ reference cycle JavaScript has: a function whose prototype was read is
//~ kept alive by it, as any `Rc` cycle in the generated runtime is.
fn smelt_function_prototype(identity: &SmeltFunctionIdentity, constructor: SmeltUnknown) -> SmeltUnknown { if let Some(value) = smelt_function_property_lookup(identity, "prototype") { return value; } let prototype = SmeltUnknown::Object(SmeltObject::new(Vec::from([("constructor".to_owned(), constructor)]))); smelt_set_function_property_key(identity, "prototype", prototype.clone()); prototype }

/// Read one own property of an erased function value (JS `f.prototype`).
fn smelt_function_value_property(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, name: &str) -> SmeltUnknown { let identity = smelt_canonical_function_identity(function); if let Some(value) = smelt_function_property_lookup(&identity, name) { return value; } if name == "prototype" { return smelt_function_prototype(&identity, SmeltUnknown::Function(function.clone())); } SmeltUnknown::Undefined }

// @smelt:item-end

// Emit the JavaScript `[[Construct]]` helper.
// @smelt:item smelt_construct when construct
/// JavaScript `new callee(args)` through a function VALUE (`[[Construct]]`).
//~
//~ Allocates an object linked to the callee's `prototype`, runs the callee with
//~ that object installed as the receiver, and answers the callee's own result
//~ only when it is an object — a constructor that returns a primitive (or
//~ nothing) yields the allocated object, which is what makes
//~ `new f() instanceof g` answerable at all. A non-callable operand answers
//~ `undefined`: JavaScript throws a TypeError there, and Smelt models an
//~ unconstructible callee the same way an uncallable one is modeled.
//~
//~ The prototype link is established by `smelt_object_from_prototype`, which
//~ records it in `SMELT_OBJECT_PROTOTYPES` under the allocated object's
//~ identity — the same table `smelt_instance_of_value` walks below, so
//~ `new F() instanceof F` holds by construction.
fn smelt_construct(callee: SmeltUnknown, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let Some(function) = smelt_callable_of(&callee) else { return SmeltUnknown::Undefined; };
    let prototype = smelt_function_value_property(&function, "prototype");
    let instance = smelt_object_from_prototype(prototype);
    let result = { let _smelt_this_guard = smelt_push_this(instance.clone()); (function)(args).unwrap_or_else(|error| smelt_panic_throw(error)) };
    match result { SmeltUnknown::Object(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => result, _ => instance }
}

//~ `new Proxy(target, handler)`. The proxy is an erased object naming its target
//~ and handler; every erased property path consults them. It also holds a copy
//~ of the target's entries, so paths that only enumerate see the target's keys.
//~ A function target stays a function: calling it runs the `apply` trap.
fn smelt_new_proxy(target: SmeltUnknown, handler: SmeltUnknown) -> SmeltUnknown {
    if let SmeltUnknown::Function(callee) = &target {
        let (callee, inner_target, inner_handler) = (callee.clone(), target.clone(), handler.clone());
        let function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args| match smelt_get_unknown_field(&inner_handler, "apply") {
            SmeltUnknown::Function(trap) => trap(vec![inner_target.clone(), SmeltUnknown::Undefined, SmeltUnknown::Array(args.into())]),
            _ => callee(args),
        });
        smelt_set_function_property(&function, "__smelt_method:__smelt_proxy_target", target);
        smelt_set_function_property(&function, "__smelt_method:__smelt_proxy_handler", handler);
        return SmeltUnknown::Function(function);
    }
    let callee = smelt_callable_of(&target);
    let inner_target = target.clone();
    let inner_handler = handler.clone();
    let proxy = SmeltObject::new(Vec::from([("__smelt_method:__smelt_proxy_target".to_owned(), target), ("__smelt_method:__smelt_proxy_handler".to_owned(), handler)]));
    smelt_proxy_sync(&proxy);
    //~ A callable object target (`Object.assign(fn, props)`) stays callable.
    if let Some(callee) = callee {
        let call: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args| match smelt_get_unknown_field(&inner_handler, "apply") {
            SmeltUnknown::Function(trap) => trap(vec![inner_target.clone(), SmeltUnknown::Undefined, SmeltUnknown::Array(args.into())]),
            _ => callee(args),
        });
        proxy.insert("__smelt_method:__smelt_proxy_call", SmeltUnknown::Function(call.clone()));
        proxy.insert("__smelt_call", SmeltUnknown::Function(call));
    }
    SmeltUnknown::Object(proxy)
}

//~ Copy the target's own entries onto the proxy, for paths that only enumerate.
fn smelt_proxy_sync(proxy: &SmeltObject) {
    let Some(SmeltUnknown::Object(target)) = proxy.get("__smelt_method:__smelt_proxy_target") else { return };
    let stale: Vec<_> = proxy.keys().into_iter().filter(|key| !key.eq_str_prefix("__smelt_method:__smelt_proxy_")).collect();
    for key in stale { proxy.remove(&key); }
    for (key, value) in target.iter() { proxy.insert(key, value); }
    if let Some(call) = proxy.get("__smelt_method:__smelt_proxy_call") { proxy.insert("__smelt_call", call); }
}

//~ The `(target, handler)` of a proxy value.
fn smelt_proxy_parts(value: &SmeltUnknown) -> Option<(SmeltUnknown, SmeltUnknown)> {
    match value {
        SmeltUnknown::Object(object) => Some((object.get("__smelt_method:__smelt_proxy_target")?, object.get("__smelt_method:__smelt_proxy_handler")?)),
        SmeltUnknown::Function(function) => match (smelt_function_value_property(function, "__smelt_method:__smelt_proxy_target"), smelt_function_value_property(function, "__smelt_method:__smelt_proxy_handler")) {
            (SmeltUnknown::Undefined, _) | (_, SmeltUnknown::Undefined) => None,
            parts => Some(parts),
        },
        _ => None,
    }
}

//~ Run handler trap `name`, or `None` when the handler has no such trap. A throw
//~ from the trap propagates like any JavaScript throw from a property access.
fn smelt_proxy_trap(handler: &SmeltUnknown, name: &str, args: Vec<SmeltUnknown>) -> Option<SmeltUnknown> {
    let SmeltUnknown::Function(trap) = smelt_get_unknown_field(handler, name) else { return None };
    Some(trap(args).unwrap_or_else(|error| smelt_panic_throw(error)))
}

//~ The property key a trap receives: a symbol is a symbol, anything else a string.
fn smelt_proxy_key(key: &str) -> SmeltUnknown {
    /*@if symbol*/if let Some(symbol) = smelt_own_symbol_key_value(key) { return symbol; }/*@end*/
    SmeltUnknown::String(key.into())
}

//~ `proxy.key`: the `get` trap, else the target's property.
fn smelt_proxy_get(proxy: &SmeltUnknown, key: &str) -> Option<SmeltUnknown> {
    let (target, handler) = smelt_proxy_parts(proxy)?;
    if key.starts_with("__smelt") && !key.starts_with("__smelt_symbol") { return Some(smelt_get_unknown_field(&target, key)); }
    Some(smelt_proxy_trap(&handler, "get", vec![target.clone(), smelt_proxy_key(key), proxy.clone()]).unwrap_or_else(|| smelt_get_unknown_field(&target, key)))
}

//~ `proxy.key = value`: the `set` trap, else a write on the target. `false` when
//~ `proxy` is not a proxy.
fn smelt_proxy_set(proxy: &SmeltUnknown, key: &str, value: &SmeltUnknown) -> bool {
    let Some((mut target, handler)) = smelt_proxy_parts(proxy) else { return false };
    if smelt_proxy_trap(&handler, "set", vec![target.clone(), smelt_proxy_key(key), value.clone(), proxy.clone()]).is_none() {
        smelt_index_assign(&mut target, key, value.clone());
    }
    if let SmeltUnknown::Object(object) = proxy { smelt_proxy_sync(object); }
    true
}

//~ `key in proxy`: the `has` trap, else the target's answer.
fn smelt_proxy_has(proxy: &SmeltUnknown, key: &str) -> Option<bool> {
    let (target, handler) = smelt_proxy_parts(proxy)?;
    Some(smelt_proxy_trap(&handler, "has", vec![target.clone(), smelt_proxy_key(key)]).map_or_else(|| smelt_has_property(&target, key), |answer| !matches!(answer, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Bool(false)) && !matches!(&answer, SmeltUnknown::Number(number) if *number == 0.0 || number.is_nan()) && !matches!(&answer, SmeltUnknown::String(text) if text.is_empty())))
}

//~ `delete proxy.key`: the `deleteProperty` trap, else a delete on the target.
//~ `None` when `proxy` is not a proxy.
fn smelt_proxy_delete(proxy: &SmeltUnknown, key: &(impl SmeltHostText + ?Sized)) -> Option<bool> {
    let key = key.host_text();
    let (target, handler) = smelt_proxy_parts(proxy)?;
    let trapped = smelt_proxy_trap(&handler, "deleteProperty", vec![target.clone(), smelt_proxy_key(&key)]);
    if trapped.is_none() && let SmeltUnknown::Object(object) = &target { object.remove(&*key); }
    let answer = trapped.is_none_or(|answer| !matches!(answer, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Bool(false)));
    if let SmeltUnknown::Object(object) = proxy { smelt_proxy_sync(object); }
    Some(answer)
}

//~ The `ownKeys` trap's answer, when the proxy has one.
fn smelt_proxy_own_keys(proxy: &SmeltObject) -> Option<Vec<SmeltUnknown>> {
    let (target, handler) = (proxy.get("__smelt_method:__smelt_proxy_target")?, proxy.get("__smelt_method:__smelt_proxy_handler")?);
    match smelt_proxy_trap(&handler, "ownKeys", vec![target])? { SmeltUnknown::Array(keys) => Some(keys.into_vec()), _ => Some(Vec::new()) }
}

/// JavaScript `new value.constructor(args)`.
//~
//~ A class instance constructs through its erased `constructor` entry, a host
//~ object through what `.constructor` reads.
//~ An array is a new array and a plain object a new plain object: their
//~ constructors are `Array` and `Object`, which no read answers as a value.
fn smelt_construct_like(value: SmeltUnknown, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    //~ `new Proxy`: `[target, handler]`, or `[target, name, trap, ..]` for a handler
    //~ literal, whose traps arrive one by one at their own static types.
    if matches!(&value, SmeltUnknown::String(name) if name == "__smelt_new_proxy") {
        let mut args = args.into_iter();
        let target = args.next().unwrap_or(SmeltUnknown::Undefined);
        let rest = args.collect::<Vec<_>>();
        let handler = if rest.len() == 1 { rest.into_iter().next().unwrap_or(SmeltUnknown::Undefined) } else {
            let mut entries = Vec::new();
            let mut rest = rest.into_iter();
            while let (Some(SmeltUnknown::String(name)), Some(trap)) = (rest.next(), rest.next()) { entries.push((name.to_string_lossy(), trap)); }
            SmeltUnknown::Object(SmeltObject::new(entries))
        };
        return smelt_new_proxy(target, handler);
    }
    //~ A class instance carries its real constructor as a prototype entry when
    //~ the program needs it, so field initialisers and the constructor body run.
    if let SmeltUnknown::Object(object) = &value && let Some(SmeltUnknown::Function(build)) = object.get("__smelt_method:constructor") {
        return (build)(args).unwrap_or_else(|error| smelt_panic_throw(error));
    }
    //~ A `Date` builds a new date from one argument, as `new Date(value)` does:
    //~ a number is a time value, a date its time value, a string a parsed date
    //~ (date-fns' `constructFrom`, `new (date.constructor)(value)`). Its
    //~ `constructor` read answers no callable that builds a date.
// @smelt:if date
    if let SmeltUnknown::Object(object) = &value && object.contains_key("__smelt_date") && let [argument] = args.as_slice() {
        // The same recovery a date takes from any erased value, then a fresh
        // date: `new Date(other)` copies the time value, not the date.
        let time = match argument {
            SmeltUnknown::Bool(flag) => if *flag { 1.0 } else { 0.0 },
            other => SmeltDate::smelt_from_unknown(other.clone()).get_time(),
        };
        return SmeltDate::new(time).into_smelt_unknown();
    }
// @smelt:end
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
//~
//~ Walks `value`'s prototype chain comparing each link by REFERENCE against
//~ the target's own `prototype` object, exactly as `OrdinaryHasInstance` does.
//~ A non-callable target, or one whose `prototype` is not an object, answers
//~ `false` (JavaScript throws for the former; Smelt reports the absence).
//~
//~ The walk stops when `smelt_proto_accessor` stops advancing, so it
//~ terminates on the opaque `__smelt_proto:*` sentinels too.
fn smelt_instance_of_value(value: &SmeltUnknown, target: &SmeltUnknown) -> bool {
    if let SmeltUnknown::Object(class) = target && let Some(SmeltUnknown::String(covered)) = class.get("__smelt_class_value") {
        let SmeltUnknown::Object(instance) = value else { return false; };
        let Some(SmeltUnknown::String(name)) = instance.get("__smelt_class") else { return false; };
        if !covered.to_string_lossy().split(',').any(|class_name| name.eq_str(class_name)) { return false; }
        // A class with per-evaluation statics is a new class at every
        // evaluation of its declaration: its instances answer only to their
        // own evaluation's class value, told apart by the frame identity both
        // carry.
        return match class.get("__smelt_method:__smelt_class_frame") {
            Some(SmeltUnknown::Number(frame)) => matches!(instance.get("__smelt_method:__smelt_class_frame"), Some(SmeltUnknown::Number(own)) if own.to_bits() == frame.to_bits()),
            _ => true,
        };
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
//~ A class as a runtime value: callable through `__smelt_call` when its
//~ constructor is known, named, carrying the classes `instanceof` accepts
//~ (itself and its subclasses, comma separated), and holding its static
//~ methods as own properties, keyed as `smelt_property_key` keys them.
//~ Interned per class (`smelt_intern_class_value`), so every read of a class
//~ is one value, except a class with per-evaluation statics, which is a new
//~ class at every evaluation of its declaration.
fn smelt_class_value(name: &str, covered: &str, constructor: SmeltUnknown, statics: Vec<(String, SmeltUnknown)>) -> SmeltUnknown { let mut entries = Vec::from([("name".to_owned(), SmeltUnknown::String(name.into())), ("__smelt_class_value".to_owned(), SmeltUnknown::String(covered.into())), ("__smelt_call".to_owned(), constructor)]); let per_evaluation = statics.iter().any(|(key, _)| key == "__smelt_method:__smelt_class_frame"); entries.extend(statics); let value = SmeltUnknown::Object(SmeltObject::new(entries)); if per_evaluation || smelt_program_class(name).is_none() { value } else { smelt_intern_class_value(name, value) } }

//~ A class's MUTABLE static fields are live storage the program reads and
//~ writes directly (`Box.PI`), so a class value cannot hold copies of them:
//~ it carries a `__smelt_static_get` hook a read of a missing own property
//~ asks (`smelt_get_object_field`) and a `__smelt_static_set` hook a property
//~ write calls through this function. A write the hook stores (an own static
//~ of the class) leaves no own data property behind, so a later read, through
//~ this value or any other, asks the storage again.
//~ A class value's enumerable properties, in JavaScript's order. Static
//~ fields are own enumerable data properties of the class, in definition
//~ order; its `name` and its static methods are non-enumerable, and its
//~ ancestors' static fields are inherited through the constructor's
//~ prototype chain (`Object.getPrototypeOf(Sub) === Box`), so `for...in`
//~ yields them after the own keys while `Object.keys` does not. The emitter
//~ names the fields in a `__smelt_class_fields` marker (`[key, own]` pairs)
//~ and the static methods in `__smelt_class_methods`; values are read now
//~ through the static getter, so the view shows every write, through the
//~ class or through any value of it. Inherited fields are recorded under the
//~ `__smelt_proto:` prefix the own-key filters skip and `for...in` walks. A
//~ property the program added to the value later (`K.extra = 1`) follows
//~ the fields, as it does in JavaScript. The view is a snapshot: a write to
//~ it does not reach the class.
/// The enumerable view of a class value; `None` for any other object.
fn smelt_class_value_view(map: &SmeltObject) -> Option<SmeltRecord<JsString, SmeltUnknown>> {
    let Some(SmeltUnknown::Array(fields)) = map.get("__smelt_class_fields") else { return None };
    let methods = match map.get("__smelt_class_methods") { Some(SmeltUnknown::Array(methods)) => methods.into_vec(), _ => Vec::new() };
    let view = SmeltRecord::new();
    let mut inherited = Vec::new();
    for field in fields.into_vec() {
        let SmeltUnknown::Array(pair) = field else { continue };
        let pair = pair.into_vec();
        let (Some(SmeltUnknown::String(key)), Some(SmeltUnknown::Bool(own))) = (pair.first(), pair.get(1)) else { continue };
        if *own { view.insert(key.clone(), smelt_get_object_field(map, &*key.to_string_lossy())); } else { inherited.push(key.clone()); }
    }
    for (key, value) in map.iter() {
        let text = key.to_string_lossy();
        if text.starts_with("__smelt") || text == "name" || view.contains_key(&*key) || methods.iter().any(|method| matches!(method, SmeltUnknown::String(method) if *method == key)) { continue; }
        view.insert(key, value);
    }
    for key in inherited {
        if view.contains_key(&*key) { continue; }
        let value = smelt_get_object_field(map, &*key.to_string_lossy());
        view.insert(JsString::from(format!("__smelt_proto:{}", key.to_string_lossy())), value);
    }
    Some(view)
}

//~ An erased object read as a string-keyed dictionary (`Object.keys`,
//~ `Object.entries`, `for...in` over an erased value) is its own store,
//~ shared (`smelt_shared_record`), except a class value: its static fields
//~ live in their storage and its store holds its non-enumerable `name` and
//~ static methods, so it answers its enumerable view, read at this point.
//~ The emitter calls this only in a program with class values; any other
//~ program reads the store directly.
/// An erased object as a dictionary of its enumerable string-keyed properties.
fn smelt_object_dict_view(object: &SmeltObject) -> SmeltRecord<JsString, SmeltUnknown> { smelt_class_value_view(object).unwrap_or_else(|| object.smelt_shared_record()) }
//~ The entries a spread (`{ ...value }`) or an object rest copies out of an
//~ erased object: its own entries, in a new record carrying its id. A class
//~ value copies its own enumerable static fields (not the inherited ones a
//~ spread never copies), not its `name`, methods and markers.
/// The own entries a spread or object rest copies out of an erased object.
fn smelt_object_copy_record(object: SmeltObject) -> SmeltRecord<JsString, SmeltUnknown> { match smelt_class_value_view(&object) { Some(view) => SmeltRecord::with_id_from_entries(object.id, view.iter().filter(|(key, _)| !key.eq_str_prefix("__smelt_proto:"))), None => SmeltRecord::with_id_from_entries(object.id, object.into_iter()) } }

/// Forward a property write on a class value to the class's live static storage.
fn smelt_class_static_write(map: &SmeltObject, key: &str, value: &SmeltUnknown) { let stored = matches!(map.get("__smelt_method:__smelt_static_set"), Some(SmeltUnknown::Function(setter)) if matches!(setter(vec![SmeltUnknown::String(key.into()), value.clone()]), Ok(SmeltUnknown::Bool(true)))); if stored { map.remove(key); } }
// @smelt:item-end
