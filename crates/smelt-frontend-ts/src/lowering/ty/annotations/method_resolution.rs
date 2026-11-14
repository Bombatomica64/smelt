//! Method resolution on a receiver type: the method a call reaches, including
//! methods of a class still being lowered and methods returning their receiver.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::support::source_spelling;
use smelt_hir::{ErasureReason, ExprKind, Item, Type};

impl ModuleBuilder<'_> {
    /// Whether the method item `item` returns its own receiver from every
    /// `return` it has.
    ///
    /// That is the body shape behind a fluent `return this`, and the only one
    /// whose declared "returns my own class" annotation is JavaScript's
    /// polymorphic `this` rather than a genuine base-instance factory. A method
    /// with no body (declared-only) or no `return` at all answers `false`.
    pub(in crate::lowering) fn method_returns_its_receiver(&self, item: smelt_hir::ItemId) -> bool {
        let Some(Item::Function(function)) = self
            .ctx
            .krate
            .items
            .get(usize::try_from(item.0).unwrap_or(usize::MAX))
        else {
            return false;
        };
        let Some(body) = function.body.and_then(|body| {
            self.ctx
                .krate
                .bodies
                .get(usize::try_from(body.0).unwrap_or(usize::MAX))
        }) else {
            return false;
        };
        let Some(receiver) = body.params.first().copied() else {
            return false;
        };
        let mut saw_return = false;
        for stmt in &body.stmts {
            let smelt_hir::Stmt::Return(Some(value)) = stmt else {
                continue;
            };
            saw_return = true;
            let Some(expr) = body.expr_at(*value) else {
                return false;
            };
            if !matches!(expr.kind, ExprKind::Local(local) if local == receiver) {
                return false;
            }
        }
        saw_return
    }

    /// Resolve a method call on a type.
    ///
    /// Concrete class receivers resolve to a real callable method item; the
    /// second element of the returned tuple is that `ItemId`. Every other
    /// receiver kind — records/dicts, interfaces, concrete unions, lists, sets,
    /// functions, futures, and erased/builtin values such as `string` or
    /// `number` — has no statically-resolvable class method item, so it lowers
    /// through the shared dynamic-dispatch boundary and returns
    /// `ItemId::MAX`.
    ///
    /// This boundary is intentionally uniform across non-class kinds: when a
    /// method on such a receiver is not a modeled builtin (those are dispatched
    /// earlier in `dispatch_builtin_call`), the call has no concrete method item
    /// to bind and is erased to an `Unknown`-typed value stub by the caller,
    /// exactly as unmodeled list/dict/union methods already are. Routing the
    /// remaining primitive and structural receiver kinds through the same
    /// boundary — rather than hard-erroring on them — keeps method-call lowering
    /// general for every library without widening the receiver's own static
    /// type. Only genuinely dynamic method calls end up here; concrete class
    /// dispatch below is always preferred when the receiver is a class value.
    pub(in crate::lowering) fn resolve_method(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
        span: oxc::span::Span,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::ItemId), SmeltError> {
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(receiver_ty).cloned() {
            let mut return_types = Vec::with_capacity(items.len());
            for item in items {
                let field_ty = self.class_field_type(item, method)?;
                let Some(Type::Function(function)) = self.ctx.krate.types.get(field_ty).cloned()
                else {
                    // An erased member type was recorded by `class_field_type`.
                    let unknown = if self.ctx.krate.types.get(field_ty) == Some(&Type::Unknown) {
                        self.unknown_type(NotNewErasure::ErasedUpstream)
                    } else {
                        self.erase(ErasureReason::ByName)
                    };
                    return Ok((unknown, smelt_hir::ItemId(u32::MAX)));
                };
                let return_ty = function.return_ty;
                if !return_types.contains(&return_ty) {
                    return_types.push(return_ty);
                }
            }
            let return_ty = match return_types.as_slice() {
                [single] => *single,
                [] => self.unknown_type(NotNewErasure::Unreachable), // not a new erasure: unreachable, only an empty union yields no arm
                _ => self.ctx.krate.types.intern(Type::Union(return_types)),
            };
            // A union call has no single static method item. MIR retains the
            // typed call and dispatches over the concrete tagged-union arms.
            return Ok((return_ty, smelt_hir::ItemId(u32::MAX)));
        }
        // A class- or interface-bounded type parameter resolves the method on its
        // bound: the call is typed by the bound's declaration and MIR dispatches
        // it by the receiver's concrete type (`Rvalue::BoundMethod`), so the
        // bound's item id is only reported for its generic-method instantiation.
        if let Some(bound) = self.type_param_member_bound(receiver_ty) {
            return self.resolve_method(bound, method, span);
        }
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(receiver_ty).cloned()
        else {
            // Non-class receiver: no concrete class method item exists. Fall to
            // the dynamic-dispatch boundary so records, interfaces, concrete
            // unions, and erased/builtin values (`string`, `number`, optionals,
            // tuples, ...) all lower uniformly instead of being rejected.
            // An already-erased receiver was recorded where it was decided.
            let ty = if self.ctx.krate.types.get(receiver_ty) == Some(&Type::Unknown) {
                self.unknown_type(NotNewErasure::ErasedUpstream)
            } else {
                self.erase(ErasureReason::ByName)
            };
            return Ok((ty, smelt_hir::ItemId(u32::MAX)));
        };
        let Some(class) = self.class_by_symbol(name).cloned() else {
            if let Some(return_ty) =
                self.in_progress_class_method_return(name, &args, method, span)?
            {
                return Ok((return_ty, smelt_hir::ItemId(u32::MAX)));
            }
            // The class is still being lowered and does not DECLARE this method,
            // so it is inherited: resolve it through the recorded base, exactly
            // as the lowered-class path below does. Falling through instead
            // typed the call as the receiver and lowered it as a dynamic call
            // through a member read, which the emitter renders as a FIELD read
            // of a name the class has as a method (`E0615: attempted to take
            // value of method`, H44 — Hono's anonymous
            // `class<T> extends RegExpRouter<T>` calling the base's protected
            // `buildAllMatchers`).
            if let Some(resolved) = self.in_progress_class_base_method(name, method, span) {
                return Ok(resolved);
            }
            // An interface-typed receiver has no concrete method item, but its
            // method signatures carry return types. Resolving them keeps the
            // call expression correctly typed (e.g. `counter.count()` is a
            // `number`, not the interface itself). Interface method calls are
            // ordinarily dispatched through the function-typed field path, so
            // this branch is the type-only fallback; there is no callable item,
            // hence `ItemId::MAX`.
            if let Some(interface) = self.find_interface(name).cloned() {
                let substitutions = self.type_argument_substitution(
                    &interface.type_params,
                    &args,
                    self.span(span.start, span.end),
                )?;
                if let Some(method_sig) = interface.methods.iter().find(|item| item.name == method)
                {
                    let return_ty =
                        self.substitute_type_params(method_sig.return_ty, &substitutions);
                    return Ok((return_ty, smelt_hir::ItemId(u32::MAX)));
                }
            }
            return Ok((receiver_ty, smelt_hir::ItemId(u32::MAX)));
        };
        for item in &class.methods {
            if let Item::Function(function) = self.item_ref(*item)
                && function.name == method
            {
                // A generic class receiver such as `Box<number>` instantiates the
                // method's declared type parameters. `get(): T` on `Box<number>`
                // returns `number`, not the bare parameter `T`, so the call
                // expression is typed with the receiver's concrete arguments
                // substituted in. Without this, the method result stays a
                // `TypeParam` and erases to `SmeltUnknown` at use sites even
                // though the emitted `impl<T> Box<T>` method already returns `T`.
                let return_ty = function.return_ty;
                let item_id = *item;
                let substitutions = self.type_argument_substitution(
                    &class.type_params,
                    &args,
                    self.span(span.start, span.end),
                )?;
                let substituted_return = self.substitute_type_params(return_ty, &substitutions);
                return Ok((substituted_return, item_id));
            }
        }
        for abstract_method in &class.abstract_methods {
            if abstract_method.name == method {
                let return_ty = abstract_method.return_ty;
                let substitutions = self.type_argument_substitution(
                    &class.type_params,
                    &args,
                    self.span(span.start, span.end),
                )?;
                let substituted_return = self.substitute_type_params(return_ty, &substitutions);
                return Ok((substituted_return, smelt_hir::ItemId(u32::MAX)));
            }
        }
        if let Some(base) = class.base {
            let substitutions = self.type_argument_substitution(
                &class.type_params,
                &args,
                self.span(span.start, span.end),
            )?;
            let base_args = class
                .base_args
                .clone()
                .into_iter()
                .map(|arg| self.substitute_type_params(arg, &substitutions))
                .collect();
            let base_ty = self.ctx.krate.types.intern(Type::Class {
                name: base,
                args: base_args,
            });
            let (return_ty, item) = self.resolve_method(base_ty, method, span)?;
            // JavaScript's polymorphic `this`: a base method annotated with its
            // own class that RETURNS the receiver answers the receiver's class,
            // not the base's. TypeScript spells the annotation nominally
            // (`route(..): Hono<..> { return this }`), and Smelt flattens
            // inheritance into separate structs, so typing the call at the base
            // would contradict what the emitted method returns. The body check
            // keeps a genuine base-instance FACTORY (`clone()`-style) at the base
            // type; the emitter applies the same predicate when it renders the
            // inherited copy's return type, so the two cannot drift.
            if self.method_returns_its_receiver(item)
                && matches!(
                    self.ctx.krate.types.get(return_ty),
                    Some(Type::Class { name: returned, .. }) if *returned == base
                )
            {
                return Ok((receiver_ty, item));
            }
            return Ok((return_ty, item));
        }
        if self
            .ctx
            .krate
            .symbols
            .get(method)
            .is_some_and(|method_text| matches!(method_text, "call" | "apply" | "bind"))
        {
            let ty = self.erase(ErasureReason::ByName);
            return Ok((ty, smelt_hir::ItemId(u32::MAX)));
        }
        let method_name = self.ctx.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(SmeltError::unsupported(
            self.span(0, 0),
            format!("unknown class method `{method_name}`"),
        ))
    }

    /// Resolve a method an IN-PROGRESS class inherits, through its base class.
    ///
    /// A class expression's members lower before its own item is registered, so
    /// `class_by_symbol` answers `None` for the class currently being lowered
    /// and its own metadata only lists the methods it declares. Its base,
    /// however, is fully lowered and recorded by `ClassRegistry::set_base`, so
    /// an inherited method resolves there and the call keeps a real method item
    /// instead of erasing to a member read.
    ///
    /// Answers `None` — never an error — when there is no base or the base
    /// chain does not have the method: this is a recovery path, and the caller's
    /// existing fall-through is still the right answer for a name that is
    /// genuinely dynamic.
    fn in_progress_class_base_method(
        &mut self,
        class: smelt_hir::Symbol,
        method: smelt_hir::Symbol,
        span: oxc::span::Span,
    ) -> Option<(smelt_hir::TypeId, smelt_hir::ItemId)> {
        // Keyed by SYMBOL, never by the source spelling: a class renamed for a
        // cross-module collision keeps its source name, so a by-name lookup of
        // another module's `Store_1` answers with THIS module's `class Store
        // extends Store_1` — whose base is the receiver again, and the walk below
        // never terminates. The symbol-keyed map answers only for classes the
        // module currently being lowered declared, which is the only set whose
        // in-progress bases this recovery path is entitled to speak for.
        let (base, base_args) = self.classes.base_of_symbol(class).cloned()?;
        if base == class {
            return None;
        }
        let base_ty = self.ctx.krate.types.intern(Type::Class {
            name: base,
            args: base_args,
        });
        self.resolve_method(base_ty, method, span).ok()
    }

    /// Resolve a method return type from metadata collected before a class item exists.
    pub(in crate::lowering) fn in_progress_class_method_return(
        &self,
        class: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        method: smelt_hir::Symbol,
        span: oxc::span::Span,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        let Some(class_name) = source_spelling(&self.ctx.krate, class).map(str::to_owned) else {
            return Ok(None);
        };
        let Some(methods) = self.classes.methods(&class_name).cloned() else {
            return Ok(None);
        };
        let Some(signature) = methods.into_iter().find(|item| item.name == method) else {
            return Ok(None);
        };
        let _ = (args, span);
        Ok(Some(signature.return_ty))
    }
}
