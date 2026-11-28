//! Recording the erasures the Rust backend decides itself.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), `src/sema/aot_rules.rs`:
//! the reason an erased value exists is named where the erasure is decided.
//! Most erasure is decided by the frontends (recorded in
//! `smelt_hir::Crate::erasures`); five causes are decided only here, by
//! [`FunctionEmitter::rust_type`]:
//!
//! - `Type::Never` rendered as `SmeltUnknown` ([`ErasureReason::NeverValue`]);
//! - a `Type::IndexedAccess` the frontend left unevaluated
//!   ([`ErasureReason::TypeLevel`]); none is expected, the frontend erases
//!   it itself wherever a value could carry it;
//! - a type parameter whose substitution resolves to erased
//!   ([`ErasureReason::ErasedGeneric`]);
//! - a `Type::Class` that names no lowered class or interface
//!   ([`ErasureReason::UnresolvedClass`]);
//! - a `Type::Union` that is not a concrete generated union
//!   ([`ErasureReason::ErasedUnion`]).
//!
//! # Span granularity
//!
//! `rust_type` renders a `TypeId`, not a local, so it does not know which
//! source value it is rendering. The record is attributed to the function
//! being emitted: the span of its entry block, which MIR lowering sets to the
//! function body. The detail names the erased construct (the type parameter,
//! the class, the union's arm count). Trial renders (a generic function's
//! body-cleanliness probe) can record the same decision twice; the consumer
//! deduplicates by span, reason and detail with
//! [`smelt_hir::normalize_erasures`].

use smelt_hir::{ErasureReason, ErasureRecord, Span, Symbol};

use crate::type_substitution::TypeSubstitution;

use super::{EmitContext, FunctionEmitter};

impl EmitContext {
    /// Take every backend erasure recorded so far.
    pub(crate) fn take_backend_erasures(&self) -> Vec<ErasureRecord> {
        std::mem::take(&mut *self.minted.backend_erasures.borrow_mut())
    }
}

impl FunctionEmitter<'_> {
    /// Record that `rust_type` erased a value of the function being emitted
    /// for `reason`; see the module docs for the span used.
    pub(super) fn note_backend_erasure(&self, reason: ErasureReason, detail: String) {
        let span = self.backend_erasure_span();
        self.context
            .minted
            .backend_erasures
            .borrow_mut()
            .push(ErasureRecord::new(reason, span, detail));
    }

    /// Record an erasure the backend decided for `reason` at an explicit
    /// `span` (an implicit type parameter's `any` annotation).
    pub(super) fn note_erasure_at(&self, reason: ErasureReason, span: Span, detail: String) {
        self.context
            .minted
            .backend_erasures
            .borrow_mut()
            .push(ErasureRecord::new(reason, span, detail));
    }

    /// The span a backend erasure in this function is attributed to: the
    /// entry block's span (the function body), or a zero span in the first
    /// file when the function has no blocks.
    fn backend_erasure_span(&self) -> Span {
        self.function
            .blocks
            .iter()
            .find(|block| block.id == self.function.entry)
            .or_else(|| self.function.blocks.first())
            .map_or(Span::new(smelt_hir::FileId(0), 0, 0), |block| block.span)
    }

    /// The detail of an `erased-generic` record for the type parameter
    /// `name`: the parameter, whose it is, and why no Rust generic carries it
    /// here (the demotion-reason report, `docs/erasure-reasons.md`).
    ///
    /// - The emitted function's own parameter: named with the function and
    ///   the lift gate's refusal ([`EmitContext::type_param_refusal`]), or
    ///   the body-cleanliness trial when the gate itself lifted it.
    /// - A callee's parameter at a call site: the binding matcher left it
    ///   unpinned, so the callee is rendered erased there.
    /// - Inside a closure: the enclosing function's parameter, which that
    ///   function does not lift (its own records name why).
    /// - Anything else: another function's parameter reaching this body
    ///   unbound, through a demoted callee's argument or result type.
    pub(super) fn erased_generic_detail(
        &self,
        name: Symbol,
        substitution: &TypeSubstitution<'_>,
    ) -> String {
        let param = self.symbol_name(name).unwrap_or("?");
        let owns = self
            .function
            .type_params
            .iter()
            .any(|type_param| type_param.name == name);
        if owns {
            let function = self
                .mir
                .symbols
                .get(self.function.name)
                .unwrap_or("<anonymous>");
            let reason = self
                .context
                .type_param_refusal(self.mir, self.function, name)
                .unwrap_or("the body-cleanliness trial demoted the function");
            return format!("{param} of {function}: {reason}");
        }
        if substitution.has_bindings() {
            return format!("{param}: a call site left the callee's parameter unpinned");
        }
        if matches!(self.role, super::emit_role::EmitRole::Closure(_)) {
            return format!("{param}: in a closure of a function that does not lift it");
        }
        if let Some(callee) = self.callee_declaring(name) {
            return format!("{param} of callee {callee}: unbound at this caller");
        }
        format!("{param}: another function's parameter (a demoted callee's result or argument)")
    }

    /// The one function this body calls statically that declares a type
    /// parameter named `name`, by name; `None` when no callee or several do
    /// (type-parameter symbols are interned by spelling, so `T` alone cannot
    /// say whose it is).
    fn callee_declaring(&self, name: Symbol) -> Option<&str> {
        let mut found: Option<smelt_mir::FuncId> = None;
        for block in &self.function.blocks {
            let Some(smelt_mir::Terminator::Call {
                callee: smelt_mir::Callee::Static(target),
                ..
            }) = &block.terminator
            else {
                continue;
            };
            let declares = self.mir.function(*target).is_some_and(|callee| {
                callee
                    .type_params
                    .iter()
                    .any(|type_param| type_param.name == name)
            });
            match found {
                _ if !declares => {}
                None => found = Some(*target),
                Some(previous) if previous == *target => {}
                Some(_) => return None,
            }
        }
        self.mir
            .function(found?)
            .and_then(|callee| self.mir.symbols.get(callee.name))
    }
}
