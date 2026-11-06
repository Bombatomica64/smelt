//! Expression lowering: operators and value references.

mod arguments;
mod array_literals;
mod logical;
pub(in crate::lowering) mod member_access;
mod object_literals;
mod operators;
mod references;
