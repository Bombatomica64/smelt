//! Closures stored on the instance they capture: the `Rc` cycle a class-field
//! arrow creates, and the rule that breaks it.
//!
//! # The cycle
//!
//! A class field initialized with an arrow function captures the instance under
//! construction (`json = (body) => this.render(body)`), and the arrow is then
//! stored IN that instance. JavaScript's collector reclaims the pair once the
//! instance is unreachable. A reference class is an `Rc<RefCell<Inner>>`
//! handle, so in the generated Rust the instance owns the closure and the
//! closure owns the instance: neither count ever reaches zero, and every such
//! instance leaks together with everything it reaches. Hono's `Context` has
//! seventeen of these fields and is built once per request, which made a
//! generated server leak its whole request graph on every request.
//!
//! # The rule
//!
//! A closure qualifies when, in the constructor of a reference class, it
//! captures the instance and its ONLY use is to be stored into a field of that
//! instance (`%t = closure [.., copy this, ..]; this.f = move %t`). Such a
//! closure captures the instance through `SmeltWeakReceiver` — a `Weak` to the
//! instance cell — and upgrades on each use. While the closure is reachable only
//! through its field, the instance is alive whenever the closure can run, so
//! the upgrade cannot fail and the behavior is exactly the strong capture's.
//!
//! The closure can stop being reachable only through its field in two ways,
//! and each restores a strong path to the instance:
//!
//! * the closure value is copied out of its field (`const f = c.json`, an
//!   argument, a callback registered elsewhere) and kept: when the LAST strong
//!   handle on the instance is dropped, the handle's `Drop` checks whether any
//!   such field's closure is still shared (`SmeltClosureShare`) and, if so,
//!   pins the instance for the rest of the program (`smelt_pin_receiver`) —
//!   exactly the old strong-capture lifetime. Reads that are only called
//!   (`c.json(x)`, including through a temporary) are gone by then and pin
//!   nothing, so no read site has to be classified;
//! * the instance is erased (`IntoSmeltUnknown`, the live callable-field
//!   getter): the erased value of each such field is wrapped to hold the
//!   instance strongly (`smelt_keep_receiver_alive`), so a callable read off the
//!   erased view keeps the instance alive as the arrow would. The erased object
//!   holding the instance does not make a cycle: the instance never holds its
//!   erasure;
//! * the field is overwritten while its closure is shared (only an erased-view
//!   write can do that, see the qualification below): the setter pins first.
//!
//! A field qualifies only when nothing but the constructor's single store
//! writes it, and its type is one whose closure the drop check can see (a
//! function, an optional function, a callable interface's `__smelt_call` slot,
//! or an erased value).
//!
//! A closure created INSIDE such a closure that also captures the instance gets
//! a strong capture (`SmeltWeakReceiver::clone`), because it may outlive the
//! call that created it (a promise continuation, a stored callback).
//!
//! Anything else keeps the strong capture: a closure that is stored and also
//! passed elsewhere, one created outside the constructor, one stored into a
//! different object. Those are not proven cycle-only, so correctness wins and
//! they keep leaking as before.

use std::collections::{HashMap, HashSet};

use smelt_hir::Symbol;
use smelt_hir::Type;
use smelt_mir::{
    BasicBlock, ClosureId, FuncId, LocalDecl, LocalId, Mir, MirFunction, Operand, Place, Rvalue,
    Statement,
};

use super::core::{rvalue_uses_local, terminator_uses_local};

/// The closures that capture their receiver weakly, and the fields they live in.
#[derive(Debug, Default, Clone)]
pub(crate) struct WeakReceiverPlan {
    /// Closures whose capture of the constructed instance is weak.
    closures: HashSet<ClosureId>,
    /// `(class, field)` pairs initialized with such a closure.
    fields: HashSet<(Symbol, Symbol)>,
    /// Classes with at least one such field.
    classes: HashSet<Symbol>,
    /// Listener closures registered on an emitter they capture, with the
    /// enclosing body's emitter local (see [`listener_registrations`]).
    listeners: HashMap<ClosureId, LocalId>,
}

