//! `Iterable<T>` values: the re-iterable, lazy carrier of the lib interface.
//!
//! TypeScript's `Iterable<T>` is the interface `{ [Symbol.iterator](): Iterator<T> }`.
//! Smelt lowers a one-method interface as that method, so an `Iterable<T>` is
//! the function `() => Iterator<T>` (`SmeltGenerator<T, (), ()>` answered by an
//! `Rc<dyn Fn()>`), here called the iterable's *thunk*:
//!
//! * a value of another type in an `Iterable<T>` slot (an argument, a return,
//!   an annotated binding) becomes a thunk over it ([`ModuleBuilder::iterable_at_slot`]):
//!   each call answers a fresh iterator over the value, an array's live element
//!   iterator, a `Set`'s members, a `Map`'s entries, a string's code points, a
//!   class's `[Symbol.iterator]()`, or the generator itself (a generator is its
//!   own iterator, so iterating it twice finds it exhausted, as in JavaScript);
//! * every consumer of an iterable (`for...of`, spread, `Array.from`, `new Set`,
//!   `new Map`, destructuring, `yield*`) reaches it through `iteration_generator`,
//!   which calls the thunk for the iterator it drives.
//!
//! Requirements (`docs/design/iterator-protocol.md`, "`Iterable<T>`"): the
//! element type is the one the source declares; nothing is drained at the
//! call, so an infinite iterable works and its side effects interleave with
//! the consumer's as in Node; iterating the value again starts a new iterator
//! (an array restarts). Like every interface-typed value, the thunk is not the
//! object it was made from (`Array.isArray` of it is `false`).

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::erasure::NotNewErasure;
use smelt_hir::{
    Body, CaptureMode, ClosureCapture, ClosureExpr, ClosureThis, DictProjectionOp, Expr, ExprId,
    ExprKind, FunctionType, LocalDecl, Param, Span, Type, TypeId,
};

