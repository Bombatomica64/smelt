//! The lexical control-scope stack and the MIR block cursor.
//!
//! Lowering a structured statement (`while`, `for`, `try`) changes where an
//! abrupt exit inside it goes: a `break` targets the innermost loop, a `throw`
//! the innermost handler, and every `finally` clause between the exit and its
//! target must run on the way out. All of that is answered by ONE stack of
//! [`ControlScope`]s, innermost last:
//!
//! * [`ControlScope::Loop`] — the `break` / `continue` targets of a loop.
//! * [`ControlScope::Protected`] — a `try` body or `catch` body: the handler a
//!   throw inside it jumps to, and the `finally` clause that guards it.
//!
//! Scopes are pushed only through [`LoweringCtx::with_scope`], which pops on
//! every exit of its closure (including an error), so push/pop pairing is
//! structural rather than a convention each lowerer must follow.
//!
//! The second half of this module is the block cursor: [`LoweringCtx::terminate`]
//! closes the current block and refuses to overwrite an existing terminator,
//! [`LoweringCtx::terminate_if_open`] is the "fall through unless the body
//! already left" edge, and [`LoweringCtx::switch_to`] moves the cursor.

use smelt_hir::BlockId as HirBlockId;

use crate::{BlockId, ExceptionHandler, GeneratorCleanup, Terminator};

use super::LowerError;
use super::context::LoweringCtx;

/// One lexical construct that redirects abrupt exits written inside it.
#[derive(Debug, Clone, Copy)]
pub(super) enum ControlScope {
    /// A loop body: where `break` and `continue` go.
    Loop(LoopTargets),
    /// A `try` body or a `catch` body.
    Protected(Protected),
}

/// Target blocks for `break` and `continue` statements.
#[derive(Debug, Clone, Copy)]
pub(super) struct LoopTargets {
    /// Target block for `break` statements.
    pub(super) break_target: BlockId,
    /// Target block for `continue` statements.
    pub(super) continue_target: BlockId,
}

/// A region guarded by a `catch` handler and/or a `finally` clause.
///
/// A `try` body is one such region (handler: the `catch` block, or the
/// `finally`'s rethrow handler when there is no `catch`); its `catch` body is a
/// second one, pushed after the first is popped (handler: the `finally`'s
/// rethrow handler, if any).
#[derive(Debug, Clone, Copy)]
pub(super) struct Protected {
    /// Where a throw inside the region goes; `None` lets it reach the next
    /// enclosing handler.
    pub(super) handler: Option<ExceptionHandler>,
    /// The `finally` clause every exit from the region must run.
    pub(super) finally: Option<FinallyClause>,
}

/// A `finally` clause as seen from inside the region it guards.
#[derive(Debug, Clone, Copy)]
pub(super) struct FinallyClause {
    /// The clause's *HIR* block: an early exit (`return`, `break`, `continue`)
    /// re-lowers these statements inline, because the single MIR block the
    /// fall-through path uses continues after the `try`.
    pub(super) hir: HirBlockId,
    /// The MIR clause entry and continuation, for abrupt generator commands.
    pub(super) cleanup: GeneratorCleanup,
}

/// Which loop edge an abrupt loop exit takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoopExit {
    /// `break`: leave the loop.
    Break,
    /// `continue`: start the next iteration.
    Continue,
}

