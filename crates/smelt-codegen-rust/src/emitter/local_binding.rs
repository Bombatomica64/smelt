//! How each MIR local is bound in the generated Rust scope.
//!
//! A local is not always a plain identifier: a closure body reaches a shared
//! capture through its `Rc<RefCell<_>>` cell, a closure parameter is named by
//! position, and a moved `async` method body reads its receiver through an owned
//! handle. The emitter records that fact here, so every later question (is this
//! a shared cell, which cell, is this a closure parameter) is answered from the
//! binding, and Rust text is only produced on the way out.

use crate::rust::RustExpr;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

/// The prefix of a shared-capture cell binding: `smelt_capture_<name>`.
const CAPTURE_CELL_PREFIX: &str = "smelt_capture_";

/// The binding an async or generator method body reads its receiver through.
pub(super) const SELF_OWNED_NAME: &str = "self_owned";

/// The binding a closure takes for a captured `self`, which `let` cannot bind.
pub(super) const SELF_CAPTURE_NAME: &str = "smelt_self";

/// What stands between a capture cell binding and the `RefCell` it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CellUpgrade {
    /// The binding is the `Rc<RefCell<_>>` itself.
    Strong,
    /// A self-recursive closure holds a `Weak` to its own cell and upgrades
    /// before borrowing, which is what stops the reference cycle.
    SelfRecursive,
    /// A weak-receiver capture (`emitter::weak_receiver`) builds a fresh capture
    /// cell around the upgraded instance.
    WeakReceiver,
}

impl CellUpgrade {
    /// The method call that reaches the `RefCell` from the cell binding.
    fn suffix(self) -> &'static str {
        match self {
            Self::Strong => "",
            Self::SelfRecursive => {
                ".upgrade().expect(\"self-recursive closure called after its defining scope returned\")"
            }
            Self::WeakReceiver => ".smelt_receiver_cell()",
        }
    }
}

/// A shared `Rc<RefCell<_>>` capture cell a local is read and written through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SharedCell {
    /// The Rust binding that holds the cell (`smelt_capture_x`).
    pub(super) cell: String,
    /// How the `RefCell` is reached from that binding.
    pub(super) upgrade: CellUpgrade,
}

impl SharedCell {
    /// The cell of the capture named `name`.
    pub(super) fn named(name: &str, upgrade: CellUpgrade) -> Self {
        Self {
            cell: format!("{CAPTURE_CELL_PREFIX}{name}"),
            upgrade,
        }
    }

    /// The place expression that reads the cell: `(*smelt_capture_x.borrow())`.
    pub(super) fn read_expr(&self) -> RustExpr {
        RustExpr::atom(format!("(*{}{}.borrow())", self.cell, self.upgrade.suffix()))
    }

    /// The place expression that writes the cell: `(*smelt_capture_x.borrow_mut())`.
    pub(super) fn write_expr(&self) -> RustExpr {
        RustExpr::atom(format!("(*{}{}.borrow_mut())", self.cell, self.upgrade.suffix()))
    }
}

/// How one MIR local is bound in the generated Rust scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LocalBinding {
    /// An ordinary identifier, a `let` binding or a named parameter.
    Plain(String),
    /// The `self` receiver of a synchronous method.
    Receiver,
    /// Parameter `index` of the closure being rendered.
    ClosureArg(usize),
    /// The owned receiver handle of a moved `async` or generator method body.
    SelfOwned,
    /// A capture read and written through a shared cell.
    SharedCapture(SharedCell),
    /// A listener's weak capture of its own emitter (see `emitter::weak_receiver`).
    WeakEmitter {
        /// The captured binding's name.
        name: String,
    },
}

impl LocalBinding {
    /// The Rust text that names this binding where a value is expected.
    ///
    /// A shared capture answers with its write form, the one that is valid in
    /// both read and write position.
    pub(super) fn text(&self) -> Cow<'_, str> {
        match self {
            Self::Plain(name) => Cow::Borrowed(name),
            Self::Receiver => Cow::Borrowed("self"),
            Self::ClosureArg(index) => Cow::Owned(format!("closure_arg_{index}")),
            Self::SelfOwned => Cow::Borrowed(SELF_OWNED_NAME),
            Self::SharedCapture(cell) => Cow::Owned(cell.write_expr().into_string()),
            Self::WeakEmitter { name } => {
                Cow::Owned(super::weak_receiver::emitter_access_expr(name).into_string())
            }
        }
    }

    /// The Rust binding a moved closure or block clones to own this value: the
    /// cell of a shared capture, the weak handle of a weak emitter, the name of
    /// anything else.
    pub(super) fn cell_handle(&self) -> String {
        match self {
            Self::SharedCapture(cell) => cell.cell.clone(),
            Self::WeakEmitter { name } => format!("smelt_weak_{name}"),
            other => other.text().into_owned(),
        }
    }

    /// The cell this binding is reached through, when it is a shared capture.
    pub(super) fn shared_cell(&self) -> Option<&SharedCell> {
        match self {
            Self::SharedCapture(cell) => Some(cell),
            _ => None,
        }
    }
}

/// The left-hand side of an assignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PlaceTarget {
    /// A binding, or a path through one, that needs no cell guard.
    Direct(String),
    /// A place behind `cell`'s mutable guard, which lives to the end of the
    /// statement.
    SharedCell {
        /// The cell the guard is taken on.
        cell: SharedCell,
        /// The place expression.
        text: String,
    },
}