impl WeakReceiverPlan {
    /// Find every qualifying closure in `mir` (see the module docs).
    pub(crate) fn of(mir: &Mir, reference_classes: &HashSet<Symbol>) -> Self {
        let mut plan = Self::default();
        for class in &mir.classes {
            if !reference_classes.contains(&class.name) {
                continue;
            }
            let Some(constructor) = class.constructor else {
                continue;
            };
            let Some(function) = usize::try_from(constructor.0)
                .ok()
                .and_then(|index| mir.functions.get(index))
            else {
                continue;
            };
            let Some(instance) =
                crate::classify::constructor_instance_local(mir, function, class.name)
            else {
                continue;
            };
            for (closure, field) in stored_self_capturing_closures(function, instance) {
                let Some(declared) = class
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == field)
                else {
                    continue;
                };
                if closure_slot(mir, declared.ty).is_none()
                    || field_written_elsewhere(mir, constructor, class.name, field)
                {
                    continue;
                }
                plan.closures.insert(closure);
                plan.fields.insert((class.name, field));
                plan.classes.insert(class.name);
            }
        }
        for function in &mir.functions {
            plan.listeners
                .extend(listener_registrations(&function.blocks));
        }
        for closure in &mir.closures {
            plan.listeners
                .extend(listener_registrations(&closure.blocks));
        }
        // A subclass constructor replays its base's initializer with the
        // subclass as the instance (the same closures, stored into the same
        // inherited fields), so a subclass of a class with weak fields has them
        // too and needs the same handle support.
        loop {
            let inherited = mir
                .classes
                .iter()
                .filter(|class| {
                    !plan.classes.contains(&class.name) && reference_classes.contains(&class.name)
                })
                .filter_map(|class| {
                    class
                        .base
                        .filter(|base| plan.classes.contains(base))
                        .map(|base| (class.name, base))
                })
                .collect::<Vec<_>>();
            if inherited.is_empty() {
                break;
            }
            for (class, base) in inherited {
                let inherited_fields = plan
                    .fields
                    .iter()
                    .filter(|(owner, _)| *owner == base)
                    .map(|(_, field)| (class, *field))
                    .collect::<Vec<_>>();
                plan.fields.extend(inherited_fields);
                plan.classes.insert(class);
            }
        }
        plan
    }

    /// Whether `class` has any weak-receiver field (and so the handle support).
    pub(crate) fn has_weak_fields(&self, class: Symbol) -> bool {
        self.classes.contains(&class)
    }

    /// Whether `closure` captures its receiver weakly.
    pub(crate) fn is_weak_closure(&self, closure: ClosureId) -> bool {
        self.closures.contains(&closure)
    }

    /// Whether `field` of `class` holds a weak-receiver closure.
    pub(crate) fn is_weak_field(&self, class: Symbol, field: Symbol) -> bool {
        self.fields.contains(&(class, field))
    }

    /// The emitter local a listener closure captures weakly, if it does.
    pub(crate) fn weak_listener_emitter(&self, closure: ClosureId) -> Option<LocalId> {
        self.listeners.get(&closure).copied()
    }

    /// Whether any class in the program has a weak-receiver field.
    pub(crate) fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }
}

/// Where the closure of a field of type `ty` lives, for the drop-time check.
///
/// `Some("")` for a value the check reads directly (a function, an optional
/// function, an erased value), `Some(".__smelt_call")` for a callable
/// interface's call slot, `None` for a type the check cannot see into.
pub(crate) fn closure_slot(mir: &Mir, ty: smelt_hir::TypeId) -> Option<&'static str> {
    match mir.types.get(ty)? {
        Type::Function(_) | Type::Unknown => Some(""),
        Type::Optional(inner) if matches!(mir.types.get(*inner), Some(Type::Function(_))) => {
            Some("")
        }
        Type::Class { name, .. } => {
            let call = |field: &smelt_mir::MirField| {
                mir.names
                    .get(field.name)
                    .or_else(|| mir.symbols.get(field.name))
                    == Some("__smelt_call")
            };
            let has_call = mir
                .interfaces
                .iter()
                .any(|interface| interface.name == *name && interface.fields.iter().any(call))
                || mir
                    .classes
                    .iter()
                    .any(|class| class.name == *name && class.fields.iter().any(call));
            has_call.then_some(".__smelt_call")
        }
        _ => None,
    }
}

