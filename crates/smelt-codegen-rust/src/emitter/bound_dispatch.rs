//! Rendering of bound traits and of the member reads that dispatch through
//! them (`Rvalue::BoundMethod`, `Rvalue::BoundField`).
//!
//! [`crate::bound_traits`] plans, from MIR alone, which trait each class or
//! interface bound gets. This module turns a plan into Rust:
//!
//! * [`emit_bound_traits`] writes `trait AnimalLike { .. }` and one `impl` per
//!   implementor, each member delegating to what a call on a value of that
//!   static type already does: the class's own (or inherited) method, the
//!   function stored in a virtual slot or interface method property, or a
//!   stored field read;
//! * [`FunctionEmitter::bound_method_expr`] / [`FunctionEmitter::bound_field_expr`]
//!   render one member access. When the receiver's type parameter is a real
//!   Rust generic in scope the access goes through the trait (`a.speak()`);
//!   otherwise the parameter erased at this function — no trait, a call site
//!   that passes an erased value, a demoted trial — and the access is the
//!   dynamic member lookup on the erased carrier, the same lookup the frontend
//!   produced before bounds were typed, now converted to the bound's declared
//!   result type.

use crate::rust::RustExpr;
use smelt_hir::{Symbol, Type, TypeId};
use smelt_mir::{Mir, Operand};

use super::{EmitContext, EmitError, FunctionEmitter};
use crate::bound_traits::{BoundMemberKind, BoundTrait, MemberImpl};
use crate::rust::RustIdent;
use crate::type_substitution::TypeSubstitution;

/// The error type every generated `Result` carries.
const ERROR_TYPE: &str = crate::thrown::ERROR_BOX;

/// Write every planned bound trait and its implementations.
///
/// Emitted at crate level next to the class `impl` blocks, so the inherent
/// methods each `impl` delegates to are in the same module.
pub(crate) fn emit_bound_traits(mir: &Mir, context: &EmitContext) -> Result<String, EmitError> {
    let mut out = String::new();
    for (_, planned) in context.bound_traits.traits() {
        out.push_str(&trait_text(mir, context, planned)?);
    }
    Ok(out)
}

/// Render a type at crate level, outside any generic scope.
fn crate_type_text(mir: &Mir, context: &EmitContext, ty: TypeId) -> Result<String, EmitError> {
    FunctionEmitter::type_text_for_with_scoped_type_params(
        mir,
        context,
        ty,
        &TypeSubstitution::lexical(&std::collections::HashSet::new()),
    )
}

/// The `fn name(&self, ..) -> R` signature of one trait member.
fn member_signature(
    mir: &Mir,
    context: &EmitContext,
    member: &crate::bound_traits::BoundMember,
) -> Result<String, EmitError> {
    match &member.kind {
        BoundMemberKind::Method { params, ret, throws } => {
            let mut parts = vec!["&self".to_owned()];
            for (index, param) in params.iter().enumerate() {
                parts.push(format!("smelt_arg_{index}: {}", crate_type_text(mir, context, *param)?));
            }
            let value_text = crate_type_text(mir, context, *ret)?;
            let ret_text = if *throws {
                format!("Result<{value_text}, {ERROR_TYPE}>")
            } else {
                value_text
            };
            Ok(format!("fn {}({}) -> {ret_text}", member.rust_name, parts.join(", ")))
        }
        BoundMemberKind::Field { ty } => Ok(format!(
            "fn {}(&self) -> {}",
            member.rust_name,
            crate_type_text(mir, context, *ty)?
        )),
    }
}

/// The trait declaration and every implementation of one planned trait.
fn trait_text(mir: &Mir, context: &EmitContext, planned: &BoundTrait) -> Result<String, EmitError> {
    let mut out = format!(
        "\n/// What generic code reads through the `{}` bound (bounded-generics plan).\ntrait {} {{\n",
        planned
            .implementors
            .first()
            .and_then(|bound| mir.symbols.get(bound.name))
            .unwrap_or("?"),
        planned.trait_name
    );
    for member in &planned.members {
        out.push_str(&format!("    {};\n", member_signature(mir, context, member)?));
    }
    out.push_str("}\n");
    for implementor in &planned.implementors {
        let type_name = RustIdent::new(
            mir.symbols
                .get(implementor.name)
                .ok_or_else(|| EmitError::new("bound implementor has unknown symbol"))?,
        )
        .into_string();
        let reference = context.is_reference_class(implementor.name);
        out.push_str(&format!("impl {} for {type_name} {{\n", planned.trait_name));
        for (member, how) in planned.members.iter().zip(&implementor.members) {
            let body = member_body(context, member, how, reference)?;
            out.push_str(&format!(
                "    {} {{ {body} }}\n",
                member_signature(mir, context, member)?
            ));
        }
        out.push_str("}\n");
    }
    Ok(out)
}

