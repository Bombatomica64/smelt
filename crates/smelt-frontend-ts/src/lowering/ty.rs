//! Type-system lowering: TS type annotations, generic type parameters, and
//! interface/heritage field lookup.

mod annotations;
mod assignability;
pub(in crate::lowering) mod computed_key_symbols;
mod generics;
mod interface_lookup;