/// Whether anything other than `constructor` writes `field` of `class`.
///
/// Every function and closure body is scanned for a store into that field of
/// a local of the class type.
fn field_written_elsewhere(mir: &Mir, constructor: FuncId, class: Symbol, field: Symbol) -> bool {
    let writes = |locals: &[LocalDecl], blocks: &[BasicBlock]| {
        blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                let Statement::AssignPlace { place: Place::Field { base, field: written }, .. } = statement else {
                    return false;
                };
                *written == field
                    && usize::try_from(base.0)
                        .ok()
                        .and_then(|index| locals.get(index))
                        .is_some_and(|decl| matches!(mir.types.get(decl.ty), Some(Type::Class { name, .. }) if *name == class))
            })
        })
    };
    mir.functions
        .iter()
        .any(|function| function.id != constructor && writes(&function.locals, &function.blocks))
        || mir
            .closures
            .iter()
            .any(|closure| writes(&closure.locals, &closure.blocks))
}

/// The closures in `function` that capture `instance` and are only stored into
/// one of its fields, with that field.
fn stored_self_capturing_closures(
    function: &MirFunction,
    instance: LocalId,
) -> Vec<(ClosureId, Symbol)> {
    let mut found = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let Statement::Assign {
                dest,
                value: Rvalue::Closure { id, captures },
            } = statement
            else {
                continue;
            };
            if *dest == instance
                || !captures
                    .iter()
                    .any(|capture| operand_is_local(capture, instance))
            {
                continue;
            }
            if let Some(field) = only_use_is_field_store(function, *dest, instance) {
                found.push((*id, field));
            }
        }
    }
    found
}

/// Listener closures registered on an emitter they capture.
///
/// The second shape of the same cycle: `req.on('end', () => use(req))` stores
/// the closure in `req`'s listener list while the closure owns `req`. A
/// listener only runs when its emitter emits, so it never needs to keep the
/// emitter alive, and the runtime models no API that hands a registered
/// listener back out (`listeners()`/`rawListeners()`). A closure qualifies when
/// its ONLY use is to be registered with `on`/`once` on a local it captures,
/// and its body builds no closure of its own (a nested closure could take the
/// emitter somewhere that outlives it).
fn listener_registrations(blocks: &[BasicBlock]) -> Vec<(ClosureId, LocalId)> {
    let statements = || blocks.iter().flat_map(|block| block.statements.iter());
    let mut found = Vec::new();
    for statement in statements() {
        let Statement::Assign {
            dest,
            value: Rvalue::Closure { id, captures },
        } = statement
        else {
            continue;
        };
        let registration = statements().find_map(|candidate| {
            let Statement::Assign {
                value:
                    Rvalue::EventEmitterOp {
                        op: smelt_hir::EventEmitterOp::On | smelt_hir::EventEmitterOp::Once,
                        emitter: receiver,
                        args,
                    },
                ..
            } = candidate
            else {
                return None;
            };
            let (Operand::Copy(Place::Local(receiver_local))
            | Operand::Move(Place::Local(receiver_local))) = receiver
            else {
                return None;
            };
            (args
                .get(1)
                .is_some_and(|listener| operand_is_local(listener, *dest)))
            .then_some(*receiver_local)
        });
        let Some(emitter) = registration else {
            continue;
        };
        let uses = statements()
            .filter(|candidate| match candidate {
                Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => {
                    rvalue_uses_local(value, *dest)
                }
                Statement::DictEntryUpdate { value, .. } => rvalue_uses_local(value, *dest),
                Statement::StorageLive(_) | Statement::StorageDead(_) => false,
            })
            .count()
            .saturating_add(
                blocks
                    .iter()
                    .filter(|block| {
                        block
                            .terminator
                            .as_ref()
                            .is_some_and(|terminator| terminator_uses_local(terminator, *dest))
                    })
                    .count(),
            );
        if uses == 1
            && captures
                .iter()
                .any(|capture| operand_is_local(capture, emitter))
        {
            found.push((*id, emitter));
        }
    }
    found
}

