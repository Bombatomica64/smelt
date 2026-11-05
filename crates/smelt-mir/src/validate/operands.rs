//! Existence checks: the "does this reference exist" validation of rvalues,
//! operands, places, and callees used by structural validation.

use crate::{Callee, GlobalProjection, Mir, MirFunction, Operand, Place, Rvalue};

use super::structure::validate_local_exists;
use super::{ValidationError, error, function_index, validate_type};

/// Validate that IDs referenced by an rvalue point to existing MIR entities.
pub(super) fn validate_rvalue_exists(
    mir: &Mir,
    function: &MirFunction,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    value.reads(|operand| validate_operand_exists(function, operand, errors));
    if let Rvalue::Closure { id, .. } = value
        && mir
            .closures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .is_none()
    {
        errors.push(error(format!(
            "closure rvalue references unknown closure {id:?}"
        )));
    }
    if let Rvalue::UnknownCast { target, .. } = value {
        validate_type(mir, *target, errors);
    }
}

/// Validate that IDs referenced by an operand point to existing MIR entities.
pub(super) fn validate_operand_exists(
    function: &MirFunction,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place_exists(function, place, errors);
        }
        Operand::Const(_) => {}
    }
}

/// Validate that a place references valid locals and projected fields.
pub(super) fn validate_place_exists(
    function: &MirFunction,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
    match place {
        Place::Local(local) => {
            validate_local_exists(function, *local, errors);
        }
        Place::Field { base, .. } => validate_local_exists(function, *base, errors),
        Place::Index { base, index, .. } => {
            validate_local_exists(function, *base, errors);
            validate_operand_exists(function, index, errors);
        }
        // There is no base local to check for existence. The global index is
        // not bounds-checked against `Mir::globals` anywhere in the validator (a
        // separate semantic change would be needed to add that check); the index
        // operand is checked here like any other.
        Place::Global { projection, .. } => {
            if let GlobalProjection::Index { index, .. } = projection {
                validate_operand_exists(function, index, errors);
            }
        }
    }
}

/// Validate that a callee target exists and references valid receiver places.
pub(super) fn validate_callee_exists(
    mir: &Mir,
    function: &MirFunction,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(func) | Callee::BaseInit(func) => {
            if mir.functions.get(function_index(*func)).is_none() {
                errors.push(error(format!("call references unknown function {func:?}")));
            }
        }
        Callee::Indirect(operand) => {
            validate_operand_exists(function, operand, errors);
        }
        Callee::Builtin(_) => {}
    }
}
