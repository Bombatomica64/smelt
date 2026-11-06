//! Name and call-site lookups over finished MIR.
//!
//! The backend asks "which class is this name" and "who calls this function" for
//! every emitted member, and a linear scan per question makes emission quadratic
//! in the program. The answers are computed once, on first use, and only asked
//! of MIR no pass is still rewriting: `opt::optimize` clears them, and cloning a
//! `Mir` starts empty.

use std::collections::HashMap;
use std::sync::OnceLock;

use smelt_hir::Symbol;

use crate::{Callee, FuncId, Mir, MirClass, MirFunction, MirInterface, Terminator};

/// The lookups, by name and by call target.
#[derive(Debug, Default)]
struct Lookups {
    /// Position of the first class of each name.
    classes: HashMap<Symbol, usize>,
    /// Position of the first interface of each name.
    interfaces: HashMap<Symbol, usize>,
    /// Positions of the functions that statically call each function, in
    /// function order.
    static_callers: HashMap<FuncId, Vec<usize>>,
}

/// Lazily built [`Lookups`], which never serialize and never survive a clone.
#[derive(Default)]
pub struct MirIndex(OnceLock<Lookups>);

impl Clone for MirIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MirIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MirIndex")
    }
}

impl Mir {
    /// The lookups, built on first use.
    fn lookups(&self) -> &Lookups {
        self.index.0.get_or_init(|| {
            let mut lookups = Lookups::default();
            for (position, class) in self.classes.iter().enumerate() {
                lookups.classes.entry(class.name).or_insert(position);
            }
            for (position, interface) in self.interfaces.iter().enumerate() {
                lookups.interfaces.entry(interface.name).or_insert(position);
            }
            for (position, function) in self.functions.iter().enumerate() {
                for block in &function.blocks {
                    if let Some(Terminator::Call { callee: Callee::Static(target), .. }) = &block.terminator {
                        let callers = lookups.static_callers.entry(*target).or_default();
                        if callers.last() != Some(&position) {
                            callers.push(position);
                        }
                    }
                }
            }
            lookups
        })
    }

    /// Drops the lookups, for a pass that has rewritten this MIR.
    pub(crate) fn clear_index(&mut self) {
        self.index = MirIndex::default();
    }

    /// The first class named `name`.
    #[must_use]
    pub fn class_named(&self, name: Symbol) -> Option<&MirClass> {
        self.classes.get(*self.lookups().classes.get(&name)?)
    }

    /// The first interface named `name`.
    #[must_use]
    pub fn interface_named(&self, name: Symbol) -> Option<&MirInterface> {
        self.interfaces.get(*self.lookups().interfaces.get(&name)?)
    }

    /// Whether a class is named `name`.
    #[must_use]
    pub fn has_class(&self, name: Symbol) -> bool {
        self.lookups().classes.contains_key(&name)
    }

    /// Whether an interface is named `name`.
    #[must_use]
    pub fn has_interface(&self, name: Symbol) -> bool {
        self.lookups().interfaces.contains_key(&name)
    }

    /// The functions holding a static call to `target`, in function order.
    pub fn static_callers(&self, target: FuncId) -> impl Iterator<Item = &MirFunction> {
        self.lookups()
            .static_callers
            .get(&target)
            .into_iter()
            .flatten()
            .filter_map(|position| self.functions.get(*position))
    }
}
