//! Module global slots: top-level bindings that are mutated, or read from a
//! separately lowered item, are lifted to module slots with a lazy initializer.

use super::super::{ConstLiteral, ModuleBuilder};
use crate::SmeltError;
use crate::lowering::js_text::js_str_literal;
use oxc::ast::ast::{BindingPattern, Declaration, Expression, Program, Statement};
use smelt_hir::{
    Body, Expr, ExprKind, Function, FunctionOwner, Item, Literal, Module, Type, Visibility,
};
use std::collections::HashSet;

impl ModuleBuilder<'_> {
    /// Classify module-level `let`/`var` bindings that are mutated anywhere in
    /// the module and lift each to a [`Item::MutableGlobal`].
    ///
    /// A binding is lifted only when it is reassigned or updated somewhere (the
    /// [`MutatedNameCollector`] scan). Lifted bindings register a HIR item so
    /// reads lower to `GlobalGet` and writes to `GlobalSet`; the item is added
    /// to the module's item list so exported globals are visible cross-module.
    /// The current rules are enforced here with named blocker errors: a declaration
    /// with no initializer is a blocker; a non-literal initializer is recorded as
    /// `Pending` when the binding carries a type annotation and is a blocker
    /// without one; a nested write (`x[a][b] = v`) is a blocker only for a
    /// non-`Copy` global; and writes through a binding lift it.
    pub(in crate::lowering) fn collect_mutable_globals(
        &mut self,
        program: &Program<'_>,
        item_reads: &HashSet<String>,
        module: &mut Module,
        errors: &mut Vec<SmeltError>,
    ) {
        let (reassigned, mutated_through, mut mutated_through_nested) =
            Self::collect_mutated_names(program);
        // A binding is module state if it is mutated AT ALL, and a write
        // *through* it counts: `let cache: Record<string, number> = {}` that is
        // only ever `cache[k] = v` is still state that every function shares,
        // and requiring a whole-binding reassignment to lift it would silently
        // leave the write on a module-local copy.
        let mut mutated: HashSet<String> = reassigned
            .into_iter()
            .chain(mutated_through.iter().cloned())
            .chain(mutated_through_nested.iter().cloned())
            .collect();
        // A write in the MODULE BODY is module state too once any item body
        // reads the binding: the reader has no module-body local to see the
        // write through, so a folded initializer (or a fabricated default)
        // would answer the value from before the write. `const zs: number[] =
        // []; zs.push(1); const cnt = () => zs.length` printed 0. A binding
        // written and read only by module-body statements keeps its ordinary
        // local, byte-identical to before.
        let (body_reassigned, body_through, body_nested) = Self::collect_all_mutated_names(program);
        let body_mutated: HashSet<String> = body_reassigned
            .into_iter()
            .chain(body_through)
            .chain(body_nested.iter().cloned())
            .collect();
        //
        // Only a binding WITH an initializer: `let cached: string[];` starts
        // as `undefined`, which the declared type cannot hold, so a slot of
        // that type would have no honest initial value, and lifting it would
        // turn a lowering that works today into the no-initializer blocker.
        // Such a binding keeps its existing path.
        let initialized = Self::initialized_module_binding_names(program);
        let lifted_by_body_write = body_mutated
            .intersection(item_reads)
            .filter(|name| initialized.contains(*name))
            .cloned()
            .collect::<HashSet<_>>();
        mutated_through_nested.extend(body_nested.intersection(&lifted_by_body_write).cloned());
        mutated.extend(lifted_by_body_write);
        self.fold_unmutated_module_let_literals(program, &body_mutated, item_reads);
        if mutated.is_empty() {
            return;
        }
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => {
                    self.register_mutable_global_decl(
                        variable,
                        &mutated,
                        &mutated_through_nested,
                        Visibility::Private,
                        module,
                        errors,
                    );
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::VariableDeclaration(variable) = &export.declaration {
                        self.register_mutable_global_decl(
                            variable,
                            &mutated,
                            &mutated_through_nested,
                            Visibility::Public,
                            module,
                            errors,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Lift each module-level binding READ FROM AN ITEM CONTEXT that has no
    /// provably identical folded value to the same module-global slot the
    /// mutable-global family uses.
    ///
    /// The rule is stated over where the binding is read, not over any type
    /// family, class list or library spelling: a binding in `read_in_items`
    /// (see [`Self::collect_item_context_reads`]) that is not already a slot,
    /// an item, or a folded literal/object/collection/regexp is lifted when its
    /// initializer is a real expression. Class instances (every modeled host
    /// class — `Headers`, `URLSearchParams`, `Request`, … — and every user
    /// class), containers, strings built by calls and erased values all take
    /// this one path; the alternative was `module_global_expression`
    /// fabricating the declared type's default.
    ///
    /// # Why a slot, and not the const-inlining path
    ///
    /// A `const` binding normally reaches its use sites by having its
    /// initializer expression CLONED into each of them
    /// (`ModuleBuilder::const_item_expression`), which is what lets a const of
    /// any expression shape cross a module boundary. For a class instance that
    /// is not merely a cost — it is a WRONG VALUE. Every non-primitive in the
    /// generated runtime carries a JavaScript reference identity, so
    ///
    /// ```ts
    /// const headers = new Headers({ "content-type": "text/plain" });
    /// function add() { headers.set("x-extra", "1"); }
    /// function read() { return headers.get("x-extra"); }
    /// ```
    ///
    /// has ONE header list that both functions see. Re-running the initializer
    /// per use site gives each function its own, so the write is invisible to
    /// the read. The const's STORAGE, not its expression, is what has to be
    /// shared — which is exactly what a module-global slot is. A `GlobalGet`
    /// hands back `.borrow().clone()`, and a runtime clone PRESERVES reference
    /// identity (it shares the `Rc` interior; `fresh_copy()` is the separate
    /// seam that mints a new one), so the slot gives the source's single object
    /// with no new mechanism.
    ///
    /// # What it replaces
    ///
    /// Before this pass such a binding was registered only in
    /// `module_globals`, and `ModuleBuilder::module_global_expression`
    /// fabricated the declared type's DEFAULT for it: an empty `DictLit`
    /// wrapped in an `UnknownCast`. That was wrong twice over — the
    /// initializer's value was silently gone, AND the cast was emitted at the
    /// record type rather than at `SmeltUnknown`, so the generated crate did
    /// not even compile (`standards-module-const-host-value.md`, in git history).
    ///
    /// A `let`/`var` that IS mutated has already been lifted by
    /// [`Self::collect_mutable_globals`]; one that is not is a `const` in all
    /// but spelling and lifts here, so the rule does not depend on which
    /// keyword the source used.
    pub(super) fn collect_class_value_globals(
        &mut self,
        program: &Program<'_>,
        read_in_items: &HashSet<String>,
        module: &mut Module,
    ) {
        if read_in_items.is_empty() {
            return;
        }
        let called = Self::collect_called_identifiers(program);
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => {
                    self.register_class_value_global_decl(
                        variable,
                        read_in_items,
                        &called,
                        Visibility::Private,
                        module,
                    );
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::VariableDeclaration(variable) = &export.declaration {
                        self.register_class_value_global_decl(
                            variable,
                            read_in_items,
                            &called,
                            Visibility::Public,
                            module,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Lift each item-read, unfolded binding in one declaration to a module global.
    ///
    /// Registers the item with a [`smelt_hir::MutableGlobalInit::Pending`]
    /// initializer; the expression is lowered into its own nullary function
    /// item when the module body reaches this same declarator, by the shared
    /// [`Self::lower_pending_mutable_global_init`].
    fn register_class_value_global_decl(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        read_in_items: &HashSet<String>,
        called: &HashSet<String>,
        visibility: Visibility,
        module: &mut Module,
    ) {
        // An ambient declaration never creates a binding, and its name resolves
        // through the host rather than through a slot of ours.
        if decl.declare {
            return;
        }
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let name = binding.name.as_str();
            // The condition the whole pass turns on: the binding is READ from a
            // hoisted item body, where there is no module-body local to read
            // through. A binding used only in module-body statements keeps its
            // ordinary local and lowers byte-identically to before, which is
            // what confines this change to the shape that was broken.
            if !read_in_items.contains(name) {
                continue;
            }
            // Already a mutable global: that pass owns the slot.
            if self.mutable_global_items.contains_key(name) {
                continue;
            }
            // A binding whose name already resolves to some other module item
            // (a function, a class declaration, a const item) is not ours.
            if self.items.contains_key(name) {
                continue;
            }
            let Some(init) = &declarator.init else {
                continue;
            };
            // A class-typed binding whose initializer is a FUNCTION is a
            // callable, not an instance, and reaches its uses through the
            // function-value path.
            if matches!(
                init,
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
            ) {
                continue;
            }
            // A binding some OTHER const path already lowers is not ours. The
            // regexp path is the one that matters: `const P = /x/g` read from
            // two functions deliberately rebuilds its `SmeltRegExp` wrapper at
            // each use site, because `lastIndex` is observable per object and a
            // shared wrapper would fuse two source objects' scan positions —
            // see `module-const-construction-cost.md` (in git history), which
            // measured that decision. A slot would be the more faithful
            // reading of `const P = /x/g` (JavaScript really does have ONE
            // object there), but changing it is that note's decision to revisit,
            // not this pass's to override silently.
            if self.consts.regexp(name).is_some()
                || self.consts.literal(name).is_some()
                || self.consts.object(name).is_some()
                || self.consts.collection(name).is_some()
            {
                continue;
            }
            // The type `collect_module_globals` already inferred for this
            // binding is the one the slot gets, so the slot and the old
            // fabricated default agree on the type and only the VALUE changes.
            let Some(ty) = self.module_globals.get(name).copied() else {
                continue;
            };
            // Every binding that reaches here is read from an item context and
            // has neither a folded value nor a slot yet, so the read would have
            // FABRICATED the declared type's default: `{}` for a record whose
            // literal initializer the folder could not fold (`{ k: [zero(),
            // 'a'] }`), `""` for a string built by a call, an empty erased
            // record for a class instance. JavaScript evaluates the initializer
            // ONCE and every reader sees that value, so the rule is stated over
            // nothing but that: the binding becomes a module slot holding its
            // one evaluated value, whatever its type. That subsumes the earlier
            // per-shape lifts (modeled host classes, non-literal containers,
            // erased values, constructed user instances), which were each this
            // rule for one type family.
            //
            // The slot takes the type `collect_module_globals` recorded; an erased
            // (`any`/union) binding's type is the dynamic boundary the source
            // declared, and the slot adds no erasure of its own.
            //
            // Two shapes keep their existing paths. A `class` expression names a
            // class, not an instance. And an ERASED binding the program CALLS
            // (`const validate = wrap(fn)`, then `validate(x)`) keeps the erased
            // call path, which a module slot does not serve yet.
            let is_erased = matches!(
                self.ctx.krate.types.get(ty),
                Some(Type::Unknown | Type::Union(_))
            );
            if matches!(init.without_parentheses(), Expression::ClassExpression(_))
                || (is_erased && called.contains(name))
            {
                continue;
            }
            // An EXPORTED object/array literal never reaches the fabricating
            // read: the export pass (`const_item_declarations`) lowers it to an
            // item of its own — a function-table namespace whose members are
            // callable items, or a const item — and every read resolves to that
            // item. A written one was already lifted by
            // `register_mutable_global_decl`.
            if matches!(visibility, Visibility::Public)
                && matches!(
                    init.without_parentheses(),
                    Expression::ObjectExpression(_) | Expression::ArrayExpression(_)
                )
            {
                continue;
            }
            let span = self.span(binding.span.start, binding.span.end);
            let symbol = self.intern_source_name(name);
            let item =
                self.ctx
                    .krate
                    .push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                        name: symbol,
                        ty,
                        init: smelt_hir::MutableGlobalInit::Pending,
                        visibility,
                        span,
                    }));
            module.items.push(item);
            self.items.insert(name.to_owned(), item);
            self.mutable_global_items.insert(name.to_owned(), item);
        }
    }

    /// Lift each mutated identifier binding in one declaration to a global.
    fn register_mutable_global_decl(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        mutated: &HashSet<String>,
        mutated_through_nested: &HashSet<String>,
        visibility: Visibility,
        module: &mut Module,
        errors: &mut Vec<SmeltError>,
    ) {
        // An ambient declaration never creates a binding, so there is nothing to
        // lift to a mutable global (see `collect_module_global_decl`).
        if decl.declare {
            return;
        }
        // `var` is treated like `let`. A `const` binding can never be
        // REASSIGNED, but the value it names can still be written THROUGH
        // (`const cache: Record<string, P> = {}; cache[key] = p`, Hono's
        // `patternCache`): JavaScript `const` freezes the binding, not the
        // object. Such a const is module state exactly like a mutated `let`,
        // and the const-item path would have cloned its `{}` initializer into
        // every use site — each write landed on a fresh empty record and every
        // read missed. It is lifted here too; a const that is only read keeps
        // the inline path.
        let is_const = matches!(decl.kind, oxc::ast::ast::VariableDeclarationKind::Const);
        if !matches!(
            decl.kind,
            oxc::ast::ast::VariableDeclarationKind::Let
                | oxc::ast::ast::VariableDeclarationKind::Var
                | oxc::ast::ast::VariableDeclarationKind::Const
        ) {
            return;
        }
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let name = binding.name.as_str();
            if !mutated.contains(name) {
                continue;
            }
            // Only a const holding a CONTAINER literal is lifted: a const whose
            // initializer is a call or class construction keeps its existing
            // path (`collect_class_value_globals` owns class instances).
            if is_const
                && !declarator.init.as_ref().is_some_and(|init| {
                    matches!(
                        init.without_parentheses(),
                        Expression::ObjectExpression(_) | Expression::ArrayExpression(_)
                    )
                })
            {
                continue;
            }
            let span = self.span(binding.span.start, binding.span.end);
            let Some(init) = &declarator.init else {
                // `let x;` with no initializer is `undefined`, which has no
                // type to give the cell. Left as a blocker rather than guessed.
                errors.push(SmeltError::unsupported(
                    span,
                    "module-level mutable binding initializer must be a literal for now",
                ));
                continue;
            };
            // A literal initializer is stored inline. Anything else is an
            // EXPRESSION that has to run, and it cannot be lowered here: this
            // pass runs before imports and function items are resolvable (see
            // `Self::program`), so `let cache: Record<string, RegExp> =
            // createNullObject()` would resolve its callee to an erased import
            // placeholder. The item is registered `Pending` — reads and writes
            // of the binding need it now — and the initializer is lowered when
            // the module body reaches this same declaration.
            let (init_kind, literal_ty) = match self.mutable_global_literal_init(init) {
                Some(literal) => (
                    smelt_hir::MutableGlobalInit::Literal(literal.literal.clone()),
                    Some(literal.ty),
                ),
                None => (smelt_hir::MutableGlobalInit::Pending, None),
            };
            let ty = match (&declarator.type_annotation, literal_ty) {
                (Some(annotation), fallback) => self
                    .ts_type_to_hir(&annotation.type_annotation)
                    .ok()
                    .or(fallback),
                (None, fallback) => fallback,
            };
            let Some(ty) = ty else {
                // No annotation and no literal to infer from: the cell's type
                // is genuinely unknown at this point in the pass order. An
                // honest blocker rather than an erased cell.
                errors.push(SmeltError::unsupported(
                    span,
                    "module-level mutable binding with a non-literal initializer needs an \
                     explicit type annotation",
                ));
                continue;
            };
            // A ONE-DEEP write through the binding (`cache[key] = value`,
            // `cache.field = value`) is lowered: `Place::Global` names the
            // cell as the assignment root, so the mutation happens inside the
            // cell and no copy is made. See
            // `hono-h6-place-global.md` (in git history).
            //
            // A NESTED write (`cache[a][b] = value`) still blocks. Its inner
            // projection has to produce a value, and whether that value shares
            // storage with the cell is the handle-versus-value question
            // `Place::Global` exists to avoid asking — a `SmeltRecord` clone
            // shares the store while a `HashMap` clone deep-copies, so
            // guessing loses the write for one of them with no diagnostic.
            // A `Copy` primitive can be written through at neither depth, so
            // the restriction is exactly: a non-`Copy` global written through a
            // nested projection.
            let is_copy_primitive = matches!(
                self.ctx.krate.types.get(ty),
                Some(Type::Float | Type::Int | Type::Bool)
            );
            if !is_copy_primitive && mutated_through_nested.contains(name) {
                errors.push(SmeltError::unsupported(
                    span,
                    format!(
                        "module-level mutable binding `{name}` is written through a nested \
                         projection (`{name}[a][b] = …` or `{name}.a.b = …`); only a \
                         one-deep write through a non-primitive mutable global is lowered"
                    ),
                ));
                continue;
            }
            let symbol = self.intern_source_name(name);
            let item =
                self.ctx
                    .krate
                    .push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                        name: symbol,
                        ty,
                        init: init_kind,
                        visibility,
                        span,
                    }));
            module.items.push(item);
            self.items.insert(name.to_owned(), item);
            self.mutable_global_items.insert(name.to_owned(), item);
        }
    }

    /// Lower a mutable global's non-literal initializer, once the module body
    /// reaches the binding's own declaration.
    ///
    /// The initializer becomes a synthesized nullary function item returning
    /// the expression, and the global's `init` moves from
    /// [`smelt_hir::MutableGlobalInit::Pending`] to `Initializer(item)`. Going
    /// through a real function item means the expression reaches codegen by the
    /// ordinary function path — MIR assigns it a `FuncId`, the emitter emits its
    /// body — and the cell's lazy initializer only has to call it.
    ///
    /// Called from the declarator-skip site (see
    /// [`Self::is_lifted_global_declarator`]), which is where the binding's own
    /// declaration is recognized and otherwise dropped. Returns `Ok(())` for a
    /// global whose initializer is already a literal, so the caller does not
    /// have to know which kind it is.
    pub(in crate::lowering) fn lower_pending_mutable_global_init(
        &mut self,
        name: &str,
        init: &Expression<'_>,
    ) -> Result<(), SmeltError> {
        let Some(item) = self.mutable_global_items.get(name).copied() else {
            return Ok(());
        };
        let (global_ty, span) = match self.item_ref(item) {
            Item::MutableGlobal(global)
                if matches!(global.init, smelt_hir::MutableGlobalInit::Pending) =>
            {
                (global.ty, global.span)
            }
            _ => return Ok(()),
        };
        let mut init_body = Body::new(None, span);
        // The initializer becomes a SEPARATE function item, so the module
        // body's locals are not in scope inside it — and its `LocalId`s do not
        // even exist in this fresh body, so leaving them registered would make
        // name resolution hand back an id that indexes nothing. An initializer
        // that references a module-body local therefore reports an unresolved
        // name instead of mis-resolving one.
        let value = self.with_empty_lexical_scope(|this| {
            this.expression_with_hint(init, &mut init_body, Some(global_ty))
        })?;
        init_body.push_stmt(smelt_hir::Stmt::Return(Some(value)));
        self.install_global_initializer(item, name, init_body, global_ty, span);
        Ok(())
    }

    /// Make `init_body` (a body returning the global's value) mutable global
    /// `item`'s lazy initializer.
    ///
    /// The body becomes a synthesized nullary function item and the global's
    /// `init` becomes `Initializer(item)`. Going through a real function item
    /// means the expression reaches codegen by the ordinary function path —
    /// MIR assigns it a `FuncId`, the emitter emits its body — and the cell's
    /// lazy initializer only has to call it. Shared by module bindings
    /// ([`Self::lower_pending_mutable_global_init`]) and static class fields
    /// (`ModuleBuilder::lower_static_field_initializer`).
    pub(in crate::lowering) fn install_global_initializer(
        &mut self,
        item: smelt_hir::ItemId,
        name: &str,
        init_body: Body,
        global_ty: smelt_hir::TypeId,
        span: smelt_hir::Span,
    ) {
        let body_id = self.ctx.krate.push_body(init_body);
        // The synthesized name is derived from the binding and the global's own
        // HIR item index, so two modules' same-named globals get distinct
        // initializers, and is interned exactly so no later name-keyed lookup
        // can collide with a source function.
        //
        // The index, not the module PATH: the path is absolute, so putting it in
        // a symbol both leaked a build-machine filesystem path into every
        // generated crate and made any golden containing the symbol
        // unreproducible outside the directory it was generated in. The item
        // index is unique crate-wide and deterministic for a given program,
        // which is all the disambiguation this needs.
        let init_name =
            self.intern_synthesized_name(&format!("smelt_global_init__{name}__{}", item.0));
        let init_item = self.ctx.krate.push_item(Item::Function(Function {
            name: init_name,
            span,
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            return_ty: global_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        if let Some(Item::MutableGlobal(global)) = self
            .ctx
            .krate
            .items
            .get_mut(usize::try_from(item.0).unwrap_or(usize::MAX))
        {
            global.init = smelt_hir::MutableGlobalInit::Initializer(init_item);
        }
    }

    /// Evaluate a module slot's expression initializer at its own declaration.
    ///
    /// A slot's cell initializes lazily, on first access. That is the module's
    /// one evaluation, but not necessarily at the source position: a first read
    /// from a function called later would run the initializer then, observing
    /// module state written in between (`let n = 1; const t = { v: f(n) }; n =
    /// 2;` would see `n = 2`) and moving any side effect it has. JavaScript
    /// evaluates the initializer exactly when the declaration executes, and a
    /// correct program cannot read the binding earlier (a `const`/`let` read
    /// before its declaration throws), so touching the slot here makes the lazy
    /// cell's single evaluation happen in source order. The read's value is
    /// discarded.
    ///
    /// A literal-initialized slot has nothing to evaluate and gets no statement.
    /// Only the module body's own declaration reaches here (the declarator is
    /// recognized by its binding span).
    pub(in crate::lowering) fn force_module_slot_init(
        &mut self,
        name: &str,
        binding_span: oxc::span::Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) {
        if !self.is_lifted_global_declarator(name, binding_span) {
            return;
        }
        let Some(item) = self.mutable_global_items.get(name).copied() else {
            return;
        };
        let Item::MutableGlobal(global) = self.item_ref(item) else {
            return;
        };
        if !matches!(global.init, smelt_hir::MutableGlobalInit::Initializer(_)) {
            return;
        }
        // The read is typed UNIT: nothing consumes it, and a unit-typed
        // `GlobalGet` is how Rust codegen knows to emit a bare touch of the
        // lazy cell (`NAME.with(|_| ())`) instead of cloning the slot's value
        // into a typed temporary.
        let span = global.span;
        let ty = self.ctx.krate.types.intern(Type::None);
        let read = body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item },
            ty,
            span,
        });
        body.push_stmt_to_block(block, smelt_hir::Stmt::Expr(read));
    }

    /// Accept only a direct number/string/bool literal initializer (through
    /// transparent parenthesis/cast wrappers) for a mutable global.
    fn mutable_global_literal_init(&mut self, expression: &Expression<'_>) -> Option<ConstLiteral> {
        match expression {
            Expression::NumericLiteral(literal) => Some(ConstLiteral {
                literal: Literal::Float(literal.value),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            Expression::StringLiteral(literal) => Some(ConstLiteral {
                literal: js_str_literal(literal.value),
                ty: self.ctx.krate.types.intern(Type::String),
            }),
            Expression::BooleanLiteral(literal) => Some(ConstLiteral {
                literal: Literal::Bool(literal.value),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            Expression::ParenthesizedExpression(inner) => {
                self.mutable_global_literal_init(&inner.expression)
            }
            Expression::TSAsExpression(inner) => {
                self.mutable_global_literal_init(&inner.expression)
            }
            Expression::TSSatisfiesExpression(inner) => {
                self.mutable_global_literal_init(&inner.expression)
            }
            Expression::TSNonNullExpression(inner) => {
                self.mutable_global_literal_init(&inner.expression)
            }
            _ => None,
        }
    }

    /// Return the mutable-global HIR item id for a name, if it was lifted.
    pub(in crate::lowering) fn mutable_global_item(&self, name: &str) -> Option<smelt_hir::ItemId> {
        let item = self.items.get(name).copied()?;
        matches!(self.item_ref(item), Item::MutableGlobal(_)).then_some(item)
    }

    /// Return whether a binding declarator IS the lifted module-level
    /// declaration of a mutable global (same name and binding span).
    pub(in crate::lowering) fn is_lifted_global_declarator(
        &self,
        name: &str,
        binding_span: oxc::span::Span,
    ) -> bool {
        let Some(item) = self.mutable_global_items.get(name).copied() else {
            return false;
        };
        let Item::MutableGlobal(global_item) = self.item_ref(item) else {
            return false;
        };
        global_item.span.start == binding_span.start && global_item.span.end == binding_span.end
    }
}