impl LoweringCtx<'_> {
    /// Runs `lower` with `scope` pushed as the innermost control scope.
    ///
    /// The stack is restored to its previous depth on every exit, error
    /// included, so no caller can leave a scope behind or pop one it did not
    /// push.
    pub(super) fn with_scope<T>(
        &mut self,
        scope: ControlScope,
        lower: impl FnOnce(&mut Self) -> Result<T, LowerError>,
    ) -> Result<T, LowerError> {
        let depth = self.scopes.len();
        self.scopes.push(scope);
        let result = lower(self);
        self.scopes.truncate(depth);
        result
    }

    /// Runs `lower` as if only the outermost `depth` scopes were active.
    ///
    /// Used while a finalizer is re-lowered inline: inside the clause of the
    /// scope at `depth`, that scope and everything nested in it are no longer
    /// in effect, so a throw goes to the next *enclosing* handler and a nested
    /// early exit runs only the finalizers outside it. The removed scopes are
    /// put back afterwards, error or not.
    fn with_scopes_truncated<T>(
        &mut self,
        depth: usize,
        lower: impl FnOnce(&mut Self) -> Result<T, LowerError>,
    ) -> Result<T, LowerError> {
        let inner = self.scopes.split_off(depth.min(self.scopes.len()));
        let result = lower(self);
        self.scopes.truncate(depth);
        self.scopes.extend(inner);
        result
    }

    /// The handler a throw at the cursor jumps to, if any `try` encloses it.
    pub(super) fn current_exception_handler(&self) -> Option<ExceptionHandler> {
        self.scopes.iter().rev().find_map(|scope| match scope {
            ControlScope::Protected(protected) => protected.handler,
            ControlScope::Loop(_) => None,
        })
    }

    /// The innermost `finally` clause's generator cleanup edge, if any.
    pub(super) fn current_generator_cleanup(&self) -> Option<GeneratorCleanup> {
        self.scopes.iter().rev().find_map(|scope| match scope {
            ControlScope::Protected(Protected {
                finally: Some(clause),
                ..
            }) => Some(clause.cleanup),
            ControlScope::Protected(_) | ControlScope::Loop(_) => None,
        })
    }

    /// Whether any `finally` clause encloses the cursor.
    pub(super) fn has_pending_finally(&self) -> bool {
        self.scopes.iter().any(|scope| {
            matches!(
                scope,
                ControlScope::Protected(Protected {
                    finally: Some(_),
                    ..
                })
            )
        })
    }

    /// Re-lowers, innermost first, every `finally` clause enclosing the cursor
    /// at a stack position `>= floor`.
    ///
    /// Returns `true` when one of the clauses terminated the block itself (its
    /// own `return` / `throw` / `break`), which — exactly as in JavaScript and
    /// Python — overrides the exit that triggered the walk; the caller must then
    /// not add its own terminator.
    ///
    /// Each clause is lowered with the stack truncated to its own scope (see
    /// [`Self::with_scopes_truncated`]), so an exit nested in a clause resumes
    /// at the next enclosing clause instead of re-entering itself.
    fn run_pending_finalizers(&mut self, floor: usize) -> Result<bool, LowerError> {
        let clauses: Vec<(usize, HirBlockId)> = self
            .scopes
            .iter()
            .enumerate()
            .skip(floor)
            .filter_map(|(depth, scope)| match scope {
                ControlScope::Protected(Protected {
                    finally: Some(clause),
                    ..
                }) => Some((depth, clause.hir)),
                ControlScope::Protected(_) | ControlScope::Loop(_) => None,
            })
            .collect();
        for (depth, hir) in clauses.into_iter().rev() {
            self.with_scopes_truncated(depth, |this| this.lower_inline_finally(hir))?;
            if self.is_terminated()? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Terminates the current block with a `return`, honoring enclosing
    /// finalizers.
    ///
    /// Outside a `try`/`finally` this is just `Terminator::Return`. Inside one,
    /// every enclosing finalizer runs *before* the function returns, and the MIR
    /// block the finalizer was lowered into cannot be reused — its exit falls
    /// through to the code after the `try`. So each enclosing finalizer is
    /// *re-lowered inline*, innermost first, ahead of the `Return` terminator:
    /// the classic finalizer-duplication strategy.
    ///
    /// Duplication rather than a shared cleanup block reached by `Goto` is
    /// deliberate. A `return` can sit arbitrarily deep inside loops and
    /// branches, and codegen reconstructs structured Rust from the CFG; a jump
    /// *out of a loop* to a shared cleanup block is a shape that reconstruction
    /// gets wrong (it degrades to a plain `break`, so control falls into the
    /// code after the loop and the wrong value is returned — it silently broke
    /// `isEqual` on arrays with differing values). Inlining introduces no new
    /// control-flow shape at all, at the cost of repeating the normally tiny
    /// cleanup once per early exit.
    ///
    /// Without any of this the finalizer was skipped entirely whenever the `try`
    /// body returned. es-toolkit's `areObjectsEqual` clears its recursion stack
    /// in a `finally` and returns from inside the `try`, so the stale entries
    /// made `isEqualWith({ constructor: [1] }, { constructor: ['1'] })` answer
    /// `true`.
    pub(super) fn lower_return(&mut self, value: crate::Operand) -> Result<(), LowerError> {
        if !self.run_pending_finalizers(0)? {
            self.terminate(Terminator::Return(value))?;
        }
        Ok(())
    }

    /// Terminates the current block with a `break` or `continue` of the
    /// innermost loop, running the `finally` clauses between the jump and the
    /// loop first.
    ///
    /// Only the clauses *inside* the loop run: a `try` around the whole loop is
    /// not left by the jump. This is the same inline duplication
    /// [`Self::lower_return`] uses, for the same reason — the jump stays a plain
    /// loop edge, the one shape codegen reconstructs. Before, the clauses were
    /// skipped entirely: `for (x of xs) { try { if (c) break } finally { log() } }`
    /// left the loop without calling `log()`.
    pub(super) fn lower_loop_exit(&mut self, exit: LoopExit) -> Result<(), LowerError> {
        let Some((depth, targets)) = self
            .scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(depth, scope)| match scope {
                ControlScope::Loop(targets) => Some((depth, *targets)),
                ControlScope::Protected(_) => None,
            })
        else {
            return Err(self.error(
                match exit {
                    LoopExit::Break => "break used outside a loop",
                    LoopExit::Continue => "continue used outside a loop",
                },
                None,
            ));
        };
        if self.run_pending_finalizers(depth.saturating_add(1))? {
            return Ok(());
        }
        let target = match exit {
            LoopExit::Break => targets.break_target,
            LoopExit::Continue => targets.continue_target,
        };
        self.terminate(Terminator::Goto(target))
    }

    /// Re-lowers one `finally` block inline at the current cursor.
    ///
    /// The HIR-expression memo is cleared for the duration: it caches "this
    /// `ExprId` already became this operand", which is exactly what must *not*
    /// hold when the same source statements are emitted a second time —
    /// otherwise the duplicated cleanup would compile to nothing. Clearing is
    /// safe because each syntactic position owns its `ExprId`, so no entry that
    /// the surrounding code still needs can be produced inside the finalizer.
    fn lower_inline_finally(&mut self, finally_hir: HirBlockId) -> Result<(), LowerError> {
        let saved_exprs = std::mem::take(&mut self.exprs);
        let result = self.lower_block_stmts(finally_hir);
        self.exprs = saved_exprs;
        result
    }

    /// Whether the block under the cursor already has a terminator.
    pub(super) fn is_terminated(&self) -> Result<bool, LowerError> {
        Ok(self.block()?.terminator.is_some())
    }

    /// Closes the block under the cursor with `terminator`.
    ///
    /// A block is terminated exactly once. Overwriting an existing terminator
    /// would silently drop the edge it already carried (and with it whatever
    /// the earlier exit meant), so that is an internal lowering error.
    pub(super) fn terminate(&mut self, terminator: Terminator) -> Result<(), LowerError> {
        let block = self.current_block;
        self.terminate_block(block, terminator)
    }

    /// Closes `block` (not necessarily the cursor's) with `terminator`; see
    /// [`Self::terminate`] for the exactly-once rule.
    pub(super) fn terminate_block(
        &mut self,
        block: BlockId,
        terminator: Terminator,
    ) -> Result<(), LowerError> {
        let index = super::usize_from_u32(block.0, "MIR block index does not fit in usize")?;
        let Some(slot) = self.function.blocks.get_mut(index) else {
            return Err(self.error("MIR block index should be valid", None));
        };
        if slot.terminator.is_some() {
            debug_assert!(
                slot.terminator.is_none(),
                "MIR block {} terminated twice",
                block.0
            );
            return Err(self.error(
                format!("internal error: MIR block {} terminated twice", block.0),
                None,
            ));
        }
        slot.terminator = Some(terminator);
        Ok(())
    }

    /// Closes the block under the cursor with `terminator` unless an abrupt
    /// exit inside it (`return`, `throw`, `break`, ...) already closed it.
    ///
    /// This is the fall-through edge at the end of a lowered body.
    pub(super) fn terminate_if_open(&mut self, terminator: Terminator) -> Result<(), LowerError> {
        if self.is_terminated()? {
            return Ok(());
        }
        self.terminate(terminator)
    }

    /// Moves the cursor to `block`; subsequent statements append there.
    pub(super) const fn switch_to(&mut self, block: BlockId) {
        self.current_block = block;
    }
}
