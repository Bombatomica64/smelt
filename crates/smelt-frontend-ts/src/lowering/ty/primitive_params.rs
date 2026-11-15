//! Function type parameters bounded by one primitive, read as that primitive.
//!
//! # The rule
//!
//! A type parameter of a function-like declaration whose constraint is
//! written as exactly one primitive keyword (`S extends string`,
//! `N extends number`, `B extends boolean`, `I extends bigint`) resolves, in
//! that declaration, to the primitive itself: `function shout<S extends
//! string>(text: S): S` lowers as `(text: string) => string`.
//!
//! This is what a hand port writes. TypeScript uses such a parameter to carry
//! a LITERAL type (`"a"`, `42`) through the signature; Rust has no literal
//! types, so every instantiation is the same Rust type, and a generic
//! parameter would have nothing to range over. Without the rule the
//! parameter reached Rust as the erased value, because codegen lifts only
//! unbounded parameters and class- or interface-bounded ones.
//!
//! # Requirements
//!
//! - Behaviour: unchanged. Every value a caller may pass for the parameter is
//!   a value of the primitive, because the constraint says so, and every
//!   operation the body may apply to it is one the primitive admits.
//! - Type preservation: a value of the parameter's type is the primitive
//!   (`JsString`, `f64`, `bool`, a bigint) instead of the erased value. The
//!   literal type TypeScript tracks through it has no Rust counterpart and is
//!   widened to the primitive, as it is everywhere else in the lowering.
//!
//! # Scope
//!
//! Only a constraint WRITTEN as the bare keyword qualifies, judged on the
//! syntax, not on what the constraint lowers to: `K extends keyof T` and
//! `K extends PropertyKey` may lower to a string-like type but stand for
//! property keys (numbers and symbols included), and a union of primitives
//! (`string | number`) has no single Rust type. Those keep the ordinary
//! scope. Closures and class methods already read every bounded parameter as
//! its bound (`push_closure_type_parameter_scope`).
//!
//! The parameter stays declared on the item (its position still receives an
//! explicit type argument at a call); it simply no longer occurs in any type.

use std::collections::HashMap;

use oxc::ast::ast::{TSType, TSTypeParameterDeclaration};
use smelt_hir::{TypeId, TypeParamDef};

use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// The parameters of `params` bounded by one primitive keyword, by source
    /// name, with the lowered primitive (`defs` are `params` lowered in the
    /// ordinary scope, in order).
    pub(in crate::lowering) fn primitive_bounded_params(
        params: &TSTypeParameterDeclaration<'_>,
        defs: &[TypeParamDef],
    ) -> HashMap<String, TypeId> {
        params
            .params
            .iter()
            .zip(defs)
            .filter(|(param, _)| {
                matches!(
                    param.constraint,
                    Some(
                        TSType::TSStringKeyword(_)
                            | TSType::TSNumberKeyword(_)
                            | TSType::TSBooleanKeyword(_)
                            | TSType::TSBigIntKeyword(_)
                    )
                )
            })
            .filter_map(|(param, def)| Some((param.name.name.to_string(), def.constraint?)))
            .collect()
    }
}
