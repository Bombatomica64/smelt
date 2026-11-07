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

#[cfg(test)]
mod tests {
    use smelt_hir::{
        BodyId, ClassKind, FileId, OriginalNameTable, Span, Symbol, SymbolInterner, TypeId,
        TypeInterner,
    };

    use crate::{
        BlockId, Callee, FuncId, HirOrigin, LocalId, Mir, MirClass, MirFunction, MirInterface,
        Terminator,
    };

    /// A zero-width span; spans play no part in the lookups.
    fn span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    /// A class named `name`, told apart by `base`.
    fn class(name: Symbol, base: Option<Symbol>) -> MirClass {
        MirClass {
            name,
            kind: ClassKind::Plain,
            type_params: Vec::new(),
            is_abstract: false,
            base,
            base_args: Vec::new(),
            fields: Vec::new(),
            static_fields: Vec::new(),
            descriptors: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            static_methods: Vec::new(),
            abstract_methods: Vec::new(),
            implements: Vec::new(),
            protocols: Vec::new(),
        }
    }

    /// An interface named `name`, told apart by nothing else.
    fn interface(name: Symbol) -> MirInterface {
        MirInterface {
            name,
            type_params: Vec::new(),
            extends: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            decode_facts: Vec::new(),
        }
    }

    /// Function `id`, whose entry block statically calls `callee` when given.
    fn function(id: u32, callee: Option<u32>) -> MirFunction {
        let mut function =
            MirFunction::new(FuncId(id), Symbol(id), HirOrigin::Body(BodyId(0)), TypeId(0), span());
        function.blocks[0].terminator = callee.map(|target| Terminator::Call {
            callee: Callee::Static(FuncId(target)),
            args: Vec::new(),
            dest: LocalId(0),
            target: BlockId(0),
            unwind: None,
        });
        function
    }

    /// An empty program.
    fn mir() -> Mir {
        Mir::new(
            TypeInterner::default(),
            SymbolInterner::default(),
            OriginalNameTable::default(),
        )
    }

    /// The first declaration wins a repeated name, as a linear `find` answers.
    #[test]
    fn a_name_finds_its_first_declaration() {
        let mut program = mir();
        program.classes = vec![
            class(Symbol(1), Some(Symbol(7))),
            class(Symbol(1), Some(Symbol(8))),
            class(Symbol(2), None),
        ];
        program.interfaces = vec![interface(Symbol(3))];
        assert_eq!(program.class_named(Symbol(1)).and_then(|found| found.base), Some(Symbol(7)));
        assert!(program.has_class(Symbol(2)));
        assert!(!program.has_class(Symbol(3)));
        assert!(program.has_interface(Symbol(3)));
        assert!(!program.has_interface(Symbol(1)));
        assert_eq!(program.interface_named(Symbol(3)).map(|found| found.name), Some(Symbol(3)));
        assert!(program.interface_named(Symbol(9)).is_none());
    }

    /// Callers come back once each, in function order, and a function nothing
    /// calls has none.
    #[test]
    fn static_callers_are_unique_and_in_function_order() {
        let mut program = mir();
        let mut twice = function(2, Some(0));
        let mut second_block = twice.blocks[0].clone();
        second_block.id = BlockId(1);
        twice.blocks.push(second_block);
        program.functions = vec![function(0, None), function(1, Some(0)), twice, function(3, Some(1))];
        let callers = |program: &Mir, target| {
            program.static_callers(FuncId(target)).map(|caller| caller.id).collect::<Vec<_>>()
        };
        assert_eq!(callers(&program, 0), [FuncId(1), FuncId(2)]);
        assert_eq!(callers(&program, 1), [FuncId(3)]);
        assert_eq!(callers(&program, 3), [] as [FuncId; 0]);
    }

    /// A clone and a cleared index both answer from the MIR as it stands, so a
    /// rewrite cannot leave a stale position behind.
    #[test]
    fn a_clone_or_a_clear_forgets_the_lookups() {
        let mut program = mir();
        program.classes = vec![class(Symbol(1), None)];
        assert!(program.has_class(Symbol(1)));
        let mut copy = program.clone();
        copy.classes = vec![class(Symbol(2), None)];
        assert!(copy.has_class(Symbol(2)));
        assert!(!copy.has_class(Symbol(1)));
        program.classes.clear();
        assert!(program.has_class(Symbol(1)), "the lookups stand until cleared");
        program.clear_index();
        assert!(!program.has_class(Symbol(1)));
    }
}
