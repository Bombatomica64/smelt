//! WHATWG `FormData` lowering.
//!
//! `FormData` is the third of the ordered name/value pair lists Smelt models,
//! after `Headers` and `URLSearchParams`, and it is deliberately the same shape.
//! Two things separate it, and both live in the TYPES rather than in the
//! dispatch:
//!
//! 1. **Names are case-SENSITIVE.** A header list lower-cases and joins
//!    duplicates; a form's `"file"` and `"File"` are two different names.
//! 2. **An entry's value is `string | File`.** Both arms are MODELED types, so
//!    nothing here has to be erased — which is exactly why this belongs after
//!    the `Blob` work. Before `Blob` was concrete the file arm would have had
//!    to be a `SmeltUnknown`, and every `get` would have crossed the dynamic
//!    boundary.
//!
//! So `get` answers `Optional<String | File>` and `getAll` a
//! `List<String | File>` where the parameter list answers plain strings. That
//! union is a real two-arm generated enum, not a boundary.
//!
//! The spec's `append`/`set` take an optional third `filename` argument, which
//! REPLACES the blob's name. That falls out for free from modeling a `File` as
//! a blob whose name is present (see `StdlibClass::File`).

use oxc::ast::ast::Expression;
use smelt_hir::{Body, Expr, ExprKind, Type};
use smelt_stdlib::RuleId;

