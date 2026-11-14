//! Type-system lowering: TS type annotations, generic type parameters, and
//! interface/heritage field lookup.

mod annotations;
mod assignability;
mod callable_members;
pub(in crate::lowering) mod computed_key_symbols;
mod decode_facts;
mod generics;
mod interface_lookup;
