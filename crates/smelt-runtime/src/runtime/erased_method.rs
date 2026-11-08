// Runtime model of `Array.prototype` / `String.prototype` methods on erased values.
//
// **Dynamic boundary.** Everything here operates on `SmeltUnknown` because it
// runs exactly where the static shape is gone: a method call whose receiver is
// typed `any` (or is a property read off one, a type parameter, an erased
// class) is lowered as a property read through `smelt_get_unknown_field`
// followed by the erased call ABI. Which prototype answers the read is decided
// by the receiver's RUNTIME tag, as JavaScript's own property lookup decides
// it, so no concrete type, generated union or scoped generic can stand in for
// the receiver at the emit site: the same `value.slice(1)` is a list slice for
// one caller and a string slice for the next. A statically typed receiver
// never reaches this module — typed lists and strings lower to their own
// typed operations before codegen ever sees an erased read (see
// `emitter/place.rs`, whose erased-read arm is gated on the receiver's type).
//
// What the module emits:
//
// * `smelt_array_prototype_member` / `smelt_string_prototype_member` — the
//   member READ: a method name from the shared tables in
//   [`smelt_stdlib::erased_prototype_methods`] becomes a callable bound to the
//   receiver, anything else is `None` (so the caller falls back to
//   `undefined`). The bound callable captures the receiver handle, and an
//   erased array's handle shares its storage, so `push`/`sort`/`splice`
//   mutate the caller's array exactly as JavaScript does.
// * `smelt_array_prototype_apply` / `smelt_string_prototype_apply` — the
//   member CALL, one arm per method, with JavaScript's argument rules:
//   callbacks receive `(element, index, array)` (`reduce` receives the
//   accumulator first), relative positions count from the end, a missing
//   callback throws `TypeError: undefined is not a function`, and an empty
//   `reduce` without an initial value throws its `TypeError`.
// * the small coercion helpers those arms share (`ToBoolean`,
//   `ToIntegerOrInfinity`, `ToString`, position clamping) and the `TypeError`
//   thrower the erased call ABI also uses for a non-callable callee
//   (`smelt_throw_not_callable`).
//
// Two deliberate simplifications, both documented at their arms: a bound
// method read does not register a canonical function identity or `length`
// (each read would otherwise grow the process-wide identity registries by
// one entry per CALL, since every `arr.map(f)` performs a read), and string
// positions count Unicode scalar values, the same unit every other erased
// string operation in the prelude (`length`, indexing) already uses.

// Write a block of generated Rust source one line at a time.
//
// The generated helpers are kept as readable multi-line Rust text in this
// module (rather than one `writer.line` per statement) so the emitted code
// can be reviewed as the Rust it is.
// @smelt:item SmeltDynCallable when unknown
/// The callable an erased value holds: a function, or a callable object's `__smelt_call`.
type SmeltDynCallable = ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>;

/// JavaScript `ToBoolean` of an erased value.
fn smelt_erased_truthy(value: &SmeltUnknown) -> bool {
    match value {
        SmeltUnknown::Null | SmeltUnknown::Undefined => false,
        SmeltUnknown::Bool(value) => *value,
        SmeltUnknown::Number(value) => *value != 0.0 && !value.is_nan(),
        SmeltUnknown::String(value) => !value.is_empty(),
        _ => true,
    }
}

/// JavaScript `ToIntegerOrInfinity` of an erased argument (`NaN` and `undefined` are `0`).
fn smelt_erased_integer(value: &SmeltUnknown) -> f64 {
    let number = smelt_unknown_to_number(value);
    if number.is_nan() { 0.0 } else { number.trunc() }
}

/// A RELATIVE position argument (`slice`, `splice`, `fill`): negative counts from the end.
///
/// A missing or `undefined` argument takes `default`; the result is clamped to `0..=len`.
fn smelt_erased_relative_position(value: Option<&SmeltUnknown>, len: usize, default: usize) -> usize {
    match value {
        None | Some(SmeltUnknown::Undefined) => default,
        Some(value) => {
            let relative = smelt_erased_integer(value);
            let len = len as f64;
            (if relative < 0.0 { (len + relative).max(0.0) } else { relative.min(len) }) as usize
        }
    }
}

