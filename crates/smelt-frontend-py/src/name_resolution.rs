//! Static name resolution for Python definition-time expressions.
//!
//! A class decorator, base class or `metaclass=` keyword is an ordinary Python
//! expression evaluated when the `class` statement runs, and a method
//! decorator is evaluated while the class body runs. What such an expression
//! denotes is therefore decided by the bindings in effect at that point, not
//! by its spelling: `abc.ABC`, `a.ABC` after `import abc as a`, and `Base`
//! after `from abc import ABC as Base` all denote `abc.ABC`, while an `ABC`
//! rebound by the module itself denotes something else.
//!
//! This module implements that rule once, for every module:
//!
//! 1. [`ModuleBindings::collect`] runs a reaching-definitions pass over the
//!    module body, and over the body of each class, and records which
//!    bindings may be in effect at the start of every class statement and
//!    every method definition.
//! 2. [`ModuleBindings::resolve`] maps a `Name` / `Attribute` chain to the set
//!    of [`Denotation`]s that can reach the use: the fully qualified name an
//!    import or builtin lookup yields, or a definition in this module.
//! 3. [`Resolution::stdlib`] matches that set against [`StdlibName`], the
//!    standard-library definitions the frontend models.
//!
//! Python resolves a module-scope name through the module globals and then
//! `builtins`, so a name with no binding reaching the use denotes
//! `builtins.<name>`. The pass follows the statement structure: the branches
//! of an `if` or `match` are joined, a loop body may run any number of times,
//! an `except` handler starts from any point of its `try` body and deletes its
//! `as` name on exit, and a short-circuit operand, conditional-expression arm
//! or comprehension may not run. A `from m import *` may or may not bind any
//! given name, so it adds `m.<name>` as one more candidate. When candidates
//! disagree on a modeled name, the use is ambiguous and callers report it
//! instead of guessing.
//!
//! Known limits, both over-approximations of what can reach a use: `break`,
//! `continue`, `return` and `raise` are treated as falling through, and a
//! `global NAME` assignment inside a function body is not seen, because
//! function bodies are not scanned.
#![allow(
    clippy::redundant_pub_crate,
    reason = "the resolver is shared by sibling lowering modules"
)]

use std::collections::{BTreeSet, HashMap};

use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{ExceptHandler, Expr, ExprContext, Pattern, Stmt};
use ruff_text_size::TextSize;

/// Standard-library definitions whose semantics the Python frontend models.
///
/// The frontend recognizes these by identity: an expression matches only when
/// every binding that can reach it denotes the variant's
/// [`qualified_name`](Self::qualified_name). The spelling at the use site
/// (bare, dotted, aliased) plays no part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StdlibName {
    /// `builtins.object`, the implicit root base class.
    Object,
    /// `abc.ABC`, a base class that marks a class abstract.
    Abc,
    /// `abc.ABCMeta`, the metaclass behind `abc.ABC`.
    AbcMeta,
    /// `abc.abstractmethod`, the abstract-method decorator.
    AbstractMethod,
    /// `dataclasses.dataclass`, the dataclass decorator.
    Dataclass,
    /// `typing.Generic`, the marker base that declares class type parameters.
    Generic,
    /// `typing.Protocol`, the marker base that declares a structural type.
    Protocol,
    /// `enum.IntEnum`, the integer enumeration base class.
    IntEnum,
}

impl StdlibName {
    /// Every modeled definition, for reverse lookup by qualified name.
    const ALL: [Self; 8] = [
        Self::Object,
        Self::Abc,
        Self::AbcMeta,
        Self::AbstractMethod,
        Self::Dataclass,
        Self::Generic,
        Self::Protocol,
        Self::IntEnum,
    ];

    /// The fully qualified name of the definition (`module.attribute`).
    pub(crate) const fn qualified_name(self) -> &'static str {
        match self {
            Self::Object => "builtins.object",
            Self::Abc => "abc.ABC",
            Self::AbcMeta => "abc.ABCMeta",
            Self::AbstractMethod => "abc.abstractmethod",
            Self::Dataclass => "dataclasses.dataclass",
            Self::Generic => "typing.Generic",
            Self::Protocol => "typing.Protocol",
            Self::IntEnum => "enum.IntEnum",
        }
    }

    /// The definition's own `__name__` (`IntEnum` for `enum.IntEnum`).
    pub(crate) fn name(self) -> &'static str {
        let qualified = self.qualified_name();
        qualified
            .rsplit_once('.')
            .map_or(qualified, |(_, name)| name)
    }

    /// Look up the modeled definition with this fully qualified name.
    fn from_qualified_name(qualified: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|name| name.qualified_name() == qualified)
    }
}

