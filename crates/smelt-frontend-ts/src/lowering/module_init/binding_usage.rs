//! Module binding usage: which top-level bindings are written, read from a
//! hoisted item body, or called, as the AST visitors find them. These facts decide
//! which bindings become module slots.

use super::super::ModuleBuilder;
use oxc::ast::ast::{BindingPattern, Declaration, Expression, Program, Statement};
use std::collections::HashSet;

impl ModuleBuilder<'_> {
    /// Collect the names of every binding reassigned or updated inside a
    /// hoisted item body: top-level function declarations, class declarations,
    /// and `const` arrow/function initializers (all of which lower to items
    /// with no access to a module-body local).
    ///
    /// Mutations written directly in module-body statements — including inline
    /// callbacks such as `forEach` bodies — are deliberately NOT collected:
    /// there the binding lowers to an ordinary mutable module-body local and
    /// the existing assignment path already works, byte-identical to today.
    /// Within each scanned subtree the collection is a conservative
    /// over-approximation (it ignores inner shadowing scopes), which is
    /// sufficient to decide which module-level `let`/`var` bindings need the
    /// mutable-global lift.
    pub(super) fn collect_mutated_names(
        program: &Program<'_>,
    ) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
        use oxc::ast_visit::Visit;
        let mut collector = MutatedNameCollector {
            names: HashSet::new(),
            mutated_through: HashSet::new(),
            mutated_through_nested: HashSet::new(),
        };
        for statement in &program.body {
            let declaration = match statement {
                Statement::FunctionDeclaration(function) => {
                    collector.visit_function(function, oxc::semantic::ScopeFlags::Function);
                    continue;
                }
                Statement::ClassDeclaration(class) => {
                    collector.visit_class(class);
                    continue;
                }
                Statement::VariableDeclaration(decl) => {
                    Self::collect_mutated_names_in_const_callables(&mut collector, decl);
                    continue;
                }
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                Statement::ExportDefaultDeclaration(_) => None,
                _ => None,
            };
            match declaration {
                Some(Declaration::FunctionDeclaration(function)) => {
                    collector.visit_function(function, oxc::semantic::ScopeFlags::Function);
                }
                Some(Declaration::ClassDeclaration(class)) => {
                    collector.visit_class(class);
                }
                Some(Declaration::VariableDeclaration(decl)) => {
                    Self::collect_mutated_names_in_const_callables(&mut collector, decl);
                }
                _ => {}
            }
        }
        (
            collector.names,
            collector.mutated_through,
            collector.mutated_through_nested,
        )
    }

    /// Collect every name assigned, updated, or written through ANYWHERE in the
    /// program — module-body statements included.
    ///
    /// [`Self::collect_mutated_names`] deliberately scans only hoisted item
    /// bodies, because a binding mutated only by module-body statements keeps an
    /// ordinary module-body local. That is right exactly until an item body
    /// READS the binding: then the write and the read have to meet in one
    /// storage location, which only a module slot provides. The caller
    /// intersects this set with the item-context reads to find those bindings.
    /// The scan ignores shadowing, so it over-approximates — a spurious entry
    /// costs a slot, which still holds the binding's one value.
    pub(super) fn collect_all_mutated_names(
        program: &Program<'_>,
    ) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
        use oxc::ast_visit::Visit;
        let mut collector = MutatedNameCollector {
            names: HashSet::new(),
            mutated_through: HashSet::new(),
            mutated_through_nested: HashSet::new(),
        };
        collector.visit_program(program);
        (
            collector.names,
            collector.mutated_through,
            collector.mutated_through_nested,
        )
    }

    /// Collect every module binding read from a body lowered SEPARATELY from
    /// the module body, closed over module-slot initializers.
    ///
    /// The seed is [`Self::collect_hoisted_body_reads`]: function and class
    /// declarations, `const` arrow/function initializers and replayed
    /// object/array literals. A module binding in that set cannot be read
    /// through a module-body local, so it becomes a module slot unless a folded
    /// value is provably identical — and a slot's initializer is itself lowered
    /// into a synthesized nullary function item (see
    /// [`Self::lower_pending_mutable_global_init`]), which has no module-body
    /// locals either. So the free names of every such binding's initializer
    /// are item-context reads too, and the set is closed to a fixed point:
    ///
    /// ```ts
    /// const double = (n: number) => n * 2;
    /// const table: Record<string, number> = { four: double(2) };
    /// function lookup(key: string) { return table[key]; }
    /// ```
    ///
    /// `table` is read by `lookup`, so its initializer runs in an item context,
    /// so `double` is read from one — and must be an item or a slot, never a
    /// default-returning stub.
    ///
    /// Over-approximation (an initializer whose binding ends up folded after
    /// all) only adds names that must then be folded or lifted themselves,
    /// which is always correct.
    pub(super) fn collect_item_context_reads(program: &Program<'_>) -> HashSet<String> {
        use oxc::ast_visit::Visit;
        let mut reads = Self::collect_hoisted_body_reads(program);
        let mut declarators: Vec<(&str, &Expression<'_>)> = Vec::new();
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
            if variable.declare {
                continue;
            }
            for declarator in &variable.declarations {
                if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                    && let Some(init) = &declarator.init
                {
                    declarators.push((binding.name.as_str(), init));
                }
            }
        }
        let mut expanded: HashSet<&str> = HashSet::new();
        loop {
            let mut changed = false;
            for (name, init) in &declarators {
                if !reads.contains(*name) || !expanded.insert(name) {
                    continue;
                }
                let mut collector = ReadNameCollector {
                    names: HashSet::new(),
                    bound: HashSet::new(),
                };
                collector.visit_expression(init);
                #[expect(clippy::iter_over_hash_type, reason = "set union")]
                for free in collector.into_free_names() {
                    changed |= reads.insert(free);
                }
            }
            if !changed {
                break;
            }
        }
        reads
    }

    /// Names of the module-level identifier bindings declared WITH an
    /// initializer (ambient declarations excluded).
    pub(super) fn initialized_module_binding_names(program: &Program<'_>) -> HashSet<String> {
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
            if variable.declare {
                continue;
            }
            for declarator in &variable.declarations {
                if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                    && declarator.init.is_some()
                {
                    names.insert(binding.name.as_str().to_owned());
                }
            }
        }
        names
    }

    /// Fold each never-mutated module `let`/`var` with a literal initializer
    /// exactly like a `const`.
    ///
    /// A binding no statement anywhere assigns is a constant in all but
    /// spelling, so its literal is provably the one value every reader sees.
    /// Only `const` was folded before, and an item body reading `let greeting =
    /// 'hello'` fabricated the declared type's default (`""`) instead.
    pub(super) fn fold_unmutated_module_let_literals(
        &mut self,
        program: &Program<'_>,
        mutated_anywhere: &HashSet<String>,
        item_reads: &HashSet<String>,
    ) {
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
            if variable.declare || variable.kind == oxc::ast::ast::VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &variable.declarations {
                let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                    continue;
                };
                let name = binding.name.as_str();
                // Only an item-read binding needs the fold: a module-body read
                // already reads the binding's own local.
                if !item_reads.contains(name)
                    || mutated_anywhere.contains(name)
                    || self.consts.literal(name).is_some()
                {
                    continue;
                }
                let Some(init) = &declarator.init else {
                    continue;
                };
                // The folded literal must carry the binding's recorded type, so
                // an item-body read sees the type a module-body read does.
                if let Ok(value) = self.literal_const_expression(init)
                    && self
                        .module_globals
                        .get(name)
                        .is_some_and(|recorded| *recorded == value.ty)
                {
                    self.consts.set_literal(name.to_owned(), value);
                }
            }
        }
    }

    /// Every identifier the program uses as a call's callee (`name(..)`).
    pub(super) fn collect_called_identifiers(program: &Program<'_>) -> HashSet<String> {
        use oxc::ast_visit::Visit;
        /// AST visitor recording identifier callees.
        struct CalledNames {
            /// The collected callee names.
            names: HashSet<String>,
        }
        impl<'a> Visit<'a> for CalledNames {
            fn visit_call_expression(&mut self, call: &oxc::ast::ast::CallExpression<'a>) {
                if let Expression::Identifier(callee) = call.callee.without_parentheses() {
                    self.names.insert(callee.name.to_string());
                }
                oxc::ast_visit::walk::walk_call_expression(self, call);
            }
        }
        let mut collector = CalledNames {
            names: HashSet::new(),
        };
        collector.visit_program(program);
        collector.names
    }

    /// Collect every identifier name read inside a hoisted item body.
    ///
    /// Same statement dispatch as [`Self::collect_mutated_names`] — top-level
    /// function and class declarations, exported or not, plus `const`
    /// arrow/function initializers — because "positions that lower to an item"
    /// is the same set for reads as for writes. Statements of the module body
    /// itself are NOT visited: a binding used only there keeps its ordinary
    /// module-body local and lowers byte-identically to before.
    fn collect_hoisted_body_reads(program: &Program<'_>) -> HashSet<String> {
        use oxc::ast_visit::Visit;
        let mut collector = ReadNameCollector {
            names: HashSet::new(),
            bound: HashSet::new(),
        };
        for statement in &program.body {
            let declaration = match statement {
                Statement::FunctionDeclaration(function) => {
                    collector.visit_function(function, oxc::semantic::ScopeFlags::Function);
                    continue;
                }
                Statement::ClassDeclaration(class) => {
                    collector.visit_class(class);
                    continue;
                }
                Statement::VariableDeclaration(decl) => {
                    Self::collect_hoisted_reads_in_const_callables(&mut collector, decl);
                    continue;
                }
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                Statement::ExportDefaultDeclaration(_) => None,
                _ => None,
            };
            match declaration {
                Some(Declaration::FunctionDeclaration(function)) => {
                    collector.visit_function(function, oxc::semantic::ScopeFlags::Function);
                }
                Some(Declaration::ClassDeclaration(class)) => {
                    collector.visit_class(class);
                }
                Some(Declaration::VariableDeclaration(decl)) => {
                    Self::collect_hoisted_reads_in_const_callables(&mut collector, decl);
                }
                _ => {}
            }
        }
        collector.into_free_names()
    }

    /// Scan module `const` initializers that are REPLAYED inside item bodies.
    ///
    /// Two shapes qualify, and for one reason: their initializer expression is
    /// re-lowered at each use site rather than evaluated once into a
    /// module-body local, so an identifier it reads is read from wherever that
    /// use site is — inside a hoisted item body, where there is no module-body
    /// local to read through.
    ///
    /// * an arrow/function initializer, whose body runs at each call;
    /// * an object or array/`as const` literal, which `self.consts` records and
    ///   rebuilds per use (`const DATA = { view: VIEW }` read by
    ///   `Object.values(DATA)` from another item rebuilds the literal there, and
    ///   with it the read of `VIEW`).
    ///
    /// Missing the second shape is what made a module-level
    /// `const VIEW = new Uint8Array(1)` referenced from an exported object
    /// literal report the class-typed-module-binding blocker: the slot pass
    /// declined the binding because it saw no item-body read, and then the read
    /// happened in an item body anyway.
    fn collect_hoisted_reads_in_const_callables(
        collector: &mut ReadNameCollector,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) {
        use oxc::ast_visit::Visit;
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return;
        }
        for declarator in &decl.declarations {
            if let Some(
                init @ (Expression::ArrowFunctionExpression(_)
                | Expression::FunctionExpression(_)
                | Expression::ObjectExpression(_)
                | Expression::ArrayExpression(_)
                | Expression::TSAsExpression(_)),
            ) = &declarator.init
            {
                collector.visit_expression(init);
            }
        }
    }

    /// Scan `const name = <arrow/function>` initializers for mutation targets.
    ///
    /// Const arrow/function bindings lift to items, so their bodies, like
    /// function declarations, have no module-body local to assign through and
    /// need the global lift.
    fn collect_mutated_names_in_const_callables(
        collector: &mut MutatedNameCollector,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) {
        use oxc::ast_visit::Visit;
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return;
        }
        for declarator in &decl.declarations {
            if let Some(
                init @ (Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)),
            ) = &declarator.init
            {
                collector.visit_expression(init);
            }
        }
    }
}