/// A CLAMPED position argument (`substring`, `startsWith`, string `indexOf`): negatives are `0`.
///
/// A missing or `undefined` argument takes `default`; the result is clamped to `0..=len`.
fn smelt_erased_clamped_position(value: Option<&SmeltUnknown>, len: usize, default: usize) -> usize {
    match value {
        None | Some(SmeltUnknown::Undefined) => default,
        Some(value) => smelt_erased_integer(value).clamp(0.0, len as f64) as usize,
    }
}

/// The in-range index `at(index)` selects, or `None` when it falls outside `0..len`.
fn smelt_erased_at_index(value: Option<&SmeltUnknown>, len: usize) -> Option<usize> {
    let relative = value.map_or(0.0, smelt_erased_integer);
    let index = if relative < 0.0 { len as f64 + relative } else { relative };
    (index >= 0.0 && index < len as f64).then_some(index as usize)
}

/// JavaScript `ToString` of an erased value; an array joins its elements with `,`.
fn smelt_erased_to_js_string(value: &SmeltUnknown) -> JsString {
    match value {
        SmeltUnknown::String(text) => text.clone(),
        SmeltUnknown::Array(values) => JsString::join(values.iter().map(|item| match item { SmeltUnknown::Null | SmeltUnknown::Undefined => JsString::new(), other => smelt_erased_to_js_string(&other) }), &JsString::from(",")),
        other => JsString::from(smelt_builtin_receiver_text(other)),
    }
}

/// The callable an erased value holds, if it is one.
fn smelt_erased_callable(value: &SmeltUnknown) -> Option<SmeltDynCallable> {
    match value {
        SmeltUnknown::Function(function) => Some(function.clone()),
        SmeltUnknown::Object(object) => match object.get("__smelt_call") { Some(SmeltUnknown::Function(function)) => Some(function), _ => None },
        _ => None,
    }
}

/// The callback argument of an iteration method, or JavaScript's `TypeError` when it is not callable.
fn smelt_erased_callback(value: Option<&SmeltUnknown>) -> SmeltDynCallable {
    let value = value.map_or(SmeltUnknown::Undefined, SmeltUnknown::clone);
    match smelt_erased_callable(&value) {
        Some(function) => function,
        None => smelt_throw_not_callable(&match &value { SmeltUnknown::Object(_) => "#<Object>".to_owned(), other => smelt_erased_to_js_string(other).to_string_lossy() }),
    }
}

/// Invoke an erased callable, re-raising what it throws.
fn smelt_erased_invoke(function: &SmeltDynCallable, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    (function)(args).unwrap_or_else(|error| smelt_panic_throw(error))
}

//~ Emit the method-name tables, rendered from the shared stdlib registry.
//~
//~ The frontend's ambiguity rule and this lookup read the same rows, so a
//~ method is dispatched at run time exactly when the frontend defers it.
/// `Array.prototype` methods an erased array answers (see `smelt_array_prototype_apply`).
const SMELT_ERASED_ARRAY_METHODS: [&str; 32] = ["map", "filter", "forEach", "reduce", "reduceRight", "some", "every", "find", "findIndex", "findLast", "findLastIndex", "includes", "indexOf", "lastIndexOf", "join", "slice", "concat", "at", "flat", "flatMap", "push", "pop", "shift", "unshift", "splice", "reverse", "sort", "fill", "keys", "values", "entries", "toString"];
/// `String.prototype` methods an erased string answers (see `smelt_string_prototype_apply`).
const SMELT_ERASED_STRING_METHODS: [&str; 21] = ["at", "charAt", "charCodeAt", "codePointAt", "concat", "endsWith", "includes", "indexOf", "lastIndexOf", "padEnd", "padStart", "repeat", "slice", "startsWith", "substring", "toLowerCase", "toUpperCase", "trim", "trimEnd", "trimStart", "toString"];

