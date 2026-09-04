//! Runtime support for JavaScript function *objects* and `[[Construct]]`.
//!
//! A JavaScript function is an object. Three consequences the generated runtime
//! has to answer, all emitted here:
//!
//! * **A function has own properties.** `f.prototype` is one of them, and source
//!   code writes others (`partialed.prototype = …`, `curried.placeholder = …`).
//!   An `Rc<dyn Fn(..)>` has nowhere to put them, so they live in a side bag
//!   keyed by *canonical function identity* — the same key
//!   `smelt_same_function_identity` compares and `Function.prototype.length`
//!   is stored under. Keying on identity rather than on an allocation address is
//!   what makes a typed callable and every erasure adapter derived from it share
//!   ONE property bag, so a write through one spelling is visible through all of
//!   them.
//!
//! * **`new f(args)` is not `f(args)`.** `[[Construct]]` allocates an object
//!   whose prototype link is `f.prototype`, runs `f` with that object installed
//!   as the receiver, and answers the allocated object unless `f` returned an
//!   object of its own. All three are observable, and none of them happens for a
//!   plain call.
//!
//! * **`x instanceof f` is a prototype-chain walk** (`OrdinaryHasInstance`),
//!   not a nominal test: it follows `x`'s chain link by link looking for the
//!   *object* `f.prototype` by reference. That is why `Object.create(p)` has to
//!   record a real link to `p` and not merely copy `p`'s members.
//!
//! The `prototype` object is materialised on FIRST READ rather than at function
//! creation. Nothing can observe the object before something reads it, so the
//! two are indistinguishable, and closures that are never constructed through
//! (the overwhelming majority) pay no allocation.
//!
//! Known infidelity: an arrow function has no `prototype` in JavaScript, and
//! this runtime cannot tell an arrow from a function expression — MIR carries no
//! arrow flag — so it answers an object for both. Reading `.prototype` off an
//! arrow is the only way to see the difference.

use crate::rust::CodeWriter;

/// Storage key holding an object's real prototype VALUE.
///
/// `Object.create(p)` copies `p`'s own members under the `__smelt_proto:` prefix
/// so they are inherited-but-not-own; that copy loses `p`'s identity, which
/// `instanceof` and `Object.getPrototypeOf` both need. This key stores the
/// prototype itself alongside the copy. It deliberately sits UNDER the same
/// `__smelt_proto:` prefix, so every own-key, enumeration, equality and JSON
/// filter that already hides that prefix hides this slot too, with no new filter
/// to keep in sync; only the two `for...in` loops that strip the prefix to yield
/// inherited keys have to skip it.
pub const PROTOTYPE_LINK_KEY: &str = "__smelt_proto:__proto__";

/// Property name of the inherited-key spelling of [`PROTOTYPE_LINK_KEY`].
///
/// What a `for...in` loop would yield if it did not skip the slot.
pub const PROTOTYPE_LINK_INHERITED_NAME: &str = "__proto__";

/// Emit the function-object and construction helpers into the generated prelude.
pub fn emit(writer: &mut CodeWriter) {
    emit_property_bag(writer);
    emit_construct(writer);
    emit_instance_of(writer);
}

