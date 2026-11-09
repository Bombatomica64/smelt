//! The lowering context of the function body currently being lowered.
//!
//! A JavaScript function body carries facts that are meaningful only while
//! that one body is lowered: whether it may `await`, its declared return type,
//! its generator yield/resume types, the arity of its own `arguments` object,
//! the HIR block that receives statements synthesized while an expression is
//! lowered, whether postfix updates are being deferred past a declaration, and
//! whether a lifted specialization keeps its concrete `this`.
//!
//! One value per body, so a nested body starts from a fresh context rather than
//! inheriting what its entry point forgot to reset: a class method lowered while
//! the enclosing statement's block is active must not push a parameter-default
//! update into a `BlockId` of the OUTER body, and a method of a class expression
//! in a `const` initializer must not append its own `x++` to the OUTER
//! declaration's deferral list.
//!
//! # The invariants this module owns
//!
//! * **One body, one context.** [`FnCtx`] is the whole per-body state, and the
//!   only way to enter a nested body is [`ModuleBuilder::with_fn_ctx`], which
//!   swaps the context in, runs the body's lowering, and swaps the outer one
//!   back on EVERY exit path (an early `return Err(..)` inside the closure
//!   included).
//! * **What a nested body inherits is decided once**, by [`FnCtx::nested`] and
//!   the [`BodyKind`] of the body being entered: an arrow inherits the lexical
//!   `arguments` object and the specialization receiver, nothing else is ever
//!   inherited. The statement sink always starts at the new body's root block
//!   and postfix updates always start immediate.
//! * **Statement sinks are scoped.** [`ModuleBuilder::with_stmt_block`] is the
//!   only way to redirect synthesized statements into a block, and it restores
//!   the previous sink on every exit path.
//! * **Deferred updates cannot be dropped.** [`ModuleBuilder::with_deferred_updates`]
//!   hands the collected stores back to its caller together with the closure's
//!   result, so the caller cannot finish the declaration without seeing them.

use std::collections::HashSet;

use smelt_hir::{BlockId, Body, Stmt, StmtId, TypeId};

use crate::lowering::state::local_scope::{BodyScopeFrame, ScopeGroups};
use crate::lowering::{GeneratorYieldAccumulator, ModuleBuilder};

/// The kind of body being entered, which decides what it inherits from the
/// enclosing body (see [`FnCtx::nested`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum BodyKind {
    /// A `function` declaration or expression: it binds its own `arguments`
    /// and its own `this`.
    Function,
    /// An arrow function: `arguments` and `this` are the enclosing body's.
    Arrow,
    /// A class method, accessor or constructor body. It has its own
    /// `arguments`, which Smelt does not model, so none is available.
    Method,
}

/// Where statements synthesized while lowering an expression are appended.
///
/// A [`BlockId`] indexes the body that owns it, so a sink is only meaningful
/// inside the body whose [`FnCtx`] holds it; entering a nested body always
/// starts over at [`StmtSink::Root`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::lowering) enum StmtSink {
    /// The current body's root block.
    #[default]
    Root,
    /// A block of the current body that the enclosing statement is filling.
    Block(BlockId),
}

impl StmtSink {
    /// The block this sink names in `body`.
    pub(in crate::lowering) const fn block(self, body: &Body) -> BlockId {
        match self {
            Self::Root => body.root,
            Self::Block(block) => block,
        }
    }
}

/// How a postfix update (`x++`) stores its new value.
#[derive(Debug, Default)]
pub(in crate::lowering) enum UpdateMode {
    /// Store immediately, into the current statement sink, and evaluate to a
    /// snapshot of the old value.
    #[default]
    Immediate,
    /// A variable initializer is being lowered: collect the store so it runs
    /// after the binding statement, which then still observes the old value.
    Deferring(Vec<Stmt>),
}

