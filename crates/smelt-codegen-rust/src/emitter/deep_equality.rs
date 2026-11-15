//! Emission of typed deep equality: the `impl SmeltDeepEq` of each record
//! [`crate::deep_equality`] plans, and the comparison a function body writes
//! where it compares such a record.
//!
//! The rule, its requirements and what it leaves out are in
//! [`crate::deep_equality`]. What is generated, for
//! `interface Team { lead: Person; size: number }` (a by-value struct) and
//! `interface Box<T> { value: T }` (a generic reference record):
//!
//! ```text
//! impl SmeltDeepEq for Team {
//!     fn smelt_deep_eq(&self, other: &Self, smelt_seen: &mut SmeltDeepSeen) -> bool {
//!         let (smelt_left, smelt_right) = (self, other);
//!         smelt_live_deep_eq(smelt_left.lead.clone(), smelt_right.lead.clone())
//!             && (smelt_left.size == smelt_right.size || (smelt_left.size.is_nan() && smelt_right.size.is_nan()))
//!     }
//! }
//! impl<T: ..> SmeltDeepEq for Box_<T> where T: SmeltDeepEq {
//!     fn smelt_deep_eq(&self, other: &Self, smelt_seen: &mut SmeltDeepSeen) -> bool {
//!         if ::std::rc::Rc::ptr_eq(&self.0, &other.0) { return true; }            // one object
//!         if let Some(smelt_answer) = smelt_seen.enter(<address of self.0>, <address of other.0>) { return smelt_answer; } // a cycle
//!         let (smelt_left, smelt_right) = (self.0.borrow(), other.0.borrow());
//!         let smelt_equal = SmeltDeepEq::smelt_deep_eq(&smelt_left.value, &smelt_right.value, smelt_seen);
//!         smelt_seen.leave();
//!         smelt_equal
//!     }
//! }
//! ```
//!
//! Every field is compared by the expression
//! `binary_ops::structural_leaf_equality_expr` writes for its type, the one a
//! function body uses for a list, tuple, dictionary or optional, so a number
//! nested in a record follows the same rule as one nested in a list. Inside an
//! impl that expression is written in [`Scope::Impl`]: a nested record is
//! handed the comparison's own in-progress set, and a field of the record's
//! type parameter goes through the parameter's bound.

use smelt_mir::{Mir, MirInterface};

use super::{EmitContext, EmitError, FunctionEmitter};
use crate::runtime_demand::Need;
use crate::deep_equality::Scope;
use crate::rust::{CodeWriter, RustExpr};

/// The name the in-progress set of a comparison is bound to inside an impl.
const SEEN: &str = "smelt_seen";