/// Read `Array.prototype[field]` off an erased array, bound to that array.
///
/// `None` when `field` is not a modeled array method, so the caller answers
/// `undefined`. The receiver handle shares the array's storage, so a mutating
/// method called through the bound value mutates the caller's array. No
/// canonical identity or `length` is registered: every `arr.map(f)` is a read,
/// and registering would grow the identity registries once per call.
fn smelt_array_prototype_member(receiver: &SmeltArray, field: &str) -> Option<SmeltUnknown> {
    let name: &'static str = SMELT_ERASED_ARRAY_METHODS.into_iter().find(|name| *name == field)?;
    let receiver = receiver.clone();
    Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_array_prototype_apply(&receiver, name, args)))))
}

/// Read `String.prototype[field]` off an erased string, bound to that string.
///
/// `None` when `field` is not a modeled string method (see the array twin).
fn smelt_string_prototype_member(receiver: &JsString, field: &str) -> Option<SmeltUnknown> {
    let name: &'static str = SMELT_ERASED_STRING_METHODS.into_iter().find(|name| *name == field)?;
    let receiver = receiver.clone();
    Some(SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_string_prototype_apply(&receiver, name, args)))))
}

/// Read a property off an erased string: `length`, a character index, or a `String.prototype` method.
fn smelt_get_string_field(text: &JsString, field: &str) -> SmeltUnknown {
    if field == "length" { return SmeltUnknown::Number(smelt_units_to_number(text.len())); }
    if let Ok(index) = field.parse::<usize>() { return text.char_at(index).map_or(SmeltUnknown::Undefined, SmeltUnknown::String); }
    smelt_string_prototype_member(text, field).unwrap_or(SmeltUnknown::Undefined)
}

/// Flatten `items` into `out`, descending `depth` levels of nested arrays (`Array.prototype.flat`).
fn smelt_erased_flatten_into(items: Vec<SmeltUnknown>, depth: f64, out: &mut Vec<SmeltUnknown>) {
    for item in items {
        match item {
            SmeltUnknown::Array(nested) if depth >= 1.0 => smelt_erased_flatten_into(nested.into_vec(), depth - 1.0, out),
            other => out.push(other),
        }
    }
}

