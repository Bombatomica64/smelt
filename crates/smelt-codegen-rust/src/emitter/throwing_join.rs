//! Rejoining the two continuations of a throwing call or `await`.
//!
//! A call or `await` with an unwind edge (`try { x = f() } catch { .. }`) is
//! emitted as a `match` over the result: the `Ok` arm continues at the call's
//! normal successor and the `Err` arm(s) at the `catch` block. Both paths
//! usually meet again right after the `try` statement. Emitting each arm "to
//! the end of the function" renders that common tail once PER ARM, and the
//! panic-routed `Err` arm makes it three copies per `try`: `n` sequential
//! `expect(() => ..).toThrow()` assertions (each a `try`) rendered the rest of
//! the test `3^n` times — 616 MB of Rust for Hono's `utils/cookie.test.ts`.
//!
//! The rejoin point is the nearest common post-dominator of the two
//! successors (`post_dominance`). When the surrounding emission region can
//! end an arm there, each arm is emitted only up to it and the tail once after
//! the `match`, exactly as a structured `if`/`else` is.

use super::*;
use control_flow::{Continuation, RegionExit};

impl FunctionEmitter<'_> {
    /// The block where a throwing terminator's normal successor `target` and
    /// its `catch` block rejoin, when `continuation` can stop both arms there.
    ///
    /// `None` keeps per-arm emission: no common post-dominator, a loop
    /// continuation (whose arms own `continue`/`break` edges), or a join the
    /// enclosing region cannot reach before its own end.
    pub(super) fn throwing_continuation_join(
        &self,
        target: smelt_mir::BlockId,
        catch_block: smelt_mir::BlockId,
        continuation: &Continuation<'_>,
    ) -> Option<smelt_mir::BlockId> {
        let mut normal_chain = Vec::new();
        let mut next = Some(target);
        while let Some(block) = next {
            if normal_chain.contains(&block) {
                break;
            }
            normal_chain.push(block);
            next = self.immediate_post_dominator(block);
        }
        let mut seen = Vec::new();
        let mut catch_next = Some(catch_block);
        let join = loop {
            let block = catch_next?;
            if normal_chain.contains(&block) {
                break block;
            }
            if seen.contains(&block) {
                return None;
            }
            seen.push(block);
            catch_next = self.immediate_post_dominator(block);
        };
        // Blocks an arm must not reach except THROUGH the join: the region's
        // end, and (in a closure) every block already on the emission path.
        let exits: Vec<smelt_mir::BlockId> = match continuation {
            Continuation::Block => Vec::new(),
            Continuation::InLoop { .. } => return None,
            Continuation::Region { stop, .. } => {
                if join == *stop {
                    return None;
                }
                vec![*stop]
            }
            Continuation::Closure { active, stop, .. } => {
                if active.contains(&join) || *stop == Some(join) {
                    return None;
                }
                active.iter().copied().chain(*stop).collect()
            }
        };
        for arm in [target, catch_block] {
            if arm != join
                && exits.iter().any(|exit| {
                    self.block_reaches_target_avoiding(arm, *exit, &[join], &mut BlockIdSet::default())
                })
            {
                return None;
            }
        }
        Some(join)
    }

    /// Emit the arms' shared tail, starting at `join`, once, in the region the
    /// throwing terminator itself was emitted in.
    pub(super) fn emit_throwing_continuation_tail(
        &self,
        join: smelt_mir::BlockId,
        continuation: &Continuation<'_>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        match continuation {
            Continuation::Block => self.emit_block(self.block(join)?, out),
            Continuation::Region { stop, visited } => self.emit_block_until_goto_inner(
                self.block(join)?,
                *stop,
                RegionExit::Join,
                out,
                &mut (*visited).clone(),
            ),
            Continuation::Closure {
                active,
                stop,
                in_loop,
            } => self.emit_closure_block_inner(
                self.block(join)?,
                out,
                &mut active.to_vec(),
                *stop,
                *in_loop,
            ),
            Continuation::InLoop { .. } => {
                Err(EmitError::new("a loop continuation has no rejoined tail"))
            }
        }
    }
}
