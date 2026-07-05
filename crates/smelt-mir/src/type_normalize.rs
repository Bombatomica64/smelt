//! MIR operational type normalization.
//!
//! This pass rewrites MIR-visible type IDs to canonical shapes before
//! optimization and codegen. It keeps source-language optional/nullish spelling
//! from leaking into generated Rust as nested `Option<Option<T>>`.

use smelt_hir::{
    TypeId,
    type_normalize::{NormalizeOptions, normalize_type},
};

use crate::{Mir, MirClass, MirClosure, MirField, MirFunction, MirInterface, Rvalue, Statement};

/// Normalize all MIR operational type references in place.
pub fn normalize_operational_types(mir: &mut Mir) {
    let mut functions = std::mem::take(&mut mir.functions);
    for function in &mut functions {
        normalize_function(mir, function);
    }
    mir.functions = functions;

    let mut closures = std::mem::take(&mut mir.closures);
    for closure in &mut closures {
        normalize_closure(mir, closure);
    }
    mir.closures = closures;

    let mut classes = std::mem::take(&mut mir.classes);
    for class in &mut classes {
        normalize_class(mir, class);
    }
    mir.classes = classes;

    let mut interfaces = std::mem::take(&mut mir.interfaces);
    for interface in &mut interfaces {
        normalize_interface(mir, interface);
    }
    mir.interfaces = interfaces;
}

/// Normalize one MIR function signature, locals, and typed statements.
fn normalize_function(mir: &mut Mir, function: &mut MirFunction) {
    normalize_type_params(mir, &mut function.type_params);
    function.return_ty = normalize(mir, function.return_ty);
    for local in &mut function.locals {
        local.ty = normalize(mir, local.ty);
    }
    for block in &mut function.blocks {
        for phi in &mut block.phis {
            phi.ty = normalize(mir, phi.ty);
        }
        for statement in &mut block.statements {
            normalize_statement(mir, statement);
        }
    }
}

/// Normalize one MIR closure signature, locals, captures, and typed statements.
fn normalize_closure(mir: &mut Mir, closure: &mut MirClosure) {
    closure.return_ty = normalize(mir, closure.return_ty);
    for local in &mut closure.locals {
        local.ty = normalize(mir, local.ty);
    }
    for capture in &mut closure.captures {
        capture.ty = normalize(mir, capture.ty);
    }
    for block in &mut closure.blocks {
        for phi in &mut block.phis {
            phi.ty = normalize(mir, phi.ty);
        }
        for statement in &mut block.statements {
            normalize_statement(mir, statement);
        }
    }
}

/// Normalize one MIR class declaration.
fn normalize_class(mir: &mut Mir, class: &mut MirClass) {
    normalize_type_params(mir, &mut class.type_params);
    for arg in &mut class.base_args {
        *arg = normalize(mir, *arg);
    }
    for field in &mut class.fields {
        normalize_field(mir, field);
    }
    for descriptor in &mut class.descriptors {
        descriptor.read_ty = normalize(mir, descriptor.read_ty);
        descriptor.write_ty = descriptor.write_ty.map(|ty| normalize(mir, ty));
        for field in &mut descriptor.value_fields {
            field.ty = normalize(mir, field.ty);
        }
    }
    for method in &mut class.abstract_methods {
        for param in &mut method.params {
            param.ty = normalize(mir, param.ty);
        }
        method.return_ty = normalize(mir, method.return_ty);
    }
}

/// Normalize one MIR interface declaration.
fn normalize_interface(mir: &mut Mir, interface: &mut MirInterface) {
    normalize_type_params(mir, &mut interface.type_params);
    for field in &mut interface.fields {
        normalize_field(mir, field);
    }
    for method in &mut interface.methods {
        for param in &mut method.params {
            param.ty = normalize(mir, param.ty);
        }
        method.return_ty = normalize(mir, method.return_ty);
    }
}

/// Normalize generic parameter constraints and defaults.
fn normalize_type_params(mir: &mut Mir, params: &mut [smelt_hir::TypeParamDef]) {
    for param in params {
        if let Some(constraint) = param.constraint {
            param.constraint = Some(normalize(mir, constraint));
        }
        if let Some(default) = param.default {
            param.default = Some(normalize(mir, default));
        }
    }
}

/// Normalize a field type.
fn normalize_field(mir: &mut Mir, field: &mut MirField) {
    field.ty = normalize(mir, field.ty);
}

/// Normalize type-bearing statement internals.
fn normalize_statement(mir: &mut Mir, statement: &mut Statement) {
    let Statement::Assign { value, .. } = statement else {
        return;
    };
    if let Rvalue::UnknownCast { target, .. } = value {
        *target = normalize(mir, *target);
    }
}

/// Normalize one type ID with the shared HIR canonicalization rules.
fn normalize(mir: &mut Mir, ty: TypeId) -> TypeId {
    normalize_type(&mut mir.types, ty, NormalizeOptions::default())
}
