//! Runtime support for reading a JavaScript builtin's members as values.
//!
//! `Array.prototype.slice` is a property read, not a call: JavaScript answers it
//! with a function object, and code stores that object, probes it with `typeof`,
//! reads its `length`, and eventually invokes it through `.call`/`.apply`. The
//! generated runtime therefore needs three things, all emitted here:
//!
//! * a value for `<Builtin>.prototype` — an interned marker record, so the
//!   member read has somewhere to land and `Array.prototype === Array.prototype`
//!   holds;
//! * one interned callable per modeled `(builtin, kind, member)` triple, carrying
//!   the member's JavaScript `length` and a stable identity;
//! * a dispatcher that applies the member to its receiver.
//!
//! The set of modeled members is [`smelt_stdlib::BUILTIN_MEMBER_FUNCTIONS`] — the
//! same registry the arity comes from — so a member Smelt cannot run is simply
//! absent and reads as `undefined` rather than becoming a callable that lies.
//!
//! A prototype method consumes its receiver as the leading argument, which is
//! what `Function.prototype.call`/`.apply` must feed it. Those two are generic
//! over any callable, so the receiver-consuming callables register themselves in
//! a side table (`smelt_register_receiver_method`) and `smelt_function_method`
//! consults it instead of unconditionally dropping the receiver.

use smelt_stdlib::{BUILTIN_MEMBER_FUNCTIONS, builtin_member_key};

use crate::rust::CodeWriter;

/// Emit the builtin-member value helpers into the generated prelude.
pub fn emit(writer: &mut CodeWriter) {
    emit_registry(writer);
    emit_prototype_object(writer);
    emit_member_value(writer);
    emit_apply(writer);
}

