//! Forward-referenced arrow consts: top-level `const f = () => ..` bindings
//! lifted to items ahead of source order, queued behind the module classes their
//! bodies read.

use std::collections::HashSet;
use crate::SmeltError;
use oxc::ast::ast::{
    BindingPattern, Declaration, Expression, ObjectPropertyKind, Program, Statement,
};
use oxc::span::GetSpan;
use smelt_hir::Module;
use super::phases::SignaturesDeclared;
use super::super::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Return true for top-level const-arrow declarations already emitted as items.
    pub(in crate::lowering) fn is_predeclared_arrow_const_statement(&self, statement: &Statement<'_>) -> bool {
        let Statement::VariableDeclaration(decl) = statement else {
            return false;
        };
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return false;
        }
        !decl.declarations.is_empty()
            && decl.declarations.iter().all(|declarator| {
                matches!(
                    declarator.init,
                    Some(Expression::ArrowFunctionExpression(_))
                ) && matches!(
                    &declarator.id,
                    BindingPattern::BindingIdentifier(binding)
                        if self.items.contains_key(binding.name.as_str())
                )
            })
    }

    /// Lower top-level local arrow `const` declarations into private callable items.
    pub(in crate::lowering) fn arrow_function_const_item_declarations(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        forward_arrow_consts: &HashSet<String>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                continue;
            };
            if !forward_arrow_consts.contains(binding.name.as_str()) {
                continue;
            }
            let type_hint = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let item = self.private_arrow_function_const_declaration(
                binding.name.as_str(),
                arrow,
                type_hint,
            )?;
            items.push(item);
        }
        Ok(items)
    }

    /// Whether an exported `const` statement can join the forward-arrow queue.
    ///
    /// Only a statement whose EVERY declarator is a plain-identifier arrow in
    /// the forward-referenced set qualifies, so lowering the whole statement
    /// early through `const_item_declarations` never also moves a non-arrow
    /// initializer (an object, a call) ahead of the values it reads.
    pub(in crate::lowering) fn is_liftable_exported_arrow_const(
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        forward_arrow_consts: &HashSet<String>,
    ) -> bool {
        !decl.declare
            && decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
            && !decl.declarations.is_empty()
            && decl.declarations.iter().all(|declarator| {
                matches!(
                    (&declarator.id, &declarator.init),
                    (
                        BindingPattern::BindingIdentifier(binding),
                        Some(Expression::ArrowFunctionExpression(_)),
                    ) if forward_arrow_consts.contains(binding.name.as_str())
                )
            })
    }

    /// Return top-level arrow binding names declared by one variable statement.
    pub(in crate::lowering) fn arrow_const_declaration_names(decl: &oxc::ast::ast::VariableDeclaration<'_>) -> Vec<String> {
        decl.declarations
            .iter()
            .filter_map(|declarator| {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    return None;
                };
                matches!(
                    declarator.init,
                    Some(Expression::ArrowFunctionExpression(_))
                )
                .then(|| binding.name.to_string())
            })
            .collect()
    }

    /// Source text of every class declaration in this module, by class name.
    ///
    /// Feeds [`ForwardArrowQueue::classes`]: a forward-lifted arrow whose body
    /// names one of these classes waits until that class is lowered.
    pub(super) fn module_class_spans(program: &Program<'_>) -> Vec<(String, oxc::span::Span)> {
        program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::ClassDeclaration(class) => Some(class.as_ref()),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::ClassDeclaration(class) => Some(class.as_ref()),
                    _ => None,
                },
                _ => None,
            })
            .filter_map(|class| {
                let name = class.id.as_ref()?.name.to_string();
                Some((name, class.span))
            })
            .collect()
    }

    /// Start offset of a top-level variable statement, plain or exported.
    ///
    /// This is the key [`ForwardArrowQueue`] entries are matched by when the
    /// main lowering loop reaches their source position.
    pub(super) fn variable_statement_start(statement: &Statement<'_>) -> Option<u32> {
        match statement {
            Statement::VariableDeclaration(variable) => Some(variable.span.start),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(variable) => Some(variable.span.start),
                _ => None,
            },
            _ => None,
        }
    }

    /// Indices of queued arrows that must not be lowered yet.
    ///
    /// An arrow waits when its body names a module class that is not lowered
    /// yet, unless that class's own source names the arrow back (a cycle: the
    /// class's method bodies need the arrow's item as much as the arrow needs
    /// the class's members, so the arrow keeps its early lift and the class
    /// bodies resolve the call). Waiting is transitive: an arrow that reads a
    /// waiting arrow waits too, so it is never forced ahead of its callee and
    /// bound to a placeholder.
    fn waiting_forward_arrows(&self, queue: &ForwardArrowQueue<'_, '_>) -> HashSet<usize> {
        let texts = queue
            .queued()
            .map(|(slot, variable)| {
                let names = Self::arrow_const_declaration_names(variable);
                (slot, variable.span, names)
            })
            .collect::<Vec<_>>();
        let mut waiting = texts
            .iter()
            .filter(|(_, span, names)| {
                queue.classes.iter().any(|class| {
                    !class.lowered
                        && self.source_facts.mentions(*span, &class.name)
                        && !names
                            .iter()
                            .any(|name| self.source_facts.mentions(class.span, name))
                })
            })
            .map(|(slot, _, _)| *slot)
            .collect::<HashSet<_>>();
        loop {
            let waiting_names = texts
                .iter()
                .filter(|(slot, _, _)| waiting.contains(slot))
                .flat_map(|(_, _, names)| names.iter().cloned())
                .collect::<Vec<_>>();
            let newly = texts
                .iter()
                .filter(|(slot, span, names)| {
                    !waiting.contains(slot)
                        && waiting_names.iter().any(|waiting_name| {
                            !names.contains(waiting_name)
                                && self.source_facts.mentions(*span, waiting_name)
                        })
                })
                .map(|(slot, _, _)| *slot)
                .collect::<Vec<_>>();
            if newly.is_empty() {
                return waiting;
            }
            waiting.extend(newly);
        }
    }

    /// Lower every queued arrow that is not waiting on an unlowered class, in
    /// arrow-dependency order (an arrow cycle falls back to queue order).
    pub(super) fn drain_forward_arrow_queue(
        &mut self,
        queue: &mut ForwardArrowQueue<'_, '_>,
        module: &mut Module,
        errors: &mut Vec<SmeltError>,
    ) {
        loop {
            let waiting = self.waiting_forward_arrows(queue);
            let ready = queue
                .queued()
                .filter(|(slot, _)| !waiting.contains(slot))
                .collect::<Vec<_>>();
            let Some(&(fallback, _)) = ready.first() else {
                return;
            };
            let lowered_arrows = queue.lowered_arrow_names();
            let slot = ready
                .iter()
                .find(|(_, variable)| {
                    self.arrow_const_dependencies_are_lowered(
                        variable,
                        &queue.forward_arrow_consts,
                        &lowered_arrows,
                    )
                })
                .map_or(fallback, |(slot, _)| *slot);
            self.lower_queued_forward_arrow(queue, slot, module, errors);
        }
    }

    /// Lower the queued arrow statement starting at `start`, if it is still
    /// queued: the main loop has reached its source position, where every
    /// class declared above it is already lowered.
    pub(super) fn lower_queued_forward_arrow_at(
        &mut self,
        queue: &mut ForwardArrowQueue<'_, '_>,
        start: u32,
        module: &mut Module,
        errors: &mut Vec<SmeltError>,
    ) {
        let slot = queue
            .queued()
            .find(|(_, variable)| variable.span.start == start)
            .map(|(slot, _)| slot);
        if let Some(slot) = slot {
            self.lower_queued_forward_arrow(queue, slot, module, errors);
        }
    }

    /// Take queue slot `slot` and lower it: an exported statement through
    /// `const_item_declarations` (marking it lifted so the main loop skips it),
    /// a private one as a private callable item.
    fn lower_queued_forward_arrow(
        &mut self,
        queue: &mut ForwardArrowQueue<'_, '_>,
        slot: usize,
        module: &mut Module,
        errors: &mut Vec<SmeltError>,
    ) {
        let Some((variable, exported)) = queue.take(slot) else {
            return;
        };
        let lowered = if exported {
            self.const_item_declarations(variable)
        } else {
            self.arrow_function_const_item_declarations(variable, &queue.forward_arrow_consts)
        };
        match lowered {
            Ok(items) => module.items.extend(items),
            Err(error) => errors.push(error),
        }
    }

    /// Check whether a private arrow constant can resolve arrow values it reads.
    ///
    /// Top-level `const` arrows are lexical values rather than hoisted function
    /// declarations. When one arrow returns or passes a later arrow value, the
    /// referenced callable item must be lowered first so the value is not
    /// replaced by a conservative unresolved-global placeholder.
    pub(in crate::lowering) fn arrow_const_dependencies_are_lowered(
        &self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        candidates: &HashSet<String>,
        lowered: &HashSet<String>,
    ) -> bool {
        let declared = Self::arrow_const_declaration_names(decl)
            .into_iter()
            .collect::<HashSet<_>>();
        decl.declarations.iter().all(|declarator| {
            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                return true;
            };
            candidates.iter().all(|candidate| {
                declared.contains(candidate)
                    || !self.source_facts.mentions(arrow.span, candidate)
                    || lowered.contains(candidate)
            })
        })
    }

    /// Find top-level arrow consts that an ITEM body may call, so they must be
    /// lowered as callable function items rather than module-body closures.
    ///
    /// An item body — a function declaration, a CLASS member (method, accessor,
    /// constructor, field initializer, static block), a default export, a module
    /// slot's initializer, an exported const, or another lifted arrow — has no
    /// module-body local to read the arrow through. Reading a closure-held arrow
    /// from there used to bind `module_global_function_expression`'s
    /// default-returning stub, so `class C { m() { return helper(1) } }` called
    /// a function that answered `0`/`""`/an empty object instead of running
    /// `helper`. Every such referrer is scanned here, classes included.
    ///
    /// An arrow that names ITSELF in its own body
    /// (`const fact = (n) => n * fact(n - 1)`) is its own referrer: the
    /// recursive call runs inside the arrow's own item context, so a self-recursive arrow is always lifted (and its item is
    /// predeclared before its body is lowered, see
    /// `arrow_function_const_declaration_inner`).
    ///
    /// "May call" is a spelling test, deliberately wider than the reads
    /// [`super::super::source_facts::SourceFacts`] records: a lifted arrow keeps its
    /// types and the closure form it would otherwise take has no statements or
    /// non-literal parameter defaults, so programs depend on the wider answer.
    pub(in crate::lowering) fn forward_arrow_const_names(&self, program: &Program<'_>) -> HashSet<String> {
        let mut arrow_consts = Vec::new();
        let mut referrer_spans = Vec::new();
        let mut self_recursive = HashSet::new();
        let mut note_self_recursive = |binding: &oxc::ast::ast::BindingIdentifier<'_>,
                                       init: &Option<Expression<'_>>| {
            if let Some(Expression::ArrowFunctionExpression(arrow)) = init
                && self.source_facts.mentions(arrow.body.span(), binding.name.as_str())
            {
                self_recursive.insert(binding.name.as_str().to_owned());
            }
        };
        for statement in &program.body {
            match statement {
                Statement::ClassDeclaration(class) => {
                    referrer_spans.push((class.span.start, class.span.end));
                }
                Statement::ExportDefaultDeclaration(export) => {
                    referrer_spans.push((export.span.start, export.span.end));
                }
                Statement::VariableDeclaration(variable) => {
                    // A module SLOT's initializer is lowered into its own
                    // nullary function item, so an arrow it names is read from
                    // an item body exactly like one a function declaration
                    // names, and must be an item it can call.
                    for declarator in &variable.declarations {
                        if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                            && self.mutable_global_items.contains_key(binding.name.as_str())
                        {
                            referrer_spans.push((declarator.span.start, declarator.span.end));
                        }
                    }
                    if variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                        continue;
                    }
                    for declarator in &variable.declarations {
                        if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                            && matches!(
                                declarator.init,
                                Some(Expression::ArrowFunctionExpression(_))
                            )
                        {
                            arrow_consts
                                .push((binding.name.as_str().to_owned(), binding.span.start));
                            referrer_spans.push((declarator.span.start, declarator.span.end));
                            note_self_recursive(binding, &declarator.init);
                        }
                        // An object or array constant is rebuilt at each read,
                        // inside item bodies too, so an arrow it names must be
                        // an item that body can reach.
                        if matches!(
                            declarator.init,
                            Some(Expression::ObjectExpression(_) | Expression::ArrayExpression(_))
                        ) {
                            referrer_spans.push((declarator.span.start, declarator.span.end));
                        }
                    }
                }
                Statement::FunctionDeclaration(function) => {
                    referrer_spans.push((function.span.start, function.span.end));
                }
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::FunctionDeclaration(function) => {
                        referrer_spans.push((function.span.start, function.span.end));
                    }
                    Declaration::ClassDeclaration(class) => {
                        referrer_spans.push((class.span.start, class.span.end));
                    }
                    Declaration::VariableDeclaration(variable) => {
                        if variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                            continue;
                        }
                        for declarator in &variable.declarations {
                            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                                && matches!(
                                    declarator.init,
                                    Some(Expression::ArrowFunctionExpression(_))
                                )
                            {
                                arrow_consts
                                    .push((binding.name.as_str().to_owned(), binding.span.start));
                                note_self_recursive(binding, &declarator.init);
                            }
                            // An arrow const's body refers to other arrows; any
                            // other EXPORTED const is inlined into its importers,
                            // so an arrow its initializer names (`export const
                            // getQueryParam = _getQueryParam as ..`) must be a
                            // module item they can call, not a body-local only
                            // this module can see. Both are referrer spans.
                            referrer_spans.push((declarator.span.start, declarator.span.end));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        arrow_consts
            .into_iter()
            .filter_map(|(name, const_start)| {
                let spelled_in = |start: u32, end: u32| {
                    self.source_facts.mentions(oxc::span::Span::new(start, end), &name)
                };
                let referenced = spelled_in(0, const_start)
                    || referrer_spans
                        .iter()
                        .filter(|(referrer_start, referrer_end)| {
                            !(const_start >= *referrer_start && const_start <= *referrer_end)
                        })
                        .any(|(function_start, function_end)| {
                            spelled_in(*function_start, *function_end)
                        });
                (referenced || self_recursive.contains(&name)).then_some(name)
            })
            .collect()
    }

    /// Drop from `candidates` every arrow whose body reads a written, non-slot
    /// module-level binding that has reference identity.
    ///
    /// Lifting a module-level `const` arrow turns it into a named MIR function,
    /// which has no capture environment: a module binding it reads is resolved
    /// by re-materializing that binding's initializer inside the function. For a
    /// scalar that is the same value either way. For an array, `Set`, `Map` or
    /// object it is a SECOND, PRIVATE value, so every write through the lifted
    /// function lands somewhere the module never sees:
    ///
    /// ```ts
    /// const rem: string[] = [];
    /// const second = () => { rem.push("second"); };
    /// const outer = () => { rem.push("first"); take(second); };
    /// ```
    ///
    /// printed `first` where Node prints `first,second`. Nothing failed to
    /// compile and nothing reported a blocker; the program simply answered a
    /// different value. `outer` was never lifted and captured `rem` correctly,
    /// which is the shape the refusal restores for `second` too.
    ///
    /// Refusing the lift is the conservative half of the choice: keep the capture,
    /// or do not lift. A genuine forward reference that also captures such a
    /// binding fails to resolve instead of diverging silently, which is the honest
    /// failure.
    ///
    /// Over-refusing costs a closure, and a closure is always correct here.
    pub(super) fn retain_capturable_arrow_consts(
        &self,
        program: &Program<'_>,
        candidates: &mut HashSet<String>,
    ) {
        // The divergence this refusal guards needs a WRITE: a re-materialized
        // copy of a binding nothing mutates is indistinguishable from reading
        // the binding. And a mutated binding an arrow body reads is a module
        // SLOT (a hoisted-body read plus a write anywhere lifts it, see
        // `collect_mutable_globals`), which a lifted function reads through the
        // slot itself — the one shared value. So only a written binding that
        // did NOT become a slot (a shape the slot passes decline) still refuses.
        let (reassigned, through, nested) = Self::collect_all_mutated_names(program);
        let mut identity_bindings = self.identity_bearing_module_binding_names(program);
        identity_bindings.retain(|name| {
            !self.mutable_global_items.contains_key(name)
                && (reassigned.contains(name) || through.contains(name) || nested.contains(name))
        });
        if identity_bindings.is_empty() {
            return;
        }
        let mut refused = HashSet::new();
        for statement in &program.body {
            let variable = match statement {
                Statement::VariableDeclaration(variable) => variable,
                Statement::ExportDeclaration(export) => {
                    let Declaration::VariableDeclaration(variable) = &export.declaration else {
                        continue;
                    };
                    variable
                }
                _ => continue,
            };
            for declarator in &variable.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                    continue;
                };
                if !candidates.contains(binding.name.as_str()) {
                    continue;
                }
                if identity_bindings
                    .iter()
                    .any(|name| self.source_facts.mentions(arrow.span, name))
                {
                    refused.insert(binding.name.as_str().to_owned());
                }
            }
        }
        candidates.retain(|name| !refused.contains(name));
    }

    /// Module-level binding names whose value has reference identity.
    ///
    /// An array, `Set`, `Map`, typed array or object literal is one value that
    /// every reader shares; a re-materialized copy is a different value with the
    /// same contents, and the difference is only observable through mutation --
    /// which is exactly the case that goes silently wrong. Scalars are excluded
    /// because re-materializing one is indistinguishable from reading it.
    pub(super) fn identity_bearing_module_binding_names(&self, program: &Program<'_>) -> HashSet<String> {
        let mut names = HashSet::new();
        for statement in &program.body {
            let variable = match statement {
                Statement::VariableDeclaration(variable) => variable,
                Statement::ExportDeclaration(export) => {
                    let Declaration::VariableDeclaration(variable) = &export.declaration else {
                        continue;
                    };
                    variable
                }
                _ => continue,
            };
            for declarator in &variable.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let Some(init) = &declarator.init else {
                    continue;
                };
                if self.is_module_global_array_initializer(init)
                    || Self::object_const_initializer(init).is_some()
                {
                    names.insert(binding.name.as_str().to_owned());
                }
            }
        }
        names
    }

    /// Find arrow consts used as values in exported object function tables.
    ///
    /// Date-fns-style tables export objects whose properties point at local
    /// arrow functions and later call them through dynamic keys. Those arrows
    /// need real callable items before the export lowering records namespace
    /// metadata for the table.
    pub(in crate::lowering) fn object_namespace_arrow_const_names(program: &Program<'_>) -> HashSet<String> {
        let mut arrow_consts = HashSet::new();
        for statement in &program.body {
            let Statement::VariableDeclaration(variable) = statement else {
                continue;
            };
            if variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &variable.declarations {
                if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                    && matches!(
                        declarator.init,
                        Some(Expression::ArrowFunctionExpression(_))
                    )
                {
                    arrow_consts.insert(binding.name.as_str().to_owned());
                }
            }
        }

        let mut referenced = HashSet::new();
        for statement in &program.body {
            let Statement::ExportDeclaration(export) = statement else {
                continue;
            };
            let Declaration::VariableDeclaration(variable) = &export.declaration else {
                continue;
            };
            for declarator in &variable.declarations {
                let Some(init) = &declarator.init else {
                    continue;
                };
                let Some(object) = Self::direct_object_initializer(init) else {
                    continue;
                };
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                        continue;
                    };
                    if object_property.computed || object_property.method {
                        continue;
                    }
                    if let Expression::Identifier(identifier) = &object_property.value {
                        let name = identifier.name.as_str();
                        if arrow_consts.contains(name) {
                            referenced.insert(name.to_owned());
                        }
                    }
                }
            }
        }
        referenced
    }
}

