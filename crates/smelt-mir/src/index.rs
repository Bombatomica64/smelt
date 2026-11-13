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

/// Two declarations of one kind share a name.
///
/// Every by-name lookup in MIR (`class_named`, inherited-type substitution,
/// bound-method throwing) assumes a class name, and an interface name, picks
/// out one declaration. [`NameIndex::new`] refuses a duplicate, and both
/// lowering and the validator build the index through it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateName {
    /// `"class"` or `"interface"`.
    pub kind: &'static str,
    /// The repeated name.
    pub name: Symbol,
    /// Table positions of the first and the repeated declaration.
    pub positions: (usize, usize),
}

impl DuplicateName {
    /// The diagnostic for this repeated declaration name.
    #[must_use]
    pub fn message(&self, mir: &Mir) -> String {
        let (first, second) = self.positions;
        format!(
            "{} `{}` is declared twice (positions {first} and {second}); every by-name lookup needs one declaration",
            self.kind,
            mir.symbols.get(self.name).unwrap_or("<unknown>")
        )
    }
}

/// Positions of classes and of interfaces by name, each name unique within
/// its kind. A class and an interface MAY share a name (TypeScript
/// declaration merging); the two kinds are looked up separately.
#[derive(Debug, Default)]
pub struct NameIndex {
    /// Position of the class of each name.
    classes: HashMap<Symbol, usize>,
    /// Position of the interface of each name.
    interfaces: HashMap<Symbol, usize>,
}

impl NameIndex {
    /// Index `mir`'s classes and interfaces by name.
    ///
    /// # Errors
    ///
    /// Every name declared twice within one kind.
    pub fn new(mir: &Mir) -> Result<Self, Vec<DuplicateName>> {
        let mut duplicates = Vec::new();
        let classes = unique_positions(
            "class",
            mir.classes.iter().map(|class| class.name),
            &mut duplicates,
        );
        let interfaces = unique_positions(
            "interface",
            mir.interfaces.iter().map(|interface| interface.name),
            &mut duplicates,
        );
        if duplicates.is_empty() {
            Ok(Self { classes, interfaces })
        } else {
            Err(duplicates)
        }
    }
}

/// Map each name to its position, recording every repeat in `duplicates`. A
/// repeated name maps to no position, so a lookup of it answers nothing rather
/// than an arbitrary one of the declarations.
fn unique_positions(
    kind: &'static str,
    names: impl Iterator<Item = Symbol>,
    duplicates: &mut Vec<DuplicateName>,
) -> HashMap<Symbol, usize> {
    let mut positions: HashMap<Symbol, usize> = HashMap::new();
    let mut repeated = Vec::new();
    for (position, name) in names.enumerate() {
        if let Some(first) = positions.get(&name) {
            duplicates.push(DuplicateName { kind, name, positions: (*first, position) });
            repeated.push(name);
        } else {
            positions.insert(name, position);
        }
    }
    for name in repeated {
        positions.remove(&name);
    }
    positions
}

/// The lookups, by name and by call target.
#[derive(Debug, Default)]
struct Lookups {
    /// Classes and interfaces by name.
    names: NameIndex,
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
            let mut lookups = Lookups {
                // A program with a duplicate never gets here (lowering and the
                // validator both reject it); should one be built by hand, the
                // ambiguous name answers nothing rather than its first entry.
                names: NameIndex::new(self).unwrap_or_else(|_| {
                    let mut ignored = Vec::new();
                    NameIndex {
                        classes: unique_positions(
                            "class",
                            self.classes.iter().map(|class| class.name),
                            &mut ignored,
                        ),
                        interfaces: unique_positions(
                            "interface",
                            self.interfaces.iter().map(|interface| interface.name),
                            &mut ignored,
                        ),
                    }
                }),
                static_callers: HashMap::new(),
            };
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

    /// The class named `name`.
    #[must_use]
    pub fn class_named(&self, name: Symbol) -> Option<&MirClass> {
        self.classes.get(*self.lookups().names.classes.get(&name)?)
    }

    /// The interface named `name`.
    #[must_use]
    pub fn interface_named(&self, name: Symbol) -> Option<&MirInterface> {
        self.interfaces.get(*self.lookups().names.interfaces.get(&name)?)
    }

    /// Whether a class is named `name`.
    #[must_use]
    pub fn has_class(&self, name: Symbol) -> bool {
        self.lookups().names.classes.contains_key(&name)
    }

    /// Whether an interface is named `name`.
    #[must_use]
    pub fn has_interface(&self, name: Symbol) -> bool {
        self.lookups().names.interfaces.contains_key(&name)
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
            statics_of: None,
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

    /// A repeated class name is refused by the index constructor and answers
    /// nothing, rather than its first declaration (audit C12); unique names
    /// answer as before, and a class may share an interface's name.
    #[test]
    #[expect(clippy::expect_used, reason = "test fixtures unwrap and index their own known-good shapes")]
    fn a_repeated_name_is_refused_not_resolved_to_its_first() {
        let mut program = mir();
        program.classes = vec![
            class(Symbol(1), Some(Symbol(7))),
            class(Symbol(1), Some(Symbol(8))),
            class(Symbol(2), None),
            class(Symbol(3), None),
        ];
        program.interfaces = vec![interface(Symbol(3))];
        let duplicates = super::NameIndex::new(&program).expect_err("duplicate class");
        assert_eq!(
            duplicates,
            [super::DuplicateName { kind: "class", name: Symbol(1), positions: (0, 1) }]
        );
        assert!(program.class_named(Symbol(1)).is_none());
        assert!(program.has_class(Symbol(3)), "a class may share an interface's name");
        assert!(program.has_class(Symbol(2)));
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