impl ModuleBuilder<'_> {
    /// The type of an `Iterable<T>` with element type `item`: the thunk
    /// `() => Iterator<T>`, whose iterator returns `undefined` and takes
    /// nothing on resume (the lib's open `TReturn`/`TNext`).
    pub(in crate::lowering) fn iterable_thunk_type(&mut self, item: TypeId) -> TypeId {
        let iterator = self.collection_iterator_type(item);
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty: iterator,
            is_async: false,
            may_throw: false,
        }))
    }

    /// The iterator type an iterable thunk of type `ty` answers, when `ty` is
    /// one ([`Self::iterable_thunk_type`] of some element type).
    ///
    /// A function of no parameters answering a synchronous generator that
    /// returns and takes `undefined` is only ever an `Iterable<T>`: TypeScript
    /// rejects iterating any other function value, or passing one where an
    /// iterable is expected, so the representations may share.
    pub(in crate::lowering) fn iterable_thunk_iterator(&self, ty: TypeId) -> Option<TypeId> {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        if !function.params.is_empty() || function.is_async {
            return None;
        }
        let Some(Type::Generator {
            is_async: false,
            return_ty,
            next_ty,
            ..
        }) = self.ctx.krate.types.get(function.return_ty)
        else {
            return None;
        };
        let undefined = |ty: TypeId| matches!(self.ctx.krate.types.get(ty), Some(Type::None));
        (undefined(*return_ty) && undefined(*next_ty)).then_some(function.return_ty)
    }

    /// `member` as a member of a union with an array arm
    /// (`ArrayLike<T> | Iterable<T>`) or of an intersection
    /// (`P & Iterable<P>`): an iterable thunk reads as the array `T[]`, the
    /// representation `Iterable<T>` had before it became lazy, so the union
    /// collapses to its array arm and the intersection keeps its shape. A
    /// value converted for such a slot is drained into the array there (no
    /// laziness); a union of an array with a function value, or an
    /// intersection with one, would otherwise have no concrete
    /// representation and erase. Any other member is returned unchanged.
    pub(in crate::lowering) fn iterable_member_as_array(&mut self, member: TypeId) -> TypeId {
        match self
            .iterable_thunk_iterator(member)
            .map(|iterator| self.ctx.krate.types.get(iterator).cloned())
        {
            Some(Some(Type::Generator { yield_ty, .. })) => {
                self.ctx.krate.types.intern(Type::List(yield_ty))
            }
            _ => member,
        }
    }

    /// The type hint a value for a slot typed `hint` is lowered with: `T[]`
    /// for an `Iterable<T>` slot, so a literal takes its element types from
    /// `T` (`[["a", 1]]` as `[string, number][]`) before
    /// [`Self::iterable_at_slot`] converts it; `hint` itself otherwise.
    pub(in crate::lowering) fn iterable_shape_hint(&mut self, hint: TypeId) -> TypeId {
        match self
            .iterable_thunk_iterator(hint)
            .map(|iterator| self.ctx.krate.types.get(iterator).cloned())
        {
            Some(Some(Type::Generator { yield_ty, .. })) => {
                self.ctx.krate.types.intern(Type::List(yield_ty))
            }
            _ => hint,
        }
    }

    /// A fresh iterator over `value`, as its `[Symbol.iterator]()` answers it:
    /// the thunk's call for an iterable, the generator itself, a live
    /// iterator over an array, a `Set`'s members or a `Map`'s entries, a
    /// string's code points, or a class's `[Symbol.iterator]()`. `None` for a
    /// value Smelt does not iterate this way (an erased one).
    ///
    /// # Errors
    /// As `iteration_generator`, for a class whose iterator is not typed.
    fn fresh_iterator(
        &mut self,
        value: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let value_ty = Self::expr_ty(body, value);
        let iterator = |this: &mut Self, body: &mut Body, kind, item| {
            let ty = this.collection_iterator_type(item);
            body.push_expr(Expr { kind, ty, span })
        };
        Ok(match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::Generator {
                is_async: false, ..
            }) => Some(value),
            Some(Type::List(item)) => Some(iterator(
                self,
                body,
                ExprKind::ListIterator { list: value },
                item,
            )),
            Some(Type::Set(item)) => Some(iterator(
                self,
                body,
                ExprKind::CollectionIterator {
                    op: DictProjectionOp::Values,
                    set: true,
                    collection: value,
                },
                item,
            )),
            Some(Type::Dict(key, entry) | Type::JsMap(key, entry)) => {
                let pair = self.ctx.krate.types.intern(Type::Tuple(vec![key, entry]));
                Some(iterator(
                    self,
                    body,
                    ExprKind::CollectionIterator {
                        op: DictProjectionOp::Entries,
                        set: false,
                        collection: value,
                    },
                    pair,
                ))
            }
            Some(Type::String) => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let chars_ty = self.ctx.krate.types.intern(Type::List(string_ty));
                let chars = body.push_expr(Expr {
                    kind: ExprKind::StringChars { haystack: value },
                    ty: chars_ty,
                    span,
                });
                Some(iterator(
                    self,
                    body,
                    ExprKind::ListIterator { list: chars },
                    string_ty,
                ))
            }
            // A typed array iterates its decoded elements.
            Some(Type::Class { .. })
                if let Some(elements) = self.typed_array_spread_list(value, span, body) =>
            {
                let float_ty = self.ctx.krate.types.intern(Type::Float);
                Some(iterator(
                    self,
                    body,
                    ExprKind::ListIterator { list: elements },
                    float_ty,
                ))
            }
            _ => self.iteration_generator(value, span, body)?,
        })
    }

    /// `value` in a slot typed `Iterable<T>` (`hint`, an iterable thunk): an
    /// immediately called synthesized closure over the value answering the
    /// thunk `() => <a fresh iterator over it>` ([`Self::fresh_iterator`],
    /// re-typed to the slot's iterator). The value is evaluated once, where
    /// the slot is; each call of the thunk starts a new iteration of it.
    /// An erased value (or erased record) is asserted to the thunk type,
    /// which the emitter reads through the erased iteration protocol.
    ///
    /// # Errors
    /// Unsupported, for a value with no iterator Smelt can drive, or whose
    /// elements do not convert to the slot's `T`; as [`Self::fresh_iterator`].
    pub(in crate::lowering) fn iterable_at_slot(
        &mut self,
        value: ExprId,
        hint: TypeId,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let Some(iterator_ty) = self.iterable_thunk_iterator(hint) else {
            return Ok(None);
        };
        let value_ty = Self::expr_ty(body, value);
        if value_ty == hint {
            return Ok(Some(value));
        }
        let span = Self::body_expr_span(body, value);

        // The thunk: captures the source, answers a fresh iterator over it.
        let source_symbol = self.intern_synthesized_name("__smelt_iterable_source");
        let mut adapter = Body::new(None, span);
        let source_param = adapter.push_local(LocalDecl {
            name: Some(source_symbol),
            ty: value_ty,
            mutable: false,
            span,
        });
        adapter.params.push(source_param);
        let mut thunk = Body::new(None, span);
        let captured = thunk.push_local(LocalDecl {
            name: Some(source_symbol),
            ty: value_ty,
            mutable: false,
            span,
        });
        let source = thunk.push_expr(Expr {
            kind: ExprKind::Local(captured),
            ty: value_ty,
            span,
        });
        // An erased value, or the erased record an object literal with methods
        // lowers to (`{ *[Symbol.iterator]() { .. } }`), is read as the
        // iterable through the erased iteration protocol: the emitter's
        // extraction of an erased value to the thunk type.
        // So is any value for a slot whose element the source declares
        // `unknown` (`Iterable<unknown>`): its elements cross into the erased
        // carrier there, which the erased protocol does (eagerly, at each
        // iteration; the erasure is the source's).
        let erased = matches!(self.ctx.krate.types.get(value_ty), Some(Type::Unknown));
        if erased || self.is_erased_record(value_ty) || self.type_mentions_unknown(iterator_ty) {
            let source = if erased {
                value
            } else {
                let unknown = self.unknown_type(NotNewErasure::ErasedUpstream);
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value },
                    ty: unknown,
                    span,
                })
            };
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: source },
                ty: hint,
                span,
            })));
        }
        let Some(fresh) = self.fresh_iterator(source, span, &mut thunk)? else {
            return Err(SmeltError::unsupported(
                span,
                "a value of this type where an `Iterable<T>` is expected is not lowered yet (it has no iterator Smelt can drive)",
            ));
        };
        // A generic slot (`Iterable<T>` of a generic function) is the call's
        // specialization to resolve: the thunk is built at the value's own
        // element type, as an array argument keeps its own element type. A
        // concrete slot takes its own iterator type.
        let fresh_ty = Self::expr_ty(&thunk, fresh);
        let (target_iterator, thunk_ty) = if self.type_mentions_type_param(iterator_ty) {
            let Some(Type::Generator { yield_ty, .. }) =
                self.ctx.krate.types.get(fresh_ty).cloned()
            else {
                return Err(SmeltError::unsupported(
                    span,
                    "an iterable's iterator is no generator",
                ));
            };
            (
                self.collection_iterator_type(yield_ty),
                self.iterable_thunk_type(yield_ty),
            )
        } else {
            (iterator_ty, hint)
        };
        let fresh = if fresh_ty == target_iterator {
            fresh
        } else {
            let retyped = self.iterator_value_at_slot(fresh, Some(target_iterator), &mut thunk)?;
            if Self::expr_ty(&thunk, retyped) != target_iterator {
                return Err(SmeltError::unsupported(
                    span,
                    "an iterable whose elements are not the `Iterable<T>` slot's `T` is not lowered yet",
                ));
            }
            retyped
        };
        thunk.blocks.first_mut().map(|root| root.tail = Some(fresh));
        let thunk_body = self.ctx.krate.push_body(thunk);
        let thunk_closure = adapter.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: Vec::new(),
                rest: None,
                required_params: Some(0),
                return_ty: target_iterator,
                captures: vec![ClosureCapture {
                    source_local: source_param,
                    body_local: Some(captured),
                    symbol: source_symbol,
                    ty: value_ty,
                    mode: CaptureMode::ByValue,
                }],
                body: thunk_body,
                name: None,
                length: None,
                function_item: None,
                this_binding: ClosureThis::Own,
                span,
            }),
            ty: thunk_ty,
            span,
        });
        adapter
            .blocks
            .first_mut()
            .map(|root| root.tail = Some(thunk_closure));
        let adapter_body = self.ctx.krate.push_body(adapter);
        let adapter_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![value_ty],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: thunk_ty,
            is_async: false,
            may_throw: false,
        }));
        let adapter_closure = body.push_expr(Expr {
            kind: ExprKind::Closure(ClosureExpr {
                params: vec![Param {
                    name: source_symbol,
                    local: source_param,
                    ty: value_ty,
                    span,
                }],
                rest: None,
                required_params: Some(1),
                return_ty: thunk_ty,
                captures: Vec::new(),
                body: adapter_body,
                name: None,
                length: None,
                function_item: None,
                this_binding: ClosureThis::Own,
                span,
            }),
            ty: adapter_ty,
            span,
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: adapter_closure,
                args: vec![value],
            },
            ty: thunk_ty,
            span,
        })))
    }

    /// The iterator an iterable thunk `value` answers: the thunk called
    /// (`[Symbol.iterator]()`). `None` when `value` is no iterable thunk.
    pub(in crate::lowering) fn iterable_thunk_call(
        &self,
        value: ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<ExprId> {
        let iterator_ty = self.iterable_thunk_iterator(Self::expr_ty(body, value))?;
        Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: value,
                args: Vec::new(),
            },
            ty: iterator_ty,
            span,
        }))
    }
}