/// Forward-referenced top-level arrow consts still to be lowered as items.
///
/// Such an arrow is lifted ahead of source order so a body calling it reaches
/// the real item rather than a placeholder. It cannot be lifted above a class
/// it reads, though: a class's fields and method signatures (the overload a
/// `req.header()` call picks, and its return type) exist only once the class
/// declaration is lowered, so a body lowered earlier resolved member calls on
/// that class against nothing. Entries therefore wait for the module classes
/// their body names, and are lowered when those are, or at their own source
/// position at the latest.
pub(super) struct ForwardArrowQueue<'a, 'src> {
    /// Every forward-lifted arrow statement, in source order.
    slots: Vec<ArrowSlot<'a, 'src>>,
    /// Every class declaration in the module, and whether it is lowered yet.
    classes: Vec<QueuedModuleClass>,
    /// Every forward-referenced arrow name (queued or not), for dependency
    /// ordering and for `arrow_function_const_item_declarations`.
    forward_arrow_consts: HashSet<String>,
}

/// One forward-lifted arrow statement and where it is in its lifecycle.
struct ArrowSlot<'a, 'src> {
    /// The `const` statement declaring the arrow(s).
    variable: &'a oxc::ast::ast::VariableDeclaration<'src>,
    /// Whether the statement is `export`ed.
    exported: bool,
    /// Whether it is still queued, and how it was lowered otherwise.
    state: ArrowSlotState,
}