/// One thing a resolved expression may denote.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Denotation {
    /// A definition made by this module itself (`class`, `def`, assignment,
    /// loop target, …), or an attribute of one.
    Local,
    /// The object an import or the `builtins` fallback yields, by fully
    /// qualified name (`abc.ABC`, `builtins.object`, `.models.Base` for a
    /// relative import).
    Qualified(String),
}

/// How a [`Resolution`] relates to the modeled [`StdlibName`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StdlibMatch {
    /// Every candidate denotes this modeled definition.
    Exact(StdlibName),
    /// At least one candidate denotes a modeled definition, but not every
    /// candidate denotes the same one; which one runs depends on control flow.
    Ambiguous,
    /// No candidate denotes a modeled definition.
    NotStdlib,
}

/// Every [`Denotation`] that can reach one use of an expression.
///
/// Never empty: a name with no reaching binding still denotes its `builtins`
/// fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolution {
    /// Sorted, deduplicated candidates.
    candidates: Vec<Denotation>,
}

impl Resolution {
    /// Build a resolution from unsorted candidates, deduplicating them.
    fn new(mut candidates: Vec<Denotation>) -> Self {
        candidates.sort();
        candidates.dedup();
        Self { candidates }
    }

    /// The candidates, sorted and deduplicated.
    #[cfg(test)]
    pub(crate) fn candidates(&self) -> &[Denotation] {
        &self.candidates
    }

    /// The fully qualified name every candidate agrees on, if any.
    pub(crate) fn qualified_name(&self) -> Option<&str> {
        match self.candidates.as_slice() {
            [Denotation::Qualified(name)] => Some(name),
            _ => None,
        }
    }

    /// Match the candidates against the modeled standard-library definitions.
    pub(crate) fn stdlib(&self) -> StdlibMatch {
        // Candidates are deduplicated, so agreeing on one name means there is
        // exactly one candidate.
        if let Some(name) = self
            .qualified_name()
            .and_then(StdlibName::from_qualified_name)
        {
            return StdlibMatch::Exact(name);
        }
        let any_modeled = self.candidates.iter().any(|candidate| {
            matches!(candidate, Denotation::Qualified(name)
                if StdlibName::from_qualified_name(name).is_some())
        });
        if any_modeled {
            StdlibMatch::Ambiguous
        } else {
            StdlibMatch::NotStdlib
        }
    }

    /// Human-readable candidate list for diagnostics (`abc.ABC or builtins.ABC`).
    pub(crate) fn describe(&self) -> String {
        self.candidates
            .iter()
            .map(|candidate| match candidate {
                Denotation::Local => "a definition in this module".to_owned(),
                Denotation::Qualified(name) => format!("'{name}'"),
            })
            .collect::<Vec<_>>()
            .join(" or ")
    }

    /// Extend every candidate by one attribute access (`q` → `q.attr`).
    fn attribute(&self, attr: &str) -> Self {
        Self::new(
            self.candidates
                .iter()
                .map(|candidate| match candidate {
                    Denotation::Local => Denotation::Local,
                    Denotation::Qualified(name) => Denotation::Qualified(format!("{name}.{attr}")),
                })
                .collect(),
        )
    }
}

/// The scope a definition-time expression is evaluated in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EvalScope {
    /// Module scope, immediately before the module-level statement that
    /// starts at `at` (a class's decorators, bases and keywords).
    Module {
        /// Start offset of the statement being evaluated.
        at: TextSize,
    },
    /// The body of the module-level class starting at `class_start`,
    /// immediately before the class-body statement that starts at `at` (a
    /// method's decorators). Names the class body has not bound fall back to
    /// module scope as of the class statement.
    ClassBody {
        /// Start offset of the enclosing `class` statement.
        class_start: TextSize,
        /// Start offset of the class-body statement being evaluated.
        at: TextSize,
    },
}

