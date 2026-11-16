//! Reference-class classification.
//!
//! JavaScript objects are reference cells with shared mutable identity, but
//! Smelt emits classes as by-value Rust structs by default. A by-value struct is
//! correct only for *value classes* — objects that are constructed and read but
//! never mutated after construction, aliased-and-observed, or allowed to let
//! `this` escape into a stored closure. Any class that violates those
//! constraints must be emitted as a **reference class**: a thin handle newtype
//! over `Rc<RefCell<Inner>>` whose `Clone` shares identity (see
//! `specs/reference-class-modeling.md`).
//!
//! This module computes, once per crate, the set of class symbols that must be
//! lifted to the handle representation. The decision mirrors the module-globals
//! "lift only if mutated" rule: the heap allocation, refcount, and runtime
//! borrow-check are only paid when shared-mutable identity is actually required.
//!
//! ## Classification rule (V1)
//!
//! A class is a reference class if ANY of the following holds, else it stays a
//! by-value value class:
//! - a non-constructor instance method assigns to `this.<field>` (post-
//!   construction mutation), or
//! - any source path writes an instance field on a class-typed binding after
//!   construction (`obj.<field> = …` or `obj[key] = …`), or
//! - `this` (a method's `self`) is captured by a closure (the escaping-`this`
//!   case that needs a shareable handle), or
//! - its instances are compared by identity (`===`/`!==` against a
//!   non-literal), which only a handle can answer (see
//!   [`collect_identity_comparison_triggers`]), or
//! - its instances are installed as a callee's `this` in a program that writes
//!   properties through erased values (see [`collect_bound_receiver_triggers`]), or
//! - a collection method mutates one of its instance fields in place (see
//!   [`collect_field_mutation_triggers`]), or
//! - it stores a virtual method SLOT (see [`collect_virtual_slot_triggers`]), or
//! - it overrides a method of an ancestor (see
//!   [`collect_override_triggers`]), or
//! - it is an object shape whose values are handed to a slot of an erased or
//!   dictionary type (see [`collect_view_flow_triggers`]), or
//! - it is an object shape converted to, or from, another object shape that
//!   shares its properties (see `crate::structural_views`), or
//! - its instances are THROWN (see [`collect_thrown_class_triggers`]), or
//! - it contains itself by value, directly or through other shapes (see
//!   [`collect_by_value_cycle_triggers`]), or
//! - it INHERITS from a reference class, or a class inheriting from it is one
//!   (see [`close_over_heritage`]).
//!
//! The heritage rule is not a heuristic: a subclass's instances are the same
//! JavaScript objects as its base's, so identity and mutation semantics cannot
//! differ between them -- and Smelt re-emits every inherited method body into the
//! subclass's own `impl`, so a subclass emitted by value would carry bodies that
//! read `self.0.borrow().field` against a struct that has no field `0`.
//!
//! Pure aliasing without any mutation (`const b = a;` where neither `a` nor `b`
//! is ever mutated) is intentionally NOT a trigger: a value class that is only
//! copied and read is byte-for-byte observationally identical under value and
//! reference semantics, so it stays a value class. This is the one narrowing
//! versus the spec's literal "reassigned or aliased and later observed" clause
//! and is sound because the observable difference only appears under mutation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "classification helpers are shared with sibling emitter modules"
)]

use std::collections::HashSet;

use smelt_hir::{Symbol, Type};
use smelt_mir::{HirOrigin, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement};

/// Compute the set of class symbols that must be emitted as reference classes.
///
/// Scans every MIR function for the V1 classification triggers documented on
/// this module. The result is consulted by the emitter to choose between the
/// handle-newtype representation and the current by-value struct.
pub(crate) fn reference_classes(mir: &Mir) -> HashSet<Symbol> {
    let mut references = HashSet::new();
    for closure in &mir.closures {
        collect_body_field_write_triggers(
            mir,
            &closure.locals,
            &closure.blocks,
            None,
            &mut references,
        );
    }
    for function in &mir.functions {
        collect_field_write_triggers(mir, function, &mut references);
        collect_field_mutation_triggers(mir, function, &mut references);
        collect_self_capture_triggers(mir, function, &mut references);
    }
    collect_bound_receiver_triggers(mir, &mut references);
    collect_identity_comparison_triggers(mir, &mut references);
    collect_view_flow_triggers(mir, &mut references);
    // A record viewed as another record type, and the type it is viewed as:
    // one object behind two static types (`crate::structural_views`).
    references.extend(crate::structural_views::StructuralViews::plan(mir).participants());
    collect_virtual_slot_triggers(mir, &mut references);
    collect_override_triggers(mir, &mut references);
    collect_thrown_class_triggers(mir, &mut references);
    // After every other trigger, so a shape those already lifted counts as the
    // indirection it is; before the heritage closure, so a lifted class's
    // chain follows it.
    collect_by_value_cycle_triggers(mir, &mut references);
    // Before the index-store deviation below, so that deviation still has the
    // final say for a class that carries a dynamic index signature.
    close_over_heritage(mir, &mut references);
    // V1 deviation: a class with a dynamic index-signature store
    // (`[key: string]: T`) keeps its existing by-value struct emission. Its
    // keyed access routes through the synthesized `__smelt_index_store` field
    // by concrete struct access, which is not yet taught to project through the
    // shared cell; lifting such a class would emit `bag.__smelt_index_store`
    // against a handle that has no such field. Aliasing an index-signature class
    // therefore does not share its store yet — a documented V1 limitation, not a
    // regression from prior behavior.
    references.retain(|name| !class_has_index_store(mir, *name));
    references
}

/// A class storing a virtual method slot: a function-typed field named like
/// one of the class's own or inherited methods (or abstract methods), which
/// TypeScript only allows because the frontend synthesized it.
///
/// A base method calling `this.g()` dispatches through that slot, so every
/// instance needs it bound to its own most-derived method from construction
/// on, not only when converted to a base type. The constructor binds it
/// through a `Weak` handle on the instance's cell
/// (`FunctionEmitter::bind_own_virtual_slots_expr`), which only the
/// reference representation has: a value struct cannot hand a closure a
/// handle to itself. Before this, `new A().f()` with `f() { return this.g() }`
/// called the slot's default closure.
fn collect_virtual_slot_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    for class in &mir.classes {
        if !crate::classes::own_virtual_slot_names(mir, class).is_empty() {
            references.insert(class.name);
        }
    }
}