/// Whether `operand` reads exactly the local `local` (not a projection of it).
fn operand_is_local(operand: &Operand, local: LocalId) -> bool {
    matches!(operand, Operand::Copy(Place::Local(candidate)) | Operand::Move(Place::Local(candidate)) if *candidate == local)
}

/// The field of `instance` that `holder` is stored into, when that store is the
/// ONLY use of `holder` anywhere in `function`.
fn only_use_is_field_store(
    function: &MirFunction,
    holder: LocalId,
    instance: LocalId,
) -> Option<Symbol> {
    let mut store = None;
    let mut uses = 0usize;
    for block in &function.blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign { dest, value } => {
                    if *dest == holder && !matches!(value, Rvalue::Closure { .. }) {
                        // Reassigned: the closure is not the only value it holds.
                        return None;
                    }
                    if rvalue_uses_local(value, holder) {
                        uses = uses.saturating_add(1);
                    }
                }
                Statement::AssignPlace { place, value } => {
                    if rvalue_uses_local(value, holder)
                        || super::core::assignment_place_reads_local(place, holder)
                    {
                        uses = uses.saturating_add(1);
                        if let (Place::Field { base, field }, Rvalue::Use(operand)) = (place, value)
                            && *base == instance
                            && operand_is_local(operand, holder)
                        {
                            store = Some(*field);
                        }
                    }
                }
                Statement::DictEntryUpdate {
                    index,
                    default,
                    value,
                    base,
                    ..
                } => {
                    if *base == holder
                        || super::core::operand_uses_local(index, holder)
                        || super::core::operand_uses_local(default, holder)
                        || rvalue_uses_local(value, holder)
                    {
                        uses = uses.saturating_add(1);
                    }
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        if block
            .terminator
            .as_ref()
            .is_some_and(|terminator| terminator_uses_local(terminator, holder))
        {
            uses = uses.saturating_add(1);
        }
    }
    if uses == 1 { store } else { None }
}

/// The runtime support every program with a weak-receiver field carries.
const PRELUDE: &str = r#"/// A reference-class handle seen through its shared cell (see `SmeltWeakReceiver`).
trait SmeltReferenceHandle: Clone + 'static {
    /// The shared cell the handle wraps.
    type Cell;
    /// The handle's shared cell.
    fn smelt_handle_cell(&self) -> &::std::rc::Rc<Self::Cell>;
    /// A handle on an existing shared cell (the SAME instance).
    fn smelt_from_handle_cell(cell: ::std::rc::Rc<Self::Cell>) -> Self;
}

/// The instance a closure stored on it captures, held weakly.
///
/// A class-field arrow lives in the instance it captures; a strong capture is
/// an `Rc` cycle that keeps both alive forever (`emitter::weak_receiver`). The
/// closure is only reachable through its field — every other path to it pins
/// or keeps the instance alive — so whenever it runs the instance is alive and
/// the upgrade succeeds.
enum SmeltWeakReceiver<H: SmeltReferenceHandle> {
    /// The capture of a closure stored on its own receiver.
    Weak(::std::rc::Weak<H::Cell>),
    /// The capture of a closure created inside one, which may outlive it.
    Strong(H),
}

