//! Shared callback helpers: return-type checks, the call forms of `.call` and
//! `.apply`, and what a receiver's type declares as a method.

use crate::lowering::{
    Body, Expr, ExprKind,
    Literal, ModuleBuilder,
    SmeltError, Span, Type,
};
impl ModuleBuilder<'_> {
    /// Validate the inferred callback return type for an array method.
    pub(in crate::lowering) fn require_callback_ty(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        context: &'static str,
    ) -> Result<(), SmeltError> {
        if actual == expected {
            Ok(())
        } else {
            Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{context} callback returns an unsupported type"),
            ))
        }
    }

    /// Whether the class named `name` itself (not its bases) declares
    /// `method` as a method; see [`Self::receiver_declares_method`].
    fn class_symbol_declares_method(&self, name: smelt_hir::Symbol, method: smelt_hir::Symbol) -> bool {
        let item_method = self.class_by_symbol(name).is_some_and(|class| {
            class.methods.iter().any(|item| {
                self.ctx
                    .krate
                    .items
                    .get(usize::try_from(item.0).unwrap_or(usize::MAX))
                    .is_some_and(|item| {
                        matches!(item, smelt_hir::Item::Function(function) if function.name == method)
                    })
            })
        });
        let sidecar_method = self
            .ctx
            .krate
            .names
            .get(name)
            .or_else(|| self.ctx.krate.symbols.get(name))
            .and_then(|class_name| self.classes.methods(class_name))
            .is_some_and(|methods| methods.iter().any(|candidate| candidate.name == method));
        // `type_alias_fields` mixes two producers: genuine object-type
        // aliases (structural closure fields) and class method surfaces
        // predeclared by `predeclare_class_method_fields` for barrel
        // cycles. Only the class-backed entries name real methods; an
        // alias-typed receiver (e.g. remeda's `Funnel.flush`) stores a
        // closure field with no class item behind it, so routing it to
        // `ExprKind::Method` makes MIR method resolution fail. A lowered
        // `Item::TypeAlias` with this symbol identifies the alias case.
        let predeclared_method = self.find_type_alias(name).is_none()
            && self
                .types
                .alias_fields(name)
                .and_then(|fields| fields.iter().find(|field| field.name == method))
                .is_some_and(|field| {
                    matches!(self.ctx.krate.types.get(field.ty), Some(Type::Function(_)))
                });
        item_method || sidecar_method || predeclared_method
    }

    /// Return whether every concrete receiver arm declares a source method.
    ///
    /// Callback migration normally treats structural callable fields as
    /// closure calls. A class method—and a method common to every class arm of
    /// a union—must remain `ExprKind::Method` so MIR can dispatch it through the
    /// concrete class or tagged-union representation.
    ///
    /// The ordinary (non-callback) member-call path applies the same rule, so
    /// this is shared rather than duplicated: see
    /// `static_member_is_concrete_class_method`.
    pub(in crate::lowering) fn receiver_declares_method(
        &self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> bool {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Union(items)) => {
                !items.is_empty()
                    && items
                        .iter()
                        .all(|item| self.receiver_declares_method(*item, method))
            }
            // An inherited method is the receiver's method too: JavaScript
            // finds it on the prototype chain before `Array.prototype` is ever
            // consulted (`class Nums extends Base {}` where `Base` declares
            // `map`), so the walk follows `extends` to the root.
            Some(Type::Class { name, .. }) => {
                let mut current = Some(*name);
                let mut seen = std::collections::HashSet::new();
                while let Some(class_name) = current
                    && seen.insert(class_name)
                {
                    if self.class_symbol_declares_method(class_name, method) {
                        return true;
                    }
                    current = self.class_by_symbol(class_name).and_then(|class| class.base);
                }
                false
            }
            // A class- or interface-bounded type parameter declares what its
            // bound declares (bounded-generics plan, increment 1), so the call
            // stays an `ExprKind::Method` that MIR dispatches through the bound.
            // An interface counts only for a METHOD signature with a typed
            // result: a function-valued property is still a callable field, and
            // an erased result would have `member_call` drop the call.
            Some(Type::TypeParam { .. }) => {
                let Some(bound) = self.type_param_member_bound(receiver_ty) else {
                    return false;
                };
                if self.receiver_declares_method(bound, method) {
                    return true;
                }
                let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(bound) else {
                    return false;
                };
                self.find_interface(*name).is_some_and(|interface| {
                    interface.methods.iter().any(|signature| {
                        signature.name == method
                            && !matches!(
                                self.ctx.krate.types.get(signature.return_ty),
                                Some(Type::Unknown)
                            )
                    })
                })
            }
            _ => false,
        }
    }

    /// Lower a `Function.prototype.apply` call inside a closure body.
    ///
    /// `fn.apply(thisArg, argsArray)` invokes `fn` with the elements of
    /// `argsArray` as positional arguments. A plain function or closure has no
    /// receiver parameter, so the leading `thisArg` is not passed positionally;
    /// it is installed as the callee's `this` for this one call via `BindThis`
    /// (see [`Self::bind_this_receiver`], which elides the bind for a
    /// null/undefined receiver and for a concrete callable-object callee), and
    /// the trailing array is spread through `ClosureCallSpread`, mirroring the
    /// spread branch of [`Self::callback_call_method_to_body_expr`]. This
    /// generalizes the closure-body method table to the `apply` form the same way
    /// `call` is already supported, without special-casing any particular library
    /// function.
    pub(in crate::lowering) fn callback_apply_method_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        _receiver_ty: smelt_hir::TypeId,
        args: &[smelt_hir::ExprId],
        ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `fn.apply(thisArg, argsArray)` binds `this` from the LEADING operand.
        // The operand is not a positional argument -- a plain function or
        // closure has no receiver parameter -- but it is not nothing either: it
        // is the receiver the callee's `this` resolves to, installed for the
        // duration of this one call by `ExprKind::BindThis`. Dropping it used to
        // be the whole story, which made `func.apply(this, args)` in a wrapper
        // such as es-toolkit's `ary` silently lose the caller's receiver.
        let callee = args.first().copied().map_or(receiver, |this_arg| {
            self.bind_this_receiver(receiver, this_arg, body, span)
        });
        // `fn.apply(thisArg)` (or `fn.apply()`) forwards no positional
        // arguments, so it is an ordinary zero-argument closure call.
        if args.len() <= 1 {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args: Vec::new(),
                },
                ty,
                span,
            }));
        }
        // The trailing operand is the array whose elements become the call
        // arguments; spread it through the packed-argument closure-call ABI.
        let arguments = *args.last().ok_or_else(|| {
            SmeltError::unsupported(span, "callback apply call requires an arguments array")
        })?;
        // A `ClosureCallSpread`'s runtime argument list is a `SmeltList<SmeltUnknown>`,
        // and the emitter resolves `List(Unknown)` out of the type table to render it.
        // Intern it here — at the one site that creates such a call from `apply` — so a
        // program whose only erased value is this argument list still declares the
        // carrier it is about to emit, instead of failing at emit time with
        // "type table does not contain literal operand type Unknown".
        // not a new erasure: names the runtime call-ABI carrier type only.
        let unknown_ty = self.unknown_type();
        self.ctx.krate.types.intern(Type::List(unknown_ty));
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCallSpread {
                callee,
                args: arguments,
            },
            ty,
            span,
        }))
    }

    /// Wrap a callable so `receiver` becomes the `this` its body observes.
    ///
    /// The result keeps the callee's own type: binding a receiver does not
    /// change what the value IS, only what `this` resolves to inside one call,
    /// so a typed function stays a typed function and never degrades to
    /// `SmeltUnknown` at the bind.
    pub(in crate::lowering) fn bind_this_receiver(
        &self,
        callee: smelt_hir::ExprId,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        let ty = Self::expr_ty(body, callee);
        // Only a value that is CALLED through a function value can carry a
        // receiver: a plain function (`Type::Function`, in either of its two
        // Rust spellings) or a dynamic call surface (`unknown`, a type
        // parameter, a union). A concrete callable OBJECT -- a generated struct
        // such as `DebouncedFunction<F>` -- dispatches through its own generated
        // call member instead, and has no function value to wrap; binding a
        // receiver onto it would have to change its Rust type. Those keep the
        // receiver-less behaviour they already had rather than being silently
        // retyped, which is a known gap rather than a modeled case.
        if !matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return callee;
        }
        // `fn.apply(null, args)` / `fn.call(undefined, ..)` supply NO receiver.
        // That is what the channel already answers, so there is nothing to
        // install: eliding the bind keeps the callee's own Rust value (and
        // therefore its call ABI) untouched for the single most common `apply`
        // spelling, instead of wrapping it to install a binding that equals the
        // default.
        if matches!(
            usize::try_from(receiver.0)
                .ok()
                .and_then(|index| body.exprs.get(index))
                .map(|expr| &expr.kind),
            Some(ExprKind::Literal(Literal::None))
        ) {
            return callee;
        }
        body.push_expr(Expr {
            kind: ExprKind::BindThis { callee, receiver },
            ty,
            span,
        })
    }

    /// Coerce a callback-body expression to `String` when it is not already one.
    ///
    /// Erased (`unknown`) and type-param receivers reaching string-only methods
    /// (such as `startsWith`/`endsWith`) are routed through a `TypeAssert` to the
    /// string type, mirroring the direct `string_affix_call` coercion.
    pub(in crate::lowering) fn callback_coerce_to_string(
        value: smelt_hir::ExprId,
        string_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        if Self::expr_ty(body, value) == string_ty {
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span,
            })
        }
    }

}