/// A class overriding a method an ancestor declares.
///
/// A base-typed value may hold an instance of the overriding class, and a call
/// through it must reach the override. Only a reference hierarchy keeps one
/// object behind both static types (`crate::class_polymorphism`): a by-value
/// base struct built from a subclass instance would run the base body. The
/// override is reached through the polymorphic handle (the base method checks
/// the shared cell's concrete class and forwards), which also serves a
/// generic method, which no `dyn Fn` slot can carry, and an override of a
/// base declared in another module, which gets no slot. The heritage closure
/// lifts the whole chain.
fn collect_override_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    for class in &mir.classes {
        let Some(base) = class.base else { continue };
        for method_id in &class.methods {
            let Some(function) = usize::try_from(method_id.0)
                .ok()
                .and_then(|index| mir.functions.get(index))
            else {
                continue;
            };
            let HirOrigin::ClassMethod { method, .. } = function.origin else {
                continue;
            };
            if class_chain_declares_method(mir, base, method) {
                references.insert(class.name);
            }
        }
    }
}

/// Whether `class` or one of its ancestors declares an instance method named
/// `method`.
fn class_chain_declares_method(mir: &Mir, class: Symbol, method: Symbol) -> bool {
    let mut current = Some(class);
    let mut seen = HashSet::new();
    while let Some(name) = current
        && seen.insert(name)
    {
        let Some(found) = mir.class_named(name) else {
            return false;
        };
        let declares = found.methods.iter().any(|id| {
            usize::try_from(id.0)
                .ok()
                .and_then(|index| mir.functions.get(index))
                .is_some_and(|function| {
                    matches!(function.origin, HirOrigin::ClassMethod { method: own, .. } if own == method)
                })
        });
        if declares {
            return true;
        }
        current = found.base;
    }
    false
}

/// A class whose instances are thrown: `throw err` with `err` a class value.
///
/// A thrown value crosses the erased channel and comes back through `catch`,
/// a rejected promise or a test matcher, and JavaScript code checks it by
/// identity there (`expect(context.error).toBe(error)`, `e === err`, a
/// rethrow). A by-value struct erases to a fresh object each time, so those
/// checks failed; a reference class's erased view carries one stable id.
fn collect_thrown_class_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    let mut note = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
        for block in blocks {
            let Some(smelt_mir::Terminator::Throw(operand)) = &block.terminator else {
                continue;
            };
            let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
            else {
                continue;
            };
            let Some(decl) = usize::try_from(local.0)
                .ok()
                .and_then(|index| locals.get(index))
            else {
                continue;
            };
            if let Some(Type::Class { name, .. }) = mir.types.get(decl.ty)
                && mir.has_class(*name)
            {
                references.insert(*name);
            }
        }
    };
    for function in &mir.functions {
        note(&function.locals, &function.blocks);
    }
    for closure in &mir.closures {
        note(&closure.locals, &closure.blocks);
    }
}

/// A shape (class or interface) that contains itself BY VALUE: through its
/// own fields, `Option`, tuples, union arms, or the by-value type arguments of
/// another by-value shape (`interface L { o?: O }` with `interface O { l?: L }`,
/// or `interface Tree { parent?: Tree }`).
///
/// A by-value Rust struct cannot hold itself (rustc E0072: a recursive type
/// has infinite size), so such a cycle needs an indirection. The reference
/// representation is that indirection: a `Name(Rc<RefCell<NameInner>>)`
/// handle, the same shape a hand-written Rust tree with parent links uses, and
/// the one JavaScript's own semantics call for (`l.o.l` IS `l` when the program
/// builds it so). Every shape on a cycle is lifted; one already lifted by
/// another trigger, and every collection (`SmeltList`, records, maps, sets,
/// functions, promises), already holds its contents behind a shared handle and
/// breaks the cycle, so `interface Tree { kids: Tree[] }` stays a value.
fn collect_by_value_cycle_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    let mut shapes: std::collections::HashMap<Symbol, (Vec<Symbol>, Vec<smelt_hir::TypeId>)> =
        std::collections::HashMap::new();
    for interface in &mir.interfaces {
        let params = interface
            .type_params
            .iter()
            .map(|param| param.name)
            .collect();
        let fields = crate::classes::effective_interface_fields(mir, interface)
            .into_iter()
            .map(|field| field.ty)
            .collect();
        shapes.insert(interface.name, (params, fields));
    }
    for class in &mir.classes {
        let params = class.type_params.iter().map(|param| param.name).collect();
        let fields = crate::classes::effective_class_fields(mir, class)
            .into_iter()
            .map(|field| field.ty)
            .collect();
        // A class and an interface of one name are one emitted type: the
        // class's fields are its storage.
        shapes.insert(class.name, (params, fields));
    }
    let graph = ByValueGraph {
        mir,
        shapes: &shapes,
        references,
    };
    let mut on_cycle = Vec::new();
    for name in shapes.keys() {
        if references.contains(name) {
            continue;
        }
        if graph.reaches(*name, *name) {
            on_cycle.push(*name);
        }
    }
    references.extend(on_cycle);
}

/// The by-value containment graph over not-yet-lifted shapes.
struct ByValueGraph<'a> {
    /// The crate.
    mir: &'a Mir,
    /// Each shape's type parameter names and stored field types.
    shapes: &'a std::collections::HashMap<Symbol, (Vec<Symbol>, Vec<smelt_hir::TypeId>)>,
    /// Shapes already emitted behind a handle (an indirection).
    references: &'a HashSet<Symbol>,
}