/// Bindings that may be in effect at the start of each class statement of a
/// module, and at the start of each method definition in those classes.
///
/// Classes nested in a class body are covered too: their statement is
/// snapshotted in the enclosing class body (where their decorators and bases
/// run), and their own body is walked as one more class scope.
#[derive(Debug, Default)]
pub(crate) struct ModuleBindings {
    /// Module-scope flow at the start of each module-level class statement.
    module: HashMap<TextSize, Flow>,
    /// Class bodies: snapshots and the scope each class statement runs in.
    classes: ClassScopes,
}

/// What the reaching-definitions pass records about every class body of a
/// module, nested ones included.
#[derive(Debug, Default)]
struct ClassScopes {
    /// Class-body flow at the start of each method and nested class
    /// statement, keyed by the start of the `class` statement and then of the
    /// member.
    bodies: HashMap<TextSize, HashMap<TextSize, Flow>>,
    /// For each class, the start of the module-level class statement that
    /// (transitively) contains it, or the class itself at module level.
    /// Python resolves a name a class body has not bound in module scope,
    /// skipping any enclosing class body, and module scope is as of the
    /// statement that started running the outermost class.
    anchors: HashMap<TextSize, TextSize>,
    /// For each nested class, the start of the class whose body contains its
    /// `class` statement.
    parents: HashMap<TextSize, TextSize>,
}

impl ModuleBindings {
    /// Run the reaching-definitions pass over a module body.
    pub(crate) fn collect(body: &[Stmt]) -> Self {
        let mut classes = ClassScopes::default();
        let mut walker = ScopeWalker {
            snapshots: HashMap::new(),
            class: None,
            classes: &mut classes,
        };
        walker.block(body, &mut Flow::default());
        Self {
            module: walker.snapshots,
            classes,
        }
    }

    /// The scope the definition-time expressions (decorators, bases,
    /// keywords) of the class statement starting at `class_start` run in.
    ///
    /// A module-level class statement runs in module scope; a class nested in
    /// a class body runs in that body, right before the nested statement.
    pub(crate) fn class_statement_scope(&self, class_start: TextSize) -> EvalScope {
        match self.classes.parents.get(&class_start) {
            Some(&parent) => EvalScope::ClassBody {
                class_start: parent,
                at: class_start,
            },
            None => EvalScope::Module { at: class_start },
        }
    }

    /// Resolve a `Name` / `Attribute` chain evaluated in `scope`.
    ///
    /// Returns `None` for any other expression shape, and when `scope` does
    /// not name the start of a class statement (or of a method in one) seen by
    /// [`Self::collect`]. Callers strip a call (`@dataclass(frozen=True)`) or a
    /// subscript (`Generic[T]`) themselves, because only they know that the
    /// callee or subscripted value is what they are asking about.
    pub(crate) fn resolve(&self, expr: &Expr, scope: EvalScope) -> Option<Resolution> {
        match expr {
            Expr::Name(name) => self.resolve_name(name.id.as_str(), scope),
            Expr::Attribute(attribute) => Some(
                self.resolve(&attribute.value, scope)?
                    .attribute(attribute.attr.as_str()),
            ),
            _ => None,
        }
    }

    /// Resolve one bare name evaluated in `scope`.
    fn resolve_name(&self, name: &str, scope: EvalScope) -> Option<Resolution> {
        match scope {
            EvalScope::Module { at } => Some(Resolution::new(self.module_candidates(name, at)?)),
            EvalScope::ClassBody { class_start, at } => {
                let flow = self.classes.bodies.get(&class_start)?.get(&at)?;
                let mut candidates = Vec::new();
                let mut falls_back = false;
                for target in flow.lookup(name) {
                    match target.denotation(name) {
                        Some(denotation) => candidates.push(denotation),
                        // A name the class body has not bound (or has deleted)
                        // is looked up in module scope.
                        None => falls_back = true,
                    }
                }
                if falls_back {
                    let anchor = self.classes.anchors.get(&class_start)?;
                    candidates.extend(self.module_candidates(name, *anchor)?);
                }
                Some(Resolution::new(candidates))
            }
        }
    }

