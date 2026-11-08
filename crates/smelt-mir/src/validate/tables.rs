//! Crate-level table checks: function ids, member ids, global initializers and
//! declaration names.
//!
//! `mir.functions` is a public `Vec`, so "`functions[i].id == i`" cannot be a
//! type-level property of the table itself; lowering makes it true by
//! construction (reserved slots) and [`crate::Mir::push_function`] assigns the
//! id, and this check makes any other writer's mistake a release-mode error
//! rather than a silent mis-call (audit finding C5). The member-id checks are
//! the range checks every `FuncId` stored in a table needs (audit C13). The
//! name checks reject a second class or interface of one name, which every
//! by-name lookup (`Mir::class_named`, inherited-type substitution, bound
//! throwing) would otherwise answer with whichever came first (audit C12).

use crate::index::NameIndex;
use crate::{FuncId, Mir, MirClassProtocol, MirGlobalInit};

use super::{ValidationError, error};

/// Run every table check.
pub(super) fn validate_tables(mir: &Mir, errors: &mut Vec<ValidationError>) {
    validate_function_ids(mir, errors);
    validate_member_ids(mir, errors);
    validate_global_initializers(mir, errors);
    validate_unique_names(mir, errors);
}

/// `functions[i].id == i` for every function.
fn validate_function_ids(mir: &Mir, errors: &mut Vec<ValidationError>) {
    for (index, function) in mir.functions.iter().enumerate() {
        if function.id.index() != index {
            errors.push(error(format!(
                "function index {index} has mismatched id {:?}",
                function.id
            )));
        }
    }
}

/// Every function id a class stores names a function.
fn validate_member_ids(mir: &Mir, errors: &mut Vec<ValidationError>) {
    for class in &mir.classes {
        let class_name = mir.symbols.get(class.name).unwrap_or("<unknown>");
        let mut check = |role: &str, id: FuncId| {
            if mir.function(id).is_none() {
                errors.push(error(format!(
                    "class `{class_name}` {role} references unknown function {id:?}"
                )));
            }
        };
        if let Some(constructor) = class.constructor {
            check("constructor", constructor);
        }
        for &method in &class.methods {
            check("method", method);
        }
        for &method in &class.static_methods {
            check("static method", method);
        }
        for descriptor in &class.descriptors {
            for id in descriptor.getter.into_iter().chain(descriptor.setter) {
                check("descriptor accessor", id);
            }
        }
        for protocol in &class.protocols {
            match protocol {
                MirClassProtocol::Add { method } => check("protocol method", *method),
            }
        }
    }
}

/// A global initialized by a call calls a function that exists.
fn validate_global_initializers(mir: &Mir, errors: &mut Vec<ValidationError>) {
    for (index, global) in mir.globals.iter().enumerate() {
        match &global.init {
            MirGlobalInit::Call(id) => {
                if mir.function(*id).is_none() {
                    errors.push(error(format!(
                        "global {index} initializer references unknown function {id:?}"
                    )));
                }
            }
            MirGlobalInit::Constant(_) => {}
        }
    }
}

/// No two classes share a name, and no two interfaces do: the same
/// [`NameIndex::new`] constructor lowering uses.
fn validate_unique_names(mir: &Mir, errors: &mut Vec<ValidationError>) {
    if let Err(duplicates) = NameIndex::new(mir) {
        for duplicate in duplicates {
            errors.push(error(duplicate.message(mir)));
        }
    }
}