impl ByValueGraph<'_> {
    /// Whether `from`'s by-value storage reaches shape `target`.
    fn reaches(&self, from: Symbol, target: Symbol) -> bool {
        let mut seen = HashSet::new();
        let mut stack = self.direct_shapes(from);
        while let Some(next) = stack.pop() {
            if next == target {
                return true;
            }
            if seen.insert(next) {
                stack.extend(self.direct_shapes(next));
            }
        }
        false
    }

    /// The shapes `shape` stores by value in its own fields.
    fn direct_shapes(&self, shape: Symbol) -> Vec<Symbol> {
        let mut out = Vec::new();
        if let Some((_, fields)) = self.shapes.get(&shape) {
            for ty in fields {
                self.collect(*ty, &mut out, &mut Vec::new(), &mut HashSet::new());
            }
        }
        out
    }

    /// Collect the by-value shapes of `ty` into `shapes`, and the type
    /// parameters it holds by value into `params`. `active` guards the
    /// recursion through generic shapes (`interface N<T> { next?: N<T> }`).
    fn collect(
        &self,
        ty: smelt_hir::TypeId,
        shapes: &mut Vec<Symbol>,
        params: &mut Vec<Symbol>,
        active: &mut HashSet<Symbol>,
    ) {
        match self.mir.types.get(ty) {
            Some(Type::Optional(inner)) => self.collect(*inner, shapes, params, active),
            Some(Type::Tuple(items) | Type::Union(items)) => {
                for item in items {
                    self.collect(*item, shapes, params, active);
                }
            }
            Some(Type::TypeParam { name, .. }) => params.push(*name),
            Some(Type::Class { name, args }) => {
                if !self.shapes.contains_key(name) || self.references.contains(name) {
                    return;
                }
                shapes.push(*name);
                // A type argument is stored by value exactly where the shape
                // stores its parameter by value (`Box<T> { v: T }`, not
                // `Holder<T> { items: T[] }`).
                let held = self.params_held_by_value(*name, active);
                if let Some((declared, _)) = self.shapes.get(name) {
                    for (param, arg) in declared.iter().zip(args) {
                        if held.contains(param) {
                            self.collect(*arg, shapes, params, active);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The type parameters of `shape` its fields store by value.
    fn params_held_by_value(&self, shape: Symbol, active: &mut HashSet<Symbol>) -> Vec<Symbol> {
        if !active.insert(shape) {
            return Vec::new();
        }
        let mut params = Vec::new();
        if let Some((_, fields)) = self.shapes.get(&shape) {
            for ty in fields {
                self.collect(*ty, &mut Vec::new(), &mut params, active);
            }
        }
        active.remove(&shape);
        params
    }
}

/// Close the reference set over the inheritance chain, in both directions.
///
/// A trigger fires on the class whose FIELD is written, which is the class that
/// declares it -- so a subclass that only inherits mutated state is never named
/// by one. It is still the same kind of object: Smelt re-emits each inherited
/// method body into the subclass's own `impl`, and those bodies were written
/// against the base's representation. Hono's
/// `class<T> extends RegExpRouter<T>` in `prepared-router.ts` is the shape:
/// `RegExpRouter` is a reference class, so the anonymous subclass's inherited
/// bodies read `self.0.borrow()._tries`, while the subclass itself was emitted
/// as a value struct -- `no field 0 on __smelt_anon_class_3050<T>` (E0609 x9 in
/// the router slice, and H30 in `hono-phase2-generated-rust.md` (in git history)).
///
/// UPWARD, a base of a reference class is a reference class too. A derived
/// constructor's `super(..)` runs the base constructor body re-emitted over the
/// derived receiver (`crate::base_init`), and inherited method bodies are
/// re-emitted the same way; each copy renders field access in the DECLARING
/// class's representation. A value base under a reference subclass therefore
/// emitted `this.x = ..` against a handle that has no field `x`. The reasoning
/// is the downward rule's: the subclass's instances ARE instances of the base,
/// so one identity model has to hold for the whole chain.
///
/// Iterated to a fixed point so a chain of any depth closes, and bounded by the
/// class count so a cyclic `base` chain (which the frontend rejects) cannot spin
/// here.
fn close_over_heritage(mir: &Mir, references: &mut HashSet<Symbol>) {
    for _ in 0..mir.classes.len() {
        let mut changed = false;
        for class in &mir.classes {
            let Some(base) = class.base else {
                continue;
            };
            if references.contains(&base) && references.insert(class.name) {
                changed = true;
            }
            if references.contains(&class.name) && mir.has_class(base) && references.insert(base) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Return whether a class carries a synthesized dynamic index-signature store.
pub(crate) fn class_has_index_store(mir: &Mir, name: Symbol) -> bool {
    let has_index_store = |fields: &[smelt_mir::MirField]| {
        fields
            .iter()
            .any(|field| mir.symbols.get(field.name) == Some(smelt_hir::CLASS_INDEX_STORE_FIELD))
    };
    crate::classes::find_class(mir, name).is_some_and(|class| has_index_store(&class.fields))
        || mir
            .interfaces
            .iter()
            .find(|interface| interface.name == name)
            .is_some_and(|interface| has_index_store(&interface.fields))
}

/// Record classes mutated through a field or index write after construction.
///
/// A write to `base.<field>`/`base[key]` where `base` is class-typed lifts that
/// class, *except* a constructor's writes to the instance it is building (its
/// own class), which are initial construction rather than post-construction
/// mutation. A constructor's `this` is a user binding, not a parameter, so this
/// is keyed on the base local's class matching the class under construction
/// (a constructor mutating a *different* object still lifts that other class).
fn collect_field_write_triggers(
    mir: &Mir,
    function: &MirFunction,
    references: &mut HashSet<Symbol>,
) {
    let constructing_class = crate::base_init::constructor_class(function);
    collect_body_field_write_triggers(
        mir,
        &function.locals,
        &function.blocks,
        constructing_class,
        references,
    );
}

/// [`collect_field_write_triggers`] over one body's locals and blocks. A
/// closure body is a body too: `{ set: (flags) => { flags.era = 7 } }` writes
/// the caller's object, and missing it left `Flags` a value struct whose copy
/// took the write.
fn collect_body_field_write_triggers(
    mir: &Mir,
    locals: &[smelt_mir::LocalDecl],
    blocks: &[smelt_mir::BasicBlock],
    constructing_class: Option<Symbol>,
    references: &mut HashSet<Symbol>,
) {
    for block in blocks {
        for statement in &block.statements {
            let Statement::AssignPlace { place, .. } = statement else {
                continue;
            };
            let base = match place {
                Place::Field { base, .. } | Place::Index { base, .. } => *base,
                // Neither names a base local whose class could be recorded.
                Place::Local(_) | Place::Global { .. } => continue,
            };
            let Some(name) = class_name_of_decl(mir, locals, base) else {
                continue;
            };
            // A constructor initializing its own instance is construction, not
            // post-construction mutation.
            if constructing_class == Some(name) {
                continue;
            }
            references.insert(name);
        }
    }
}

/// Record classes whose instances are installed as a call's `this` in a
/// program that writes properties through erased values.
///
/// A call through a function-valued field (`tally.describe(..)`) installs the
/// instance as the callee's `this` ([`Rvalue::BindThis`]), and the callee sees
/// it only as an erased view. A property write through that view
/// (`this.builds = ..`) is a post-construction mutation of THE instance, and
/// `emitter::view_write_through` forwards it there — which needs the shared
/// identity only the handle representation has: a by-value instance's view is
/// a copy, so the write would land on nothing the program can read back.
/// Gated on the program containing an erased property write at all, the only
/// statement the view can be mutated through, so programs that merely bind a
/// receiver keep their value classes.
fn collect_bound_receiver_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    if !crate::emitter::view_write_through::program_writes_erased_fields(mir) {
        return;
    }
    let mut record = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
        for statement in blocks.iter().flat_map(|block| &block.statements) {
            let Statement::Assign {
                value: Rvalue::BindThis { receiver, .. },
                ..
            } = statement
            else {
                continue;
            };
            let Some(local) = operand_base_local(receiver) else {
                continue;
            };
            let Some(decl) = usize::try_from(local.0)
                .ok()
                .and_then(|index| locals.get(index))
            else {
                continue;
            };
            if let Some(Type::Class { name, .. }) = mir.types.get(decl.ty)
                && mir.has_class(*name)
            {
                references.insert(*name);
            }
        }
    };
    for function in &mir.functions {
        record(&function.locals, &function.blocks);
    }
    for closure in &mir.closures {
        record(&closure.locals, &closure.blocks);
    }
}

/// Record classes whose instances are compared by JavaScript identity.
///
/// `a === b` on two objects asks whether they are the SAME object, and a
/// by-value struct has no identity to answer with: every copy is
/// indistinguishable, so the emitter could only fold the comparison to a
/// constant `false` — even for `a === a`, and for a value narrowed back from an
/// erased view of `a`. The handle representation carries identity (its cell
/// address), so a class whose values reach `===`/`!==` or `Object.is` against
/// another non-literal value is lifted to it. A comparison against a literal
/// (`c === null`, `c !== undefined`) is a presence test, not an identity one,
/// and does not lift anything.
fn collect_identity_comparison_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    let class_of = |locals: &[smelt_mir::LocalDecl], operand: &Operand| -> Option<Symbol> {
        let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand
        else {
            return None;
        };
        let mut ty = locals.get(usize::try_from(local.0).ok()?)?.ty;
        if let Some(Type::Optional(inner)) = mir.types.get(ty) {
            ty = *inner;
        }
        match mir.types.get(ty) {
            Some(Type::Class { name, .. }) if is_record_type_symbol(mir, *name) => Some(*name),
            _ => None,
        }
    };
    let mut record = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
        for statement in blocks.iter().flat_map(|block| &block.statements) {
            let Statement::Assign {
                value: Rvalue::Binary { op, lhs, rhs },
                ..
            } = statement
            else {
                continue;
            };
            if !matches!(
                op,
                smelt_hir::BinOp::StrictEq
                    | smelt_hir::BinOp::StrictNotEq
                    | smelt_hir::BinOp::JsStrictEq
                    | smelt_hir::BinOp::JsStrictNotEq
            ) || matches!(lhs, Operand::Const(_))
                || matches!(rhs, Operand::Const(_))
            {
                continue;
            }
            for operand in [lhs, rhs] {
                if let Some(name) = class_of(locals, operand) {
                    references.insert(name);
                }
            }
        }
    };
    for function in &mir.functions {
        record(&function.locals, &function.blocks);
    }
    for closure in &mir.closures {
        record(&closure.locals, &closure.blocks);
    }
}

/// An object shape whose value is handed to a slot of an erased or dictionary
/// type: an argument, an assignment or a return whose destination is
/// `unknown`/`any`, a type parameter or `Record<K, V>`
/// ([`for_each_record_flow`]), or an operation that puts it into an erased
/// container (`unknown[]`, `Set<unknown>`, a dictionary;
/// [`for_each_record_beside_an_erased_container`]).
///
/// JavaScript hands that slot the object itself. Whoever holds it can write a
/// property, add or delete one, keep it and read it later, or compare it by
/// identity, and none of that is visible in the shape's own typed uses, which
/// are all the other triggers look at. A by-value struct erases to a fresh
/// snapshot each time, so the callee of `function bump(p: any) { p.x = 1 }`
/// wrote a copy and `held === value` through two `unknown` slots was `false`.
/// The reference representation gives the slot a view of the one object
/// (`emitter::live_view`).
///
/// Only shapes: a class instance's erased view is the snapshot-plus-hook of
/// `emitter::view_write_through`, with its own trigger
/// ([`collect_bound_receiver_triggers`]). Not a callable object
/// ([`is_callable_shape`]), which crosses as a function value, nor a shape
/// with a getter-backed property ([`has_accessor_fields`]), nor a GENERIC
/// shape: its erased view is still the snapshot (`emitter::live_view`), so
/// lifting it would share nothing, and a value of it at other type arguments
/// (`Matcher<unknown, string>` pushed into a `Matcher<unknown, unknown>[]`,
/// ts-pattern) is converted field by field, which only the by-value struct
/// spells. Only a value
/// read straight from a local is seen ([`for_each_record_flow`]); a value that
/// reaches such a slot some other way keeps the representation its other uses
/// give it, as before this trigger existed.
fn collect_view_flow_triggers(mir: &Mir, references: &mut HashSet<Symbol>) {
    let liftable = |name: Symbol| {
        mir.interfaces.iter().any(|interface| {
            interface.name == name
                && interface.type_params.is_empty()
                && !is_callable_shape(mir, interface)
                && !has_accessor_fields(mir, interface)
        })
    };
    for_each_record_flow(mir, |name, slot| {
        let view_slot = matches!(
            mir.types.get(slot),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Dict(_, _))
        );
        if view_slot && liftable(name) {
            references.insert(name);
        }
    });
    for_each_record_beside_an_erased_container(mir, |name| {
        if liftable(name) {
            references.insert(name);
        }
    });
}

/// Whether a value of type `ty` holds, at any depth of its element types, a
/// slot an object crosses into erased: `unknown`/`any`, a type parameter, or a
/// dictionary. A record's own fields are not looked into (a record is a
/// record), nor a function's signature (a call is judged at the call).
fn holds_erased_slot(mir: &Mir, ty: smelt_hir::TypeId, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    match mir.types.get(ty) {
        Some(Type::Unknown | Type::TypeParam { .. } | Type::Dict(_, _)) => true,
        Some(Type::Class { .. } | Type::Function(_)) | None => false,
        Some(other) => other
            .children()
            .any(|child| holds_erased_slot(mir, child, depth.saturating_add(1))),
    }
}

/// Call `beside(record)` for every record value an operation reads next to an
/// erased container: the operation's destination, the container it assigns
/// into, or another of its operands has a type that holds an erased slot
/// ([`holds_erased_slot`]).
///
/// This is how a record reaches an erased slot WITHOUT a call, assignment or
/// return ([`for_each_record_flow`]): `registry.push(record)` into an
/// `unknown[]`, `seen.add(record)` on a `Set<unknown>`, `[record]` built at
/// `Record<string, unknown>[]`, `bag[key] = record`. No operation is named:
/// the IR's own traversal ([`Statement::reads`]) says what each one reads, so
/// a new collection operation is covered the day it is added. An operation
/// whose types hold no erased slot (`records.push(record)` on a `Point[]`,
/// `JSON.stringify(record)`) hands the record to nothing that could keep or
/// write it erased, and is not a flow.
fn for_each_record_beside_an_erased_container(mir: &Mir, mut beside: impl FnMut(Symbol)) {
    let strip = |mut ty: smelt_hir::TypeId| {
        while let Some(Type::Optional(inner)) = mir.types.get(ty) {
            ty = *inner;
        }
        ty
    };
    let local_ty = |locals: &[smelt_mir::LocalDecl], local: LocalId| {
        usize::try_from(local.0)
            .ok()
            .and_then(|index| locals.get(index))
            .map(|decl| decl.ty)
    };
    let mut scan = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
        for statement in blocks.iter().flat_map(|block| &block.statements) {
            // A plain move or copy between locals is a flow of its own kind.
            if matches!(
                statement,
                Statement::Assign {
                    value: Rvalue::Use(_),
                    ..
                }
            ) {
                continue;
            }
            let mut records = Vec::new();
            let mut erased = match statement {
                Statement::Assign { dest, .. } => {
                    local_ty(locals, *dest).is_some_and(|ty| holds_erased_slot(mir, ty, 0))
                }
                // A projected target's container is visited below as the
                // statement's base read, and a fused entry update's `current`
                // is a primitive scalar, which holds no erased slot.
                Statement::AssignPlace { .. }
                | Statement::DictEntryUpdate { .. }
                | Statement::StorageLive(_) => false,
            };
            statement.reads(|read| {
                let local = match read {
                    smelt_mir::Read::Base(local) => Some(local),
                    smelt_mir::Read::Operand(
                        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)),
                    ) => Some(*local),
                    smelt_mir::Read::Operand(_) => None,
                };
                let Some(ty) = local.and_then(|read_local| local_ty(locals, read_local)) else {
                    return;
                };
                match mir.types.get(strip(ty)) {
                    Some(Type::Class { name, .. }) if is_record_type_symbol(mir, *name) => {
                        records.push(*name);
                    }
                    _ => erased |= holds_erased_slot(mir, ty, 0),
                }
            });
            if erased {
                records.into_iter().for_each(&mut beside);
            }
        }
    };
    for function in &mir.functions {
        scan(&function.locals, &function.blocks);
    }
    for closure in &mir.closures {
        scan(&closure.locals, &closure.blocks);
    }
}