impl<H: SmeltReferenceHandle> SmeltWeakReceiver<H> {
    /// Capture `receiver` weakly.
    fn new(receiver: &H) -> Self { Self::Weak(::std::rc::Rc::downgrade(receiver.smelt_handle_cell())) }
    /// The receiver itself.
    fn smelt_receiver(&self) -> H {
        match self {
            Self::Weak(cell) => H::smelt_from_handle_cell(cell.upgrade().expect("a closure stored on an instance ran after the instance was dropped")),
            Self::Strong(receiver) => receiver.clone(),
        }
    }
    /// The receiver, in the shape shared captures read
    /// (`(*capture.smelt_receiver_cell().borrow_mut())`): an upgraded handle,
    /// not a new cell, so a use costs one refcount and no allocation.
    fn smelt_receiver_cell(&self) -> SmeltReceiverRef<H> { SmeltReceiverRef(self.smelt_receiver()) }
    /// The capture a NESTED closure takes: STRONG, because a closure created
    /// while this one runs (a promise continuation, a stored callback) can
    /// outlive every other handle on the receiver.
    ///
    /// Deliberately an inherent `clone` rather than `Clone`: a nested closure's
    /// capture prelude is `let c = c.clone();` for every capture kind, and this
    /// is the one kind whose copy must change strength.
    #[allow(clippy::should_implement_trait)]
    fn clone(&self) -> Self { Self::Strong(self.smelt_receiver()) }
}

/// Whether the closure held in a weak-receiver field is shared beyond the field.
///
/// Read by a weak-receiver class's `Drop` when its LAST strong handle goes: a
/// closure someone else still holds can still run, so its instance is pinned.
trait SmeltClosureShare { fn smelt_closure_shared(&self) -> bool; }

impl<F: ?Sized> SmeltClosureShare for ::std::rc::Rc<F> {
    fn smelt_closure_shared(&self) -> bool { ::std::rc::Rc::strong_count(self) > 1 }
}

impl<T: SmeltClosureShare> SmeltClosureShare for Option<T> {
    fn smelt_closure_shared(&self) -> bool { self.as_ref().is_some_and(SmeltClosureShare::smelt_closure_shared) }
}

/// An upgraded weak receiver, read the way a shared capture cell is read.
///
/// `borrow`/`borrow_mut` hand out the handle itself; the handle's own cell
/// does the interior borrowing, exactly as for a strongly captured `this`.
struct SmeltReceiverRef<H>(H);

impl<H> SmeltReceiverRef<H> {
    #[allow(dead_code)]
    fn borrow(&self) -> &H { &self.0 }
    #[allow(dead_code)]
    fn borrow_mut(&mut self) -> &mut H { &mut self.0 }
}

/// Keep `receiver` alive for the rest of the program.
///
/// Called when a weak-receiver closure can outlive every handle on its
/// instance (see `SmeltClosureShare`): a strong capture would have kept the
/// instance alive for as long as the closure lives, and leaking one strong
/// handle is exactly that lifetime with no table to maintain.
#[allow(dead_code)]
fn smelt_pin_receiver<H: SmeltReferenceHandle>(receiver: &H) {
    ::std::mem::forget(::std::rc::Rc::clone(receiver.smelt_handle_cell()));
}"#;

/// The erasure-side helper, emitted only with the erased runtime.
const UNKNOWN_PRELUDE: &str = r#"/// An erased field value is shared when its callable (or its object) is.
impl SmeltClosureShare for SmeltUnknown {
    fn smelt_closure_shared(&self) -> bool {
        match self {
            SmeltUnknown::Function(function) => ::std::rc::Rc::strong_count(function) > 1,
            SmeltUnknown::Object(object) => ::std::rc::Rc::strong_count(&object.store) > 1 || object.store.borrow().get("__smelt_call").is_some_and(|call| matches!(call, SmeltUnknown::Function(function) if ::std::rc::Rc::strong_count(function) > 1)),
            _ => false,
        }
    }
}

