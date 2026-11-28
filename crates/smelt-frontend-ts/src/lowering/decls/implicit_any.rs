//! Implicit type parameters for `any` parameters of module functions.
//!
//! A hand-written port of `function forward(x: any, label: string) { return
//! sink(x, label); }` would not take the erased carrier: the body only hands
//! `x` on, so the port writes `fn forward<A>(x: A, ..)` and every caller keeps
//! its value's type up to the place where the value really meets a dynamic
//! boundary. This module is the frontend half of that rule
//! (`docs/design/any-as-generic.md`): it declares, for each candidate
//! parameter, a fresh type parameter whose [`TypeParamDef::implicit_param`]
//! names the parameter. Nothing else changes: the parameter's local, the
//! item's signature and every expression keep the erased type the checker
//! gave them, so MIR lowering and every caller see the program they saw
//! before. Codegen (`smelt_codegen_rust::settle_implicit_generics`) retypes
//! the local to the type parameter after MIR lowering where the body passes
//! the value along, and drops the declaration everywhere else. A read of the
//! parameter is then a move of a `T` into an erased temporary, which crosses
//! the `IntoSmeltUnknown` boundary every lifted parameter carries.

use oxc::ast::ast::{BindingPattern, FormalParameter, Function, TSType};
use smelt_hir::{Param, Type, TypeParamDef};

use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Declare a fresh implicit type parameter for every candidate `any`
    /// parameter of the module function `function`, returning the new
    /// parameters to append to `declared`.
    ///
    /// A candidate is a required, non-rest parameter bound to a plain name
    /// whose annotation is exactly `any` and whose lowered type is still the
    /// erased carrier (an `as any` object's unchecked parameters, an overload's
    /// widened slot and a defaulted slot are not). A generator or a function
    /// that reads `arguments` keeps its erased parameters: its parameter list
    /// is rebuilt from the actual arguments. The `explicit-any` records made
    /// at a candidate's annotation are retracted; codegen records the erasure
    /// again where the parameter stays erased.
    pub(in crate::lowering) fn implicit_any_type_params(
        &mut self,
        function: &Function<'_>,
        declared: &[TypeParamDef],
        params: &[Param],
        rebuilt_params: bool,
    ) -> Vec<TypeParamDef> {
        if function.generator || rebuilt_params {
            return Vec::new();
        }
        let mut implicit = Vec::new();
        let mut taken: Vec<String> = declared
            .iter()
            .filter_map(|param| self.ctx.krate.symbols.get(param.name).map(str::to_owned))
            .collect();
        for (index, formal) in function.params.items.iter().enumerate() {
            let Some(annotation_span) = Self::plain_any_annotation(formal) else {
                continue;
            };
            let Some(param) = params.get(index) else {
                continue;
            };
            if !matches!(self.ctx.krate.types.get(param.ty), Some(Type::Unknown)) {
                continue;
            }
            let mut counter = implicit.len();
            let name_text = loop {
                let candidate = format!("Any{counter}");
                if !taken.contains(&candidate) {
                    break candidate;
                }
                counter += 1;
            };
            taken.push(name_text.clone());
            let name = self.ctx.krate.symbols.intern(&name_text);
            let span = self.span(annotation_span.start, annotation_span.end);
            self.ctx.krate.erasures.retain(|record| {
                !(record.reason == smelt_hir::ErasureReason::ExplicitAny && record.span == span)
            });
            implicit.push(TypeParamDef {
                name,
                constraint: None,
                default: None,
                span,
                implicit_param: Some(index),
            });
        }
        implicit
    }

    /// The span of `formal`'s annotation when the parameter is a required,
    /// undefaulted plain name annotated exactly `any`.
    fn plain_any_annotation(formal: &FormalParameter<'_>) -> Option<oxc::span::Span> {
        if formal.optional || formal.initializer.is_some() {
            return None;
        }
        if !matches!(formal.pattern, BindingPattern::BindingIdentifier(_)) {
            return None;
        }
        let annotation = formal.type_annotation.as_ref()?;
        match &annotation.type_annotation {
            TSType::TSAnyKeyword(keyword) => Some(keyword.span),
            _ => None,
        }
    }
}