/// The lifecycle of one [`ArrowSlot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrowSlotState {
    /// Not lowered yet: waiting for the classes it names or for its turn.
    Queued,
    /// Lowered by the queue as a private callable item.
    Lowered,
    /// An exported statement lowered by the queue; the main loop skips it.
    LiftedExported,
}

/// A module class declaration a queued arrow may wait for.
struct QueuedModuleClass {
    /// The class's source name.
    name: String,
    /// The class declaration's span.
    span: oxc::span::Span,
    /// Whether the declaration has been lowered.
    lowered: bool,
}

impl<'a, 'src> ForwardArrowQueue<'a, 'src> {
    /// Queue `pending` statements (in source order) against the module's
    /// `(name, span)` class declarations.
    ///
    /// Requires the signature phase: a queued arrow is lowered as soon as it
    /// is ready, and its body calls predeclared functions.
    pub(super) fn new(
        pending: Vec<(&'a oxc::ast::ast::VariableDeclaration<'src>, bool)>,
        classes: Vec<(String, oxc::span::Span)>,
        forward_arrow_consts: HashSet<String>,
        _signatures: &SignaturesDeclared,
    ) -> Self {
        Self {
            slots: pending
                .into_iter()
                .map(|(variable, exported)| ArrowSlot {
                    variable,
                    exported,
                    state: ArrowSlotState::Queued,
                })
                .collect(),
            classes: classes
                .into_iter()
                .map(|(name, span)| QueuedModuleClass {
                    name,
                    span,
                    lowered: false,
                })
                .collect(),
            forward_arrow_consts,
        }
    }

