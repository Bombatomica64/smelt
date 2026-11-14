// Boxed primitives and erased iteration.

// `[Symbol.iterator]()` on an erased iterable may return a plain array,
// a string, nothing, or a live iterator object obeying the JavaScript
// iterator protocol (an erased generator or hand-written `{ next }`
// iterator). Only the protocol itself is observable across the erased
// boundary, so list extraction drains `next()` until `done`.
// @smelt:item smelt_unknown_iterator_items when unknown
/// Collect an erased `[Symbol.iterator]()` result into its item values.
fn smelt_unknown_iterator_items(source: SmeltUnknown) -> Vec<SmeltUnknown> { match source { SmeltUnknown::Null | SmeltUnknown::Undefined => Vec::new(), SmeltUnknown::Array(values) => values.into_vec(), SmeltUnknown::String(value) => value.code_point_strings().into_iter().map(SmeltUnknown::String).collect::<Vec<_>>(), SmeltUnknown::Object(object) => { let Some(SmeltUnknown::Function(next)) = object.get("next") else { panic!("unknown iterator did not return an iterable") }; let mut items = Vec::new(); loop { let step = next(vec![]).unwrap_or(SmeltUnknown::Undefined); let SmeltUnknown::Object(step) = step else { break }; if matches!(step.get("done"), Some(SmeltUnknown::Bool(true))) { break; } items.push(step.get("value").unwrap_or(SmeltUnknown::Undefined)); } items } _ => panic!("unknown iterator did not return an iterable") } }

//~ `Function.prototype.apply`/`call` on an erased receiver. A
//~ `SmeltUnknown::Function` receiver is not an object, so the plain
//~ erased-object field read (`smelt_get_object_field`) finds nothing and
//~ the value falls through to a null callback. This helper binds the
//~ callable directly: `apply` installs the `this` argument and spreads the
//~ trailing array, `call` installs `this` and forwards the remaining
//~ positional arguments. Object receivers keep the ordinary field read so
//~ user-defined `.apply`/`.call` properties still resolve — but a
//~ *callable object* (a JS function carrying properties, erased here to an
//~ object with a `__smelt_call` slot: a vitest mock, `partial.placeholder`,
//~ a debounced function) has no own `apply`/`call`/`bind` property, and in
//~ JavaScript those names resolve on `Function.prototype` of the
//~ underlying callable. Fall back to the `__smelt_call` slot when the own
//~ field read finds nothing, so `mock.apply(this, args)` actually invokes
//~ and records the call instead of yielding `undefined`.
/// Bind `Function.prototype.apply`/`call`/`bind` on an erased receiver, or read the field of an object receiver.
fn smelt_function_method(receiver: SmeltUnknown, method: &str) -> SmeltUnknown {
    match receiver {
        SmeltUnknown::Function(function) => {
            //~ `Function.prototype.bind(thisArg, ...bound)`: the bound function
            //~ installs `thisArg` and prepends the bound arguments.
// @smelt:if this_channel
            if method == "bind" {
                return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
                    let mut args = args.into_iter();
                    let this_arg = args.next().unwrap_or(SmeltUnknown::Undefined);
                    let bound: Vec<SmeltUnknown> = args.collect();
                    let target = function.clone();
                    Ok(smelt_bind_this(SmeltUnknown::Function(::std::rc::Rc::new(move |rest: Vec<SmeltUnknown>| { let mut all = bound.clone(); all.extend(rest); target(all) })), this_arg))
                }));
            }
// @smelt:end
            let method = method.to_owned();
            SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {
                let receiver_method = smelt_is_receiver_method(&function);
                let this_arg = args.first().map_or(SmeltUnknown::Undefined, Clone::clone);
                let forwarded: Vec<SmeltUnknown> = if method == "apply" { let mut forwarded = if receiver_method { ::std::vec![this_arg.clone()] } else { Vec::new() }; if let Some(SmeltUnknown::Array(values)) = args.get(1) { forwarded.extend(values.clone().into_vec()); } forwarded } else if receiver_method { args.into_iter().collect() } else { args.into_iter().skip(1).collect() };
                //~ The first argument is the `this` the callee observes for
                //~ this one call, as `SmeltErasedFunction::smelt_bind_this`
                //~ installs it; a program with no `this` read has no channel
                //~ and nothing to observe.
// @smelt:if this_channel
                let _smelt_this_guard = smelt_push_this(this_arg);
// @smelt:else
//=                 let _ = this_arg;
// @smelt:end
                function(forwarded)
            }))
        }
        SmeltUnknown::Object(map) => match smelt_get_object_field(&map, method) { SmeltUnknown::Undefined => match map.get("__smelt_call") { Some(callable @ SmeltUnknown::Function(_)) => smelt_function_method(callable, method), _ => SmeltUnknown::Undefined }, value => value },
        _ => SmeltUnknown::Undefined,
    }
}

