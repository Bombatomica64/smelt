//! Every position in MIR that holds a [`TypeId`], walked by one visitor.
//!
//! Three consumers need "every type the backend may read": operational type
//! normalization rewrites them, the validator checks that each exists, and the
//! inherited-member pass substitutes and interns those of a method body. Each
//! used to keep its own partial list, and all three skipped the same positions
//! (`JsonParse.target`, `MirGlobal.ty`, an `UnknownCast` stored through a place
//! or a dict-entry update, a fallible op's render type), so a type there could
//! be unnormalized or dangling (audit finding C6).
//!
//! This module is the one list. The `Rvalue` arms are generated from the op
//! table: a `(ty)` field is visited directly and a carried `[T]` field through
//! [`OpFieldTypes`], so a new op cannot hide a type from any consumer. The
//! hand-written `Rvalue` variants and every statement, terminator and table
//! entry are matched exhaustively, without wildcards.
//!
//! Each walk comes in a visiting form (`&self`) and a mapping form
//! (`&mut self`) written side by side, so the two cannot list different
//! positions.

use smelt_hir::TypeId;
use smelt_hir::op_field_types::OpFieldTypes;

use crate::{
    BasicBlock, Callee, Mir, MirClass, MirClosure, MirFunction, MirGlobal, MirInterface, Rvalue,
    Statement, Terminator,
};

/// Visits (or maps) the type ids one op-table field holds, by its table kind.
macro_rules! op_field_types {
    (visit $visit:ident, (ty), $field:ident) => {
        $visit(*$field)
    };
    (visit $visit:ident, [$data:ty], $field:ident) => {
        OpFieldTypes::visit_types($field, $visit)
    };
    (visit $visit:ident, $other:tt, $field:ident) => {{
        let _ = $field;
    }};
    (map $map:ident, (ty), $field:ident) => {
        *$field = $map(*$field)
    };
    (map $map:ident, [$data:ty], $field:ident) => {
        OpFieldTypes::map_types($field, $map)
    };
    (map $map:ident, $other:tt, $field:ident) => {{
        let _ = $field;
    }};
}

/// Generates [`Rvalue::visit_types`] and [`Rvalue::map_types`] over the
/// hand-written variants and every op-table entry.
macro_rules! define_rvalue_types {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl Rvalue {
            /// Visit every type id this rvalue carries.
            ///
            /// The hand-written MIR-only variants carry none; each is still
            /// listed so a new one has to be classified here.
            pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
                match self {
                    Self::Use(..)
                    | Self::DictEntryPush { .. }
                    | Self::GeneratorYield { .. }
                    | Self::List(..)
                    | Self::Set(..)
                    | Self::Dict(..)
                    | Self::Tuple(..)
                    | Self::Binary { .. }
                    | Self::Unary { .. }
                    | Self::UnionMethod { .. }
                    | Self::BoundMethod { .. }
                    | Self::BoundField { .. }
                    | Self::Struct { .. }
                    | Self::ExternalClassInstance { .. }
                    | Self::Closure { .. }
                    | Self::GlobalGet { .. }
                    | Self::GlobalSet { .. }
                    | Self::Await(..) => {}
                    $(
                        Self::$name $({ $($field,)* })? => {
                            $($(op_field_types!(visit visit, $kind, $field);)*)?
                        }
                    )*
                }
            }

            /// Rewrite every type id [`Self::visit_types`] visits.
            pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
                match self {
                    Self::Use(..)
                    | Self::DictEntryPush { .. }
                    | Self::GeneratorYield { .. }
                    | Self::List(..)
                    | Self::Set(..)
                    | Self::Dict(..)
                    | Self::Tuple(..)
                    | Self::Binary { .. }
                    | Self::Unary { .. }
                    | Self::UnionMethod { .. }
                    | Self::BoundMethod { .. }
                    | Self::BoundField { .. }
                    | Self::Struct { .. }
                    | Self::ExternalClassInstance { .. }
                    | Self::Closure { .. }
                    | Self::GlobalGet { .. }
                    | Self::GlobalSet { .. }
                    | Self::Await(..) => {}
                    $(
                        Self::$name $({ $($field,)* })? => {
                            $($(op_field_types!(map map, $kind, $field);)*)?
                        }
                    )*
                }
            }
        }
    };
}

smelt_hir::for_each_op!(define_rvalue_types);