/// The erased value of a weak-receiver field, holding its instance strongly.
///
/// A callable read off an erased view can outlive the view and the instance's
/// other handles, so it carries a strong handle the way the arrow's own
/// capture would. It is the same JavaScript function (it shares the field
/// value's identity, and through it its `length` and properties).
#[allow(dead_code)]
fn smelt_keep_receiver_alive<H: SmeltReferenceHandle>(erased: SmeltUnknown, receiver: &H) -> SmeltUnknown {
    let wrap = |function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>| -> SmeltUnknown {
        let keep = receiver.clone();
        let inner = function.clone();
        let kept: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let _ = &keep; inner(args) });
        smelt_link_function_identity(&kept, &function);
        SmeltUnknown::Function(kept)
    };
    match erased {
        SmeltUnknown::Function(function) => wrap(function),
        // A callable-interface field erases to a FRESH `{ __smelt_call, .. }`
        // object per erasure, so its call slot is rewrapped in place.
        SmeltUnknown::Object(object) => { if let Some(SmeltUnknown::Function(function)) = object.get("__smelt_call") { object.insert("__smelt_call".to_owned(), wrap(function)); } SmeltUnknown::Object(object) }
        other => other,
    }
}"#;

/// Emit the weak-receiver runtime; `needs_unknown` adds the erasure helpers,
/// `needs_erased_function` the share check of `SmeltErasedFunction`.
pub(crate) fn emit_prelude(
    writer: &mut crate::rust::CodeWriter,
    needs_unknown: bool,
    needs_erased_function: bool,
) {
    for line in PRELUDE.lines() {
        writer.line(line);
    }
    writer.blank_line();
    if needs_unknown {
        for line in UNKNOWN_PRELUDE.lines() {
            writer.line(line);
        }
        writer.blank_line();
    }
    if needs_erased_function {
        writer.line("impl SmeltClosureShare for SmeltErasedFunction { fn smelt_closure_shared(&self) -> bool { ::std::rc::Rc::strong_count(&self.callback) > 1 } }");
        writer.blank_line();
    }
}

/// The `Drop` of a reference class with weak-receiver fields.
///
/// `fields` pairs each weak field's Rust name with its closure slot (see
/// [`closure_slot`]). Only the LAST strong handle checks: while any other
/// handle exists the instance is alive anyway. `type_params` is the struct's
/// own (unbounded) generic list: a `Drop` impl may not add bounds, which is
/// also why the pin leaks a handle rather than storing it in a table.
pub(crate) fn drop_impl_text(
    name: &str,
    type_params: &str,
    type_args: &str,
    fields: &[(String, &str)],
) -> String {
    let shared = fields
        .iter()
        .map(|(field, slot)| format!("inner.{field}{slot}.smelt_closure_shared()"))
        .collect::<Vec<_>>()
        .join(" || ");
    format!(
        "impl{type_params} Drop for {name}{type_args} {{ fn drop(&mut self) {{ if ::std::rc::Rc::strong_count(&self.0) == 1 {{ let shared = self.0.try_borrow().map_or(true, |inner| {shared}); if shared {{ ::std::mem::forget(::std::rc::Rc::clone(&self.0)); }} }} }} }}"
    )
}

/// The `SmeltReferenceHandle` impl of one reference class with weak fields.
pub(crate) fn handle_impl_text(name: &str, impl_generics: &str, type_args: &str) -> String {
    format!(
        "impl{impl_generics} SmeltReferenceHandle for {name}{type_args} {{ type Cell = ::std::cell::RefCell<{name}Inner{type_args}>; fn smelt_handle_cell(&self) -> &::std::rc::Rc<Self::Cell> {{ &self.0 }} fn smelt_from_handle_cell(cell: ::std::rc::Rc<Self::Cell>) -> Self {{ Self(cell) }} }}"
    )
}

