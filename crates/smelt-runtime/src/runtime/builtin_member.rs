// Runtime support for reading a JavaScript builtin's members as values.
//
// `Array.prototype.slice` is a property read, not a call: JavaScript answers it
// with a function object, and code stores that object, probes it with `typeof`,
// reads its `length`, and eventually invokes it through `.call`/`.apply`. The
// generated runtime therefore needs three things, all emitted here:
//
// * a value for `<Builtin>.prototype` — an interned marker record, so the
//   member read has somewhere to land and `Array.prototype === Array.prototype`
//   holds;
// * one interned callable per modeled `(builtin, kind, member)` triple, carrying
//   the member's JavaScript `length` and a stable identity;
// * a dispatcher that applies the member to its receiver.
//
// The set of modeled members is [`smelt_stdlib::BUILTIN_MEMBER_FUNCTIONS`] — the
// same registry the arity comes from — so a member Smelt cannot run is simply
// absent and reads as `undefined` rather than becoming a callable that lies.
//
// A prototype method consumes its receiver as the leading argument, which is
// what `Function.prototype.call`/`.apply` must feed it. Those two are generic
// over any callable, so the receiver-consuming callables register themselves in
// a side table (`smelt_register_receiver_method`) and `smelt_function_method`
// consults it instead of unconditionally dropping the receiver.