impl FunctionEmitter<'_> {
    /// The expression comparing `left` and `right`, two values of a type that
    /// compares through `SmeltDeepEq` (`DeepEquality::compares_through_trait`,
    /// or a record's own type parameter inside its impl).
    ///
    /// In a function body the comparison is one of its own (`smelt_deep_eq`
    /// starts an empty in-progress set); inside an impl it continues the
    /// comparison it is part of. Either operand may be a value or a borrow of
    /// one: the trait is implemented for `&T` too.
    pub(super) fn trait_deep_equality_expr(
        left: &RustExpr,
        right: &RustExpr,
        scope: Scope,
    ) -> RustExpr {
        let (left, right) = (
            left.parenthesized_if_needed(),
            right.parenthesized_if_needed(),
        );
        RustExpr::atom(match scope {
            Scope::Body => format!("smelt_deep_eq(&{left}, &{right})"),
            Scope::Impl => format!("SmeltDeepEq::smelt_deep_eq(&{left}, &{right}, {SEEN})"),
        })
    }

    /// The comparison of two callable records `self` and `other`: JavaScript
    /// compares functions by identity. A reference record is its cell; a
    /// by-value one is the function it holds in its call slot, compared as
    /// every function value is (`binary_ops::function_ptr_eq_expr`: through
    /// the identity links of the erased runtime when the program carries it).
    fn callable_identity_text(
        &self,
        interface: &MirInterface,
        reference: bool,
    ) -> Result<String, EmitError> {
        if reference {
            return Ok("::std::rc::Rc::ptr_eq(&self.0, &other.0)".to_owned());
        }
        let slot = crate::classes::effective_interface_fields(self.mir, interface)
            .into_iter()
            .find(|field| {
                self.mir
                    .names
                    .get(field.name)
                    .or_else(|| self.mir.symbols.get(field.name))
                    == Some(smelt_stdlib::runtime_symbols::markers::SMELT_CALL)
            })
            .ok_or_else(|| EmitError::new("a callable record has no call slot"))?;
        let slot = self.context.names().field_text(self.mir, slot.name);
        Ok(if self.context.demand().has(Need::Unknown) {
            format!("smelt_same_function_identity(&self.{slot}, &other.{slot})")
        } else {
            format!(
                "::std::ptr::addr_eq(::std::rc::Rc::as_ptr(&self.{slot}), ::std::rc::Rc::as_ptr(&other.{slot}))"
            )
        })
    }

    /// The `impl SmeltDeepEq` of the planned record `interface`, whose Rust
    /// type is `name{type_args}`.
    ///
    /// `self` is an emitter for any function of the program: the field
    /// comparisons are written from the fields' types alone.
    fn deep_eq_impl_text(
        &self,
        interface: &MirInterface,
        name: &str,
        impl_generics: &str,
        type_args: &str,
        bounded_params: &[String],
    ) -> Result<String, EmitError> {
        let reference = self.context.is_reference_class(interface.name);
        let mut body = Vec::new();
        let comparison = if self.context.is_live_record(interface.name) {
            // A live record is compared through its views, which also see
            // what a view added to or deleted from the object.
            "smelt_live_deep_eq(self.clone(), other.clone())".to_owned()
        } else if crate::classify::is_callable_shape(self.mir, interface) {
            self.callable_identity_text(interface, reference)?
        } else {
            if reference {
                let address = |handle: &str| {
                    format!("::std::rc::Rc::as_ptr(&{handle}.0).cast::<()>() as usize")
                };
                body.push(
                    "if ::std::rc::Rc::ptr_eq(&self.0, &other.0) { return true; }".to_owned(),
                );
                body.push(format!(
                    "if let Some(smelt_answer) = {SEEN}.enter({}, {}) {{ return smelt_answer; }}",
                    address("self"),
                    address("other")
                ));
                body.push(
                    "let (smelt_left, smelt_right) = (self.0.borrow(), other.0.borrow());"
                        .to_owned(),
                );
            } else {
                body.push("let (smelt_left, smelt_right) = (self, other);".to_owned());
            }
            let mut parts = Vec::new();
            for field in crate::classes::effective_interface_fields(self.mir, interface)
                .iter()
                .filter(|field| field.visibility.is_own_property())
            {
                let field_name = self.context.names().field_text(self.mir, field.name);
                let place =
                    |side: &str| RustExpr::chain(format!("smelt_{side}.{field_name}"));
                let part = if crate::accessor_prelude::is_accessor_field(
                    self.mir,
                    interface.name,
                    field.name,
                ) {
                    // A getter-backed property is the value its getter
                    // answers, read once per side. The block is parenthesized:
                    // one that opens the function's tail would be a statement.
                    let equal = self.structural_leaf_equality_in(
                        &RustExpr::path("smelt_left_read"),
                        &RustExpr::path("smelt_right_read"),
                        field.ty,
                        Scope::Impl,
                    )?;
                    RustExpr::atom(format!(
                        "({{ let (smelt_left_read, smelt_right_read) = ({}.get(), {}.get()); {} }})",
                        place("left").tight(),
                        place("right").tight(),
                        equal.delimited()
                    ))
                } else {
                    self.structural_leaf_equality_in(
                        &place("left"),
                        &place("right"),
                        field.ty,
                        Scope::Impl,
                    )?
                };
                parts.push(part.as_binary_operand());
            }
            let equal = if parts.is_empty() {
                "true".to_owned()
            } else {
                parts.join(" && ")
            };
            if reference {
                // The pair is left once compared, whatever the answer.
                body.push(format!("let smelt_equal = {equal};"));
                body.push(format!("{SEEN}.leave();"));
                "smelt_equal".to_owned()
            } else {
                equal
            }
        };
        let where_clause = if bounded_params.is_empty() {
            String::new()
        } else {
            format!(
                " where {}",
                bounded_params
                    .iter()
                    .map(|param| format!("{param}: SmeltDeepEq"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let mut writer = CodeWriter::new();
        writer.block(
            format!("impl{impl_generics} SmeltDeepEq for {name}{type_args}{where_clause}"),
            |impl_writer| {
                impl_writer.block(
                    format!(
                        "fn smelt_deep_eq(&self, other: &Self, {SEEN}: &mut SmeltDeepSeen) -> bool"
                    ),
                    |fn_writer| {
                        for line in &body {
                            fn_writer.line(line);
                        }
                        fn_writer.line(&comparison);
                    },
                );
            },
        );
        Ok(writer.finish())
    }
}

/// Writes the `impl SmeltDeepEq` of `interface` when the program's deep
/// comparisons reach it (see [`crate::deep_equality`]); nothing otherwise.
///
/// `impl_generics` and `type_args` are the generics of the record's other
/// impls, and `param_idents` the type parameters its Rust type declares: each
/// is bounded by the trait, since a field of that type is compared through it.
/// `trailing_blank` asks for a blank line after the impl, where the record's
/// storage has already written its own.
pub(crate) fn emit_deep_eq_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    interface: &MirInterface,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    param_idents: &[String],
    trailing_blank: bool,
) -> Result<(), EmitError> {
    if !context.deep_equality().has_impl(interface.name) {
        return Ok(());
    }
    let host = mir
        .functions
        .first()
        .ok_or_else(|| EmitError::new("a deep-compared record needs a function to render in"))?;
    let text = FunctionEmitter::new(mir, context, host)?.deep_eq_impl_text(
        interface,
        name,
        impl_generics,
        type_args,
        param_idents,
    )?;
    writer.line(text.trim_end());
    if trailing_blank {
        writer.blank_line();
    }
    Ok(())
}