/// Call `flow(record, slot)` for every value of a record type (a class or an
/// object shape, named `record`) that a body hands to a slot of type `slot`:
/// an argument of a direct or indirect call against the callee's parameter,
/// an assignment to a local, a `return` against the body's return type.
/// `Optional` is looked through on both sides.
///
/// Only a value read straight from a local is seen. The two readers, the
/// classification above and the runtime demand scan
/// (`RuntimeDemand::of`, for a record converted at a `Record<K, V>` slot),
/// each treat a flow this misses as they did before either existed.
pub(crate) fn for_each_record_flow(mir: &Mir, mut flow: impl FnMut(Symbol, smelt_hir::TypeId)) {
    let strip = |mut ty: smelt_hir::TypeId| {
        while let Some(Type::Optional(inner)) = mir.types.get(ty) {
            ty = *inner;
        }
        ty
    };
    for_each_value_flow(mir, |source, slot| {
        if let Some(Type::Class { name, .. }) = mir.types.get(strip(source))
            && is_record_type_symbol(mir, *name)
        {
            flow(*name, strip(slot));
        }
    });
}

/// Call `flow(source, slot)` for every value a body hands to a slot, with the
/// value's own type and the slot's: an argument of a direct or indirect call
/// against the callee's parameter, an assignment to a local, a `return`
/// against the body's return type.
///
/// Only a value read straight from a local is seen; see
/// [`for_each_record_flow`], which is this restricted to record values.
pub(crate) fn for_each_value_flow(
    mir: &Mir,
    flow: impl FnMut(smelt_hir::TypeId, smelt_hir::TypeId),
) {
    scan_value_flows(mir, false, flow);
}