use crate::error::SmeltError;
use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Dispatch a modeled `FormData` method on a concrete receiver.
    ///
    /// Registered in the builtin call-handler chain next to the `Headers` and
    /// `URLSearchParams` dispatches, and recognized the same way: the shared
    /// registry names the receiver/member pairs, and the receiver's lowered type
    /// decides.
    ///
    /// Declines through `receiver_class_hint` BEFORE lowering the receiver where
    /// the receiver's class is knowable from the binding, because `get`, `set`,
    /// `has`, `delete`, `keys`, `values`, `entries` and `forEach` are members of
    /// several modeled receivers and a handler that lowers first leaves a
    /// duplicate expression behind on a miss.
    pub(in crate::lowering) fn dispatch_form_data_method(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let member_name = member.property.name.as_str();
        let Some(rule) = smelt_stdlib::typescript_method_rule(
            smelt_stdlib::TypeScriptReceiverKind::FormData,
            member_name,
        ) else {
            return Ok(None);
        };
        // Decline before lowering when the receiver's class is knowable and is
        // not a form.
        if let Some(hint) = self.receiver_class_hint(&member.object, body)
            && hint != smelt_stdlib::StdlibClass::FormData
        {
            return Ok(None);
        }
        let Ok(receiver) = self.expression(&member.object, body) else {
            return Ok(None);
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        if !self.is_form_data_type(receiver_ty) {
            return Ok(None);
        }
        let Some(op) = Self::form_data_method_op(rule, member_name) else {
            return Ok(None);
        };
        let required = Self::form_data_op_required_arity(op);
        if call.arguments.len() < required {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("`FormData.{member_name}` requires {required} argument(s)"),
            ));
        }
        // A CALLBACK member's argument is lowered with the parameter types the
        // modeled surface declares, not as an ordinary value. Without them the
        // arrow's parameters have no type, so `formData.forEach((value, key) =>
        // key.endsWith('[]'))` — Hono's `convertFormDataToBodyData` — reported
        // "string prefix/suffix methods require string receiver and argument"
        // for a `key` the surface has always known to be a `string`. The types
        // come from `form_data_op_callback_param_types`, beside the result
        // types, so a member's callback shape and its result cannot disagree.
        let args = if let Some(param_tys) = self.form_data_op_callback_param_types(op) {
            let Some(argument) = call.arguments.first() else {
                return Ok(None);
            };
            let callback =
                self.callback_argument(argument, &param_tys, "FormData.forEach", body)?;
            Vec::from([callback.expr])
        } else {
            // `append`/`set` accept an OPTIONAL third `filename`, so the accepted
            // count is a range rather than a fixed arity. Extra arguments beyond it
            // are dropped, as they are for every other modeled member.
            let accepted = Self::form_data_op_max_arity(op).min(call.arguments.len());
            call.arguments
                .iter()
                .take(accepted)
                .map(|argument| self.argument(argument, body))
                .collect::<Result<Vec<_>, _>>()?
        };
        let ty = self.form_data_op_result_type(op);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::FormDataOp {
                op,
                form: receiver,
                args,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return the modeled `FormData` class type.
    pub(in crate::lowering) fn form_data_type(&mut self) -> smelt_hir::TypeId {
        let name = self.intern_type_name("FormData");
        self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })
    }

    /// Return whether a lowered type is the modeled `FormData` class.
    pub(in crate::lowering) fn is_form_data_type(&self, ty: smelt_hir::TypeId) -> bool {
        self.stdlib_class_of_type(ty) == Some(smelt_stdlib::StdlibClass::FormData)
            && !self.user_class_shadows("FormData")
    }

    /// The `string | File` type of a form entry's VALUE.
    ///
    /// A real two-arm union of modeled types. Interned in one place so the
    /// result types of `get`, `getAll`, `values`, `entries` and `forEach` cannot
    /// disagree about what a form holds — they are all the same union, and a
    /// consumer that matches one arm can match them all.
    pub(in crate::lowering) fn form_data_value_type(&mut self) -> smelt_hir::TypeId {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let file_name = self.intern_type_name("File");
        let file_ty = self.ctx.krate.types.intern(Type::Class {
            name: file_name,
            args: Vec::new(),
        });
        self.ctx
            .krate
            .types
            .intern(Type::Union(Vec::from([string_ty, file_ty])))
    }

    /// Map a recognized rule and member spelling to its form operation.
    fn form_data_method_op(rule: RuleId, member: &str) -> Option<smelt_hir::FormDataOp> {
        use smelt_hir::FormDataOp as Op;
        match rule {
            RuleId::TsFormDataRead => match member {
                "get" => Some(Op::Get),
                "getAll" => Some(Op::GetAll),
                "has" => Some(Op::Has),
                _ => None,
            },
            RuleId::TsFormDataMutation => match member {
                "set" => Some(Op::Set),
                "append" => Some(Op::Append),
                "delete" => Some(Op::Delete),
                _ => None,
            },
            RuleId::TsFormDataProjection => match member {
                "keys" => Some(Op::Keys),
                "values" => Some(Op::Values),
                "entries" => Some(Op::Entries),
                "forEach" => Some(Op::ForEach),
                _ => None,
            },
            _ => None,
        }
    }

    /// How many source arguments an operation REQUIRES.
    const fn form_data_op_required_arity(op: smelt_hir::FormDataOp) -> usize {
        use smelt_hir::FormDataOp as Op;
        match op {
            Op::Get | Op::GetAll | Op::Has | Op::Delete | Op::ForEach => 1,
            Op::Set | Op::Append => 2,
            Op::Keys | Op::Values | Op::Entries => 0,
        }
    }

    /// How many source arguments an operation CONSUMES at most.
    ///
    /// Differs from the required count only for `append`/`set`, whose optional
    /// third argument is the spec's `filename`.
    const fn form_data_op_max_arity(op: smelt_hir::FormDataOp) -> usize {
        use smelt_hir::FormDataOp as Op;
        match op {
            Op::Set | Op::Append => 3,
            other => Self::form_data_op_required_arity(other),
        }
    }

    /// The parameter types a form operation's CALLBACK argument declares.
    ///
    /// `None` for every member that takes plain values. `forEach` is the one
    /// callback member on this surface, and the spec calls back with
    /// `(value, key, form)` — the value being the same `string | File` union
    /// every other member answers, so a consumer that matches one arm matches
    /// them all.
    ///
    /// Returning the types here rather than at the call site is what lets the
    /// callback's own parameters be typed before its body is lowered: a
    /// parameter with no type erases, and an erased `key` cannot answer
    /// `key.endsWith('[]')`.
    fn form_data_op_callback_param_types(
        &mut self,
        op: smelt_hir::FormDataOp,
    ) -> Option<Vec<smelt_hir::TypeId>> {
        if !matches!(op, smelt_hir::FormDataOp::ForEach) {
            return None;
        }
        let value_ty = self.form_data_value_type();
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let form_ty = self.form_data_type();
        Some(Vec::from([value_ty, string_ty, form_ty]))
    }

    /// Return the exact source result type of a form operation.
    fn form_data_op_result_type(&mut self, op: smelt_hir::FormDataOp) -> smelt_hir::TypeId {
        use smelt_hir::FormDataOp as Op;
        let value_ty = self.form_data_value_type();
        let string_ty = self.ctx.krate.types.intern(Type::String);
        match op {
            Op::Get => self.ctx.krate.types.intern(Type::Optional(value_ty)),
            Op::Has => self.ctx.krate.types.intern(Type::Bool),
            Op::Set | Op::Append | Op::Delete | Op::ForEach => {
                self.ctx.krate.types.intern(Type::None)
            }
            // `keys()` answers one name PER ENTRY, so a duplicated name appears
            // once per entry — the spec's iterator walks the entry list rather
            // than a set of names.
            Op::Keys => self.ctx.krate.types.intern(Type::List(string_ty)),
            Op::GetAll | Op::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            Op::Entries => {
                let pair_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(Vec::from([string_ty, value_ty])));
                self.ctx.krate.types.intern(Type::List(pair_ty))
            }
        }
    }
}
