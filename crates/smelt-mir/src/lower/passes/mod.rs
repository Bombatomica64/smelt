//! Whole-program MIR analyses that run after core HIR-to-MIR lowering.
//!
//! These passes operate over already-lowered MIR and are kept separate from
//! expression lowering so ABI and capture invariants have a narrow home.
//!
//! Closure-specific analyses (escape analysis and throwing-closure type
//! widening) live in [`super::closures`], which owns the closure ABI end to
//! end. This module hosts the general function-throwing propagation in
//! [`throwing`], whose `can_throw` results the closure widening consumes, as well
//! as the [`generic_records`] pass.

pub(super) mod generic_records;
pub(super) mod inherited_types;
pub(super) mod throwing;
pub(super) mod unbound_bindings;