/// [`for_each_value_flow`], and also the flows into a slot that is part of
/// another value: a property of a record literal against the property's
/// declared type, an element of an array literal against the array's element
/// type, a store into a field (`holder.part = value`) or a module-level
/// binding against its declared type.
///
/// Read by `crate::structural_views` only, which pairs record types on both
/// sides; the narrower scan keeps deciding what the erased-slot trigger lifts.
pub(crate) fn for_each_value_flow_into_parts(
    mir: &Mir,
    flow: impl FnMut(smelt_hir::TypeId, smelt_hir::TypeId),
) {
    scan_value_flows(mir, true, flow);
}

/// The declared type of the field `field` of the record type named `record`
/// (a class, heritage included, or an object shape).
fn record_field_type(mir: &Mir, record: Symbol, field: Symbol) -> Option<smelt_hir::TypeId> {
    let fields = match crate::classes::find_class(mir, record) {
        Some(class) => crate::classes::effective_class_fields(mir, class),
        None => crate::classes::effective_interface_fields(
            mir,
            mir.interfaces
                .iter()
                .find(|interface| interface.name == record)?,
        ),
    };
    fields
        .into_iter()
        .find(|candidate| candidate.name == field)
        .map(|candidate| candidate.ty)
}

/// The declared type of the property spelled `key` in source of the record
/// type named `record`.
fn record_property_type(mir: &Mir, record: Symbol, key: &str) -> Option<smelt_hir::TypeId> {
    let fields = match crate::classes::find_class(mir, record) {
        Some(class) => crate::classes::effective_class_fields(mir, class),
        None => crate::classes::effective_interface_fields(
            mir,
            mir.interfaces
                .iter()
                .find(|interface| interface.name == record)?,
        ),
    };
    fields
        .into_iter()
        .find(|candidate| {
            mir.names
                .get(candidate.name)
                .or_else(|| mir.symbols.get(candidate.name))
                == Some(key)
        })
        .map(|candidate| candidate.ty)
}