// Emit the `(class, kind, member, arity)` table and its lookup.
// @smelt:item SMELT_BUILTIN_MEMBERS when unknown
/// Every modeled builtin member: `(class, kind, member, length, key)`.
const SMELT_BUILTIN_MEMBERS: [(&str, &str, &str, f64, &'static str); 17] = [("Array", "prototype", "slice", 2.0, "Array.prototype.slice"), ("Array", "prototype", "concat", 1.0, "Array.prototype.concat"), ("Array", "prototype", "indexOf", 1.0, "Array.prototype.indexOf"), ("Array", "prototype", "lastIndexOf", 1.0, "Array.prototype.lastIndexOf"), ("Array", "prototype", "includes", 1.0, "Array.prototype.includes"), ("Array", "prototype", "join", 1.0, "Array.prototype.join"), ("Array", "static", "isArray", 1.0, "Array.isArray"), ("Math", "static", "random", 0.0, "Math.random"), ("Math", "static", "abs", 1.0, "Math.abs"), ("Math", "static", "floor", 1.0, "Math.floor"), ("Math", "static", "ceil", 1.0, "Math.ceil"), ("Math", "static", "round", 1.0, "Math.round"), ("Math", "static", "trunc", 1.0, "Math.trunc"), ("Math", "static", "sign", 1.0, "Math.sign"), ("Math", "static", "sqrt", 1.0, "Math.sqrt"), ("Math", "static", "max", 2.0, "Math.max"), ("Math", "static", "min", 2.0, "Math.min")];
/// Look one modeled builtin member up, returning its `length` and dispatch key.
fn smelt_builtin_member_entry(class: &str, kind: &str, member: &str) -> Option<(f64, &'static str)> { SMELT_BUILTIN_MEMBERS.into_iter().find(|(entry_class, entry_kind, entry_member, _, _)| *entry_class == class && *entry_kind == kind && *entry_member == member).map(|(_, _, _, length, key)| (length, key)) }

//~ Emit the interned `<Builtin>.prototype` marker record.
/// The interned value of `<Builtin>.prototype`.
///
/// JavaScript has exactly one prototype object per builtin, so the record is
/// cached by class name: `Array.prototype === Array.prototype` holds, and a
/// member read off it resolves through the modeled-member registry. The record
/// carries only the marker, so it never enumerates as data.
thread_local! { static SMELT_BUILTIN_PROTOTYPES: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }
fn smelt_builtin_prototype_object(class: &str) -> SmeltUnknown { SMELT_BUILTIN_PROTOTYPES.with(|cache| cache.borrow_mut().entry(class.to_owned()).or_insert_with(|| SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_builtin_prototype".to_owned(), SmeltUnknown::String(class.into()))])))).clone()) }

//~ Emit the interned per-member callable and the receiver-method side table.
/// Callables that consume their receiver as the leading argument.
///
/// A prototype method's `this` is its receiver, and Smelt passes it as the
/// first argument. `Function.prototype.call`/`.apply` drop the receiver for an
/// ordinary erased callable (which has no `this` channel), so they must not do
/// that for these: membership here is the difference.
///
/// The flag lives in the callable's entry of the bounded callable table.
fn smelt_register_receiver_method<T: ?Sized + 'static>(function: &::std::rc::Rc<T>) { smelt_update_callable(function, |meta| meta.receiver_method = true); }
fn smelt_is_receiver_method<T: ?Sized + 'static>(function: &::std::rc::Rc<T>) -> bool { smelt_read_callable(function, |meta| Some(meta.receiver_method)).unwrap_or(false) }

/// Read a modeled member off a builtin, or `None` when it is not modeled.
///
/// The callable is interned per member so two reads are the same function
/// value, carries the member's JavaScript `length`, and gets the member's key
/// as its identity — the same treatment `Object.prototype`'s members get.
thread_local! { static SMELT_BUILTIN_MEMBER_VALUES: ::std::cell::RefCell<::std::collections::HashMap<&'static str, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }
fn smelt_builtin_member_value(class: &str, kind: &str, member: &str) -> Option<SmeltUnknown> {
    let (length, key) = smelt_builtin_member_entry(class, kind, member)?;
    let receiver_method = kind == "prototype";
    Some(SMELT_BUILTIN_MEMBER_VALUES.with(|values| values.borrow_mut().entry(key).or_insert_with(|| { let function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_builtin_member_apply(key, args))); smelt_link_function_identity_key(&function, smelt_method_identity(key)); smelt_register_function_length(&function, length); if receiver_method { smelt_register_receiver_method(&function); } SmeltUnknown::Function(function) }).clone()))
}

//~ Emit the member dispatcher and its receiver-coercion helpers.
/// The receiver of a `String.prototype` method as a Rust string.
///
/// A boxed `String` object unboxes first, so `new String('ab')` and `'ab'`
/// behave the same through a prototype method, as they do in JavaScript.
fn smelt_builtin_receiver_text(value: &SmeltUnknown) -> String { match smelt_unbox_primitive(value.clone()) { SmeltUnknown::String(text) => text.to_string(), other => other.to_string() } }
/// The receiver of an `Array.prototype` method as its element vector.
fn smelt_builtin_receiver_elements(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> { match value { SmeltUnknown::Array(values) => Some(values.clone().into_vec()), _ => None } }
/// Resolve a JavaScript relative index argument against a length.
///
/// `undefined` takes the caller's default, a negative counts from the end, and
/// anything past either end clamps — the `slice` index rules, shared by the
/// string and array arms so they cannot drift apart.
fn smelt_builtin_relative_index(value: &SmeltUnknown, len: usize, default: usize) -> usize { match value { SmeltUnknown::Undefined => default, other => { let raw = match other { SmeltUnknown::Number(value) => *value, _ => 0.0 }; if raw.is_nan() { 0 } else if raw < 0.0 { let from_end = len as f64 + raw; if from_end < 0.0 { 0 } else { from_end as usize } } else { (raw as usize).min(len) } } } }

/// Apply one modeled builtin member to its arguments.
///
/// For a prototype method the leading argument is the receiver; for a static
/// function there is no receiver and the arguments start at index zero. Every
/// arm is keyed by the registry's dispatch key, so adding a member is a table
/// entry plus its arm and nothing else.
fn smelt_builtin_member_apply(key: &str, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let receiver = args.first().map_or(SmeltUnknown::Undefined, Clone::clone);
    let first = args.get(1).map_or(SmeltUnknown::Undefined, Clone::clone);
    let second = args.get(2).map_or(SmeltUnknown::Undefined, Clone::clone);
    match key {
        //~ Array.prototype
        "Array.prototype.slice" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let len = values.len(); let start = smelt_builtin_relative_index(&first, len, 0); let end = smelt_builtin_relative_index(&second, len, len); SmeltUnknown::Array(values.into_iter().skip(start).take(end.saturating_sub(start)).collect::<Vec<_>>().into()) } None => SmeltUnknown::Undefined }
        "Array.prototype.concat" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let mut result = values; for argument in args.into_iter().skip(1) { match argument { SmeltUnknown::Array(items) => result.extend(items.into_vec()), other => result.push(other) } } SmeltUnknown::Array(result.into()) } None => SmeltUnknown::Undefined }
        "Array.prototype.indexOf" | "Array.prototype.lastIndexOf" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let mut found: f64 = -1.0; for (index, item) in values.iter().enumerate() { if item.same_js_key(&first) { found = index as f64; if key == "Array.prototype.indexOf" { break; } } } SmeltUnknown::Number(found) } None => SmeltUnknown::Undefined }
        "Array.prototype.includes" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => SmeltUnknown::Bool(values.iter().any(|item| item.same_js_key(&first))), None => SmeltUnknown::Undefined }
        "Array.prototype.join" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let separator = match &first { SmeltUnknown::Undefined => ",".to_owned(), other => smelt_builtin_receiver_text(other) }; SmeltUnknown::String(values.into_iter().map(|item| match item { SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), other => smelt_builtin_receiver_text(&other) }).collect::<Vec<_>>().join(&separator).into()) } None => SmeltUnknown::Undefined }
        //~ Statics
        "Array.isArray" => SmeltUnknown::Bool(matches!(receiver, SmeltUnknown::Array(_))),
        //~ `Math` statics: no receiver, so `receiver` is the first argument. Each
        //~ argument goes through JS `ToNumber` (`smelt_unknown_to_number`).
        //~ `Math.random` draws from std's per-instance randomly keyed `RandomState`
        //~ rather than the `rand` crate, so the erased runtime (always emitted)
        //~ does not pull a dependency into every generated crate.
        "Math.random" => { use ::std::hash::BuildHasher; let bits = ::std::collections::hash_map::RandomState::new().hash_one(0_u64); SmeltUnknown::Number((bits >> 11) as f64 / (1_u64 << 53) as f64) }
        "Math.abs" => SmeltUnknown::Number(smelt_unknown_to_number(&receiver).abs()),
        "Math.floor" => SmeltUnknown::Number(smelt_unknown_to_number(&receiver).floor()),
        "Math.ceil" => SmeltUnknown::Number(smelt_unknown_to_number(&receiver).ceil()),
        "Math.round" => SmeltUnknown::Number((smelt_unknown_to_number(&receiver) + 0.5).floor()),
        "Math.trunc" => SmeltUnknown::Number(smelt_unknown_to_number(&receiver).trunc()),
        "Math.sign" => { let value = smelt_unknown_to_number(&receiver); SmeltUnknown::Number(if value.is_nan() || value == 0.0 { value } else { value.signum() }) }
        "Math.sqrt" => SmeltUnknown::Number(smelt_unknown_to_number(&receiver).sqrt()),
        "Math.max" | "Math.min" => { let is_max = key == "Math.max"; let mut result = if is_max { f64::NEG_INFINITY } else { f64::INFINITY }; for argument in &args { let value = smelt_unknown_to_number(argument); if value.is_nan() { result = f64::NAN; break; } if (is_max && value > result) || (!is_max && value < result) { result = value; } } SmeltUnknown::Number(result) }
        _ => SmeltUnknown::Undefined,
    }
}

// @smelt:item-end