/// Collects the identifiers READ inside a hoisted item body that the item does
/// not bind itself.
///
/// The mirror of [`MutatedNameCollector`] for the read direction, and driven by
/// the same hoisted-body traversal: a module-level binding read from a position
/// that lowers to an ITEM (a function or class declaration, or a `const`
/// arrow/function initializer) has no module-body local to read through, which
/// is the condition [`ModuleBuilder::collect_class_value_globals`] lifts on.
///
/// # Shadowing
///
/// `bound` records every name the item introduces anywhere inside itself — a
/// parameter, a `let`/`const`/`var`, a catch binding, a nested function's name
/// — and [`Self::into_free_names`] subtracts it from the reads. A function
/// whose PARAMETER happens to share a module binding's name does not read that
/// module binding, and counting it would lift a binding nothing outside the
/// module body ever looks at.
///
/// Bindings are subtracted per lexical scope: each function and block keeps its
/// own, and what stays free in it reaches the enclosing scope. A parameter of one
/// function says nothing about what its neighbour reads.
struct ReadNameCollector {
    /// Identifier names referenced inside the item.
    names: HashSet<String>,
    /// Names the item binds itself, at any depth.
    bound: HashSet<String>,
}

impl ReadNameCollector {
    /// Run `visit` as one lexical scope: what it leaves free joins this scope's
    /// reads, and what it binds stays inside.
    fn in_own_scope(&mut self, visit: impl FnOnce(&mut Self)) {
        let names = std::mem::take(&mut self.names);
        let bound = std::mem::take(&mut self.bound);
        visit(self);
        let inner = std::mem::replace(self, Self { names, bound });
        self.names.extend(inner.into_free_names());
    }