//~ JavaScript `Object.prototype.valueOf` / boxed-primitive unwrapping.
//~
//~ `Object(1)` / `new Number(1)` erase to a marker object
//~ `{ __smelt_number: true, value: 1 }`, so `.valueOf()` used to read a
//~ missing own field, fall through to the null callback, and answer `null`.
//~ Deep-equality code branches on exactly this: es-toolkit `isEqualWith`
//~ compares a boxed and an unboxed primitive through
//~ `Object.is(a.valueOf(), b.valueOf())` after their
//~ `Object.prototype.toString` tags matched, and `cloneDeepWith` rebuilds a
//~ wrapper with `new Number(valueToClone.valueOf())`.
//~
//~ Unboxing is defined for every erased value, not just the wrappers: a
//~ primitive is its own `valueOf`, a Date unwraps to its epoch
//~ milliseconds, and any other object is itself (JS
//~ `Object.prototype.valueOf` returns the receiver).
/// Unwrap a boxed primitive wrapper (or a Date) to the primitive it holds.
fn smelt_unbox_primitive(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => { if map.contains_key("__smelt_number") || map.contains_key("__smelt_boolean") || map.contains_key("__smelt_string") || map.contains_key("__smelt_symbol") { return map.get("value").unwrap_or(SmeltUnknown::Undefined); } if let Some(millis @ SmeltUnknown::Number(_)) = map.get("__smelt_date") { return millis; } SmeltUnknown::Object(map) }, other => other } }
//~ Like every prototype method, `valueOf` takes its receiver from the CALL
//~ (`smelt_unbound_prototype_method`): a number answers
//~ `Number.prototype.valueOf`, an object's own override wins, and anything
//~ else answers one shared unbound `Object.prototype.valueOf`, which throws
//~ for a `null`/`undefined` receiver (`const v = x.valueOf; v()`).
/// Read `valueOf` off an erased receiver: its own override or the prototype's unbound method.
fn smelt_value_of_method(receiver: SmeltUnknown) -> SmeltUnknown {
    match receiver {
        SmeltUnknown::Object(map) => { if let own @ SmeltUnknown::Function(_) = smelt_get_object_field(&map, "valueOf") { return own; } }
        number @ SmeltUnknown::Number(_) => { if let Some(method) = smelt_unbound_prototype_method(SmeltPrototypeFamily::Number, &number, "valueOf") { return method; } }
// @smelt:if !this_channel
//=         other => { let unboxed = smelt_unbox_primitive(other); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(unboxed.clone()))); }
// @smelt:end
        _ => {}
    }
// @smelt:if !this_channel
//=     SmeltUnknown::Undefined
// @smelt:end
// @smelt:if this_channel
    thread_local! { static SMELT_OBJECT_VALUE_OF: SmeltUnknown = { let method: SmeltDynCallable = ::std::rc::Rc::new(|_args: Vec<SmeltUnknown>| match smelt_this() { SmeltUnknown::Null | SmeltUnknown::Undefined => Err(smelt_prototype_type_error("Cannot convert undefined or null to object".to_owned())), receiver => Ok(smelt_unbox_primitive(receiver)) }); smelt_register_function_length(&method, 0.0); SmeltUnknown::Function(method) }; }
    SMELT_OBJECT_VALUE_OF.with(Clone::clone)
// @smelt:end
}
//~ `Object(value)` called as a FUNCTION (not `new`) boxes a primitive and
//~ passes objects through unchanged. `Object(null)` / `Object(undefined)`
//~ yield a fresh empty object. The wrapper shapes match what
//~ `new Number(..)` / `new Boolean(..)` / `new String(..)` build, so the
//~ `Object.prototype.toString` tag arms and `smelt_unbox_primitive` treat
//~ both spellings identically.
//~ Strings are deliberately NOT boxed, mirroring `new String(x)`, which
//~ lowers to the plain string: a
//~ string wrapper would have to re-expose the whole `String.prototype`
//~ surface — `length`, character indexing, and every string method — on a
//~ marker object, and Smelt models a JS string as a Rust `String`. Keeping
//~ both spellings unboxed keeps the two consistent. The visible cost is
//~ that `new String(x) === x` reads as `true` here and `false` in JS.
/// Box a primitive the way `Object(value)` does; objects and strings pass through.
fn smelt_box_value(value: SmeltUnknown) -> SmeltUnknown { let (marker, boxed) = match value { SmeltUnknown::Number(_) => ("__smelt_number", value), SmeltUnknown::Bool(_) => ("__smelt_boolean", value), SmeltUnknown::Symbol(_) => ("__smelt_symbol", value), SmeltUnknown::Null | SmeltUnknown::Undefined => return SmeltUnknown::Object(SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new())), other => return other }; let fields = Vec::from([(marker.to_owned(), SmeltUnknown::Bool(true)), ("value".to_owned(), boxed)]); SmeltUnknown::Object(SmeltObject::new(fields)) }

// @smelt:item-end