/// The per-body lowering context; see the module docs.
#[derive(Debug, Default)]
pub(in crate::lowering) struct FnCtx {
    /// Whether the body may `await`.
    pub(in crate::lowering) is_async: bool,
    /// Declared (or contextual) return type of the body, if any.
    pub(in crate::lowering) return_ty: Option<TypeId>,
    /// Yield/resume types of the body when it is a generator.
    ///
    /// Set by the body's lowering once the accumulator locals exist, so it is
    /// always `None` on entry.
    pub(in crate::lowering) generator: Option<GeneratorYieldAccumulator>,
    /// Fixed arity of the `arguments` object visible in the body, if any.
    ///
    /// A `function` body sets its own once its parameters are bound; an arrow
    /// inherits the enclosing one.
    pub(in crate::lowering) arguments_arity: Option<usize>,
    /// Where synthesized statements go; see [`StmtSink`].
    stmt_sink: StmtSink,
    /// How postfix updates store; see [`UpdateMode`].
    update_mode: UpdateMode,
    /// Whether a lifted specialization callable keeps its concrete `this` type
    /// through `this as T` assertions.
    pub(in crate::lowering) preserve_receiver: bool,
    /// Names whose local this body reserved ahead of their declaration
    /// because a closure in an EARLIER statement already reads them.
    ///
    /// The declaration must assign INTO that reserved local instead of
    /// pushing a second one, or the earlier capture would observe a slot
    /// nothing ever writes; it takes its reservation with
    /// [`ModuleBuilder::take_forward_local_reservation`]. Reservations name
    /// locals of THIS body, so they live and die with its context: one left
    /// behind by a declaration that failed to lower cannot be taken by a
    /// same-named declaration in a sibling body.
    forward_referenced_locals: HashSet<String>,
}

impl FnCtx {
    /// The context of the module's top-level code.
    ///
    /// Top-level code is the program's entry point, so it may await (ES
    /// modules have top-level await); nothing else is set.
    pub(in crate::lowering) fn module() -> Self {
        Self {
            is_async: true,
            ..Self::default()
        }
    }

    /// The context of a body of `kind` nested in the body this context
    /// describes.
    ///
    /// This is the single place that decides what crosses a body boundary: an
    /// arrow keeps the lexical `arguments` object and the specialization
    /// receiver; every other fact starts fresh.
    pub(in crate::lowering) fn nested(
        &self,
        kind: BodyKind,
        is_async: bool,
        return_ty: Option<TypeId>,
    ) -> Self {
        let (arguments_arity, preserve_receiver) = match kind {
            BodyKind::Arrow => (self.arguments_arity, self.preserve_receiver),
            BodyKind::Function | BodyKind::Method => (None, false),
        };
        Self {
            is_async,
            return_ty,
            generator: None,
            arguments_arity,
            stmt_sink: StmtSink::Root,
            update_mode: UpdateMode::Immediate,
            preserve_receiver,
            forward_referenced_locals: HashSet::new(),
        }
    }
}

