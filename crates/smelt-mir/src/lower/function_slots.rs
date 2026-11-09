//! Reserved function slots: the one place lowering hands out [`FuncId`]s.
//!
//! Calls resolve their callee before the callee's body is lowered, so every
//! function item gets its id up front. The id has to equal the function's final
//! position in [`crate::Mir::functions`]. Assigning ids and pushing bodies in two
//! separate loops made that an assumption: a loop that skipped a body after its
//! id was assigned shifted every later function, and every `Callee::Static`
//! after it called the wrong function.
//!
//! [`FunctionSlots`] makes it a construction property instead. An id is a
//! reserved slot; a body can only be stored INTO its slot (which writes the id
//! into the function); and the table only becomes the `Vec` once every slot is
//! filled, so a skipped body is a lowering error, never a shifted table.

use crate::{FuncId, MirFunction};

use super::{LowerError, u32_from_usize};

/// The function table under construction, indexed by reserved [`FuncId`].
#[derive(Debug, Default)]
pub(super) struct FunctionSlots {
    /// One entry per reserved id; `None` until its body is stored.
    slots: Vec<Option<MirFunction>>,
}

impl FunctionSlots {
    /// Reserve the next id. Its body must be stored with [`Self::fill`] before
    /// [`Self::finish`].
    pub(super) fn reserve(&mut self) -> Result<FuncId, LowerError> {
        let id = FuncId(u32_from_usize(
            self.slots.len(),
            "MIR function index does not fit in u32",
        )?);
        self.slots.push(None);
        Ok(id)
    }

    /// Store `function` as the body of the reserved slot `id`, writing `id`
    /// into it.
    ///
    /// # Errors
    ///
    /// An id that was never reserved, or a slot that is already filled.
    pub(super) fn fill(&mut self, id: FuncId, mut function: MirFunction) -> Result<(), LowerError> {
        let Some(slot) = self.slots.get_mut(id.index()) else {
            return Err(slot_error(format!("function {id:?} was never reserved")));
        };
        if slot.is_some() {
            return Err(slot_error(format!("function {id:?} was lowered twice")));
        }
        function.id = id;
        *slot = Some(function);
        Ok(())
    }

    /// The finished table, in id order.
    ///
    /// # Errors
    ///
    /// Every reserved slot whose body was never stored.
    pub(super) fn finish(self) -> FinishedTable {
        let mut functions = Vec::with_capacity(self.slots.len());
        let mut errors = Vec::new();
        for (index, slot) in self.slots.into_iter().enumerate() {
            match slot {
                Some(function) => functions.push(function),
                None => errors.push(slot_error(format!(
                    "function slot {index} was reserved but its body was never lowered"
                ))),
            }
        }
        if errors.is_empty() { Ok(functions) } else { Err(errors) }
    }
}

/// The finished function table, or one error per empty slot.
type FinishedTable = Result<Vec<MirFunction>, Vec<LowerError>>;

/// A spanless lowering error about the function table.
const fn slot_error(message: String) -> LowerError {
    LowerError { message, span: None }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test fixtures unwrap and index their own known-good shapes")]
mod tests {
    //! The slot table refuses every way an id could stop matching a position.

    use smelt_hir::{BodyId, FileId, Span, Symbol, TypeId};

    use super::FunctionSlots;
    use crate::{FuncId, HirOrigin, MirFunction};

    /// A body whose own `id` field is deliberately wrong.
    fn body() -> MirFunction {
        MirFunction::new(
            FuncId(77),
            Symbol(0),
            HirOrigin::Body(BodyId(0)),
            TypeId(0),
            Span::new(FileId(0), 0, 0),
        )
    }

    /// Filling slots out of order still yields `functions[i].id == i`.
    #[test]
    fn filled_slots_are_positioned_by_id() {
        let mut slots = FunctionSlots::default();
        let first = slots.reserve().expect("reserve");
        let second = slots.reserve().expect("reserve");
        slots.fill(second, body()).expect("fill");
        slots.fill(first, body()).expect("fill");
        let functions = slots.finish().expect("finish");
        for (index, function) in functions.iter().enumerate() {
            assert_eq!(function.id.index(), index);
        }
    }

    /// A reserved id whose body is never stored is an error, not a shift
    /// (the defect describes).
    #[test]
    fn a_skipped_body_is_an_error() {
        let mut slots = FunctionSlots::default();
        let _skipped = slots.reserve().expect("reserve");
        let kept = slots.reserve().expect("reserve");
        slots.fill(kept, body()).expect("fill");
        assert!(slots.finish().is_err());
    }

    /// Storing twice, or into an id never reserved, is refused.
    #[test]
    fn double_or_unreserved_fills_are_refused() {
        let mut slots = FunctionSlots::default();
        let id = slots.reserve().expect("reserve");
        slots.fill(id, body()).expect("fill");
        assert!(slots.fill(id, body()).is_err());
        assert!(slots.fill(FuncId(5), body()).is_err());
    }
}