/// The capture-prelude line of a weak-receiver capture named `name`.
///
/// `shared` says whether the instance binding already lives in a shared
/// capture cell (`smelt_capture_<name>`) or is a plain binding.
pub(crate) fn capture_prelude_text(name: &str, source: &str, shared: bool) -> String {
    if shared {
        format!(
            "let smelt_capture_{name} = SmeltWeakReceiver::new(&*smelt_capture_{name}.borrow());"
        )
    } else {
        format!("let smelt_capture_{name} = SmeltWeakReceiver::new(&{source});")
    }
}

/// How a weak-receiver capture named `name` is read inside the closure body.
///
/// The shape of a shared capture (`(*smelt_capture_x.borrow_mut())`), so
/// nested closures and the rendered-text rewrites treat it as one; see
/// `rendered_text_rewrite::WEAK_RECEIVER_UPGRADE`.
pub(crate) fn capture_access_text(name: &str) -> String {
    format!(
        "(*smelt_capture_{name}{}.borrow_mut())",
        super::rendered_text_rewrite::WEAK_RECEIVER_UPGRADE
    )
}

impl super::FunctionEmitter<'_> {
    /// Whether `capture` of closure `closure` is the weak capture of the
    /// emitter the closure is registered on (see [`listener_registrations`]).
    ///
    /// Only emitter types with a weak handle qualify (`SmeltEmitterHandle`).
    pub(super) fn is_weak_emitter_capture(
        &self,
        closure: ClosureId,
        capture: &smelt_mir::MirClosureCapture,
    ) -> bool {
        if self.context.weak_receivers().weak_listener_emitter(closure)
            != Some(capture.source_local)
        {
            return false;
        }
        if self
            .mir
            .closures
            .get(usize::try_from(closure.0).unwrap_or(usize::MAX))
            .is_none_or(|body| {
                body.blocks.iter().any(|block| {
                    block.statements.iter().any(|statement| {
                        matches!(
                            statement,
                            Statement::Assign {
                                value: Rvalue::Closure { .. },
                                ..
                            }
                        )
                    })
                })
            })
        {
            return false;
        }
        self.local_decl(capture.source_local)
            .ok()
            .and_then(|local| {
                self.rust_type(
                    local.ty,
                    false,
                    &crate::type_substitution::TypeSubstitution::erased(),
                )
                .ok()
            })
            .is_some_and(|text| {
                matches!(text.as_str(), "SmeltEventEmitter" | "SmeltIncomingMessage")
            })
    }

    /// Whether `field` of the class this function belongs to is a
    /// weak-receiver field.
    pub(super) fn weak_receiver_field(&self, field: Symbol) -> bool {
        let class = match self.function.origin {
            smelt_mir::HirOrigin::ClassMethod { class, .. }
            | smelt_mir::HirOrigin::ClassConstructor { class, .. } => class,
            _ => return false,
        };
        self.context.weak_receivers().is_weak_field(class, field)
    }

    /// Whether `capture` of closure `closure` is the weak capture of the
    /// instance the closure is stored on (see the module docs).
    pub(super) fn is_weak_receiver_capture(
        &self,
        closure: ClosureId,
        capture: &smelt_mir::MirClosureCapture,
    ) -> bool {
        if !self.context.weak_receivers().is_weak_closure(closure) {
            return false;
        }
        let Ok(local) = self.local_decl(capture.source_local) else {
            return false;
        };
        let Some(Type::Class { name, .. }) = self.mir.types.get(local.ty) else {
            return false;
        };
        self.context.weak_receivers().has_weak_fields(*name)
            && crate::classify::constructor_instance_local(self.mir, self.function, *name)
                == Some(capture.source_local)
    }
}

/// The capture-prelude line of a weak emitter capture named `name`.
pub(crate) fn emitter_prelude_text(name: &str, source: &str) -> String {
    format!("let smelt_weak_{name} = SmeltWeakEmitter::new(&{source});")
}

/// How a weak emitter capture named `name` is read inside the listener body.
pub(crate) fn emitter_access_text(name: &str) -> String {
    format!("smelt_weak_{name}.smelt_emitter()")
}