impl PlaceTarget {
    /// The place expression.
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Direct(text) | Self::SharedCell { text, .. } => text,
        }
    }

    /// The cell whose mutable guard this assignment takes, if any.
    pub(super) fn guarded_cell(&self) -> Option<&SharedCell> {
        match self {
            Self::Direct(_) => None,
            Self::SharedCell { cell, .. } => Some(cell),
        }
    }
}

/// Facts recorded while a function body and its nested closures are rendered.
///
/// Whether a body touched its owned receiver, or mutably borrowed a capture
/// cell, is only known once the text exists. One instance is shared between a
/// function's emitter and the emitters of the closures nested in it, so a use
/// inside a closure counts for the enclosing body.
#[derive(Debug, Default)]
pub(super) struct BindingUses {
    /// Whether the rendered text named [`LocalBinding::SelfOwned`].
    self_owned: Cell<bool>,
    /// Capture cells mutably borrowed since the last [`Self::take_mut_borrows`].
    mut_borrowed_cells: RefCell<HashSet<String>>,
    /// Whether a body wrote an early `return Ok(..)`.
    early_return: Cell<bool>,
}

impl BindingUses {
    /// Records that `binding` was rendered into the output.
    pub(super) fn record(&self, binding: &LocalBinding) {
        match binding {
            LocalBinding::SelfOwned => self.self_owned.set(true),
            LocalBinding::SharedCapture(cell) => self.record_mut_borrow(cell),
            _ => {}
        }
    }

    /// Records that `cell` was rendered in its mutably borrowing form.
    pub(super) fn record_mut_borrow(&self, cell: &SharedCell) {
        self.mut_borrowed_cells.borrow_mut().insert(cell.cell.clone());
    }

    /// Forgets the receiver uses seen so far, before a body is rendered.
    pub(super) fn reset_self_owned(&self) {
        self.self_owned.set(false);
    }

    /// Whether the receiver handle was named since [`Self::reset_self_owned`].
    pub(super) fn self_owned_used(&self) -> bool {
        self.self_owned.get()
    }

    /// Records that the body wrote an early `return Ok(..)`.
    pub(super) fn record_early_return(&self) {
        self.early_return.set(true);
    }

    /// Runs `render`, answering whether it wrote an early return. A use inside
    /// counts for the enclosing body too.
    pub(super) fn tracking_early_return<T>(&self, render: impl FnOnce() -> T) -> (T, bool) {
        let outer = self.early_return.replace(false);
        let result = render();
        let inner = self.early_return.get();
        self.early_return.set(outer || inner);
        (result, inner)
    }

    /// Answers the cells mutably borrowed so far and clears the record.
    pub(super) fn take_mut_borrows(&self) -> HashSet<String> {
        self.mut_borrowed_cells.take()
    }
}

/// The per-function handle on [`BindingUses`].
pub(super) type SharedBindingUses = Rc<BindingUses>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_cell_reads_and_writes_through_its_own_borrow() {
        let cell = SharedCell::named("total", CellUpgrade::Strong);
        assert_eq!(cell.cell, "smelt_capture_total");
        assert_eq!(cell.read_expr().as_str(), "(*smelt_capture_total.borrow())");
        assert_eq!(cell.write_expr().as_str(), "(*smelt_capture_total.borrow_mut())");
    }

    #[test]
    fn an_upgrading_cell_keeps_its_upgrade_between_cell_and_borrow() {
        let weak = SharedCell::named("this", CellUpgrade::WeakReceiver);
        assert_eq!(weak.read_expr().as_str(), "(*smelt_capture_this.smelt_receiver_cell().borrow())");
        let recursive = SharedCell::named("walk", CellUpgrade::SelfRecursive);
        assert!(recursive.write_expr().starts_with("(*smelt_capture_walk.upgrade()"));
        assert!(recursive.write_expr().ends_with(".borrow_mut())"));
    }

    #[test]
    fn a_moved_body_owns_the_handle_not_the_access_expression() {
        let shared = LocalBinding::SharedCapture(SharedCell::named("x", CellUpgrade::SelfRecursive));
        assert_eq!(shared.cell_handle(), "smelt_capture_x");
        let emitter = LocalBinding::WeakEmitter { name: "bus".to_owned() };
        assert_eq!(emitter.cell_handle(), "smelt_weak_bus");
        assert_eq!(LocalBinding::ClosureArg(2).cell_handle(), "closure_arg_2");
    }

    #[test]
    fn rendering_a_binding_records_what_the_body_will_need() {
        let uses = BindingUses::default();
        uses.record(&LocalBinding::Plain("x".to_owned()));
        assert!(!uses.self_owned_used());
        uses.record(&LocalBinding::SelfOwned);
        assert!(uses.self_owned_used());
        uses.reset_self_owned();
        assert!(!uses.self_owned_used());

        let cell = SharedCell::named("x", CellUpgrade::Strong);
        uses.record(&LocalBinding::SharedCapture(cell.clone()));
        assert!(uses.take_mut_borrows().contains(&cell.cell));
        assert!(uses.take_mut_borrows().is_empty());
    }
}