/// The scan behind [`for_each_value_flow`] (`parts` off) and
/// [`for_each_value_flow_into_parts`] (`parts` on).
fn scan_value_flows(
    mir: &Mir,
    parts: bool,
    mut flow: impl FnMut(smelt_hir::TypeId, smelt_hir::TypeId),
) {
    let strip = |mut ty: smelt_hir::TypeId| {
        while let Some(Type::Optional(inner)) = mir.types.get(ty) {
            ty = *inner;
        }
        ty
    };
    let local_ty = |locals: &[smelt_mir::LocalDecl], local: LocalId| {
        usize::try_from(local.0)
            .ok()
            .and_then(|index| locals.get(index))
            .map(|decl| decl.ty)
    };
    let mut scan = |locals: &[smelt_mir::LocalDecl],
                    blocks: &[smelt_mir::BasicBlock],
                    return_ty: smelt_hir::TypeId| {
        let mut hand = |operand: &Operand, slot: smelt_hir::TypeId| {
            let (Operand::Copy(place) | Operand::Move(place)) = operand else {
                return;
            };
            let handed = match place {
                Place::Local(local) => local_ty(locals, *local),
                // A value read out of a record's field or a container's
                // element, handed on without a local of its own.
                Place::Field { base, field } if parts => {
                    match local_ty(locals, *base).and_then(|ty| mir.types.get(strip(ty))) {
                        Some(Type::Class { name, .. }) => record_field_type(mir, *name, *field),
                        _ => None,
                    }
                }
                Place::Index { base, .. } if parts => {
                    match local_ty(locals, *base).and_then(|ty| mir.types.get(strip(ty))) {
                        Some(Type::List(element) | Type::Dict(_, element)) => Some(*element),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(source) = handed {
                flow(source, slot);
            }
        };
        for block in blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Use(operand),
                } = statement
                    && let Some(slot) = local_ty(locals, *dest)
                {
                    hand(operand, slot);
                }
                if !parts {
                    continue;
                }
                match statement {
                    Statement::Assign {
                        value: Rvalue::Struct { class, fields },
                        ..
                    } => {
                        for (field, operand) in fields {
                            if let Some(slot) = record_field_type(mir, *class, *field) {
                                hand(operand, slot);
                            }
                        }
                    }
                    // A record literal is a keyed aggregate at a record
                    // type: each value lands in the property its key names.
                    Statement::Assign {
                        dest,
                        value: Rvalue::Dict(entries),
                    } => {
                        if let Some(Type::Class { name, .. }) =
                            local_ty(locals, *dest).and_then(|ty| mir.types.get(strip(ty)))
                        {
                            for (key, operand) in entries {
                                let Operand::Const(smelt_mir::Constant::String(property)) = key
                                else {
                                    continue;
                                };
                                if let Some(slot) = record_property_type(mir, *name, property) {
                                    hand(operand, slot);
                                }
                            }
                        }
                    }
                    Statement::Assign {
                        dest,
                        value: Rvalue::List(items),
                    } => {
                        if let Some(Type::List(element)) =
                            local_ty(locals, *dest).and_then(|ty| mir.types.get(strip(ty)))
                        {
                            for operand in items {
                                hand(operand, *element);
                            }
                        }
                    }
                    Statement::Assign {
                        value: Rvalue::GlobalSet { global, value },
                        ..
                    } => {
                        if let Some(slot) = usize::try_from(*global)
                            .ok()
                            .and_then(|index| mir.globals.get(index))
                        {
                            hand(value, slot.ty);
                        }
                    }
                    Statement::AssignPlace {
                        place: Place::Field { base, field },
                        value: Rvalue::Use(operand),
                    } => {
                        if let Some(Type::Class { name, .. }) =
                            local_ty(locals, *base).and_then(|ty| mir.types.get(strip(ty)))
                            && let Some(slot) = record_field_type(mir, *name, *field)
                        {
                            hand(operand, slot);
                        }
                    }
                    _ => {}
                }
            }
            match &block.terminator {
                Some(smelt_mir::Terminator::Call { callee, args, .. }) => {
                    let static_params = |function: &smelt_mir::FuncId| -> Vec<smelt_hir::TypeId> {
                        usize::try_from(function.0)
                            .ok()
                            .and_then(|index| mir.functions.get(index))
                            .map(|target| {
                                target
                                    .params
                                    .iter()
                                    .filter_map(|param| local_ty(&target.locals, *param))
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    // `(arguments, parameter types)`: a base initializer's
                    // first argument is the derived receiver, and the rest are
                    // the base constructor's own arguments (`super(record)`).
                    let (handed, params): (&[Operand], Vec<smelt_hir::TypeId>) = match callee {
                        smelt_mir::Callee::Static(function) => (args, static_params(function)),
                        smelt_mir::Callee::BaseInit(function) => {
                            (args.get(1..).unwrap_or_default(), static_params(function))
                        }
                        smelt_mir::Callee::Indirect(operand) => {
                            let params = operand
                                .local()
                                .and_then(|local| local_ty(locals, local))
                                .and_then(|ty| mir.types.get(strip(ty)));
                            if let Some(Type::Function(function)) = params {
                                (args, function.params.clone())
                            } else {
                                (args, Vec::new())
                            }
                        }
                        // A builtin's or an op's operands are not parameters
                        // of a body: what they keep is read beside the
                        // container they write (see above).
                        smelt_mir::Callee::Builtin(_) | smelt_mir::Callee::Op(..) => {
                            (args, Vec::new())
                        }
                    };
                    for (operand, slot) in handed.iter().zip(params) {
                        hand(operand, slot);
                    }
                }
                Some(smelt_mir::Terminator::Return(operand)) => hand(operand, return_ty),
                _ => {}
            }
        }
    };
    for function in &mir.functions {
        scan(&function.locals, &function.blocks, function.return_ty);
    }
    for closure in &mir.closures {
        scan(&closure.locals, &closure.blocks, closure.return_ty);
    }
}

/// Whether some body compares, with the deep `==`/`!=` ([`BinOp::Eq`],
/// [`BinOp::NotEq`]: `toEqual` and the structural comparisons, never `===`),
/// two values whose type is or holds a reference object shape of `references`.
///
/// A reference record's own `PartialEq` is identity, so such a comparison is
/// emitted over the records' views (`binary_ops::structural_leaf_equality_expr`,
/// `smelt_live_deep_eq`). Views exist only in a program that carries the
/// erased runtime, so the runtime demand scan asks this to carry it.
///
/// "Holds" follows the containers that comparison walks: a list, an optional,
/// a tuple and a dictionary's values. Only a non-generic shape counts, the
/// ones that can be live records (`emitter::live_view`).
///
/// [`BinOp::Eq`]: smelt_hir::BinOp::Eq
/// [`BinOp::NotEq`]: smelt_hir::BinOp::NotEq
pub(crate) fn deep_compares_a_reference_shape(mir: &Mir, references: &HashSet<Symbol>) -> bool {
    fn holds(mir: &Mir, references: &HashSet<Symbol>, ty: smelt_hir::TypeId, depth: u8) -> bool {
        if depth > 16 {
            return false;
        }
        let deeper = depth.saturating_add(1);
        match mir.types.get(ty) {
            Some(Type::Class { name, args }) => {
                args.is_empty() && references.contains(name) && !mir.has_class(*name)
            }
            Some(Type::List(item) | Type::Optional(item) | Type::Dict(_, item)) => {
                holds(mir, references, *item, deeper)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .any(|item| holds(mir, references, *item, deeper)),
            _ => false,
        }
    }
    if references.is_empty() {
        return false;
    }
    let compares = |locals: &[smelt_mir::LocalDecl], blocks: &[smelt_mir::BasicBlock]| {
        blocks
            .iter()
            .flat_map(|block| &block.statements)
            .any(|statement| {
                let Statement::Assign {
                    value:
                        Rvalue::Binary {
                            op: smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq,
                            lhs,
                            rhs,
                        },
                    ..
                } = statement
                else {
                    return false;
                };
                [lhs, rhs].into_iter().any(|operand| {
                    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) =
                        operand
                    else {
                        return false;
                    };
                    usize::try_from(local.0)
                        .ok()
                        .and_then(|index| locals.get(index))
                        .is_some_and(|decl| holds(mir, references, decl.ty, 0))
                })
            })
    };
    mir.functions
        .iter()
        .any(|function| compares(&function.locals, &function.blocks))
        || mir
            .closures
            .iter()
            .any(|closure| compares(&closure.locals, &closure.blocks))
}

/// Whether `interface` is a callable object: a shape with a call signature,
/// which carries the function in a synthetic `__smelt_call` slot.
///
/// A callable object crosses an erased slot as a FUNCTION value carrying its
/// properties (`emitter::callable_object`), not as an object view, and is
/// built and read as the by-value struct that emitter spells. It is therefore
/// neither lifted by [`collect_view_flow_triggers`] nor a live record.
pub(crate) fn is_callable_shape(mir: &Mir, interface: &smelt_mir::MirInterface) -> bool {
    // The slot is named by its SOURCE spelling, and an interface that extends
    // a callable one inherits it.
    crate::classes::effective_interface_fields(mir, interface)
        .iter()
        .any(|field| {
            mir.names
                .get(field.name)
                .or_else(|| mir.symbols.get(field.name))
                == Some(smelt_stdlib::runtime_symbols::markers::SMELT_CALL)
        })
}

/// Whether some property of `interface` is stored as an accessor: an object
/// literal supplies it through `get f()` (`accessor_prelude`).
///
/// Accessor storage is spelled on the by-value struct only; the shared-cell
/// record has plain fields, which would freeze a getter at the value it had
/// when the record was built. Such a shape is therefore not lifted by
/// [`collect_view_flow_triggers`], and is not a live record.
pub(crate) fn has_accessor_fields(mir: &Mir, interface: &smelt_mir::MirInterface) -> bool {
    mir.accessor_fields
        .iter()
        .any(|(owner, _)| *owner == interface.name)
}

/// Whether `name` is a record type this crate declares: a class, or an object
/// shape (`interface` / lowered type literal). See [`class_name_of_local`].
fn is_record_type_symbol(mir: &Mir, name: Symbol) -> bool {
    mir.has_class(name)
        || mir
            .interfaces
            .iter()
            .any(|interface| interface.name == name)
}

/// Record classes mutated through an in-place collection method on a field.
///
/// A call such as `this.data.set(k, v)` or `this.items.push(x)` lowers to a
/// `&mut`-taking collection method on `this.<field>`. If the owning class is
/// left as a by-value struct its methods take `&self`, so the generated field
/// mutation fails to borrow-check (`E0596`). Detecting the mutation here lifts
/// the class to the reference (handle) representation, whose interior
/// mutability lets `&self` methods mutate the shared field — matching the
/// direct-field-write trigger in [`collect_field_write_triggers`]. The same
/// constructor exclusion applies: a constructor populating the instance it is
/// building is construction, not post-construction mutation.
fn collect_field_mutation_triggers(
    mir: &Mir,
    function: &MirFunction,
    references: &mut HashSet<Symbol>,
) {
    let constructing_class = crate::base_init::constructor_class(function);
    for block in &function.blocks {
        for statement in &block.statements {
            let Statement::Assign { value, .. } = statement else {
                continue;
            };
            let Some(mutated) = value.mutated_receiver() else {
                continue;
            };
            // Only an in-place mutation of an instance *field* matters; a
            // mutation of a plain local collection borrows that local mutably
            // and needs no shared cell.
            let base = match operand_field_base(mutated) {
                Some(base) => base,
                None => continue,
            };
            let Some(name) = class_name_of_local(mir, function, base) else {
                continue;
            };
            if constructing_class == Some(name) {
                continue;
            }
            references.insert(name);
        }
    }
}

/// Return the base local of an operand that reads through a field/index
/// projection (`this.<field>`), or `None` for a bare local or constant.
fn operand_field_base(operand: &Operand) -> Option<LocalId> {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Const(_) => return None,
    };
    match place {
        Place::Field { base, .. } | Place::Index { base, .. } => Some(*base),
        Place::Local(_) | Place::Global { .. } => None,
    }
}

/// Record classes whose method captures `this` into a closure.
///
/// A closure that captures the method receiver needs a shareable handle to the
/// same object (the resolver stored in `deferredTasks` is the canonical case),
/// which only the reference representation provides.
fn collect_self_capture_triggers(
    mir: &Mir,
    function: &MirFunction,
    references: &mut HashSet<Symbol>,
) {
    let (class, self_local) = match function.origin {
        HirOrigin::ClassMethod { class, .. } => {
            let Some(self_local) = function.params.first().copied() else {
                return;
            };
            (class, self_local)
        }
        // A CONSTRUCTOR whose instance escapes into a closure needs the same
        // handle. A class-field arrow (`status = (s) => { this.#status = s }`)
        // is initialized in the constructor and captures the instance under
        // construction, then mutates it long after the constructor returned.
        // With a by-value struct the closure mutates the constructor's cell and
        // the returned struct is a snapshot of it, so every write through such
        // an arrow is lost. The constructor's own direct writes stay excluded in
        // `collect_field_write_triggers` -- those really are construction; this
        // trigger is about the instance ESCAPING, which construction is not.
        HirOrigin::ClassConstructor { class, .. } => {
            let Some(self_local) = constructor_instance_local(mir, function, class) else {
                return;
            };
            (class, self_local)
        }
        // A static method or a free body has no instance to capture.
        HirOrigin::ClassStaticMethod { .. } | HirOrigin::Body(_) => return,
    };
    for block in &function.blocks {
        for statement in &block.statements {
            let Statement::Assign {
                value: Rvalue::Closure { captures, .. },
                ..
            } = statement
            else {
                continue;
            };
            if captures
                .iter()
                .any(|capture| operand_base_local(capture) == Some(self_local))
            {
                references.insert(class);
            }
        }
    }
}

/// Return the local a constructor builds its instance in (`this`).
///
/// A constructor's instance is a user binding (not a parameter, which is what
/// [`collect_self_capture_triggers`] uses for a method's receiver), so it is
/// identified by its type: the first user binding whose type is the class under
/// construction. That is the local a class-field arrow captures.
pub(crate) fn constructor_instance_local(
    mir: &Mir,
    function: &MirFunction,
    class: Symbol,
) -> Option<LocalId> {
    function
        .locals
        .iter()
        .enumerate()
        .filter(|(_, decl)| matches!(decl.kind, smelt_mir::LocalKind::UserBinding(_)))
        .filter_map(|(index, _)| u32::try_from(index).ok().map(LocalId))
        .find(|local| class_name_of_local(mir, function, *local) == Some(class))
}

/// Return the class symbol of a local's type, if the local is a *nominal class*.
///
/// Only classes declared in `mir.classes` can carry the handle representation;
/// interfaces and other structurally-typed records are also spelled
/// `Type::Class` internally but stay plain value structs, so they are excluded
/// here to avoid routing their field access through a nonexistent cell.
fn class_name_of_local(mir: &Mir, function: &MirFunction, local: LocalId) -> Option<Symbol> {
    class_name_of_decl(mir, &function.locals, local)
}

/// [`class_name_of_local`] over a body's own locals.
fn class_name_of_decl(
    mir: &Mir,
    locals: &[smelt_mir::LocalDecl],
    local: LocalId,
) -> Option<Symbol> {
    let decl = locals.get(usize::try_from(local.0).ok()?)?;
    match mir.types.get(decl.ty) {
        // An object *shape* — an `interface`, or an inline object type literal
        // lowered to a synthetic one — is emitted as a Rust struct exactly like a
        // value class, and is just as wrong by value once a field is written:
        // `const b = a; b.x = 1` must be visible through `a`. Shapes therefore
        // classify on the same rule as classes.
        Some(Type::Class { name, .. }) if is_record_type_symbol(mir, *name) => Some(*name),
        _ => None,
    }
}

/// Return the base local an operand reads from, ignoring field/index projection.
fn operand_base_local(operand: &Operand) -> Option<LocalId> {
    let place = match operand {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Const(_) => return None,
    };
    match place {
        Place::Local(local) => Some(*local),
        Place::Field { base, .. } | Place::Index { base, .. } => Some(*base),
        // A global-rooted place is an assignment target, never an operand, so
        // it names no local here.
        Place::Global { .. } => None,
    }
}