    /// The names read but not bound by the item.
    fn into_free_names(self) -> HashSet<String> {
        let Self { names, bound } = self;
        names.difference(&bound).cloned().collect()
    }
}

impl<'a> oxc::ast_visit::Visit<'a> for ReadNameCollector {
    /// Records the name of each identifier reference.
    fn visit_identifier_reference(&mut self, identifier: &oxc::ast::ast::IdentifierReference<'a>) {
        self.names.insert(identifier.name.as_str().to_owned());
    }

    fn visit_binding_identifier(&mut self, binding: &oxc::ast::ast::BindingIdentifier<'a>) {
        self.bound.insert(binding.name.as_str().to_owned());
    }

    fn visit_function(
        &mut self,
        function: &oxc::ast::ast::Function<'a>,
        flags: oxc::semantic::ScopeFlags,
    ) {
        self.in_own_scope(|this| oxc::ast_visit::walk::walk_function(this, function, flags));
    }

    fn visit_arrow_function_expression(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) {
        self.in_own_scope(|this| {
            oxc::ast_visit::walk::walk_arrow_function_expression(this, arrow);
        });
    }

    fn visit_block_statement(&mut self, block: &oxc::ast::ast::BlockStatement<'a>) {
        self.in_own_scope(|this| oxc::ast_visit::walk::walk_block_statement(this, block));
    }
}

