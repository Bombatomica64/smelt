//! Existence checks: the "does this reference exist" validation of rvalues,
//! operands, places, and callees used by structural validation.

use crate::{MirBody, Callee, GlobalProjection, Mir, Operand, Place, Rvalue};

use super::structure::validate_local_exists;
use super::{ValidationError, error};

/// Validate that IDs referenced by an rvalue point to existing MIR entities.
pub(super) fn validate_rvalue_exists(
    mir: &Mir,
    body: MirBody<'_>,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    value.reads(|operand| validate_operand_exists(mir, body, operand, errors));
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
    if let Rvalue::GlobalGet { global } | Rvalue::GlobalSet { global, .. } = value {
        validate_global_exists(mir, *global, errors);
    }
}

/// Ensure a global index names an entry of [`Mir::globals`].
fn validate_global_exists(mir: &Mir, global: u32, errors: &mut Vec<ValidationError>) {
    if mir.global(global).is_none() {
        errors.push(error(format!("MIR references unknown global {global}")));
    }
}

/// Validate that IDs referenced by an operand point to existing MIR entities.
pub(super) fn validate_operand_exists(
    mir: &Mir,
    body: MirBody<'_>,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place_exists(mir, body, place, errors);
        }
        Operand::Const(_) => {}
    }
}

/// Validate that a place references valid locals and projected fields.
pub(super) fn validate_place_exists(
    mir: &Mir,
    body: MirBody<'_>,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
    match place {
        Place::Local(local) => {
            validate_local_exists(body, *local, errors);
        }
        Place::Field { base, .. } => validate_local_exists(body, *base, errors),
        Place::Index { base, index, .. } => {
            validate_local_exists(body, *base, errors);
            validate_operand_exists(mir, body, index, errors);
        }
        // There is no base local; the base is an index into `Mir::globals`,
        // checked for range, and the index operand is checked like any other.
        Place::Global { base, projection } => {
            validate_global_exists(mir, *base, errors);
            if let GlobalProjection::Index { index, .. } = projection {
                validate_operand_exists(mir, body, index, errors);
            }
        }
    }
}

/// Validate that a callee target exists and references valid receiver places.
pub(super) fn validate_callee_exists(
    mir: &Mir,
    body: MirBody<'_>,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(func) | Callee::BaseInit(func) => {
            if mir.function(*func).is_none() {
                errors.push(error(format!("call references unknown function {func:?}")));
            }
        }
        Callee::Indirect(operand) => {
            validate_operand_exists(mir, body, operand, errors);
        }
        Callee::Op(op, _) => op.reads(|operand| validate_operand_exists(mir, body, operand, errors)),
        Callee::Builtin(_) => {}
    }
}