/// The body of one member in one implementor's `impl`.
///
/// `reference` selects the handle representation (`self.0.borrow().x`) over the
/// by-value struct (`self.x`). A stored function is cloned out of the cell
/// before it is called, so the call may borrow the instance again.
fn member_body(
    context: &EmitContext,
    member: &crate::bound_traits::BoundMember,
    how: &MemberImpl,
    reference: bool,
) -> Result<String, EmitError> {
    let field_access = |name: &str| {
        if reference {
            format!("self.0.borrow().{name}.clone()")
        } else {
            format!("self.{name}.clone()")
        }
    };
    let (args, trait_throws) = match &member.kind {
        BoundMemberKind::Method { params, throws, .. } => (
            (0..params.len())
                .map(|index| format!("smelt_arg_{index}"))
                .collect::<Vec<_>>()
                .join(", "),
            *throws,
        ),
        BoundMemberKind::Field { .. } => (String::new(), false),
    };
    let wrap = |call: String, impl_throws: bool| {
        if trait_throws && !impl_throws {
            format!("Ok({call})")
        } else {
            call
        }
    };
    Ok(match how {
        MemberImpl::Field => field_access(&member.rust_name),
        MemberImpl::FunctionField { throws } => wrap(
            format!(
                "let smelt_callable = {}; (smelt_callable)({args})",
                field_access(&member.rust_name)
            ),
            *throws,
        ),
        MemberImpl::Method { function, throws } => {
            let rust_name = context
                .function_names
                .get(function)
                .cloned()
                .unwrap_or_else(|| member.rust_name.clone());
            let receiver_and_args = if args.is_empty() {
                "self".to_owned()
            } else {
                format!("self, {args}")
            };
            wrap(format!("Self::{rust_name}({receiver_and_args})"), *throws)
        }
    })
}

impl FunctionEmitter<'_> {
    /// The bound trait member a receiver of type parameter `receiver_ty`
    /// dispatches `member` through, when that parameter is a real generic in
    /// this function; `None` when the access must take the erased path.
    fn bound_trait_member(
        &self,
        receiver_ty: TypeId,
        bound: Symbol,
        member: Symbol,
    ) -> Option<&crate::bound_traits::BoundMember> {
        let Some(Type::TypeParam { name }) = self.mir.types.get(receiver_ty) else {
            return None;
        };
        if !self.current_function_type_params().contains(name) {
            return None;
        }
        self.context.bound_traits.trait_for(bound)?.member(member)
    }

    /// Render `receiver.method(args)` where the receiver's type is a bounded
    /// type parameter (`Rvalue::BoundMethod`).
    pub(super) fn bound_method_expr(
        &self,
        receiver: &Operand,
        bound: Symbol,
        method: Symbol,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_text = self.bound_receiver_expr(receiver)?;
        if let Some(member) = self.bound_trait_member(receiver_ty, bound, method)
            && let BoundMemberKind::Method { params, ret, throws } = &member.kind
        {
            let args_text = args
                .iter()
                .zip(params)
                .map(|(arg, param)| self.value_at_type(arg, *param))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            let suffix = if *throws { "?" } else { "" };
            let call = RustExpr::opaque(format!("{receiver_text}.{}({args_text}){suffix}", member.rust_name));
            return self.rendered_at_type(&call, *ret, dest_ty, &self.render_scope());
        }
        // The parameter erased here: look the method up on the erased value,
        // as JavaScript does, and call it through the erased call ABI.
        let erased_args = args
            .iter()
            .map(|arg| self.erase_rendered(&self.operand_expr(arg)?, self.operand_ty(arg)?))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let method_name = self.symbol_name(method)?;
        let propagate = if self.function.can_throw && self.bound_method_may_throw(bound, method) {
            "?"
        } else {
            ".unwrap_or_else(|error| smelt_panic_throw(error))"
        };
        let call = RustExpr::atom(self.carrier_format(format_args!(
            "match smelt_get_unknown_field(&{receiver_text}, {method_name:?}) {{ SmeltUnknown::Function(smelt_method) => (smelt_method)(vec![{erased_args}]){propagate}, _ => SmeltUnknown::Undefined }}"
        )));
        // MIR interns `unknown` for every program with a bound member read
        // (`intern_bound_dispatch_carrier_type`).
        let unknown_ty = self.type_id(Type::Unknown)?;
        self.rendered_at_type(&call, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Render `receiver.field` where the receiver's type is a bounded type
    /// parameter (`Rvalue::BoundField`).
    pub(super) fn bound_field_expr(
        &self,
        receiver: &Operand,
        bound: Symbol,
        field: Symbol,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let receiver_text = self.bound_receiver_expr(receiver)?;
        if let Some(member) = self.bound_trait_member(receiver_ty, bound, field)
            && let BoundMemberKind::Field { ty } = &member.kind
        {
            let read = RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.{}()", member.rust_name));
            return self.rendered_at_type(&read, *ty, dest_ty, &self.render_scope());
        }
        let field_name = self.symbol_name(field)?;
        let read = RustExpr::chain(format!("smelt_get_unknown_field(&{receiver_text}, {field_name:?})"));
        let unknown_ty = self.type_id(Type::Unknown)?;
        self.rendered_at_type(&read, unknown_ty, dest_ty, &self.render_scope())
    }

    /// The receiver of a bound member access, borrowed in place: both the
    /// trait method (`&self`) and the erased lookup (`&SmeltUnknown`) only
    /// borrow it, so a whole-local operand is read without the copy
    /// `operand_expr` would render for it.
    fn bound_receiver_expr(&self, receiver: &Operand) -> Result<RustExpr, EmitError> {
        match receiver {
            Operand::Copy(smelt_mir::Place::Local(local))
            | Operand::Move(smelt_mir::Place::Local(local)) => self.local_value(*local),
            Operand::Copy(_) | Operand::Move(_) | Operand::Const(_) => self.operand_expr(receiver),
        }
    }

    /// Whether a method called through `bound` can throw: the planned trait's
    /// answer when there is one, otherwise conservatively yes (MIR's
    /// `bound_method_can_throw` decided the caller's `can_throw` the same way).
    fn bound_method_may_throw(&self, bound: Symbol, method: Symbol) -> bool {
        self.context
            .bound_traits
            .trait_for(bound)
            .and_then(|planned| planned.member(method))
            .is_none_or(|member| {
                matches!(member.kind, BoundMemberKind::Method { throws: true, .. })
            })
    }
}
