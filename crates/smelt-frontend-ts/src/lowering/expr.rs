//! Expression lowering: the dispatch over expression shapes (`dispatch`) and
//! one module per expression family (operators, conditions and conditionals,
//! templates, `await`, literals, member access and value references).

mod arguments;
mod array_literals;
mod awaits;
mod condition;
mod conditional;
mod dispatch;
mod logical;
pub(in crate::lowering) mod member_access;
mod object_literals;
mod operators;
mod references;
mod templates;