/// The names a module's statements assign to, and how deeply.
///
/// One pass collects all three sets so a module-level binding's treatment --
/// a reassigned cell, a write THROUGH a cell, or a nested write the lowering
/// still refuses -- is decided from the whole module rather than per statement.
struct MutatedNameCollector {
    /// Names observed as an assignment or update target.
    names: HashSet<String>,
    /// Names observed as the DIRECT base of a member or index assignment
    /// target — `name[key] = …`, `name.field = …`, one projection deep.
    ///
    /// Tracked separately from [`Self::names`] because the two need different
    /// treatments for a mutable global: reassigning the binding replaces the
    /// cell's whole value (`GlobalSet`), while writing *through* it mutates the
    /// value the cell holds. `Place::Global` lowers exactly this one-deep shape
    /// by naming the cell as the assignment root.
    mutated_through: HashSet<String>,
    /// Names reached as the base of a NESTED assignment target — `name[a][b] =
    /// …`, `name.a.b = …`.
    ///
    /// Still blocks. The inner projection has to produce a value, and whether
    /// that value shares storage with the cell is the handle-versus-value
    /// question `Place::Global` exists to avoid asking; guessing it is how a
    /// write gets silently lost. See `hono-h6-place-global.md` (in git history).
    mutated_through_nested: HashSet<String>,
}

