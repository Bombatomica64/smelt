//! Base-constructor initializers: how a derived constructor's `super(..)` runs.
//!
//! Smelt flattens inheritance (a derived struct carries its base's fields, see
//! [`crate::classes::effective_class_fields`]) and re-emits every inherited
//! method body into the derived `impl` (see
//! [`crate::classes::effective_class_methods`]). A derived constructor's
//! `super(args)` follows the same rule: the BASE constructor's MIR body is
//! re-emitted into the derived `impl` as an initializer over the receiver,
//!
//! ```text
//! fn __smelt_init_Base(smelt_receiver: Self, args..) -> Self {
//!     let mut this: Self = smelt_receiver;   // instead of allocating a fresh `Base`
//!     ..the base constructor body..
//!     return this;
//! }
//! ```
//!
//! and `super(args)` (MIR [`Callee::BaseInit`]) becomes
//! `this = Self::__smelt_init_Base(this, args)`.
//!
//! This is what makes the JavaScript "one object" semantics hold: the base's
//! field initializers and body write the derived instance, a closure the base
//! stores captures the derived instance as `this` (for a reference class the
//! handle passed in and returned is the same `Rc`), and a method the base body
//! calls on `this` resolves statically to the derived override because the body
//! is emitted with `Self` = the derived class. Passing the receiver by value and
//! returning it keeps the constructor MIR unchanged — the only difference from
//! `new` is that `this` arrives instead of being allocated — and works the same
//! for by-value structs and reference handles.
#![expect(
    clippy::redundant_pub_crate,
    reason = "initializer helpers are shared with the emitter and the crate emitter"
)]

use std::collections::HashSet;

use smelt_mir::{Callee, FuncId, HirOrigin, Mir, MirClass, MirFunction, Terminator};

use crate::{EmitError, id_index, sanitize_ident};

/// The parameter name an initializer copy receives the instance under.
///
/// The copied constructor body's first statement (`this = <fresh struct>`)
/// renders this name instead of the struct literal.
pub(crate) const BASE_INIT_RECEIVER: &str = "smelt_receiver";

/// The Rust method name of `base`'s constructor run as an initializer.
///
/// Named after the class whose constructor body it is, so a class several
/// levels down carries one initializer per ancestor and the names never clash
/// with each other or with a source method.
pub(crate) fn base_initializer_name(mir: &Mir, base: smelt_hir::Symbol) -> Result<String, EmitError> {
    let name = mir
        .symbols
        .get(base)
        .ok_or_else(|| EmitError::new("base initializer references an unknown class symbol"))?;
    Ok(format!("__smelt_init_{}", sanitize_ident(name)))
}

/// The type a class's constructor builds, `Class { name, args }` as interned.
///
/// Read off the constructor's declared return so it is exactly the TypeId MIR
/// already interned (codegen cannot intern new types). `None` for a class
/// without a constructor.
pub(crate) fn constructed_class_ty(
    mir: &Mir,
    class: smelt_hir::Symbol,
) -> Result<Option<smelt_hir::TypeId>, EmitError> {
    let Some(constructor) = mir
        .classes
        .iter()
        .find(|candidate| candidate.name == class)
        .and_then(|candidate| candidate.constructor)
    else {
        return Ok(None);
    };
    Ok(mir
        .functions
        .get(id_index(constructor.0, "constructor index does not fit usize")?)
        .map(|function| function.return_ty))
}

/// The class whose constructor `function` is, if it is a constructor at all.
pub(crate) const fn constructor_class(function: &MirFunction) -> Option<smelt_hir::Symbol> {
    match function.origin {
        HirOrigin::ClassConstructor { class, .. } => Some(class),
        _ => None,
    }
}

/// The ancestor constructors `class`'s own constructor runs as initializers.
///
/// Walks `Callee::BaseInit` calls from the class's constructor, then from each
/// initializer found (its own `super(..)`), so a multi-level chain yields every
/// ancestor whose body must be re-emitted into `class`'s `impl`, base-most last,
/// each once. An abstract class emits no constructor and therefore no
/// initializers of its own; a concrete descendant re-emits the abstract class's
/// constructor as one of ITS initializers instead.
pub(crate) fn base_initializer_chain(mir: &Mir, class: &MirClass) -> Result<Vec<FuncId>, EmitError> {
    let mut chain = Vec::new();
    if class.is_abstract {
        return Ok(chain);
    }
    let Some(constructor) = class.constructor else {
        return Ok(chain);
    };
    let mut seen = HashSet::new();
    let mut pending = vec![constructor];
    while let Some(func) = pending.pop() {
        let function = mir
            .functions
            .get(id_index(func.0, "constructor index does not fit usize")?)
            .ok_or_else(|| EmitError::new("base initializer references an unknown function"))?;
        for block in &function.blocks {
            let Some(Terminator::Call {
                callee: Callee::BaseInit(base_constructor),
                ..
            }) = &block.terminator
            else {
                continue;
            };
            if seen.insert(*base_constructor) {
                chain.push(*base_constructor);
                pending.push(*base_constructor);
            }
        }
    }
    Ok(chain)
}