impl Statement {
    /// Visit every type id the statement carries: those of its stored rvalue.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        match self {
            Self::Assign { value, .. }
            | Self::AssignPlace { value, .. }
            | Self::DictEntryUpdate { value, .. } => value.visit_types(visit),
        }
    }

    /// Rewrite every type id [`Self::visit_types`] visits.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        match self {
            Self::Assign { value, .. }
            | Self::AssignPlace { value, .. }
            | Self::DictEntryUpdate { value, .. } => value.map_types(map),
        }
    }
}

impl Terminator {
    /// Visit every type id the terminator carries: a fallible op's own types
    /// and the type it renders at.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        match self {
            Self::Call { callee, .. } => match callee {
                Callee::Op(op, render_ty) => {
                    op.visit_types(visit);
                    if let Some(ty) = render_ty {
                        visit(*ty);
                    }
                }
                Callee::Static(_) | Callee::Indirect(_) | Callee::Builtin(_) | Callee::BaseInit(_) => {}
            },
            Self::Goto(_)
            | Self::Await { .. }
            | Self::Switch { .. }
            | Self::Match { .. }
            | Self::Return(_)
            | Self::Throw(_) => {}
        }
    }

    /// Rewrite every type id [`Self::visit_types`] visits.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        match self {
            Self::Call { callee, .. } => match callee {
                Callee::Op(op, render_ty) => {
                    op.map_types(map);
                    if let Some(ty) = render_ty {
                        *ty = map(*ty);
                    }
                }
                Callee::Static(_) | Callee::Indirect(_) | Callee::Builtin(_) | Callee::BaseInit(_) => {}
            },
            Self::Goto(_)
            | Self::Await { .. }
            | Self::Switch { .. }
            | Self::Match { .. }
            | Self::Return(_)
            | Self::Throw(_) => {}
        }
    }
}

/// Visit every type id in a body's blocks.
fn visit_block_types(blocks: &[BasicBlock], visit: &mut dyn FnMut(TypeId)) {
    for block in blocks {
        for statement in &block.statements {
            statement.visit_types(visit);
        }
        if let Some(terminator) = &block.terminator {
            terminator.visit_types(visit);
        }
    }
}

/// Rewrite every type id in a body's blocks.
fn map_block_types(blocks: &mut [BasicBlock], map: &mut dyn FnMut(TypeId) -> TypeId) {
    for block in blocks {
        for statement in &mut block.statements {
            statement.map_types(map);
        }
        if let Some(terminator) = &mut block.terminator {
            terminator.map_types(map);
        }
    }
}

/// Visit a type parameter's constraint and default.
fn visit_type_params(params: &[smelt_hir::TypeParamDef], visit: &mut dyn FnMut(TypeId)) {
    for param in params {
        param.constraint.visit_types(visit);
        param.default.visit_types(visit);
    }
}

/// Rewrite a type parameter's constraint and default.
fn map_type_params(params: &mut [smelt_hir::TypeParamDef], map: &mut dyn FnMut(TypeId) -> TypeId) {
    for param in params {
        param.constraint.map_types(map);
        param.default.map_types(map);
    }
}

/// Visit a method signature's parameter and return types.
fn visit_method_sig(method: &smelt_hir::MethodSig, visit: &mut dyn FnMut(TypeId)) {
    for param in &method.params {
        visit(param.ty);
    }
    visit(method.return_ty);
}

/// Rewrite a method signature's parameter and return types.
fn map_method_sig(method: &mut smelt_hir::MethodSig, map: &mut dyn FnMut(TypeId) -> TypeId) {
    for param in &mut method.params {
        param.ty = map(param.ty);
    }
    method.return_ty = map(method.return_ty);
}

impl MirFunction {
    /// Visit every type id the function's emission reads: its type parameters'
    /// bounds and defaults, return type, local types and every type its
    /// statements and terminators carry.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit_type_params(&self.type_params, visit);
        visit(self.return_ty);
        for local in &self.locals {
            visit(local.ty);
        }
        visit_block_types(&self.blocks, visit);
    }

    /// Rewrite every type id [`Self::visit_types`] visits.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        map_type_params(&mut self.type_params, map);
        self.return_ty = map(self.return_ty);
        for local in &mut self.locals {
            local.ty = map(local.ty);
        }
        map_block_types(&mut self.blocks, map);
    }
}

