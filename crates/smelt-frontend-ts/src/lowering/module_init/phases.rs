//! The ordered prepasses of module lowering, as a typestate.
//!
//! Lowering a module's statements needs a stack of module-wide facts that are
//! each computed from the ones before: host imports can only be classified
//! once every import is read and the module is known to be a test module or
//! not; signatures can only be lowered once the module's own type names
//! shadow the global ones and its class names are pending; a function body can
//! only be lowered once every signature it may call is predeclared. That order
//! used to be a comment-annotated sequence of a dozen calls in
//! `ModuleBuilder::lower_program`, where moving one line compiled fine and
//! broke resolution silently.
//!
//! Each phase here consumes the token the previous one returned and returns
//! the next. The tokens have a private field, so only the phase that precedes
//! them can make one: calling a phase out of order — or skipping one — is a
//! compile error. The individual passes a phase runs are called from that
//! phase only.

use std::collections::HashSet;

use oxc::ast::ast::{Program, Statement};
use oxc::span::GetSpan;
use smelt_hir::Module;

use super::super::support::implemented_function_names;
use super::ModuleBuilder;
use crate::SmeltError;

/// Every import declaration has been read; host imports are still pending.
pub(in crate::lowering) struct ImportsRead {
    /// Unforgeable outside this module.
    _sealed: (),
}

/// Module-wide facts the phases establish and later lowering reads.
pub(in crate::lowering) struct ModuleFacts {
    /// Whether the module is a test module (it keeps erased host bindings and
    /// collects top-level test setup).
    pub(in crate::lowering) test_module: bool,
    /// Names of functions the module implements (overload signatures of these
    /// are not lowered on their own).
    pub(in crate::lowering) implemented_functions: HashSet<String>,
}

/// Host imports are classified and the module's own type names shadow the
/// global ones; type aliases, enums and module globals are declared.
pub(in crate::lowering) struct ModuleTypesDeclared {
    /// Facts established so far.
    facts: ModuleFacts,
    /// Unforgeable outside this module.
    _sealed: (),
}

/// Mutable and class-value module globals are lifted to slots.
pub(in crate::lowering) struct ModuleBindingsCollected {
    /// Facts established so far.
    facts: ModuleFacts,
    /// Module bindings read from a separately lowered body.
    item_reads: HashSet<String>,
    /// Unforgeable outside this module.
    _sealed: (),
}

/// Class and interface names are pending, and class type-parameter defaults
/// and constructor parameter types are readable.
pub(in crate::lowering) struct ClassScopeDeclared {
    /// Facts established so far.
    facts: ModuleFacts,
    /// Module bindings read from a separately lowered body.
    item_reads: HashSet<String>,
    /// Unforgeable outside this module.
    _sealed: (),
}

/// Every function signature, overload, type predicate and function static
/// property is declared: bodies may now be lowered.
pub(in crate::lowering) struct SignaturesDeclared {
    /// Unforgeable outside this module.
    _sealed: (),
}