    /// Candidates for `name` in module scope at the class statement starting
    /// at `at`, with `builtins.<name>` standing in for "no module binding".
    fn module_candidates(&self, name: &str, at: TextSize) -> Option<Vec<Denotation>> {
        let flow = self.module.get(&at)?;
        Some(
            flow.lookup(name)
                .iter()
                .map(|target| {
                    target
                        .denotation(name)
                        .unwrap_or_else(|| Denotation::Qualified(format!("builtins.{name}")))
                })
                .collect(),
        )
    }
}

/// What a name may be bound to at one point of a scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    /// Bound to this.
    Denotes(Denotation),
    /// Not bound in this scope (never bound, `del NAME`, or the end of
    /// `except ... as NAME`), so lookup continues in the enclosing scope.
    Unbound,
    /// Possibly bound by `from <module> import *`.
    Star(String),
}

impl Target {
    /// What this target denotes for `name`, or `None` if lookup continues in
    /// the enclosing scope.
    fn denotation(&self, name: &str) -> Option<Denotation> {
        match self {
            Self::Denotes(denotation) => Some(denotation.clone()),
            Self::Unbound => None,
            Self::Star(module) => Some(Denotation::Qualified(qualify_member(module, name))),
        }
    }
}

/// The targets each name may have at one point of a scope.
///
/// A name absent from `names` may have any target in `others`: `Unbound` at
/// the start of a scope, plus a `Star` entry per star import seen since.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flow {
    /// Names bound at least once on some path to this point.
    names: HashMap<String, BTreeSet<Target>>,
    /// Targets of every other name.
    others: BTreeSet<Target>,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            names: HashMap::new(),
            others: BTreeSet::from([Target::Unbound]),
        }
    }
}

impl Flow {
    /// The targets `name` may have here.
    fn lookup(&self, name: &str) -> &BTreeSet<Target> {
        self.names.get(name).unwrap_or(&self.others)
    }

    /// Bind `name` on every path through this point.
    fn bind(&mut self, name: &str, target: Target) {
        self.names
            .insert(name.to_owned(), BTreeSet::from([target]));
    }

    /// Record a `from module import *`, which may bind any name.
    fn star_import(&mut self, module: &str) {
        let star = Target::Star(module.to_owned());
        #[expect(
            clippy::iter_over_hash_type,
            reason = "inserts one target into every set"
        )]
        for targets in self.names.values_mut() {
            targets.insert(star.clone());
        }
        self.others.insert(star);
    }

    /// Merge the paths of `other` into this point (control-flow join).
    fn join(&mut self, other: &Self) {
        let joined: HashMap<String, BTreeSet<Target>> = self
            .names
            .keys()
            .chain(other.names.keys())
            .map(|name| {
                let mut targets = self.lookup(name).clone();
                targets.extend(other.lookup(name).iter().cloned());
                (name.clone(), targets)
            })
            .collect();
        self.names = joined;
        self.others.extend(other.others.iter().cloned());
    }
}

/// Join an import source module and a member name (`abc` + `ABC` → `abc.ABC`,
/// `.` + `models` → `.models`).
fn qualify_member(module: &str, member: &str) -> String {
    if module.is_empty() || module.ends_with('.') {
        format!("{module}{member}")
    } else {
        format!("{module}.{member}")
    }
}

/// Sanity bound on loop-body passes, checked in debug builds only.
///
/// It is NOT a cut-off: [`ScopeWalker::loop_body`] always runs to its fixpoint
/// (see there for why that terminates). Real loop bodies settle in two or
/// three passes, so reaching this means a non-monotone change to [`Flow`].
const LOOP_PASS_SANITY_BOUND: usize = 1024;

/// Statement walker for one scope's reaching-definitions pass.
///
/// It stops at nested function and lambda scopes. Class bodies, nested ones
/// included, are walked as their own scope.
struct ScopeWalker<'a> {
    /// Flow at the start of each class statement, and (in a class body) of
    /// each method definition.
    snapshots: HashMap<TextSize, Flow>,
    /// `None` in module scope; in a class body, the start of that class
    /// statement and of the module-level class statement containing it.
    class: Option<(TextSize, TextSize)>,
    /// Shared output for every class body walked.
    classes: &'a mut ClassScopes,
}

