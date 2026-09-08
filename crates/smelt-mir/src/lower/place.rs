//! Lvalue (place) lowering for assignment targets.
//!
//! Only a handful of expression kinds denote assignable MIR [`Place`]s: locals,
//! field and index projections, tuple indexing, and the transparent
//! `TypeAssert`/`UnknownCast` wrappers. [`LoweringCtx::lower_place`] handles
//! those; every other [`ExprKind`] is a value expression and is rejected by
//! [`LoweringCtx::place_unsupported`], whose wildcard-free exhaustive `match`
//! forces a compile error (and a deliberate decision) whenever a new
//! `ExprKind` variant is added.

use smelt_hir::{ExprId, ExprKind};

use crate::{Constant, GlobalProjection, Operand, Place};

use super::context::LoweringCtx;
use super::{LoweredPlace, LoweredPlaceBase, LowerError, PlaceWritebacks};

impl LoweringCtx<'_> {
    /// Lowers an lvalue expression to a MIR place for assignment targets.
    ///
    /// Only a handful of expression kinds form assignable places; the wildcard
    /// arm routes everything else to [`Self::place_unsupported`], which owns the
    /// exhaustive listing that keeps compile-time coverage of new `ExprKind`s.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "compile-time exhaustiveness for non-place kinds is enforced in place_unsupported"
    )]
    pub(super) fn lower_place(
        &mut self,
        expr_id: ExprId,
    ) -> Result<LoweredPlace, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        match &expr.kind {
            ExprKind::Local(local) => {
                let local_id = self.locals.get(local).copied().ok_or_else(|| {
                    self.error("assignment references an unknown local", Some(expr.span))
                })?;
                Ok((Place::Local(local_id), PlaceWritebacks::new()))
            }
            ExprKind::Field { receiver, field } => {
                // A write THROUGH a module-level mutable global must name the
                // cell, not a copy read out of it: `materialize_operand_local`
                // below would lower the receiver to a `GlobalGet`, whose clone
                // shares the store for a handle type and deep-copies for a
                // value type, so the write would be correct for one and
                // silently lost for the other. See
                // `blocker-logs/hono-h6-place-global.md`.
                if let Some(base) = self.mutable_global_receiver(*receiver) {
                    return Ok((
                        Place::Global {
                            base,
                            projection: GlobalProjection::Field(*field),
                        },
                        PlaceWritebacks::new(),
                    ));
                }
                let (base, writebacks) = self.place_base_local(*receiver, expr.span)?;
                Ok((
                    Place::Field {
                        base,
                        field: *field,
                    },
                    writebacks,
                ))
            }
            ExprKind::Index { receiver, index } => {
                if let Some(base) = self.mutable_global_receiver(*receiver) {
                    // The index is lowered here, BEFORE the cell is borrowed at
                    // emission time, which is what keeps `cache[cache_key()] =
                    // v` from double-borrowing the `RefCell` at runtime.
                    let index_operand = self.lower_expr(*index)?;
                    return Ok((
                        Place::Global {
                            base,
                            projection: GlobalProjection::Index {
                                index: Box::new(index_operand),
                                negative: self.negative_index_policy(expr.span),
                            },
                        },
                        PlaceWritebacks::new(),
                    ));
                }
                let (base, writebacks) = self.place_base_local(*receiver, expr.span)?;
                let index_operand = self.lower_expr(*index)?;
                Ok((
                    Place::Index {
                        base,
                        index: Box::new(index_operand),
                        negative: self.negative_index_policy(expr.span),
                    },
                    writebacks,
                ))
            }
            ExprKind::TupleIndex { tuple, index } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let tuple_ty = self.hir_expr(*tuple)?.ty;
                let base = self.materialize_operand_local(tuple_operand, tuple_ty, expr.span)?;
                let tuple_index = i64::try_from(*index).map_err(|_error| {
                    self.error("tuple index does not fit in MIR integer", Some(expr.span))
                })?;
                Ok((
                    Place::Index {
                        base,
                        index: Box::new(Operand::Const(Constant::Int(tuple_index))),
                        // A tuple index is a resolved, non-negative position; the
                        // policy never applies, so record the language's anyway.
                        negative: self.negative_index_policy(expr.span),
                    },
                    PlaceWritebacks::new(),
                ))
            }
            // A write through a receiver whose HIR type is still optional.
            //
            // `tsc` has already proved the receiver present at this write —
            // `let parts: number[] | undefined; parts = []; parts[i] = v` is
            // accepted only because the assignment narrows `parts` to
            // `number[]`, and `parts![i] = v` says so outright. The frontend's
            // narrowing does not always reach the write target, so the target
            // arrives as `OptionalField` / `OptionalIndex` and used to be
            // rejected outright (`partOffsets[p] = offset` in Hono's trie
            // router). A hand-written Rust team would bind the unwrapped
            // receiver and assign through it, which is exactly what
            // `narrowed_receiver_base` builds: a temporary of the INNER type,
            // whose `Rvalue::Use` of an optional operand emits the narrowing
            // unwrap, then an ordinary field or index projection on it.
            ExprKind::OptionalField { receiver, field } => {
                let (base, writebacks) = self.narrowed_receiver_base(*receiver, expr.span)?;
                Ok((
                    Place::Field {
                        base,
                        field: *field,
                    },
                    writebacks,
                ))
            }
            ExprKind::OptionalIndex { receiver, index } => {
                let (base, writebacks) = self.narrowed_receiver_base(*receiver, expr.span)?;
                let index_operand = self.lower_expr(*index)?;
                Ok((
                    Place::Index {
                        base,
                        index: Box::new(index_operand),
                        negative: self.negative_index_policy(expr.span),
                    },
                    writebacks,
                ))
            }
            ExprKind::TypeAssert { value } | ExprKind::UnknownCast { value, .. } => {
                self.lower_place(*value)
            }
            // Every other expression kind produces a value, not an assignable
            // place. The exhaustive listing that documents and enforces this at
            // compile time lives in `place_unsupported`, keeping this match short.
            _ => Err(self.place_unsupported(&expr)),
        }
    }

    /// The local a place projection is rooted at, plus any writeback the root
    /// needs.
    ///
    /// A plain local receiver (`x[i] = v`) is its own root and needs nothing. A
    /// PROJECTED receiver (`a.b[i] = v`, `a.b.c[i] = v`) has to be copied into a
    /// temporary, because a MIR place is rooted at a local -- and that copy is
    /// exactly the silent-wrong-value bug H31 names: for a value representation
    /// the write lands in the copy and never reaches `a`. So the projection is
    /// returned alongside the temporary as a writeback for the caller to replay
    /// after the write, the same contract [`Self::lower_mutation_receiver`] uses
    /// for `a.b.push(x)`. Recursing through [`Self::lower_place`] rather than
    /// [`Self::lower_expr`] is what makes depth work: each level contributes its
    /// own entry, and `PlaceWritebacks` documents the replay order.
    ///
    /// A receiver that is not a projection at all (a call, a `new`, a literal)
    /// keeps the ordinary materialize-a-value path: there is nowhere to commit
    /// it back to, and nothing observes it.
    fn place_base_local(
        &mut self,
        receiver: ExprId,
        span: smelt_hir::Span,
    ) -> Result<LoweredPlaceBase, LowerError> {
        let receiver_expr = self.hir_expr(receiver)?.clone();
        // The transparent wrappers (`TypeAssert`, `UnknownCast`) are deliberately
        // NOT here. `bucket![0] = 9` roots at the local `bucket`, whose declared
        // type is still `T[] | undefined`, and an optional-typed place base has
        // no index-write spelling -- the write was silently discarded. Their
        // `lower_expr` does the narrowing the write needs, so a wrapped receiver
        // keeps the materialize-a-value path. `OptionalField`/`OptionalIndex`
        // are here: `lower_place` roots those at a temporary of the INNER type
        // (`narrowed_receiver_base`), which is a place base a write can use.
        if !matches!(
            receiver_expr.kind,
            ExprKind::Field { .. }
                | ExprKind::Index { .. }
                | ExprKind::TupleIndex { .. }
                | ExprKind::OptionalField { .. }
                | ExprKind::OptionalIndex { .. }
        ) {
            let receiver_operand = self.lower_expr(receiver)?;
            let local =
                self.materialize_operand_local(receiver_operand, receiver_expr.ty, span)?;
            return Ok((local, PlaceWritebacks::new()));
        }
        let (place, mut writebacks) = self.lower_place(receiver)?;
        if let Place::Local(local) = place {
            return Ok((local, writebacks));
        }
        // A mutable global's cell is already the assignment root: `Place::Global`
        // exists precisely so the write does not go through a copy, and it has
        // no lvalue fragment to compose a nested projection onto. Fall back to
        // the value read for it, which is what the previous code always did.
        if matches!(place, Place::Global { .. }) {
            let receiver_operand = self.lower_expr(receiver)?;
            let local =
                self.materialize_operand_local(receiver_operand, receiver_expr.ty, span)?;
            return Ok((local, writebacks));
        }
        let local = self.push_temp(receiver_expr.ty, span);
        self.block_mut()?.statements.push(crate::Statement::Assign {
            dest: local,
            value: crate::Rvalue::Use(Operand::Copy(place.clone())),
        });
        writebacks.push((place, local));
        Ok((local, writebacks))
    }

    /// Materializes an assignment receiver whose type is still optional, as a
    /// local of the INNER type, and returns that local for use as a place base
    /// together with the writebacks the roots need.
    ///
    /// The temporary is typed with the optional's inner type, so the
    /// `Rvalue::Use` that fills it converts an `Optional<T>` operand to `T`
    /// through the emitter's narrowing unwrap. A receiver that is not optional
    /// keeps its own type and goes through the ordinary root, so this is safe to
    /// call for any receiver.
    ///
    /// The receiver is rooted through [`Self::place_base_local`], not through
    /// `lower_expr`: a PROJECTED optional receiver
    /// (`this.branches[b].leaves[k] = leaf`) would otherwise be read as a value
    /// -- `tmp.as_ref().map(|v| v.leaves.clone())` -- and the write would land
    /// in that clone. Rooting it at a place makes each level of the projection
    /// commit back through its own place (H31).
    ///
    /// The unwrap ITSELF gets a writeback: the inner value is a copy of what the
    /// optional holds, so it is stored back into the optional place afterwards
    /// (the emitter re-wraps an inner-typed operand for an optional
    /// destination). For a shared handle that store is a no-op in effect; for a
    /// value representation it is the difference between keeping and losing the
    /// write.
    fn narrowed_receiver_base(
        &mut self,
        receiver: ExprId,
        span: smelt_hir::Span,
    ) -> Result<LoweredPlaceBase, LowerError> {
        let (base, mut writebacks) = self.place_base_local(receiver, span)?;
        let receiver_ty = self.hir_expr(receiver)?.ty;
        let Some(smelt_hir::Type::Optional(inner)) = self.krate.types.get(receiver_ty) else {
            return Ok((base, writebacks));
        };
        let inner_ty = *inner;
        // Always a FRESH temporary: reusing the optional local as the place base
        // would keep the optional type on it, and the projection needs the
        // unwrapped value.
        let local = self.push_temp(inner_ty, span);
        self.block_mut()?.statements.push(crate::Statement::Assign {
            dest: local,
            value: crate::Rvalue::Use(Operand::Copy(Place::Local(base))),
        });
        writebacks.push((Place::Local(base), local));
        Ok((local, writebacks))
    }

    /// The MIR global index when `receiver` reads a module-level mutable global.
    ///
    /// Returns `None` for every other receiver, so the ordinary
    /// materialize-a-local path runs unchanged. Only a DIRECT read of the
    /// binding qualifies: a nested projection (`cache[a][b] = v`) has an inner
    /// receiver that is itself an `Index`, which is not a `GlobalGet` and so
    /// falls through to the existing path and its blocker. That is deliberate —
    /// the inner read has to produce a value, and whether that value shares
    /// with the cell is the question `Place::Global` exists to avoid asking.
    fn mutable_global_receiver(&self, receiver: ExprId) -> Option<u32> {
        let expr = self.hir_expr(receiver).ok()?;
        match &expr.kind {
            ExprKind::GlobalGet { item } => self.global_ids.get(item).copied(),
            _ => None,
        }
    }

    /// Builds the rejection error for expression kinds that cannot form a place.
    ///
    /// Only [`ExprKind::Local`], [`ExprKind::Field`], [`ExprKind::Index`],
    /// [`ExprKind::TupleIndex`], and the transparent [`ExprKind::TypeAssert`] /
    /// [`ExprKind::UnknownCast`] wrappers lower to assignable places; every other
    /// kind is a value expression and is rejected here.
    ///
    /// The exhaustive `match` below is intentional: it has no wildcard arm, so
    /// adding a new `ExprKind` variant forces a compile error here and a decision
    /// about whether that variant can be an assignment target. This is the single
    /// place-lowering exhaustiveness site that [`Self::lower_place`] delegates to.
    pub(super) fn place_unsupported(&self, expr: &smelt_hir::Expr) -> LowerError {
        match &expr.kind {
            ExprKind::Local(_)
            | ExprKind::ThisRead
            | ExprKind::BindThis { .. }
            | ExprKind::Field { .. }
            | ExprKind::Index { .. }
            | ExprKind::TupleIndex { .. }
            | ExprKind::TypeAssert { .. }
            | ExprKind::UnknownCast { .. }
            | ExprKind::Literal(_)
            | ExprKind::Item(_)
            // Global reads/writes are values, not assignable places: a write to
            // a mutable global lowers to `Rvalue::GlobalSet`, never through here.
            | ExprKind::GlobalGet { .. }
            | ExprKind::GlobalSet { .. }
            | ExprKind::Call { .. }
            | ExprKind::ClosureCall { .. }
            | ExprKind::Construct { .. }
            | ExprKind::ClosureCallSpread { .. }
            | ExprKind::Method { .. }
            // `OptionalField` / `OptionalIndex` ARE assignable (see
            // `lower_place`); they reach this exhaustive listing only because it
            // enumerates every variant, and naming them here keeps that
            // enumeration complete.
            | ExprKind::OptionalField { .. }
            | ExprKind::OptionalIndex { .. }
            | ExprKind::OptionalMethod { .. }
            | ExprKind::OptionalCoalesce { .. }
            | ExprKind::Len { .. }
            | ExprKind::NumericAbs { .. }
            | ExprKind::NumericRound { .. }
            | ExprKind::NumericExtrema { .. }
            | ExprKind::NumericHypot { .. }
            | ExprKind::NumericPredicate { .. }
            | ExprKind::NumericUnaryFunc { .. }
            | ExprKind::NumericPow { .. }
            | ExprKind::NumericAtan2 { .. }
            | ExprKind::NumericRandom
            | ExprKind::NumericRandomInt { .. }
            | ExprKind::NumericToStringRadix { .. }
            | ExprKind::NumericToFixed { .. }
            | ExprKind::ParseIntRadix { .. }
            | ExprKind::PrimitiveCast { .. }
            | ExprKind::StringCase { .. }
            | ExprKind::StringNormalize { .. }
            | ExprKind::UriTranscode { .. }
            | ExprKind::ObjectToStringTag { .. }
            | ExprKind::StructuredClone { .. }
            | ExprKind::StringTrim { .. }
            | ExprKind::StringLocaleCompare { .. }
            | ExprKind::StringAffix { .. }
            | ExprKind::StringSearch { .. }
            | ExprKind::StringReplace { .. }
            | ExprKind::StringRemoveAffix { .. }
            | ExprKind::StringRepeat { .. }
            | ExprKind::StringPad { .. }
            | ExprKind::StringPredicate { .. }
            | ExprKind::RegexIsMatch { .. }
            | ExprKind::RegexReplace { .. }
            | ExprKind::RegexReplaceCallback { .. }
            | ExprKind::RegexReplaceFirstMatchUppercase { .. }
            | ExprKind::RegexSplit { .. }
            | ExprKind::RegexFind { .. }
            | ExprKind::EventEmitterNew
            | ExprKind::EventEmitterOp { .. }
            | ExprKind::HttpCreateServer { .. }
            | ExprKind::HttpServerOp { .. }
            | ExprKind::IncomingMessageOp { .. }
            | ExprKind::ServerResponseOp { .. }
            | ExprKind::RequestNew { .. }
            | ExprKind::RequestOp { .. }
            | ExprKind::ResponseNew { .. }
            | ExprKind::ResponseOp { .. }
            | ExprKind::TextEncoderNew
            | ExprKind::TextDecoderNew { .. }
            | ExprKind::TextEncoderOp { .. }
            | ExprKind::TextDecoderOp { .. }
            | ExprKind::ByteArrayOp { .. }
            | ExprKind::UrlSearchParamsNew { .. }
            | ExprKind::FormDataNew
            | ExprKind::UrlSearchParamsOp { .. }
            | ExprKind::FormDataOp { .. }
            | ExprKind::CryptoOp { .. }
            | ExprKind::AbortSignalOp { .. }
            | ExprKind::HeadersNew { .. }
            | ExprKind::HeadersOp { .. }
            | ExprKind::RegexExec { .. }
            | ExprKind::RegexMatchAll { .. }
            | ExprKind::StringCharAt { .. }
            | ExprKind::StringCharCodeAt { .. }
            | ExprKind::StringContains { .. }
            | ExprKind::StringSlice { .. }
            | ExprKind::ListContains { .. }
            | ExprKind::SetContains { .. }
            | ExprKind::SetDisjoint { .. }
            | ExprKind::SetRelation { .. }
            | ExprKind::SetAdd { .. }
            | ExprKind::SetRemove { .. }
            | ExprKind::SetClear { .. }
            | ExprKind::SetCopy { .. }
            | ExprKind::ListToSet { .. }
            | ExprKind::ListPairsToDict { .. }
            | ExprKind::SetBinary { .. }
            | ExprKind::SetProjection { .. }
            | ExprKind::ListConcat { .. }
            | ExprKind::ConcatSpread { .. }
            | ExprKind::ListSearch { .. }
            | ExprKind::ListCallback { .. }
            | ExprKind::ListFromLength { .. }
            | ExprKind::ListRepeat { .. }
            | ExprKind::ListFromLengthMap { .. }
            | ExprKind::ListReduce { .. }
            | ExprKind::ListSlice { .. }
            | ExprKind::ListSplice { .. }
            | ExprKind::ListFill { .. }
            | ExprKind::ListCopyWithin { .. }
            | ExprKind::ListWith { .. }
            | ExprKind::ListFlat { .. }
            | ExprKind::ListProjection { .. }
            | ExprKind::ListPush { .. }
            | ExprKind::GeneratorYield { .. }
            | ExprKind::GeneratorNext { .. }
            | ExprKind::GeneratorDone { .. }
            | ExprKind::GeneratorValue { .. }
            | ExprKind::GeneratorDelegate { .. }
            | ExprKind::ListExtend { .. }
            | ExprKind::ListInsert { .. }
            | ExprKind::ListUnshift { .. }
            | ExprKind::ListReverse { .. }
            | ExprKind::ListClear { .. }
            | ExprKind::ListCopy { .. }
            | ExprKind::TupleToList { .. }
            | ExprKind::ListToTuple { .. }
            | ExprKind::TupleToSet { .. }
            | ExprKind::ListCount { .. }
            | ExprKind::ListSum { .. }
            | ExprKind::ListBoolFold { .. }
            | ExprKind::ListSorted { .. }
            | ExprKind::ListReversed { .. }
            | ExprKind::ListEnumerate { .. }
            | ExprKind::ListZip { .. }
            | ExprKind::ListRange { .. }
            | ExprKind::ListRandomChoice { .. }
            | ExprKind::ListIndex { .. }
            | ExprKind::ListRemove { .. }
            | ExprKind::ListSort { .. }
            | ExprKind::ListPop { .. }
            | ExprKind::ListShift { .. }
            | ExprKind::ListNext { .. }
            | ExprKind::IteratorDone { .. }
            | ExprKind::IteratorValue { .. }
            | ExprKind::TupleContains { .. }
            | ExprKind::TupleSlice { .. }
            | ExprKind::DictContainsKey { .. }
            | ExprKind::DictSet { .. }
            | ExprKind::DictRemoveKey { .. }
            | ExprKind::DictGet { .. }
            | ExprKind::DictSetDefault { .. }
            | ExprKind::DictClear { .. }
            | ExprKind::DictPop { .. }
            | ExprKind::DictUpdate { .. }
            | ExprKind::DictAssign { .. }
            | ExprKind::CallableObjectAssign { .. }
            | ExprKind::DictCopy { .. }
            | ExprKind::DictProjection { .. }
            | ExprKind::StringSplit { .. }
            | ExprKind::StringChars { .. }
            | ExprKind::StringJoin { .. }
            | ExprKind::JsonStringify { .. }
            | ExprKind::JsonParse { .. }
            | ExprKind::HttpGetText { .. }
            | ExprKind::DateNow
            | ExprKind::DateSetNow { .. }
            | ExprKind::DateResetNow
            | ExprKind::VitestRestoreAllMocks
            | ExprKind::DateTimezoneOffset
            | ExprKind::DateSetTimezoneOffset { .. }
            | ExprKind::DateResetTimezoneOffset
            | ExprKind::VitestMockFn { .. }
            | ExprKind::VitestMockCalledTimes { .. }
            | ExprKind::VitestMockCalledWith { .. }
            | ExprKind::VitestSpyOn { .. }
            | ExprKind::VitestAsymmetricEqual { .. }
            | ExprKind::VitestMockLastResolvedWith { .. }
            | ExprKind::DateTimezoneContext { .. }
            | ExprKind::DateToIsoString { .. }
            | ExprKind::DateToString { .. }
            | ExprKind::DateFromParts { .. }
            | ExprKind::DateFromValue { .. }
            | ExprKind::DateGetPart { .. }
            | ExprKind::DateSetPart { .. }
            | ExprKind::UrlField { .. }
            | ExprKind::FileReadText { .. }
            | ExprKind::FileWriteText { .. }
            | ExprKind::BlobOp { .. }
            | ExprKind::BlobFromParts { .. }
            | ExprKind::HostConstruct { .. }
            | ExprKind::BuiltinNamespace { .. }
            | ExprKind::ArgumentsObject { .. }
            | ExprKind::HostGlobalRead { .. }
            | ExprKind::HostGlobalWrite { .. }
            | ExprKind::HostGlobalPresent { .. }
            | ExprKind::BinOp { .. }
            | ExprKind::UnaryOp { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::FunctionTableLookup { .. }
            | ExprKind::InstanceOf { .. }
            | ExprKind::InstanceOfValue { .. }
            | ExprKind::UnknownIs { .. }
            | ExprKind::TypeofValue { .. }
            | ExprKind::PrototypeSentinel { .. }
            | ExprKind::BoxPrimitive { .. }
            | ExprKind::ObjectFromPrototype { .. }
            | ExprKind::DefineProperties { .. }
            | ExprKind::Block(_)
            | ExprKind::Lambda { .. }
            | ExprKind::Closure(_)
            | ExprKind::ListLit(_)
            | ExprKind::SetLit(_)
            | ExprKind::DictLit(_)
            | ExprKind::TupleLit(_)
            | ExprKind::New { .. }
            | ExprKind::Await(_)
            | ExprKind::AsyncOp { .. } => self.error(
                format!(
                    "only local, field, and index expressions can be assigned, not `{}`",
                    Self::expr_kind_name(&expr.kind)
                ),
                Some(expr.span),
            ),
        }
    }

    /// The variant name of an `ExprKind`, for a diagnostic that has to say which
    /// expression it refused.
    ///
    /// `Debug` on an `ExprKind` prints the whole subtree, which is unreadable in
    /// a diagnostic, so this keeps just the leading variant name — enough to tell
    /// a reader (or the next round) which shape reached a rejection site.
    fn expr_kind_name(kind: &ExprKind) -> String {
        let rendered = format!("{kind:?}");
        match rendered.split_once(|ch: char| !ch.is_ascii_alphanumeric()) {
            Some((name, _)) => name.to_owned(),
            None => rendered,
        }
    }
}
