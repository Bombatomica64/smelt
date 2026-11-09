//! Closure-table and capture-ABI validation.
//!
//! These checks enforce the closure ABI contract published by the
//! `lower::closures` module:
//!
//! - closure-table ids equal their positions;
//! - each closure is constructed at most once, and its construction site's
//!   captured operands are exactly the locals its capture table names
//!   (`source_local` is derived from the site;;
//! - every capture's source local exists in the constructing body and its
//!   target local exists in the closure body;
//! - a capture's type, its source local's type and its target local's type are
//!   one type (invariant 4;;
//! - escaping closures capture by value (invariant 1).
//!
//! The closure BODY is checked like any function body, by
//! `structure::validate_body` over [`Mir::bodies`].

use crate::{MirBody, Mir, SiteLookup};

use super::{ValidationError, error};

/// Validate MIR closure table entries, their construction sites and the
/// escape-specific capture rules.
pub(super) fn validate_closures(mir: &Mir, errors: &mut Vec<ValidationError>) {
    let sites = mir.closure_sites();
    for (idx, closure) in mir.closures.iter().enumerate() {
        if closure.id.index() != idx {
            errors.push(error(format!(
                "closure index {idx} has mismatched id {:?}",
                closure.id
            )));
        }
        let construction = match sites.get(idx).copied() {
            Some(SiteLookup::One(found)) => Some(found),
            Some(SiteLookup::Many) => {
                errors.push(error(format!(
                    "closure {:?} is constructed more than once",
                    closure.id
                )));
                None
            }
            Some(SiteLookup::Unconstructed) | None => None,
        };
        let construction_body = construction.and_then(|found| mir.body(found.owner));
        if let Some(found) = construction
            && found.captures.len() != closure.captures.len()
        {
            errors.push(error(format!(
                "closure {:?} is constructed with {} captured operands for {} captures",
                closure.id,
                found.captures.len(),
                closure.captures.len()
            )));
        }
        for (position, capture) in closure.captures.iter().enumerate() {
            let target_decl_ty = capture.target_local.map(|target| {
                closure.body().local_decl(target).map(|decl| decl.ty)
            });
            if target_decl_ty == Some(None) {
                errors.push(error(format!(
                    "closure {:?} capture targets unknown local {:?}",
                    closure.id, capture.target_local
                )));
            }
            if let Some(Some(target_ty)) = target_decl_ty
                && target_ty != capture.ty
            {
                errors.push(error(format!(
                    "closure {:?} capture {position} has type {:?} but its target local {:?} has type {target_ty:?}",
                    closure.id, capture.ty, capture.target_local
                )));
            }
            if let (Some(found), Some(body)) = (construction, construction_body) {
                validate_capture_source(
                    (closure.id, position),
                    capture,
                    found.captures.get(position),
                    body,
                    errors,
                );
            }
            // Closure ABI contract invariant 1 (see `lower::closures`):
            // escaping closures capture by value only.
            if closure.escapes && capture.mode != smelt_hir::CaptureMode::ByValue {
                errors.push(error(format!(
                    "escaping closure {:?} captures {:?} without owning it",
                    closure.id, capture.source_local
                )));
            }
        }
    }
}

/// Check one capture against its construction site: the site reads exactly the
/// capture's `source_local`, that local exists in the constructing body, and
/// its type is the capture's type.
fn validate_capture_source(
    (closure, position): (crate::ClosureId, usize),
    capture: &crate::MirClosureCapture,
    site_operand: Option<&crate::Operand>,
    site_body: MirBody<'_>,
    errors: &mut Vec<ValidationError>,
) {
    let Some(operand) = site_operand else {
        return;
    };
    if operand.local() != Some(capture.source_local) {
        errors.push(error(format!(
            "closure {closure:?} capture {position} records source {:?} but its construction captures {operand:?}",
            capture.source_local
        )));
    }
    match site_body.local_decl(capture.source_local) {
        None => errors.push(error(format!(
            "closure {closure:?} capture {position} source {:?} is not a local of {:?}",
            capture.source_local, site_body.owner
        ))),
        Some(decl) if decl.ty != capture.ty => errors.push(error(format!(
            "closure {closure:?} capture {position} has type {:?} but its source local {:?} has type {:?}",
            capture.ty, capture.source_local, decl.ty
        ))),
        Some(_) => {}
    }
}