impl ModuleBuilder<'_> {
    /// Lower a nested body under `ctx`, restoring the enclosing context on
    /// every exit path of `lower`.
    pub(in crate::lowering) fn with_fn_ctx<R>(
        &mut self,
        ctx: FnCtx,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let outer = std::mem::replace(&mut self.fn_ctx, ctx);
        let result = lower(self);
        let inner = std::mem::replace(&mut self.fn_ctx, outer);
        debug_assert!(
            matches!(inner.update_mode, UpdateMode::Immediate),
            "a deferral scope outlived the body that opened it"
        );
        result
    }

    /// Lower a nested body under `ctx` with the `groups` of the local scope
    /// taken out for it, giving both back on every exit path of `lower`.
    ///
    /// This is the one entry point for a nested body: the function-body
    /// context and the local-scope frame are entered and left together, so no
    /// site can restore one and forget the other.
    pub(in crate::lowering) fn with_nested_body<R>(
        &mut self,
        ctx: FnCtx,
        groups: ScopeGroups,
        lower: impl FnOnce(&mut Self, &BodyScopeFrame) -> R,
    ) -> R {
        let frame = self.scope.enter_body(groups);
        let shadows = self.classes.shadow_depth();
        self.classes.note_body(true);
        let result = self.with_fn_ctx(ctx, |this| lower(this, &frame));
        self.classes.note_body(false);
        self.classes.close_shadows(shadows);
        self.scope.leave_body(frame);
        result
    }

    /// Run `lower` in a nested lexical scope: what it binds ends with it, on every
    /// exit.
    pub(in crate::lowering) fn with_lexical_scope<R>(
        &mut self,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved = self.scope.snapshot_bindings();
        let shadows = self.classes.shadow_depth();
        let result = lower(self);
        self.classes.close_shadows(shadows);
        self.scope.restore_bindings(saved);
        result
    }

    /// Run `lower` with no name of the enclosing body in scope.
    ///
    /// For code that lowers into a different body, whose `LocalId`s the enclosing
    /// bindings do not name.
    pub(in crate::lowering) fn with_empty_lexical_scope<R>(
        &mut self,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved = self.scope.take_bindings();
        let result = lower(self);
        self.scope.restore_bindings(saved);
        result
    }

    /// The context for a body of `kind` nested in the current one; see
    /// [`FnCtx::nested`].
    pub(in crate::lowering) fn nested_fn_ctx(
        &self,
        kind: BodyKind,
        is_async: bool,
        return_ty: Option<TypeId>,
    ) -> FnCtx {
        self.fn_ctx.nested(kind, is_async, return_ty)
    }

    /// Run `lower` with synthesized statements redirected into `block` of the
    /// current body, restoring the previous sink on every exit path.
    pub(in crate::lowering) fn with_stmt_block<R>(
        &mut self,
        block: BlockId,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.fn_ctx.stmt_sink, StmtSink::Block(block));
        let result = lower(self);
        self.fn_ctx.stmt_sink = previous;
        result
    }

    /// The block synthesized statements currently go to in `body`.
    pub(in crate::lowering) const fn stmt_sink_block(&self, body: &Body) -> BlockId {
        self.fn_ctx.stmt_sink.block(body)
    }

    /// Append a synthesized statement to the current statement sink.
    pub(in crate::lowering) fn push_stmt_to_sink(&self, body: &mut Body, stmt: Stmt) -> StmtId {
        body.push_stmt_to_block(self.stmt_sink_block(body), stmt)
    }

    /// Lower a variable initializer with postfix updates deferred.
    ///
    /// Returns the closure's result together with the stores the updates
    /// deferred, in source order; the caller emits them after the binding.
    /// The enclosing update mode is restored on every exit path, and a
    /// deferral never crosses into a nested body because entering one starts
    /// over at [`UpdateMode::Immediate`].
    pub(in crate::lowering) fn with_deferred_updates<R>(
        &mut self,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> (R, Vec<Stmt>) {
        let previous = std::mem::replace(
            &mut self.fn_ctx.update_mode,
            UpdateMode::Deferring(Vec::new()),
        );
        let result = lower(self);
        let deferred = match std::mem::replace(&mut self.fn_ctx.update_mode, previous) {
            UpdateMode::Deferring(stores) => stores,
            UpdateMode::Immediate => Vec::new(),
        };
        (result, deferred)
    }

    /// Reserve the local of `name` for its later declaration in this body; see
    /// `FnCtx::forward_referenced_locals`.
    pub(in crate::lowering) fn reserve_forward_local(&mut self, name: String) {
        self.fn_ctx.forward_referenced_locals.insert(name);
    }

    /// Take this body's reservation for `name`, returning whether there was one.
    pub(in crate::lowering) fn take_forward_local_reservation(&mut self, name: &str) -> bool {
        self.fn_ctx.forward_referenced_locals.remove(name)
    }

    /// Whether this body reserved `name` ahead of its declaration.
    pub(in crate::lowering) fn is_forward_referenced_local(&self, name: &str) -> bool {
        self.fn_ctx.forward_referenced_locals.contains(name)
    }

    /// Defer a postfix update's store when a variable initializer is being
    /// lowered. Hands the store back when updates are immediate, so the
    /// caller must emit it itself.
    pub(in crate::lowering) fn defer_postfix_update(&mut self, store: Stmt) -> Result<(), Stmt> {
        match &mut self.fn_ctx.update_mode {
            UpdateMode::Deferring(stores) => {
                stores.push(store);
                Ok(())
            }
            UpdateMode::Immediate => Err(store),
        }
    }
}