/// Apply `Array.prototype[name]` to an erased array receiver.
///
/// Iteration reads the array LIVE (`receiver.get(index)`) over the length it had
/// when the call began, as the specification's algorithms do, so a callback that
/// mutates the array observes the same values it would in JavaScript. Callbacks
/// receive `(element, index, array)`.
fn smelt_array_prototype_apply(receiver: &SmeltArray, name: &str, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let arg = |index: usize| args.get(index).map_or(SmeltUnknown::Undefined, SmeltUnknown::clone);
    let array = || SmeltUnknown::Array(receiver.clone());
    let len = receiver.len();
    let item = |index: usize| receiver.get(index).unwrap_or(SmeltUnknown::Undefined);
    let call = |function: &SmeltDynCallable, index: usize| smelt_erased_invoke(function, vec![item(index), SmeltUnknown::Number(index as f64), array()]);
    match name {
        "map" => { let function = smelt_erased_callback(args.first()); SmeltUnknown::Array((0..len).map(|index| call(&function, index)).collect::<Vec<_>>().into()) }
        "filter" => { let function = smelt_erased_callback(args.first()); SmeltUnknown::Array((0..len).filter_map(|index| { let value = item(index); smelt_erased_truthy(&call(&function, index)).then_some(value) }).collect::<Vec<_>>().into()) }
        "forEach" => { let function = smelt_erased_callback(args.first()); for index in 0..len { call(&function, index); } SmeltUnknown::Undefined }
        "some" => { let function = smelt_erased_callback(args.first()); SmeltUnknown::Bool((0..len).any(|index| smelt_erased_truthy(&call(&function, index)))) }
        "every" => { let function = smelt_erased_callback(args.first()); SmeltUnknown::Bool((0..len).all(|index| smelt_erased_truthy(&call(&function, index)))) }
        "find" | "findIndex" | "findLast" | "findLastIndex" => {
            let function = smelt_erased_callback(args.first());
            let from_end = name.starts_with("findLast");
            let mut indices: Box<dyn Iterator<Item = usize>> = if from_end { Box::new((0..len).rev()) } else { Box::new(0..len) };
            let found = indices.find(|index| smelt_erased_truthy(&call(&function, *index)));
            if name.ends_with("Index") { SmeltUnknown::Number(found.map_or(-1.0, |index| index as f64)) } else { found.map_or(SmeltUnknown::Undefined, item) }
        }
        "reduce" | "reduceRight" => {
            let function = smelt_erased_callback(args.first());
            let mut indices: Box<dyn Iterator<Item = usize>> = if name == "reduceRight" { Box::new((0..len).rev()) } else { Box::new(0..len) };
            let mut accumulator = if args.len() >= 2 { arg(1) } else { match indices.next() { Some(index) => item(index), None => smelt_throw_builtin_error("TypeError", "Reduce of empty array with no initial value".to_owned()) } };
            for index in indices { accumulator = smelt_erased_invoke(&function, vec![accumulator, item(index), SmeltUnknown::Number(index as f64), array()]); }
            accumulator
        }
        "includes" => { let needle = arg(0); let start = smelt_erased_relative_position(args.get(1), len, 0); SmeltUnknown::Bool((start..len).any(|index| item(index).same_js_key(&needle))) }
        "indexOf" => { let needle = arg(0); let start = smelt_erased_relative_position(args.get(1), len, 0); SmeltUnknown::Number((start..len).find(|index| item(*index).js_strict_eq(&needle)).map_or(-1.0, |index| index as f64)) }
        "lastIndexOf" => {
            let needle = arg(0);
            let last = match args.get(1) { None => len as f64 - 1.0, Some(value) => { let from = smelt_erased_integer(value); if from < 0.0 { len as f64 + from } else { from.min(len as f64 - 1.0) } } };
            if last < 0.0 { SmeltUnknown::Number(-1.0) } else { SmeltUnknown::Number((0..=last as usize).rev().find(|index| item(*index).js_strict_eq(&needle)).map_or(-1.0, |index| index as f64)) }
        }
        "join" | "toString" => {
            let separator = match args.first() { Some(value) if name == "join" && !matches!(value, SmeltUnknown::Undefined) => smelt_erased_to_js_string(value), _ => JsString::from(",") };
            SmeltUnknown::String(JsString::join((0..len).map(|index| match item(index) { SmeltUnknown::Null | SmeltUnknown::Undefined => JsString::new(), other => smelt_erased_to_js_string(&other) }), &separator))
        }
        "slice" => { let start = smelt_erased_relative_position(args.first(), len, 0); let end = smelt_erased_relative_position(args.get(1), len, len); SmeltUnknown::Array((start..end.max(start)).map(item).collect::<Vec<_>>().into()) }
        "concat" => { let mut result = receiver.clone().into_vec(); for argument in args { match argument { SmeltUnknown::Array(items) => result.extend(items.into_vec()), other => result.push(other) } } SmeltUnknown::Array(result.into()) }
        "at" => smelt_erased_at_index(args.first(), len).map_or(SmeltUnknown::Undefined, item),
        "flat" => { let depth = match args.first() { None | Some(SmeltUnknown::Undefined) => 1.0, Some(value) => smelt_erased_integer(value) }; let mut out = Vec::new(); smelt_erased_flatten_into(receiver.clone().into_vec(), depth, &mut out); SmeltUnknown::Array(out.into()) }
        "flatMap" => { let function = smelt_erased_callback(args.first()); let mapped = (0..len).map(|index| call(&function, index)).collect::<Vec<_>>(); let mut out = Vec::new(); smelt_erased_flatten_into(mapped, 1.0, &mut out); SmeltUnknown::Array(out.into()) }
        "push" => { for argument in args { receiver.push(argument); } SmeltUnknown::Number(receiver.len() as f64) }
        "pop" => { let storage = receiver.storage(); let popped = storage.borrow_mut().pop(); popped.unwrap_or(SmeltUnknown::Undefined) }
        "shift" => { let storage = receiver.storage(); let shifted = { let mut values = storage.borrow_mut(); if values.is_empty() { None } else { Some(values.remove(0)) } }; shifted.unwrap_or(SmeltUnknown::Undefined) }
        "unshift" => { let storage = receiver.storage(); storage.borrow_mut().splice(0..0, args).for_each(drop); SmeltUnknown::Number(receiver.len() as f64) }
        "splice" => {
            let start = smelt_erased_relative_position(args.first(), len, 0);
            let delete_count = match args.len() { 0 => 0, 1 => len - start, _ => { let count = smelt_erased_integer(&arg(1)); if count > 0.0 { (count as usize).min(len - start) } else { 0 } } };
            let storage = receiver.storage();
            let removed = storage.borrow_mut().splice(start..start + delete_count, args.into_iter().skip(2)).collect::<Vec<_>>();
            SmeltUnknown::Array(removed.into())
        }
        "reverse" => { receiver.storage().borrow_mut().reverse(); array() }
        "sort" => {
            let compare = match args.first() { None | Some(SmeltUnknown::Undefined) => None, Some(value) => match smelt_erased_callable(value) { Some(function) => Some(function), None => smelt_throw_builtin_error("TypeError", "The comparison function must be either a function or undefined".to_owned()) } };
            // Sort a snapshot: the comparator is user code and may read the
            // array, so no borrow of the storage may be held across it.
            let (mut defined, undefined): (Vec<SmeltUnknown>, Vec<SmeltUnknown>) = receiver.clone().into_vec().into_iter().partition(|value| !matches!(value, SmeltUnknown::Undefined));
            defined.sort_by(|left, right| match &compare {
                Some(function) => { let order = smelt_unknown_to_number(&smelt_erased_invoke(function, vec![left.clone(), right.clone()])); if order < 0.0 { ::std::cmp::Ordering::Less } else if order > 0.0 { ::std::cmp::Ordering::Greater } else { ::std::cmp::Ordering::Equal } }
                None => smelt_erased_to_js_string(left).cmp(&smelt_erased_to_js_string(right)),
            });
            defined.extend(undefined);
            receiver.replace_all(defined);
            array()
        }
        "fill" => { let value = arg(0); let start = smelt_erased_relative_position(args.get(1), len, 0); let end = smelt_erased_relative_position(args.get(2), len, len); for index in start..end { receiver.set_index(index, value.clone()); } array() }
        "keys" | "values" | "entries" => smelt_erased_array_iterator(receiver.clone(), if name == "keys" { 0 } else if name == "values" { 1 } else { 2 }),
        _ => SmeltUnknown::Undefined,
    }
}