impl ScopeWalker<'_> {
    /// Walk a statement sequence, threading `flow` through it.
    fn block(&mut self, body: &[Stmt], flow: &mut Flow) {
        for stmt in body {
            self.stmt(stmt, flow);
        }
    }

    /// Walk `body` from a copy of `flow` and return the resulting flow.
    fn branch(&mut self, body: &[Stmt], flow: &Flow) -> Flow {
        let mut branch = flow.clone();
        self.block(body, &mut branch);
        branch
    }

    /// Walk one statement.
    fn stmt(&mut self, stmt: &Stmt, flow: &mut Flow) {
        match stmt {
            Stmt::FunctionDef(function) => {
                if self.class.is_some() {
                    self.snapshots.insert(function.range.start(), flow.clone());
                }
                // Decorators and defaults run in this scope; the body (and a
                // PEP 695 type-parameter scope) does not.
                let mut exprs = ExprWalker::new(flow);
                for decorator in &function.decorator_list {
                    exprs.visit_decorator(decorator);
                }
                exprs.visit_parameters(&function.parameters);
                flow.bind(function.name.as_str(), Target::Denotes(Denotation::Local));
            }
            Stmt::ClassDef(class) => {
                let start = class.range.start();
                self.snapshots.insert(start, flow.clone());
                let anchor = self.class.map_or(start, |(_, anchor)| anchor);
                if let Some((parent, _)) = self.class {
                    self.classes.parents.insert(start, parent);
                }
                let mut body = ScopeWalker {
                    snapshots: HashMap::new(),
                    class: Some((start, anchor)),
                    classes: &mut *self.classes,
                };
                body.block(&class.body, &mut Flow::default());
                let snapshots = body.snapshots;
                self.classes.bodies.insert(start, snapshots);
                self.classes.anchors.insert(start, anchor);
                let mut exprs = ExprWalker::new(flow);
                for decorator in &class.decorator_list {
                    exprs.visit_decorator(decorator);
                }
                if let Some(arguments) = &class.arguments {
                    exprs.visit_arguments(arguments);
                }
                flow.bind(class.name.as_str(), Target::Denotes(Denotation::Local));
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    let module = alias.name.as_str();
                    // `import a.b as x` binds `x` to the submodule `a.b`, while
                    // `import a.b` binds only the top-level package `a`.
                    let (local, denoted) = alias.asname.as_ref().map_or_else(
                        || {
                            let top = module.split_once('.').map_or(module, |(top, _)| top);
                            (top, top)
                        },
                        |asname| (asname.as_str(), module),
                    );
                    flow.bind(
                        local,
                        Target::Denotes(Denotation::Qualified(denoted.to_owned())),
                    );
                }
            }
            Stmt::ImportFrom(import) => {
                // A relative import keeps its leading dots (`.models.Base`): it
                // denotes a module of the current package, which never equals
                // an absolute standard-library name.
                let level = usize::try_from(import.level).unwrap_or(usize::MAX);
                let mut module = ".".repeat(level);
                if let Some(name) = &import.module {
                    module.push_str(name.as_str());
                }
                for alias in &import.names {
                    let imported = alias.name.as_str();
                    if imported == "*" {
                        flow.star_import(&module);
                        continue;
                    }
                    let local = alias
                        .asname
                        .as_ref()
                        .map_or(imported, |asname| asname.as_str());
                    flow.bind(
                        local,
                        Target::Denotes(Denotation::Qualified(qualify_member(&module, imported))),
                    );
                }
            }
            // An annotation without a value declares a type but binds nothing.
            Stmt::AnnAssign(assign) if assign.value.is_none() => {}
            Stmt::If(stmt_if) => {
                // `cur` is the path on which every test so far was false.
                let mut cur = flow.clone();
                ExprWalker::new(&mut cur).visit_expr(&stmt_if.test);
                let mut out = self.branch(&stmt_if.body, &cur);
                let mut has_else = false;
                for clause in &stmt_if.elif_else_clauses {
                    match &clause.test {
                        Some(test) => ExprWalker::new(&mut cur).visit_expr(test),
                        None => has_else = true,
                    }
                    out.join(&self.branch(&clause.body, &cur));
                }
                if !has_else {
                    out.join(&cur);
                }
                *flow = out;
            }
            Stmt::While(stmt_while) => {
                ExprWalker::new(flow).visit_expr(&stmt_while.test);
                self.loop_body(flow, |walker, iteration| {
                    walker.block(&stmt_while.body, iteration);
                    ExprWalker::new(iteration).visit_expr(&stmt_while.test);
                });
                let orelse = self.branch(&stmt_while.orelse, flow);
                flow.join(&orelse);
            }
            Stmt::For(stmt_for) => {
                ExprWalker::new(flow).visit_expr(&stmt_for.iter);
                self.loop_body(flow, |walker, iteration| {
                    ExprWalker::new(iteration).visit_expr(&stmt_for.target);
                    walker.block(&stmt_for.body, iteration);
                });
                let orelse = self.branch(&stmt_for.orelse, flow);
                flow.join(&orelse);
            }
            Stmt::Try(stmt_try) => self.try_stmt(stmt_try, flow),
            Stmt::Match(stmt_match) => {
                ExprWalker::new(flow).visit_expr(&stmt_match.subject);
                // `cur` is the path on which every case so far failed; a failed
                // pattern may still have bound some of its captures.
                let mut cur = flow.clone();
                let mut matched: Option<Flow> = None;
                for case in &stmt_match.cases {
                    let mut case_flow = cur.clone();
                    let mut exprs = ExprWalker::new(&mut case_flow);
                    exprs.visit_pattern(&case.pattern);
                    if let Some(guard) = &case.guard {
                        exprs.visit_expr(guard);
                    }
                    cur.join(&case_flow);
                    let case_out = self.branch(&case.body, &case_flow);
                    match &mut matched {
                        Some(paths) => paths.join(&case_out),
                        None => matched = Some(case_out),
                    }
                }
                let mut out = matched.unwrap_or_else(|| cur.clone());
                out.join(&cur);
                *flow = out;
            }
            _ => {
                // Simple statements and `with`: every expression (and nested
                // block) runs in order on the current path.
                let mut exprs = ExprWalker::new(flow);
                match stmt {
                    Stmt::With(stmt_with) => {
                        for item in &stmt_with.items {
                            exprs.visit_with_item(item);
                        }
                        self.block(&stmt_with.body, flow);
                    }
                    _ => exprs.visit_stmt(stmt),
                }
            }
        }
    }

    /// Run a loop body zero or more times: join every iteration's result into
    /// `flow` until nothing changes.
    ///
    /// This always reaches the fixpoint: a pass cap could stop it early and leave
    /// the snapshots taken inside the body describing a flow that is not the loop's
    /// final one. It terminates because `flow` only grows -- `join` is a union --
    /// and every target it can hold is drawn from finitely many sources (this
    /// scope's binding statements, `Unbound`, and its star imports), so each
    /// non-final pass adds at least one of finitely many (name, target) pairs.
    fn loop_body(&mut self, flow: &mut Flow, mut iterate: impl FnMut(&mut Self, &mut Flow)) {
        for pass in 1.. {
            let mut iteration = flow.clone();
            iterate(self, &mut iteration);
            let before = flow.clone();
            flow.join(&iteration);
            if *flow == before {
                return;
            }
            debug_assert!(
                pass < LOOP_PASS_SANITY_BOUND,
                "name-resolution loop flow did not converge in {LOOP_PASS_SANITY_BOUND} passes"
            );
        }
    }

    /// Walk a `try` statement.
    ///
    /// Without a handler, an exception leaves the scope altogether, so later
    /// code runs only after the whole body ran. With handlers, a handler may
    /// start from any point of the body; it binds its `as` name on entry and
    /// deletes it on exit. `else` runs after a body that raised nothing, and
    /// `finally` after whichever path ran.
    fn try_stmt(&mut self, stmt_try: &ruff_python_ast::StmtTry, flow: &mut Flow) {
        if stmt_try.handlers.is_empty() {
            self.block(&stmt_try.body, flow);
            self.block(&stmt_try.orelse, flow);
            self.block(&stmt_try.finalbody, flow);
            return;
        }
        let mut raised = flow.clone();
        for stmt in &stmt_try.body {
            self.stmt(stmt, flow);
            raised.join(flow);
        }
        self.block(&stmt_try.orelse, flow);
        for ExceptHandler::ExceptHandler(clause) in &stmt_try.handlers {
            let mut caught = raised.clone();
            if let Some(type_) = &clause.type_ {
                ExprWalker::new(&mut caught).visit_expr(type_);
            }
            if let Some(name) = &clause.name {
                caught.bind(name.as_str(), Target::Denotes(Denotation::Local));
            }
            self.block(&clause.body, &mut caught);
            if let Some(name) = &clause.name {
                caught.bind(name.as_str(), Target::Unbound);
            }
            flow.join(&caught);
        }
        self.block(&stmt_try.finalbody, flow);
    }
}