/// Emit the identity-keyed own-property bag of function values.
fn emit_property_bag(writer: &mut CodeWriter) {
    writer.line("/// Inherited-key spelling of the hidden prototype-link slot.");
    writer.line("///");
    writer.line("/// The `for...in` key walks strip the `__smelt_proto:` prefix to yield inherited");
    writer.line("/// members, which would expose the link slot as a `__proto__` key; they compare");
    writer.line("/// against this name to skip it.");
    writer.line(format!(
        "const SMELT_PROTOTYPE_LINK_NAME: &str = {PROTOTYPE_LINK_INHERITED_NAME:?};"
    ));
    writer.blank_line();
    writer.line("thread_local! {");
    writer.line("    /// Own JavaScript properties of each function value, keyed by canonical");
    writer.line("    /// function identity so a typed callable and every erasure adapter derived");
    writer.line("    /// from it share one bag.");
    writer.line("    static SMELT_FUNCTION_PROPERTIES: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltObject>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
    writer.line("}");
    writer.blank_line();
    writer.line("/// Read one own property of the function value with this canonical identity.");
    writer.line("fn smelt_function_property_lookup(identity: usize, name: &str) -> Option<SmeltUnknown> { SMELT_FUNCTION_PROPERTIES.with(|bags| bags.borrow().get(&identity).and_then(|bag| bag.get(name))) }");
    writer.blank_line();
    writer.line("/// Store one own property on the function value with this canonical identity.");
    writer.line("fn smelt_set_function_property_key(identity: usize, name: &str, value: SmeltUnknown) { SMELT_FUNCTION_PROPERTIES.with(|bags| { bags.borrow_mut().entry(identity).or_insert_with(|| SmeltObject::new(Vec::new())).insert(name.to_owned(), value); }); }");
    writer.blank_line();
    writer.line("/// Store one own property on a function value (JS `f.name = value`).");
    writer.line("///");
    writer.line("/// Accepts any callable handle, typed or erased, and resolves it to the one");
    writer.line("/// canonical identity every representation of that function shares.");
    writer.line("fn smelt_set_function_property<F: ?Sized + 'static>(function: &::std::rc::Rc<F>, name: &str, value: SmeltUnknown) { smelt_set_function_property_key(smelt_canonical_function_identity(function), name, value); }");
    writer.blank_line();
    writer.line("/// The `prototype` object of a function value, created on first read.");
    writer.line("///");
    writer.line("/// JavaScript gives every non-arrow function a fresh `{ constructor: f }`");
    writer.line("/// object when the function is created. Creating it lazily is observationally");
    writer.line("/// the same and costs nothing for a function nothing constructs through.");
    writer.line("fn smelt_function_prototype(identity: usize, constructor: SmeltUnknown) -> SmeltUnknown { if let Some(value) = smelt_function_property_lookup(identity, \"prototype\") { return value; } let prototype = SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"constructor\".to_owned(), constructor)]))); smelt_set_function_property_key(identity, \"prototype\", prototype.clone()); prototype }");
    writer.blank_line();
    writer.line("/// Read one own property of an erased function value (JS `f.prototype`).");
    writer.line("fn smelt_function_value_property(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, name: &str) -> SmeltUnknown { let identity = smelt_canonical_function_identity(function); if let Some(value) = smelt_function_property_lookup(identity, name) { return value; } if name == \"prototype\" { return smelt_function_prototype(identity, SmeltUnknown::Function(function.clone())); } SmeltUnknown::Undefined }");
    writer.blank_line();
    writer.line("/// The callable behind an erased value: a function, or a callable object's");
    writer.line("/// `__smelt_call` slot.");
    writer.line("fn smelt_callable_of(value: &SmeltUnknown) -> Option<::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>> { match value { SmeltUnknown::Function(function) => Some(function.clone()), SmeltUnknown::Object(object) => match object.get(\"__smelt_call\") { Some(SmeltUnknown::Function(function)) => Some(function), _ => None }, _ => None } }");
    writer.blank_line();
}

/// Emit the JavaScript `[[Construct]]` helper.
fn emit_construct(writer: &mut CodeWriter) {
    writer.line("/// JavaScript `new callee(args)` through a function VALUE (`[[Construct]]`).");
    writer.line("///");
    writer.line("/// Allocates an object linked to the callee's `prototype`, runs the callee with");
    writer.line("/// that object installed as the receiver, and answers the callee's own result");
    writer.line("/// only when it is an object — a constructor that returns a primitive (or");
    writer.line("/// nothing) yields the allocated object, which is what makes");
    writer.line("/// `new f() instanceof g` answerable at all. A non-callable operand answers");
    writer.line("/// `undefined`: JavaScript throws a TypeError there, and Smelt models an");
    writer.line("/// unconstructible callee the same way an uncallable one is modeled.");
    writer.line("fn smelt_construct(callee: SmeltUnknown, args: Vec<SmeltUnknown>) -> SmeltUnknown {");
    writer.line("    let Some(function) = smelt_callable_of(&callee) else { return SmeltUnknown::Undefined; };");
    writer.line("    let prototype = smelt_function_value_property(&function, \"prototype\");");
    writer.line("    let instance = smelt_object_from_prototype(prototype);");
    writer.line("    let result = { let _smelt_this_guard = smelt_push_this(instance.clone()); (function)(args).unwrap_or_else(|error| panic!(\"{}\", error)) };");
    writer.line("    match result { SmeltUnknown::Object(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => result, _ => instance }");
    writer.line("}");
    writer.blank_line();
}

/// Emit the `OrdinaryHasInstance` prototype-chain walk.
fn emit_instance_of(writer: &mut CodeWriter) {
    writer.line("/// JavaScript `value instanceof target` for a runtime constructor value.");
    writer.line("///");
    writer.line("/// Walks `value`'s prototype chain comparing each link by REFERENCE against");
    writer.line("/// the target's own `prototype` object, exactly as `OrdinaryHasInstance` does.");
    writer.line("/// A non-callable target, or one whose `prototype` is not an object, answers");
    writer.line("/// `false` (JavaScript throws for the former; Smelt reports the absence).");
    writer.line("///");
    writer.line("/// The walk stops when `smelt_proto_accessor` stops advancing, so it");
    writer.line("/// terminates on the opaque `__smelt_proto:*` sentinels too.");
    writer.line("fn smelt_instance_of_value(value: &SmeltUnknown, target: &SmeltUnknown) -> bool {");
    writer.line("    let Some(function) = smelt_callable_of(target) else { return false; };");
    writer.line("    let prototype = smelt_function_value_property(&function, \"prototype\");");
    writer.line("    let SmeltUnknown::Object(prototype) = prototype else { return false; };");
    writer.line("    let mut current = smelt_proto_accessor(value);");
    writer.line("    loop {");
    writer.line("        match &current {");
    writer.line("            SmeltUnknown::Null | SmeltUnknown::Undefined => return false,");
    writer.line("            SmeltUnknown::Object(link) if link.id == prototype.id => return true,");
    writer.line("            _ => {}");
    writer.line("        }");
    writer.line("        let next = smelt_proto_accessor(&current);");
    writer.line("        if smelt_same_prototype_link(&next, &current) { return false; }");
    writer.line("        current = next;");
    writer.line("    }");
    writer.line("}");
    writer.blank_line();
    writer.line("/// Whether two prototype-chain links are the same link, so a walk that reached");
    writer.line("/// one has made no progress and must stop.");
    writer.line("fn smelt_same_prototype_link(left: &SmeltUnknown, right: &SmeltUnknown) -> bool { match (left, right) { (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right, _ => false } }");
    writer.blank_line();
}
