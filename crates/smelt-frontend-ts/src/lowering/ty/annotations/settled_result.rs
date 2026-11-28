//! The lib's `Promise.allSettled` entry types, as concrete records.
//!
//! `lib.es2020.promise.d.ts` declares
//!
//! ```ts
//! interface PromiseFulfilledResult<T> { status: "fulfilled"; value: T; }
//! interface PromiseRejectedResult { status: "rejected"; reason: any; }
//! type PromiseSettledResult<T> = PromiseFulfilledResult<T> | PromiseRejectedResult;
//! ```
//!
//! Lib declarations are not lowered, so these names reached codegen as classes
//! nothing declares and every settled entry erased to `SmeltUnknown`
//! (`unresolved-class`), its `value` read through the erased object. A hand
//! port writes the two structs and the tagged enum over them; this module
//! synthesizes exactly those declarations, the way an anonymous record type is
//! synthesized (`decls::anonymous_record`). Each `status` field records its
//! literal as a decode fact, so the union is discriminated by it like any
//! source-declared tagged union, and `value` keeps the input's type `T`.
//!
//! `reason` is the one dynamic field: it holds whatever was thrown, which can
//! be any value (the runtime carries it as the thrown payload,
//! `smelt_thrown_value`). It is modeled as `unknown`, TypeScript's own type for
//! a caught value under `useUnknownInCatchVariables`, a genuine dynamic
//! boundary rather than the lib's `any`.
//!
//! A source declaration of any of the three names wins, as for every lib name
//! `type_references` models.

use smelt_hir::{
    ErasureReason, Field, FieldDecodeFacts, Literal, Type, TypeId, TypeParamDef, Visibility,
};

use crate::lowering::ModuleBuilder;
use crate::lowering::decls::types_iface::StructuralMembers;

/// The lib name of the fulfilled entry interface.
const FULFILLED: &str = "PromiseFulfilledResult";
/// The lib name of the rejected entry interface.
const REJECTED: &str = "PromiseRejectedResult";

impl ModuleBuilder<'_> {
    /// `PromiseSettledResult<value>`: the union of the fulfilled entry at
    /// `value` and the rejected entry.
    pub(in crate::lowering) fn settled_result_type(&mut self, value: TypeId) -> TypeId {
        let fulfilled = self.fulfilled_result_type(value);
        let rejected = self.rejected_result_type();
        self.ctx
            .krate
            .types
            .intern(Type::Union(vec![fulfilled, rejected]))
    }

    /// `PromiseFulfilledResult<value>`: `{ status: "fulfilled"; value: T }` at
    /// `T = value`.
    pub(in crate::lowering) fn fulfilled_result_type(&mut self, value: TypeId) -> TypeId {
        let name = self.intern_type_name(FULFILLED);
        if self.find_interface(name).is_none() {
            let span = self.span(0, 0);
            let param = self.intern_type_name("T");
            let param_ty = self.ctx.krate.types.intern(Type::TypeParam { name: param });
            let value_field = self.intern_source_name("value");
            let fields = vec![
                self.status_field(span),
                Self::public_field(value_field, param_ty, span),
            ];
            let decode_facts = vec![self.status_literal_fact("fulfilled")];
            let type_params = vec![TypeParamDef {
                name: param,
                constraint: None,
                default: None,
                span,
                implicit_param: None,
            }];
            self.register_settled_interface(name, FULFILLED, type_params, fields, decode_facts);
        }
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: vec![value],
        })
    }

    /// `PromiseRejectedResult`: `{ status: "rejected"; reason: unknown }`.
    pub(in crate::lowering) fn rejected_result_type(&mut self) -> TypeId {
        let name = self.intern_type_name(REJECTED);
        if self.find_interface(name).is_none() {
            let span = self.span(0, 0);
            let reason_field = self.intern_source_name("reason");
            // A thrown value is any value: the genuine dynamic boundary of the
            // record (see the module docs).
            let reason_ty = self.erase(ErasureReason::SourceUnknown);
            let fields = vec![
                self.status_field(span),
                Self::public_field(reason_field, reason_ty, span),
            ];
            let decode_facts = vec![self.status_literal_fact("rejected")];
            self.register_settled_interface(name, REJECTED, Vec::new(), fields, decode_facts);
        }
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// The `status` field: a string whose literal is in the decode facts.
    fn status_field(&mut self, span: smelt_hir::Span) -> Field {
        let status = self.intern_source_name("status");
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Self::public_field(status, string_ty, span)
    }

    /// The decode fact pinning `status` to `literal`.
    fn status_literal_fact(&mut self, literal: &str) -> FieldDecodeFacts {
        FieldDecodeFacts {
            name: self.intern_source_name("status"),
            may_be_absent: false,
            admits_null: false,
            literals: vec![Literal::String(literal.to_owned())],
        }
    }

    /// A required public field.
    const fn public_field(name: smelt_hir::Symbol, ty: TypeId, span: smelt_hir::Span) -> Field {
        Field {
            name,
            ty,
            visibility: Visibility::Public,
            optional: false,
            span,
        }
    }

    /// Declare one of the entry interfaces, as a synthesized record is.
    fn register_settled_interface(
        &mut self,
        name: smelt_hir::Symbol,
        name_text: &str,
        type_params: Vec<TypeParamDef>,
        fields: Vec<Field>,
        decode_facts: Vec<FieldDecodeFacts>,
    ) {
        let span = self.span(0, 0);
        let members = StructuralMembers {
            fields,
            decode_facts,
            ..Default::default()
        };
        let item =
            self.register_callable_object_interface(name, name_text, type_params, members, span);
        self.items.insert(name_text.to_owned(), item);
    }
}