impl ModuleBuilder<'_> {
    /// Phase 1: read every import declaration of the module.
    pub(in crate::lowering) fn read_imports(
        &mut self,
        program: &Program<'_>,
        module: &mut Module,
    ) -> ImportsRead {
        for statement in &program.body {
            if let Statement::ImportDeclaration(import) = statement {
                self.anchor_top_level_statement(
                    self.span(statement.span().start, statement.span().end),
                );
                self.import_declaration(import, module);
            }
        }
        ImportsRead { _sealed: () }
    }

    /// Phase 2: classify the host imports and declare the module's own type
    /// names, type aliases, enums and globals.
    pub(in crate::lowering) fn declare_module_types(
        &mut self,
        program: &Program<'_>,
        _imports: ImportsRead,
    ) -> ModuleTypesDeclared {
        let test_module = self.is_test_tier_program(program);
        if test_module {
            self.mark_ambient_test_builtins(program);
        }
        self.classify_pending_host_imports(test_module);
        let implemented_functions = implemented_function_names(program);
        self.shadow_cross_module_overloads(&implemented_functions);
        // Declared before any type reference is lowered (the alias predeclaration
        // below is the first), so a module type that shares a global lib type's
        // spelling shadows it everywhere in the module; see
        // `ModuleBuilder::module_type_shadows_global`.
        self.types
            .declare_module_type_names(Self::program_type_declaration_names(program));
        self.predeclare_type_alias_items(program);
        self.collect_module_enums(program);
        self.collect_module_globals(program);
        ModuleTypesDeclared {
            facts: ModuleFacts {
                test_module,
                implemented_functions,
            },
            _sealed: (),
        }
    }

    /// Phase 3: lift module bindings that separately lowered bodies read into
    /// module slots.
    pub(in crate::lowering) fn collect_module_bindings(
        &mut self,
        program: &Program<'_>,
        module: &mut Module,
        errors: &mut Vec<SmeltError>,
        types: ModuleTypesDeclared,
    ) -> ModuleBindingsCollected {
        // Every module binding a separately lowered body reads — a hoisted
        // item, a replayed const initializer, or a module slot's own
        // initializer function — has no module-body local to read through.
        // Each one must resolve to a provably identical folded value or to a
        // module slot; see `collect_item_context_reads`.
        let item_reads = Self::collect_item_context_reads(program);
        self.collect_mutable_globals(program, &item_reads, module, errors);
        self.forget_lifted_const_collections();
        self.collect_class_value_globals(program, &item_reads, module);
        ModuleBindingsCollected {
            facts: types.facts,
            item_reads,
            _sealed: (),
        }
    }

    /// Phase 4: make the module's class and interface names pending, with the
    /// class type surfaces signatures need.
    pub(in crate::lowering) fn declare_class_scope(
        &mut self,
        program: &Program<'_>,
        bindings: ModuleBindingsCollected,
    ) -> ClassScopeDeclared {
        // A module top-level `function Foo(){ this.a = … }` used with `new Foo()`,
        // `x instanceof Foo`, or `Foo.prototype.m = …` is a JavaScript
        // constructor function, not a plain function. Both name sets are handed
        // to the registry in one call, before any function item is predeclared:
        // it treats every constructor-function name as a pending class name so a
        // `new Foo()` lowered before the synthesis still resolves nominally, and
        // `predeclare_function_item` skips the names it reports.
        self.classes.declare_module_scope(
            Self::program_class_names(program),
            Self::module_constructor_function_names(program),
        );
        self.interfaces
            .declare_module_scope(Self::program_interface_names(program));
        // Class TYPE parameters — and in particular their DEFAULTS — have to be
        // readable before any signature is lowered: the forward-function-type
        // and predeclaration passes below lower type references while no class
        // item exists yet, and a reference that omits defaulted trailing
        // arguments must still take them. Runs after the class name scope is
        // declared so each declaration is recorded under the symbol a later
        // reference resolves to.
        self.collect_class_type_parameter_defaults(program);
        self.collect_class_constructor_param_types(program);
        ClassScopeDeclared {
            facts: bindings.facts,
            item_reads: bindings.item_reads,
            _sealed: (),
        }
    }

    /// Phase 5: declare every signature a body may call, then refine what
    /// could only be typed once they exist. Returns the facts the statement
    /// lowering reads, with the token that allows it.
    pub(in crate::lowering) fn declare_signatures(
        &mut self,
        program: &Program<'_>,
        errors: &mut Vec<SmeltError>,
        scope: ClassScopeDeclared,
    ) -> (SignaturesDeclared, ModuleFacts) {
        let ClassScopeDeclared {
            facts, item_reads, ..
        } = scope;
        self.collect_overload_signatures(program, &facts.implemented_functions);
        self.collect_forward_function_types(program, &facts.implemented_functions);
        self.predeclare_function_items(program, &facts.implemented_functions, errors);
        // Type predicates carried by arrows, predicate-typed variables and class
        // methods are recorded before any body is lowered, like the function
        // declarations' own (`predeclare_function_item`), so a hoisted body
        // calling one declared further down still narrows through it.
        self.collect_type_predicate_signatures(program);
        // `collect_module_globals` ran before any function item existed, so an
        // unannotated `const x = makeX()` could not resolve its callee and was
        // recorded `Unknown`. Now that every function signature is declared,
        // those bindings are typed again from their initializers.
        self.refine_unknown_module_global_types(program, &item_reads);
        // Static properties on function declarations (`partial.placeholder = …`)
        // are collected as a PREPASS, right after the function items they hang off
        // are predeclared and before any body is lowered. A function's own body
        // routinely reads its own static — es-toolkit `partial` passes
        // `partial.placeholder` to its implementation — which is a forward
        // reference to a statement that appears after the declaration, so
        // collecting these in source order would leave that read unresolved.
        self.collect_function_static_properties(program, errors);
        (SignaturesDeclared { _sealed: () }, facts)
    }
}