/// Emit the `(class, kind, member, arity)` table and its lookup.
fn emit_registry(writer: &mut CodeWriter) {
    let rows = BUILTIN_MEMBER_FUNCTIONS
        .iter()
        .map(|entry| {
            format!(
                "({class:?}, {kind:?}, {member:?}, {arity}.0, {key:?})",
                class = entry.class,
                kind = entry.kind.tag(),
                member = entry.member,
                arity = entry.arity,
                key = builtin_member_key(entry),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    writer.line("/// Every modeled builtin member: `(class, kind, member, length, key)`.");
    writer.line(format!(
        "const SMELT_BUILTIN_MEMBERS: [(&str, &str, &str, f64, &'static str); {count}] = [{rows}];",
        count = BUILTIN_MEMBER_FUNCTIONS.len(),
    ));
    writer.line("/// Look one modeled builtin member up, returning its `length` and dispatch key.");
    writer.line("fn smelt_builtin_member_entry(class: &str, kind: &str, member: &str) -> Option<(f64, &'static str)> { SMELT_BUILTIN_MEMBERS.into_iter().find(|(entry_class, entry_kind, entry_member, _, _)| *entry_class == class && *entry_kind == kind && *entry_member == member).map(|(_, _, _, length, key)| (length, key)) }");
    writer.blank_line();
}

/// Emit the interned `<Builtin>.prototype` marker record.
fn emit_prototype_object(writer: &mut CodeWriter) {
    writer.line("/// The interned value of `<Builtin>.prototype`.");
    writer.line("///");
    writer.line("/// JavaScript has exactly one prototype object per builtin, so the record is");
    writer.line("/// cached by class name: `Array.prototype === Array.prototype` holds, and a");
    writer.line("/// member read off it resolves through the modeled-member registry. The record");
    writer.line("/// carries only the marker, so it never enumerates as data.");
    writer.line("thread_local! { static SMELT_BUILTIN_PROTOTYPES: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
    writer.line("fn smelt_builtin_prototype_object(class: &str) -> SmeltUnknown { SMELT_BUILTIN_PROTOTYPES.with(|cache| cache.borrow_mut().entry(class.to_owned()).or_insert_with(|| SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"__smelt_builtin_prototype\".to_owned(), SmeltUnknown::String(class.into()))])))).clone()) }");
    writer.blank_line();
}

/// Emit the interned per-member callable and the receiver-method side table.
fn emit_member_value(writer: &mut CodeWriter) {
    writer.line("/// Callables that consume their receiver as the leading argument.");
    writer.line("///");
    writer.line("/// A prototype method's `this` is its receiver, and Smelt passes it as the");
    writer.line("/// first argument. `Function.prototype.call`/`.apply` drop the receiver for an");
    writer.line("/// ordinary erased callable (which has no `this` channel), so they must not do");
    writer.line("/// that for these: membership here is the difference.");
    writer.line("thread_local! { static SMELT_RECEIVER_METHODS: ::std::cell::RefCell<::std::collections::HashSet<usize>> = ::std::cell::RefCell::new(::std::collections::HashSet::new()); }");
    writer.line("fn smelt_register_receiver_method<T: ?Sized + 'static>(function: &::std::rc::Rc<T>) { let key = smelt_retain_callable_key(function); SMELT_RECEIVER_METHODS.with(|methods| { methods.borrow_mut().insert(key); }); }");
    writer.line("fn smelt_is_receiver_method<T: ?Sized + 'static>(function: &::std::rc::Rc<T>) -> bool { let key = smelt_retain_callable_key(function); SMELT_RECEIVER_METHODS.with(|methods| methods.borrow().contains(&key)) }");
    writer.blank_line();
    writer.line("/// Read a modeled member off a builtin, or `None` when it is not modeled.");
    writer.line("///");
    writer.line("/// The callable is interned per member so two reads are the same function");
    writer.line("/// value, carries the member's JavaScript `length`, and gets the member's key");
    writer.line("/// as its identity — the same treatment `Object.prototype`'s members get.");
    writer.line("thread_local! { static SMELT_BUILTIN_MEMBER_VALUES: ::std::cell::RefCell<::std::collections::HashMap<&'static str, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
    writer.line("fn smelt_builtin_member_value(class: &str, kind: &str, member: &str) -> Option<SmeltUnknown> {");
    writer.line("    let (length, key) = smelt_builtin_member_entry(class, kind, member)?;");
    writer.line("    let receiver_method = kind == \"prototype\";");
    writer.line("    Some(SMELT_BUILTIN_MEMBER_VALUES.with(|values| values.borrow_mut().entry(key).or_insert_with(|| { let function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_builtin_member_apply(key, args))); smelt_link_function_identity_key(&function, smelt_method_identity(key)); smelt_register_function_length(&function, length); if receiver_method { smelt_register_receiver_method(&function); } SmeltUnknown::Function(function) }).clone()))");
    writer.line("}");
    writer.blank_line();
}

/// Emit the member dispatcher and its receiver-coercion helpers.
fn emit_apply(writer: &mut CodeWriter) {
    writer.line("/// The receiver of a `String.prototype` method as a Rust string.");
    writer.line("///");
    writer.line("/// A boxed `String` object unboxes first, so `new String('ab')` and `'ab'`");
    writer.line("/// behave the same through a prototype method, as they do in JavaScript.");
    writer.line("fn smelt_builtin_receiver_text(value: &SmeltUnknown) -> String { match smelt_unbox_primitive(value.clone()) { SmeltUnknown::String(text) => text.to_string(), other => other.to_string() } }");
    writer.line("/// The receiver of an `Array.prototype` method as its element vector.");
    writer.line("fn smelt_builtin_receiver_elements(value: &SmeltUnknown) -> Option<Vec<SmeltUnknown>> { match value { SmeltUnknown::Array(values) => Some(values.clone().into_vec()), _ => None } }");
    writer.line("/// Resolve a JavaScript relative index argument against a length.");
    writer.line("///");
    writer.line("/// `undefined` takes the caller's default, a negative counts from the end, and");
    writer.line("/// anything past either end clamps — the `slice` index rules, shared by the");
    writer.line("/// string and array arms so they cannot drift apart.");
    writer.line("fn smelt_builtin_relative_index(value: &SmeltUnknown, len: usize, default: usize) -> usize { match value { SmeltUnknown::Undefined => default, other => { let raw = match other { SmeltUnknown::Number(value) => *value, _ => 0.0 }; if raw.is_nan() { 0 } else if raw < 0.0 { let from_end = len as f64 + raw; if from_end < 0.0 { 0 } else { from_end as usize } } else { (raw as usize).min(len) } } } }");
    writer.blank_line();
    writer.line("/// Apply one modeled builtin member to its arguments.");
    writer.line("///");
    writer.line("/// For a prototype method the leading argument is the receiver; for a static");
    writer.line("/// function there is no receiver and the arguments start at index zero. Every");
    writer.line("/// arm is keyed by the registry's dispatch key, so adding a member is a table");
    writer.line("/// entry plus its arm and nothing else.");
    writer.line("fn smelt_builtin_member_apply(key: &str, args: Vec<SmeltUnknown>) -> SmeltUnknown {");
    writer.line("    let receiver = args.first().map_or(SmeltUnknown::Undefined, Clone::clone);");
    writer.line("    let first = args.get(1).map_or(SmeltUnknown::Undefined, Clone::clone);");
    writer.line("    let second = args.get(2).map_or(SmeltUnknown::Undefined, Clone::clone);");
    writer.line("    match key {");
    // Array.prototype
    writer.line("        \"Array.prototype.slice\" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let len = values.len(); let start = smelt_builtin_relative_index(&first, len, 0); let end = smelt_builtin_relative_index(&second, len, len); SmeltUnknown::Array(values.into_iter().skip(start).take(end.saturating_sub(start)).collect::<Vec<_>>().into()) } None => SmeltUnknown::Undefined }");
    writer.line("        \"Array.prototype.concat\" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let mut result = values; for argument in args.into_iter().skip(1) { match argument { SmeltUnknown::Array(items) => result.extend(items.into_vec()), other => result.push(other) } } SmeltUnknown::Array(result.into()) } None => SmeltUnknown::Undefined }");
    writer.line("        \"Array.prototype.indexOf\" | \"Array.prototype.lastIndexOf\" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let mut found: f64 = -1.0; for (index, item) in values.iter().enumerate() { if item.same_js_key(&first) { found = index as f64; if key == \"Array.prototype.indexOf\" { break; } } } SmeltUnknown::Number(found) } None => SmeltUnknown::Undefined }");
    writer.line("        \"Array.prototype.includes\" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => SmeltUnknown::Bool(values.iter().any(|item| item.same_js_key(&first))), None => SmeltUnknown::Undefined }");
    writer.line("        \"Array.prototype.join\" => match smelt_builtin_receiver_elements(&receiver) { Some(values) => { let separator = match &first { SmeltUnknown::Undefined => \",\".to_owned(), other => smelt_builtin_receiver_text(other) }; SmeltUnknown::String(values.into_iter().map(|item| match item { SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), other => smelt_builtin_receiver_text(&other) }).collect::<Vec<_>>().join(&separator).into()) } None => SmeltUnknown::Undefined }");
    // Statics
    writer.line("        \"Array.isArray\" => SmeltUnknown::Bool(matches!(receiver, SmeltUnknown::Array(_))),");
    writer.line("        _ => SmeltUnknown::Undefined,");
    writer.line("    }");
    writer.line("}");
    writer.blank_line();
}
