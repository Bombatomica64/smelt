//! Module lowering driver: `ModuleBuilder::program` runs the prepass phases
//! (`phases`) and then lowers the top-level statements into the module body.
//!
//! Each prepass lives in the child module for its concern: imports, exports and
//! re-exports (cross-module name resolution), declaration names and
//! predeclared signatures, module globals and slots, const items, forward
//! arrows and object consts. This file keeps the statement loop and the
//! module-exit async drain.

mod phases;
mod construction;
pub(in crate::lowering) mod test_tier;
mod imports;
mod module_exports;
mod reexports;
mod type_declarations;
mod predeclare_members;
mod predeclare_functions;
mod globals;
mod mutable_globals;
mod binding_usage;
mod const_collections;
mod const_items;
mod forward_arrows;
mod object_consts;

use crate::SmeltError;
use oxc::ast::ast::{Declaration, Program, Statement};
use oxc::span::GetSpan;
use forward_arrows::ForwardArrowQueue;
use phases::ModuleFacts;
use smelt_hir::{Body, Expr, ExprKind, Item, Language, Module, ModuleId, SourceFile, Span, Type};
use super::ModuleBuilder;
use super::state::fn_ctx::FnCtx;
use super::support::is_implemented_overload_signature;

impl ModuleBuilder<'_> {
    /// Lower a TypeScript program to HIR module.
    pub(super) fn program(&mut self, program: &Program<'_>) -> Result<ModuleId, Vec<SmeltError>> {
        let span = self.span(program.span.start, program.span.end);
        // Bodies this module lowers, for the int32 store proofs that run once
        // every signature they consult exists (see `finish_int32_bodies`).
        let first_body = self.ctx.krate.bodies.len();
        let mut body = Body::new(None, span);
        let mut errors = Vec::new();
        self.presence_widened_bindings =
            super::presence_tested_locals::presence_tested_keyed_bindings(program);
        // Top-level code is the program's ENTRY POINT, so it may await: the
        // module body becomes the emitted `main`, which is a
        // `#[tokio::main] async fn` whenever it needs to be. Awaiting here was
        // rejected outright ("await expressions are only lowered inside async
        // functions"), which is not a rule any runtime has -- ES modules have
        // had top-level await since ES2022 and Node runs it.
        //
        // Restored after the body so a nested non-async function still rejects
        // `await` in its own body.
        let outer_fn_ctx = std::mem::replace(&mut self.fn_ctx, FnCtx::module());

        let mut module = Module::new(
            "main",
            SourceFile {
                path: self.path.clone(),
                language: Language::TypeScript,
                file: self.file_id,
            },
        );

        let mut before_each = Vec::new();
        let mut after_each = Vec::new();
        let mut top_level_test_setup = Vec::new();
        let imports = self.read_imports(program, &mut module);
        let types = self.declare_module_types(program, imports);
        let bindings = self.collect_module_bindings(program, &mut module, &mut errors, types);
        let class_scope = self.declare_class_scope(program, bindings);
        let (signatures, facts) = self.declare_signatures(program, &mut errors, class_scope);
        let ModuleFacts {
            test_module,
            implemented_functions,
        } = facts;
        let mut forward_arrow_consts = self.forward_arrow_const_names(program);
        forward_arrow_consts.extend(Self::object_namespace_arrow_const_names(program));
        // An arrow that reads a module binding with REFERENCE IDENTITY cannot be
        // lifted: see `identity_bearing_module_binding_names`.
        self.retain_capturable_arrow_consts(program, &mut forward_arrow_consts);
        // Forward-referenced arrow consts are lowered BEFORE any body that may
        // call them, in dependency order. An EXPORTED arrow const is the same
        // lexical binding as a private one (`export` changes visibility, not
        // evaluation), so it joins the same queue: lowered in source order
        // instead, a function body calling an exported arrow declared further
        // down resolved the name before its item existed and bound a
        // default-returning placeholder (radash `retry` -> `tryit`).
        let pending_arrows = program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::VariableDeclaration(variable) => Some((variable.as_ref(), false)),
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(variable)
                        if Self::is_liftable_exported_arrow_const(
                            variable,
                            &forward_arrow_consts,
                        ) =>
                    {
                        Some((variable.as_ref(), true))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        // A lifted arrow body also reads the MEMBERS of this module's classes
        // (`req.header()`), and a class's field and method signatures exist only
        // once its declaration is lowered. So an arrow naming a module class
        // waits for that class instead of being hoisted above it: the queue is
        // drained once here and again after each class declaration below. See
        // `ForwardArrowQueue`.
        let mut arrow_queue = ForwardArrowQueue::new(
            pending_arrows,
            Self::module_class_spans(program),
            forward_arrow_consts,
            &signatures,
        );
        self.drain_forward_arrow_queue(
            &mut arrow_queue,
            &mut module,
            &mut errors,
        );
        // The previous top-level statement, examined once it is lowered
        // (`note_lowered_function`): the loop body leaves through `continue`s.
        let mut previous: Option<&Statement<'_>> = None;
        for statement in &program.body {
            if let Some(lowered) = previous.take() {
                self.note_lowered_function(lowered, &module.items);
            }
            if let Statement::ImportDeclaration(_) = statement {
                continue;
            }
            previous = Some(statement);
            self.anchor_top_level_statement(self.span(statement.span().start, statement.span().end));
            // A queued arrow still waiting on a class is lowered no later than
            // its own source position, where every class above it is lowered.
            if let Some(start) = Self::variable_statement_start(statement) {
                self.lower_queued_forward_arrow_at(
                    &mut arrow_queue,
                    start,
                    &mut module,
                    &mut errors,
                );
            }
            if let Statement::VariableDeclaration(_) = statement {
                if !self.is_predeclared_arrow_const_statement(statement) {
                    top_level_test_setup.push(statement);
                }
                continue;
            }
            if let Statement::FunctionDeclaration(function) = statement {
                if function.declare {
                    continue;
                }
                if is_implemented_overload_signature(function, &implemented_functions) {
                    continue;
                }
                if self.is_module_constructor_function(function) {
                    if let Err(error) =
                        self.synthesize_constructor_function_class(function, &program.body)
                    {
                        errors.push(error);
                    }
                    continue;
                }
                match self.function_declaration(function) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ClassDeclaration(class) = statement {
                match self.class_declaration(class) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                if let Some(id) = &class.id {
                    arrow_queue.mark_class_lowered(id.name.as_str());
                }
                self.drain_forward_arrow_queue(
                    &mut arrow_queue,
                    &mut module,
                    &mut errors,
                );
                continue;
            }
            if let Statement::TSInterfaceDeclaration(interface) = statement {
                match self.interface_declaration(interface) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::TSNamespaceDeclaration(module_decl) = statement {
                match self.type_namespace_declaration(module_decl) {
                    Ok(items) => module.items.extend(items),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::TSTypeAliasDeclaration(alias) = statement {
                match self.type_alias_declaration(alias) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && self.collect_lifecycle_hook(
                    &expr_stmt.expression,
                    &mut before_each,
                    &mut after_each,
                )
            {
                continue;
            }
            // `partial.placeholder = Symbol(…)` — a static property on a function
            // declaration, already recorded as a const item by the prepass above.
            // See `lowering::function_statics`.
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && self.function_static_property_recorded(&expr_stmt.expression)
            {
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(table_call) = self.table_test_call(&expr_stmt.expression)
            {
                match self.table_test_declarations(
                    table_call,
                    None,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                    &[],
                ) {
                    Ok(items) => module.items.extend(items),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && self.dynamic_test_alias_call(&expr_stmt.expression)
                && let oxc::ast::ast::Expression::CallExpression(call) = &expr_stmt.expression
                && let Some(guard) = self.conditional_test_alias(call, &top_level_test_setup)
            {
                match self.guarded_test_case_declaration(
                    call,
                    None,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                    &[],
                    guard.into_guard(),
                ) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(test_call) = self.test_case_call(&expr_stmt.expression)
            {
                match self.test_case_declaration(
                    test_call,
                    None,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                    &[],
                ) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(describe_call) = self.describe_call(&expr_stmt.expression)
            {
                match self.describe_declaration(
                    describe_call,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                ) {
                    Ok(items) => module.items.extend(items),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            // Any other top-level expression statement of a test module is part
            // of the module evaluation every test runs after
            // (`Object.assign(global, { .. })` installing a host mock), so it
            // replays in each test's setup in source order beside the
            // top-level bindings — exactly as statements inside a `describe`
            // body already do. It is still lowered into the module body below.
            if test_module && matches!(statement, Statement::ExpressionStatement(_)) {
                top_level_test_setup.push(statement);
            }
            if let Statement::ExportDeclaration(export) = statement
                && let decl = &export.declaration
            {
                if let Declaration::FunctionDeclaration(function) = decl {
                    if function.declare {
                        continue;
                    }
                    if is_implemented_overload_signature(function, &implemented_functions) {
                        continue;
                    }
                    if self.is_module_constructor_function(function) {
                        // An exported constructor function is exported as the
                        // synthesized class, so the class item joins the module's
                        // items under the same source name.
                        match self.synthesize_constructor_function_class(function, &program.body) {
                            Ok(()) => {
                                if let Some(item) = function
                                    .id
                                    .as_ref()
                                    .and_then(|id| self.classes.item(id.name.as_str()))
                                {
                                    module.items.push(item);
                                }
                            }
                            Err(error) => errors.push(error),
                        }
                        continue;
                    }
                    match self.function_declaration(function) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::ClassDeclaration(class) = decl {
                    match self.class_declaration(class) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                    if let Some(id) = &class.id {
                        arrow_queue.mark_class_lowered(id.name.as_str());
                    }
                    self.drain_forward_arrow_queue(
                        &mut arrow_queue,
                        &mut module,
                        &mut errors,
                    );
                } else if let Declaration::TSInterfaceDeclaration(interface) = decl {
                    match self.interface_declaration(interface) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::TSTypeAliasDeclaration(alias) = decl {
                    match self.type_alias_declaration(alias) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::TSNamespaceDeclaration(module_decl) = decl {
                    match self.type_namespace_declaration(module_decl) {
                        Ok(items) => module.items.extend(items),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::VariableDeclaration(variable) = decl {
                    // Already lowered by the forward-arrow queue above.
                    if arrow_queue.is_lifted_exported(variable.span.start) {
                        continue;
                    }
                    match self.const_item_declarations(variable) {
                        Ok(items) => module.items.extend(items),
                        Err(error) => errors.push(error),
                    }
                    self.alias_exported_function_consts(variable);
                }
            } else if let Statement::ExportNamedDeclaration(export) = statement {
                self.reexport_named_declaration(export);
            } else if let Statement::ExportFromDeclaration(export) = statement {
                self.reexport_from_declaration(export, &mut module);
            } else if let Statement::ExportAllDeclaration(export) = statement {
                self.reexport_all_declaration(export, &mut module);
            }
        }

        for statement in &program.body {
            self.anchor_top_level_statement(self.span(statement.span().start, statement.span().end));
            if matches!(
                statement,
                Statement::FunctionDeclaration(_)
                    | Statement::ClassDeclaration(_)
                    | Statement::TSInterfaceDeclaration(_)
                    | Statement::TSNamespaceDeclaration(_)
                    // `declare module "x"` is its own node in oxc. Skip it rather than falling
                    // through to the unsupported-statement diagnostic.
                    | Statement::TSExternalModuleDeclaration(_)
                    | Statement::TSTypeAliasDeclaration(_)
                    // Enums are consumed during the collection phase
                    // (`collect_module_enums`): their members are const-folded
                    // rather than lowered into a statement, so skip them here.
                    | Statement::TSEnumDeclaration(_)
                    | Statement::ImportDeclaration(_)
                    // The oxc 0.147 export split: `export const x` is an
                    // `ExportDeclaration` and `export {a} from 'm'` an
                    // `ExportFromDeclaration`. All three were the one
                    // `ExportNamedDeclaration` node before, and all three are
                    // consumed by the item pass above.
                    | Statement::ExportDeclaration(_)
                    | Statement::ExportFromDeclaration(_)
                    | Statement::ExportNamedDeclaration(_)
                    | Statement::ExportAllDeclaration(_)
                    | Statement::ExportDefaultDeclaration(_)
            ) {
                continue;
            }
            if self.is_predeclared_arrow_const_statement(statement) {
                continue;
            }
            // A static property on a function declaration was already recorded as a
            // const item by the prepass; lowering the assignment again here would put
            // it back into the never-called module-init body, where MIR rejects the
            // member target outright.
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && self.function_static_property_recorded(&expr_stmt.expression)
            {
                continue;
            }

            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }

        // A module-scope `fn.prop = value` claimed onto a function-typed *local*
        // is only ever restored by a callable-interface coercion; nothing else
        // consumes the module body's registry, so a leftover here is a write
        // that would vanish from the output.
        self.report_unconsumed_callable_props(&mut errors);

        // TypeScript declarations are visible throughout their module. A class
        // may therefore implement an interface declared later in the file. Its
        // eager validation skips that not-yet-lowered shape; validate every
        // emitted module class once more after declaration lowering completes.
        for item_id in module.items.clone() {
            if matches!(self.item_ref(item_id), Item::Class(_))
                && let Err(error) = self.validate_implements(item_id)
            {
                errors.push(error);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        if let Some(lowered) = previous.take() {
            self.note_lowered_function(lowered, &module.items);
        }
        self.close_unchecked_generic_returns(&module.items);
        self.record_module_exports(&module, signatures);

        self.fn_ctx = outer_fn_ctx;
        // The body needs the async runtime when it awaits, or when it starts
        // work the event loop must finish before the program exits: a floating
        // promise (`run();` on an async function, whose result nobody awaits)
        // or a timer. Without this the emitted `main` returned while the work
        // sat queued, and an async top-level program printed nothing at all.
        module.is_async = self.body_needs_async_runtime(&body);
        if module.is_async {
            self.append_module_exit_drain(&mut body, span);
            // Every async body carries its await-point metadata; the module
            // body is no exception, and MIR rejects an async body without it.
            body.finish_async();
        }
        let body_id = self.ctx.krate.push_body(body);
        module.body = Some(body_id);
        self.finish_int32_bodies(first_body, &module.items)
            .map_err(|error| vec![error])?;
        Ok(self.ctx.krate.push_module(module))
    }

    /// Append the event-loop drain that runs before the program exits.
    ///
    /// JavaScript does not exit while work is queued: Node drains its microtask
    /// queue and runs due timers, which is why a floating promise
    /// (`run();` on an async function) still prints there. Smelt returned from
    /// `main` immediately, so such a program printed NOTHING.
    ///
    /// The drain is [`smelt_hir::AsyncOp::ExitDrain`] — the runtime's
    /// run-until-idle entry, which polls queued promise tasks and fires due
    /// timers until neither has anything left, AND THEN stays alive while a
    /// referenced handle is open. Lowering it as the body's last statement
    /// rather than wrapping the emitted `main` keeps it part of what the
    /// program does: it composes with the body's own return, and it is an
    /// ordinary awaited op, so the throwing analysis treats it like any other
    /// and `main`'s signature stays consistent with its body.
    ///
    /// Node keeps the process alive while a handle is open — a listening
    /// `http.Server` is one — so `createServer(..).listen(3000)` must not return
    /// from `main`. A mid-program `await sleep(0)` must NOT wait on handles, so
    /// the exit drain is an operation of its own rather than a flag on `Sleep`.
    ///
    /// A module body has no early `return` to skip it — `return` at module
    /// scope is not legal TypeScript — so appending once at the end is enough.
    fn append_module_exit_drain(&mut self, body: &mut Body, span: Span) {
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let future_ty = self.ctx.krate.types.intern(Type::Future(none_ty));
        let sleep = body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: smelt_hir::AsyncOp::ExitDrain,
                args: Vec::new(),
            },
            ty: future_ty,
            span,
        });
        let awaited = body.push_expr(Expr {
            kind: ExprKind::Await(sleep),
            ty: none_ty,
            span,
        });
        // `push_stmt` adds to the root block, which is the module body itself.
        body.push_stmt(smelt_hir::Stmt::Expr(awaited));
    }

    /// Return whether a module body needs the async runtime.
    ///
    /// Three things put work on the event loop, and each means the entry point
    /// cannot be a plain synchronous `fn main`:
    ///
    /// * an `await` -- the body suspends;
    /// * a value of future type produced anywhere in it. A floating promise
    ///   (`run();` where `run` is `async`) is exactly this: nothing awaits the
    ///   future, so only a drain at exit can run it. Node drains its microtask
    ///   queue before exiting, which is why the same program prints there;
    /// * a timer, whose callback likewise runs on the loop.
    ///
    /// Deliberately a scan of the lowered body rather than a flag threaded
    /// through every lowering path: the question is "did anything in here reach
    /// the event loop", and a body that did cannot be missed by a scan of what
    /// it actually produced.
    fn body_needs_async_runtime(&self, body: &Body) -> bool {
        self.body_reaches_event_loop(body, &mut Vec::new())
    }

    /// Whether `body`, or a function it calls, puts work on the event loop.
    ///
    /// The module-body scan above only saw the body's OWN expressions, so a
    /// program whose top level calls a synchronous helper that starts a
    /// server (`serve({ fetch: app.fetch, port })`, with `createServer(..)`
    /// and `listen(..)` inside `serve`) emitted a synchronous `main`: no
    /// runtime for the accept loop's `spawn_local` (it panicked with "there is
    /// no reactor running") and no exit drain to keep the process serving.
    /// Every function item the body references is followed (a reference is
    /// how a call names its callee); closures are not, because defining one
    /// schedules nothing. `visited` stops recursion.
    fn body_reaches_event_loop(&self, body: &Body, visited: &mut Vec<smelt_hir::BodyId>) -> bool {
        let reaches_through_callee = |builder: &Self, seen: &mut Vec<smelt_hir::BodyId>, id: smelt_hir::BodyId| {
            if seen.contains(&id) {
                return false;
            }
            seen.push(id);
            let Some(callee_body) = usize::try_from(id.0).ok().and_then(|index| builder.ctx.krate.bodies.get(index)) else {
                return false;
            };
            builder.body_reaches_event_loop(callee_body, seen)
        };
        for expr in &body.exprs {
            let nested = match &expr.kind {
                ExprKind::Item(item) => match usize::try_from(item.0).ok().and_then(|index| self.ctx.krate.items.get(index)) {
                    Some(Item::Function(function)) => function.body,
                    _ => None,
                },
                _ => None,
            };
            if let Some(id) = nested
                && reaches_through_callee(self, visited, id)
            {
                return true;
            }
        }
        body.exprs.iter().any(|expr| {
            // An `AsyncOp` covers the timers (`Sleep`, `SetTimeout`) as well as
            // the HTTP operations, so one arm answers for every op that reaches
            // the loop directly.
            if matches!(expr.kind, ExprKind::Await(_) | ExprKind::AsyncOp { .. }) {
                return true;
            }
            // A `node:http` server is event-loop work with no `await` in sight:
            // `createServer(handler).listen(3000)` is a complete Node program
            // that runs forever, and its accept loop is a spawned task. Without
            // this arm such a program emitted a SYNCHRONOUS `main` — no runtime
            // for `spawn_local` to attach to, and no exit drain to keep the
            // process alive — so it bound a socket and returned immediately.
            if matches!(
                expr.kind,
                ExprKind::HttpCreateServer { .. }
                    | ExprKind::HttpCreateFetchServer { .. }
                    | ExprKind::HttpServerOp { .. }
            ) {
                return true;
            }
            // A value of FUTURE type anywhere in the body is a promise that
            // something has to drive. The floating case is the one that was
            // silently dropped: `run();` on an async function produces a future
            // nobody awaits, and only a drain at exit runs it.
            matches!(self.ctx.krate.types.get(expr.ty), Some(Type::Future(_)))
        })
    }
}
