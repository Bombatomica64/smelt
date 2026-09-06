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

use crate::{Constant, Operand, Place};

use super::LowerError;
use super::context::LoweringCtx;

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
    pub(super) fn lower_place(&mut self, expr_id: ExprId) -> Result<Place, LowerError> {
        let expr = self.hir_expr(expr_id)?.clone();
        match &expr.kind {
            ExprKind::Local(local) => {
                let local_id = self.locals.get(local).copied().ok_or_else(|| {
                    self.error("assignment references an unknown local", Some(expr.span))
                })?;
                Ok(Place::Local(local_id))
            }
            ExprKind::Field { receiver, field } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                Ok(Place::Field {
                    base,
                    field: *field,
                })
            }
            ExprKind::Index { receiver, index } => {
                let receiver_operand = self.lower_expr(*receiver)?;
                let receiver_ty = self.hir_expr(*receiver)?.ty;
                let base =
                    self.materialize_operand_local(receiver_operand, receiver_ty, expr.span)?;
                let index_operand = self.lower_expr(*index)?;
                Ok(Place::Index {
                    base,
                    index: Box::new(index_operand),
                    negative: self.negative_index_policy(expr.span),
                })
            }
            ExprKind::TupleIndex { tuple, index } => {
                let tuple_operand = self.lower_expr(*tuple)?;
                let tuple_ty = self.hir_expr(*tuple)?.ty;
                let base = self.materialize_operand_local(tuple_operand, tuple_ty, expr.span)?;
                let tuple_index = i64::try_from(*index).map_err(|_error| {
                    self.error("tuple index does not fit in MIR integer", Some(expr.span))
                })?;
                Ok(Place::Index {
                    base,
                    index: Box::new(Operand::Const(Constant::Int(tuple_index))),
                    // A tuple index is a resolved, non-negative position; the
                    // policy never applies, so record the language's anyway.
                    negative: self.negative_index_policy(expr.span),
                })
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
            | ExprKind::RequestNew { .. }
            | ExprKind::RequestOp { .. }
            | ExprKind::ResponseNew { .. }
            | ExprKind::ResponseOp { .. }
            | ExprKind::UrlSearchParamsNew { .. }
            | ExprKind::UrlSearchParamsOp { .. }
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
                "only local, field, and index expressions can be assigned",
                Some(expr.span),
            ),
        }
    }
}