/// Expression visitor that applies the bindings an expression makes to a
/// [`Flow`]: assignment and loop targets, `del` targets, `with ... as`
/// targets, pattern captures and assignment expressions.
///
/// It stops at lambda bodies. Comprehension targets are skipped because they
/// bind in the comprehension's own scope, while an assignment expression
/// inside a comprehension binds in the enclosing scope (PEP 572). Operands
/// that may not run (later short-circuit operands, conditional-expression
/// arms, comprehension bodies) are walked on a copy that is joined back.
struct ExprWalker<'flow> {
    /// The path being updated.
    flow: &'flow mut Flow,
    /// Whether the walk is inside a comprehension.
    in_comprehension: bool,
}

impl<'flow> ExprWalker<'flow> {
    /// Start a walk that updates `flow`, outside any comprehension.
    fn new(flow: &'flow mut Flow) -> Self {
        Self {
            flow,
            in_comprehension: false,
        }
    }

    /// A walk over a sub-expression that updates `flow` (usually a copy of
    /// this walk's path) in the same context as this walk.
    fn child<'sub>(&self, flow: &'sub mut Flow) -> ExprWalker<'sub> {
        ExprWalker {
            flow,
            in_comprehension: self.in_comprehension,
        }
    }

    /// A walk over a comprehension's clauses, which bind their targets in the
    /// comprehension's own scope.
    const fn comprehension(flow: &'flow mut Flow) -> Self {
        Self {
            flow,
            in_comprehension: true,
        }
    }
}