    /// The still-queued slots, in source order, with their statements.
    fn queued(
        &self,
    ) -> impl Iterator<Item = (usize, &'a oxc::ast::ast::VariableDeclaration<'src>)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.state == ArrowSlotState::Queued)
            .map(|(index, slot)| (index, slot.variable))
    }

    /// Take a queued slot for lowering, recording how it is lowered; `None`
    /// when the slot is out of range or no longer queued.
    fn take(
        &mut self,
        slot: usize,
    ) -> Option<(&'a oxc::ast::ast::VariableDeclaration<'src>, bool)> {
        let entry = self.slots.get_mut(slot)?;
        if entry.state != ArrowSlotState::Queued {
            return None;
        }
        entry.state = if entry.exported {
            ArrowSlotState::LiftedExported
        } else {
            ArrowSlotState::Lowered
        };
        Some((entry.variable, entry.exported))
    }

    /// Names declared by the statements the queue has lowered so far.
    fn lowered_arrow_names(&self) -> HashSet<String> {
        self.slots
            .iter()
            .filter(|slot| slot.state != ArrowSlotState::Queued)
            .flat_map(|slot| ModuleBuilder::arrow_const_declaration_names(slot.variable))
            .collect()
    }

    /// Whether the exported statement starting at `start` was lowered by the
    /// queue, so the main loop must skip it.
    pub(super) fn is_lifted_exported(&self, start: u32) -> bool {
        self.slots.iter().any(|slot| {
            slot.state == ArrowSlotState::LiftedExported && slot.variable.span.start == start
        })
    }

    /// Record that the module class `name` has been lowered.
    pub(super) fn mark_class_lowered(&mut self, name: &str) {
        for class in &mut self.classes {
            if class.name == name {
                class.lowered = true;
            }
        }
    }
}