impl MutatedNameCollector {
    /// Record the root identifier of a member/index assignment target's object,
    /// unwrapping the type-level wrappers that never change the value.
    fn record_write_through_base(&mut self, object: &Expression<'_>, direct: bool) {
        match object {
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str().to_owned();
                if direct {
                    self.mutated_through.insert(name);
                } else {
                    self.mutated_through_nested.insert(name);
                }
            }
            // Type-level wrappers never change the value, so they do not make a
            // direct write nested.
            Expression::ParenthesizedExpression(parenthesized) => {
                self.record_write_through_base(&parenthesized.expression, direct);
            }
            Expression::TSAsExpression(as_expr) => {
                self.record_write_through_base(&as_expr.expression, direct);
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.record_write_through_base(&satisfies.expression, direct);
            }
            Expression::TSNonNullExpression(non_null) => {
                self.record_write_through_base(&non_null.expression, direct);
            }
            // A nested base (`a[b][c] = …`, `a.b.c = …`) still bottoms out at a
            // root identifier, and that root is the binding being mutated.
            Expression::ComputedMemberExpression(member) => {
                self.record_write_through_base(&member.object, false);
            }
            Expression::StaticMemberExpression(member) => {
                self.record_write_through_base(&member.object, false);
            }
            _ => {}
        }
    }
}

impl<'a> oxc::ast_visit::Visit<'a> for MutatedNameCollector {
    /// A MUTATING builtin method called directly on a binding
    /// (`seen.push(x)`, `cache.set(k, v)`) writes through it exactly like an
    /// index assignment does, so the binding is module state.
    fn visit_call_expression(&mut self, call: &oxc::ast::ast::CallExpression<'a>) {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && matches!(
                member.property.name.as_str(),
                "push"
                    | "pop"
                    | "shift"
                    | "unshift"
                    | "splice"
                    | "sort"
                    | "reverse"
                    | "fill"
                    | "copyWithin"
                    | "set"
                    | "add"
                    | "delete"
                    | "clear"
            )
        {
            self.record_write_through_base(&member.object, true);
        }
        oxc::ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_simple_assignment_target(
        &mut self,
        target: &oxc::ast::ast::SimpleAssignmentTarget<'a>,
    ) {
        match target {
            oxc::ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                self.names.insert(identifier.name.as_str().to_owned());
            }
            oxc::ast::ast::SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.record_write_through_base(&member.object, true);
            }
            oxc::ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.record_write_through_base(&member.object, true);
            }
            _ => {}
        }
        oxc::ast_visit::walk::walk_simple_assignment_target(self, target);
    }
}