/// A live JavaScript array iterator over an erased array.
///
/// `kind` selects what each step yields: `0` the index (`keys`), `1` the element
/// (`values`), `2` an `[index, element]` pair (`entries`). The iterator obeys the
/// protocol the erased runtime drives (`next()` returning `{ value, done }`, see
/// `smelt_unknown_iterator_items`) and is itself iterable through
/// `__smelt_symbol_iterator`, so `for...of`, spread and `Array.from` accept it.
/// It reads the array live and, once done, stays done, as JavaScript's does.
fn smelt_erased_array_iterator(receiver: SmeltArray, kind: u8) -> SmeltUnknown {
    let position = ::std::rc::Rc::new(::std::cell::Cell::new(0_usize));
    let next: SmeltDynCallable = ::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| {
        let index = position.get();
        let step = if index < receiver.len() {
            position.set(index + 1);
            let element = receiver.get(index).unwrap_or(SmeltUnknown::Undefined);
            let value = match kind { 0 => SmeltUnknown::Number(index as f64), 1 => element, _ => SmeltUnknown::Array(vec![SmeltUnknown::Number(index as f64), element].into()) };
            vec![("value".to_owned(), value), ("done".to_owned(), SmeltUnknown::Bool(false))]
        } else {
            position.set(usize::MAX);
            vec![("value".to_owned(), SmeltUnknown::Undefined), ("done".to_owned(), SmeltUnknown::Bool(true))]
        };
        Ok(SmeltUnknown::Object(SmeltObject::new(step)))
    });
    let iterable_next = next.clone();
    let iterator: SmeltDynCallable = ::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Object(SmeltObject::new(vec![("next".to_owned(), SmeltUnknown::Function(iterable_next.clone()))]))));
    SmeltUnknown::Object(SmeltObject::new(vec![("next".to_owned(), SmeltUnknown::Function(next)), ("__smelt_symbol_iterator".to_owned(), SmeltUnknown::Function(iterator))]))
}