impl<'ast> Visitor<'ast> for ExprWalker<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match stmt {
            // Nested scopes; `ScopeWalker` handles these itself.
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            _ => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match expr {
            Expr::Name(name) => match name.ctx {
                ExprContext::Store if !self.in_comprehension => {
                    self.flow
                        .bind(name.id.as_str(), Target::Denotes(Denotation::Local));
                }
                ExprContext::Del => self.flow.bind(name.id.as_str(), Target::Unbound),
                ExprContext::Store | ExprContext::Load | ExprContext::Invalid => {}
            },
            Expr::Named(named) => {
                self.visit_expr(&named.value);
                if let Expr::Name(target) = named.target.as_ref() {
                    self.flow
                        .bind(target.id.as_str(), Target::Denotes(Denotation::Local));
                }
            }
            Expr::Lambda(lambda) => {
                // Defaults run here; the body is the lambda's own scope.
                if let Some(parameters) = &lambda.parameters {
                    self.visit_parameters(parameters);
                }
            }
            Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::Generator(_) => {
                let mut branch = self.flow.clone();
                visitor::walk_expr(&mut ExprWalker::comprehension(&mut branch), expr);
                self.flow.join(&branch);
            }
            Expr::BoolOp(bool_op) => {
                let mut values = bool_op.values.iter();
                if let Some(first) = values.next() {
                    self.visit_expr(first);
                }
                // Each later operand runs only if the earlier ones did not
                // decide the result.
                let mut reached = self.flow.clone();
                for value in values {
                    self.child(&mut reached).visit_expr(value);
                    self.flow.join(&reached);
                }
            }
            Expr::If(ternary) => {
                self.visit_expr(&ternary.test);
                let mut body = self.flow.clone();
                self.child(&mut body).visit_expr(&ternary.body);
                self.visit_expr(&ternary.orelse);
                self.flow.join(&body);
            }
            _ => visitor::walk_expr(self, expr),
        }
    }

    fn visit_pattern(&mut self, pattern: &'ast Pattern) {
        let captured = match pattern {
            Pattern::MatchAs(capture) => capture.name.as_ref(),
            Pattern::MatchStar(star) => star.name.as_ref(),
            Pattern::MatchMapping(mapping) => mapping.rest.as_ref(),
            _ => None,
        };
        if let Some(name) = captured {
            self.flow
                .bind(name.as_str(), Target::Denotes(Denotation::Local));
        }
        visitor::walk_pattern(self, pattern);
    }
}