impl MirClosure {
    /// Visit every type id of the closure: return type, locals, captures and
    /// its blocks' types.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit(self.return_ty);
        for local in &self.locals {
            visit(local.ty);
        }
        for capture in &self.captures {
            visit(capture.ty);
        }
        visit_block_types(&self.blocks, visit);
    }

    /// Rewrite every type id [`Self::visit_types`] visits.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        self.return_ty = map(self.return_ty);
        for local in &mut self.locals {
            local.ty = map(local.ty);
        }
        for capture in &mut self.captures {
            capture.ty = map(capture.ty);
        }
        map_block_types(&mut self.blocks, map);
    }
}

impl MirClass {
    /// Visit every type id of the class declaration.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit_type_params(&self.type_params, visit);
        for arg in &self.base_args {
            visit(*arg);
        }
        for field in &self.fields {
            visit(field.ty);
        }
        for field in &self.static_fields {
            visit(field.ty);
        }
        for descriptor in &self.descriptors {
            visit(descriptor.read_ty);
            descriptor.write_ty.visit_types(visit);
            for field in &descriptor.value_fields {
                visit(field.ty);
            }
        }
        for method in &self.abstract_methods {
            visit_method_sig(method, visit);
        }
    }

    /// Rewrite every type id [`Self::visit_types`] visits.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        map_type_params(&mut self.type_params, map);
        for arg in &mut self.base_args {
            *arg = map(*arg);
        }
        for field in &mut self.fields {
            field.ty = map(field.ty);
        }
        for field in &mut self.static_fields {
            field.ty = map(field.ty);
        }
        for descriptor in &mut self.descriptors {
            descriptor.read_ty = map(descriptor.read_ty);
            descriptor.write_ty.map_types(map);
            for field in &mut descriptor.value_fields {
                field.ty = map(field.ty);
            }
        }
        for method in &mut self.abstract_methods {
            map_method_sig(method, map);
        }
    }
}

impl MirInterface {
    /// Visit every type id of the interface declaration.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit_type_params(&self.type_params, visit);
        for heritage in &self.extends {
            heritage.args.visit_types(visit);
        }
        for field in &self.fields {
            visit(field.ty);
        }
        for method in &self.methods {
            visit_method_sig(method, visit);
        }
    }

    /// Rewrite every type id [`Self::visit_types`] visits.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        map_type_params(&mut self.type_params, map);
        for heritage in &mut self.extends {
            heritage.args.map_types(map);
        }
        for field in &mut self.fields {
            field.ty = map(field.ty);
        }
        for method in &mut self.methods {
            map_method_sig(method, map);
        }
    }
}

impl MirGlobal {
    /// Visit the global's type.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        visit(self.ty);
    }

    /// Rewrite the global's type.
    pub fn map_types(&mut self, map: &mut dyn FnMut(TypeId) -> TypeId) {
        self.ty = map(self.ty);
    }
}

impl Mir {
    /// Visit every type id in the program: every function, closure, class,
    /// interface and global.
    pub fn visit_types(&self, visit: &mut dyn FnMut(TypeId)) {
        for function in &self.functions {
            function.visit_types(visit);
        }
        for closure in &self.closures {
            closure.visit_types(visit);
        }
        for class in &self.classes {
            class.visit_types(visit);
        }
        for interface in &self.interfaces {
            interface.visit_types(visit);
        }
        for global in &self.globals {
            global.visit_types(visit);
        }
    }

    /// Rewrite every type id [`Self::visit_types`] visits, with access to the
    /// type table while doing so.
    ///
    /// The tables are taken out of `self` for the walk so `map` may intern
    /// through `types`; they are put back before this returns.
    pub fn map_types(&mut self, mut map_in: impl FnMut(&mut smelt_hir::TypeInterner, TypeId) -> TypeId) {
        let mut functions = std::mem::take(&mut self.functions);
        let mut closures = std::mem::take(&mut self.closures);
        let mut classes = std::mem::take(&mut self.classes);
        let mut interfaces = std::mem::take(&mut self.interfaces);
        let mut globals = std::mem::take(&mut self.globals);
        let types = &mut self.types;
        let mut map = |ty| map_in(types, ty);
        for function in &mut functions {
            function.map_types(&mut map);
        }
        for closure in &mut closures {
            closure.map_types(&mut map);
        }
        for class in &mut classes {
            class.map_types(&mut map);
        }
        for interface in &mut interfaces {
            interface.map_types(&mut map);
        }
        for global in &mut globals {
            global.map_types(&mut map);
        }
        self.functions = functions;
        self.closures = closures;
        self.classes = classes;
        self.interfaces = interfaces;
        self.globals = globals;
    }
}