/// Pad `text` to `target` code units with repetitions of `fill` (`padStart`/`padEnd`).
fn smelt_erased_padding(len: usize, target: f64, fill: &JsStr) -> JsString {
    let target_units = smelt_units_from_number(target);
    if fill.is_empty() || target_units <= len { return JsString::new(); }
    let needed = target_units - len;
    fill.repeat(needed.div_ceil(fill.len())).slice(0, needed)
}

/// The code unit index an erased `charAt`-style argument names in a string of
/// `len` units: `None` when it is negative or past the end.
fn smelt_erased_unit_index(value: &SmeltUnknown, len: usize) -> Option<usize> {
    let index = smelt_erased_integer(value);
    if index < 0.0 { return None; }
    Some(smelt_units_from_number(index)).filter(|unit| *unit < len)
}

/// Apply `String.prototype[name]` to an erased string receiver.
///
/// Positions count UTF-16 code units, as JavaScript's do.
fn smelt_string_prototype_apply(text: &JsStr, name: &str, args: Vec<SmeltUnknown>) -> SmeltUnknown {
    let arg = |index: usize| args.get(index).map_or(SmeltUnknown::Undefined, SmeltUnknown::clone);
    let arg_text = |index: usize| smelt_erased_to_js_string(&arg(index));
    let len = text.len();
    let string = SmeltUnknown::String;
    match name {
        "at" => smelt_erased_at_index(args.first(), len).map_or(SmeltUnknown::Undefined, |index| string(text.slice(index, index + 1))),
        "charAt" => string(smelt_erased_unit_index(&arg(0), len).map_or_else(JsString::new, |index| text.slice(index, index + 1))),
        "charCodeAt" | "codePointAt" => smelt_erased_unit_index(&arg(0), len).map_or_else(
            || if name == "charCodeAt" { SmeltUnknown::Number(f64::NAN) } else { SmeltUnknown::Undefined },
            |index| { let code = if name == "charCodeAt" { text.unit_at(index).map_or(0, u32::from) } else { text.code_point_at(index).unwrap_or(0) }; SmeltUnknown::Number(f64::from(code)) },
        ),
        "concat" => { let mut result = text.to_owned(); for argument in &args { result.push_str(&smelt_erased_to_js_string(argument)); } string(result) }
        "includes" | "indexOf" => { let needle = arg_text(0); let from = smelt_erased_clamped_position(args.get(1), len, 0); let found = text.index_of(&needle, from); if name == "includes" { SmeltUnknown::Bool(found.is_some()) } else { SmeltUnknown::Number(found.map_or(-1.0, smelt_units_to_number)) } }
        "lastIndexOf" => {
            let needle = arg_text(0);
            let from = match args.get(1) { None | Some(SmeltUnknown::Undefined) => len, Some(value) => { let number = smelt_unknown_to_number(value); if number.is_nan() { len } else { smelt_units_from_number(number).min(len) } } };
            SmeltUnknown::Number(text.last_index_of(&needle, from).map_or(-1.0, smelt_units_to_number))
        }
        "startsWith" => { let needle = arg_text(0); let start = smelt_erased_clamped_position(args.get(1), len, 0); SmeltUnknown::Bool(text.sub(start, len).starts_with(&needle)) }
        "endsWith" => { let needle = arg_text(0); let end = smelt_erased_clamped_position(args.get(1), len, len); SmeltUnknown::Bool(text.sub(0, end).ends_with(&needle)) }
        "padStart" | "padEnd" => {
            let fill = match args.get(1) { None | Some(SmeltUnknown::Undefined) => JsString::from(" "), Some(value) => smelt_erased_to_js_string(value) };
            let padding = smelt_erased_padding(len, smelt_erased_integer(&arg(0)), &fill);
            string(if name == "padStart" { padding + text } else { text.to_owned() + &padding })
        }
        "repeat" => { let count = smelt_erased_integer(&arg(0)); if count < 0.0 || count.is_infinite() { smelt_throw_builtin_error("RangeError", format!("Invalid count value: {}", smelt_erased_to_js_string(&arg(0)))) } string(text.repeat(count as usize)) }
        "slice" => { let start = smelt_erased_relative_position(args.first(), len, 0); let end = smelt_erased_relative_position(args.get(1), len, len); string(text.slice(start, end)) }
        "substring" => { let start = smelt_erased_clamped_position(args.first(), len, 0); let end = smelt_erased_clamped_position(args.get(1), len, len); string(text.slice(start.min(end), start.max(end))) }
        "toLowerCase" => string(text.to_lower_case()),
        "toUpperCase" => string(text.to_upper_case()),
        "trim" => string(text.trim().to_owned()),
        "trimStart" => string(text.trim_start().to_owned()),
        "trimEnd" => string(text.trim_end().to_owned()),
        "toString" => string(text.to_owned()),
        _ => SmeltUnknown::Undefined,
    }
}

//~ Emit the `TypeError` thrower shared by the method arms and the call ABI.
//~
//~ The thrown value is the same erased `TypeError` record
//~ `new TypeError(message)` builds (see `thrown::error_payload_record_expr`),
//~ so a `catch` sees `e instanceof TypeError` and `e.message` exactly as for a
//~ source-level throw.
/// Throw a JavaScript error of builtin class `class` through the error channel.
fn smelt_throw_builtin_error(class: &str, message: String) -> ! { smelt_panic_throw(smelt_throw(SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_error".to_owned(), SmeltUnknown::String(class.into())), ("message".to_owned(), SmeltUnknown::String(message.into())), ("stack".to_owned(), SmeltUnknown::Undefined), ("cause".to_owned(), SmeltUnknown::Undefined)]))))) }
/// Throw JavaScript's `TypeError: <callee> is not a function`.
///
/// `callee` is the source spelling of what was called (`x.map`), as V8 words it.
fn smelt_throw_not_callable(callee: &str) -> ! { smelt_throw_builtin_error("TypeError", format!("{callee} is not a function")) }
/// Throw JavaScript's `TypeError` for a call of a value that holds no function,
/// where the source spelling of the callee is not known.
fn smelt_value_not_callable() -> ! { smelt_throw_not_callable("value") }
/// The value of an import with no model: every call answers it, and so does every
/// member read (see `smelt_get_unknown_field`).
fn smelt_unmodeled_import() -> SmeltUnknown { SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_unmodeled".to_owned(), SmeltUnknown::Bool(true)), ("__smelt_call".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(|_smelt_args: Vec<SmeltUnknown>| Ok(smelt_unmodeled_import()))))]))) }

// @smelt:item-end
