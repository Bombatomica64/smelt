//! Inherited mutable statics of an ordinary (module-level) subclass.
//!
//! JavaScript resolves a static member read through the class's prototype
//! chain, and a static write creates an own property of the class written:
//!
//! ```ts
//! class Base { static x = 1; }
//! class Sub extends Base {}
//! Sub.x;      // 1, Base's: Sub has no own `x` yet (and follows `Base.x = 2`)
//! Sub.x = 5;  // Sub's own `x`; Base.x is unchanged
//! ```
//!
//! A static the class declares is one module global (`StaticField::storage`).
//! A mutable static the class INHERITS gets the same accessor pair a
//! per-evaluation subclass frame gives it (`decls::evaluation_statics`,
//! `InheritedStatic`), over two module globals instead of two frame fields:
//!
//! - `<Sub>__smelt_own_<x>: bool`, whether `Sub` has its own `x` yet;
//! - `<Sub>__smelt_shadow_<x>`, that own value (`undefined` until written);
//! - `Sub::__smelt_get_<x>()`: the own value once written, otherwise the base
//!   class's read of `x` (its storage, or its own getter two levels down);
//! - `Sub::__smelt_set_<x>(value)`: stores the own value, sets the flag and
//!   answers the value, so `Sub.x = v` composes as an expression.
//!
//! The pair is recorded as a [`StaticField`] of `Sub` with
//! [`StaticField::inherited`] set, so every reader of a class's statics finds
//! it: direct reads, writes and updates (`specialization`, `stmt::assignments`),
//! and the class value's static hooks in codegen. A readonly static is never
//! written, so it keeps resolving through the chain to its declaration.

use smelt_hir::{
    Body, Expr, ExprKind, InheritedStaticAccess, Item, Literal, LocalDecl, Param, Span,
    StaticField, Stmt, Type, Visibility,
};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// The inherited mutable statics of the class `class_name` (registered as
    /// `class_text`) extending `base`, as static fields carrying their
    /// accessor pair, plus the accessor functions to add to the class's
    /// static methods. `own` are the statics the class declares itself,
    /// which shadow an ancestor's of the same name.
    pub(in crate::lowering) fn inherited_static_fields(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        base: Option<smelt_hir::Symbol>,
        own: &[StaticField],
        span: Span,
    ) -> Result<(Vec<StaticField>, Vec<smelt_hir::ItemId>), SmeltError> {
        let Some(base_class) = base.and_then(|base| self.class_by_symbol(base)) else {
            return Ok((Vec::new(), Vec::new()));
        };
        // The base's statics already list everything IT inherits as its own
        // accessor-backed entries, so one level answers the whole chain.
        let candidates = base_class
            .static_fields
            .iter()
            .filter(|field| {
                field.mutable
                    && field.storage.is_some()
                    && field.visibility != Visibility::Hidden
                    && !own.iter().any(|declared| declared.name == field.name)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut fields = Vec::with_capacity(candidates.len());
        let mut methods = Vec::with_capacity(candidates.len() * 2);
        for base_field in candidates {
            let (field, getter, setter) =
                self.inherited_static_field(class_text, class_name, &base_field, span)?;
            fields.push(field);
            methods.push(getter);
            methods.push(setter);
        }
        Ok((fields, methods))
    }

    /// One inherited static: its two globals, its getter and setter, and the
    /// static field recording them.
    fn inherited_static_field(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        base_field: &StaticField,
        span: Span,
    ) -> Result<(StaticField, smelt_hir::ItemId, smelt_hir::ItemId), SmeltError> {
        let spelling = super::super::support::source_spelling(&self.ctx.krate, base_field.name)
            .unwrap_or("field")
            .to_owned();
        let ty = base_field.ty;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        // `undefined` until the class writes its own value; a static whose
        // type admits `undefined` already is its own shadow type.
        let shadow_ty = if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
            ty
        } else {
            self.ctx.krate.types.intern(Type::Optional(ty))
        };
        let own_name = self.intern_synthesized_name(&format!("{class_text}__smelt_own_{spelling}"));
        let own = self
            .ctx
            .krate
            .push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                name: own_name,
                ty: bool_ty,
                init: smelt_hir::MutableGlobalInit::Literal(Literal::Bool(false)),
                visibility: Visibility::Public,
                span,
            }));
        let shadow_name =
            self.intern_synthesized_name(&format!("{class_text}__smelt_shadow_{spelling}"));
        let shadow = self
            .ctx
            .krate
            .push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                name: shadow_name,
                ty: shadow_ty,
                init: smelt_hir::MutableGlobalInit::Literal(Literal::Undefined),
                visibility: Visibility::Public,
                span,
            }));
        let getter = self.inherited_static_getter(
            class_name, &spelling, base_field, own, shadow, shadow_ty, span,
        )?;
        let setter =
            self.inherited_static_setter(class_name, &spelling, ty, own, shadow, shadow_ty, span);
        let field = StaticField {
            name: base_field.name,
            ty,
            visibility: base_field.visibility,
            value: None,
            mutable: true,
            storage: Some(shadow),
            inherited: Some(InheritedStaticAccess {
                own,
                getter,
                setter,
            }),
            span,
        };
        Ok((field, getter, setter))
    }

    /// `static __smelt_get_x(): T { if (own) { return shadow; } return Base.x; }`
    #[expect(
        clippy::too_many_arguments,
        reason = "the accessor reads every piece of the inherited static it was built from"
    )]
    fn inherited_static_getter(
        &mut self,
        class_name: smelt_hir::Symbol,
        spelling: &str,
        base_field: &StaticField,
        own: smelt_hir::ItemId,
        shadow: smelt_hir::ItemId,
        shadow_ty: smelt_hir::TypeId,
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let name = self.intern_synthesized_name(&format!("__smelt_get_{spelling}"));
        let mut body = Body::new(None, span);
        let has_own = body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item: own },
            ty: bool_ty,
            span,
        });
        let then_block = body.push_block(span);
        let own_value = body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item: shadow },
            ty: shadow_ty,
            span,
        });
        body.push_stmt_to_block(then_block, Stmt::Return(Some(own_value)));
        body.push_stmt(Stmt::If {
            cond: has_own,
            then_block,
            else_block: None,
        });
        let inherited = self
            .static_field_read(base_field, span, &mut body)?
            .ok_or_else(|| {
                SmeltError::unsupported(span, "an inherited static has no readable value")
            })?;
        body.push_stmt(Stmt::Return(Some(inherited)));
        let body = self.ctx.krate.push_body(body);
        Ok(self
            .ctx
            .krate
            .push_item(Item::Function(smelt_hir::Function {
                name,
                span,
                type_params: Vec::new(),
                params: Vec::new(),
                rest: None,
                required_params: None,
                return_ty: base_field.ty,
                is_async: false,
                is_test: false,
                body: Some(body),
                owner: smelt_hir::FunctionOwner::ClassStaticMethod {
                    class: class_name,
                    method: name,
                },
            })))
    }

    /// `static __smelt_set_x(value: T): T { shadow = value; own = true; return value; }`
    #[expect(
        clippy::too_many_arguments,
        reason = "the accessor writes every piece of the inherited static it was built from"
    )]
    fn inherited_static_setter(
        &mut self,
        class_name: smelt_hir::Symbol,
        spelling: &str,
        ty: smelt_hir::TypeId,
        own: smelt_hir::ItemId,
        shadow: smelt_hir::ItemId,
        shadow_ty: smelt_hir::TypeId,
        span: Span,
    ) -> smelt_hir::ItemId {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let name = self.intern_synthesized_name(&format!("__smelt_set_{spelling}"));
        let value_symbol = self.ctx.krate.symbols.intern("value");
        let mut body = Body::new(None, span);
        let value_local = body.push_local(LocalDecl {
            name: Some(value_symbol),
            ty,
            mutable: false,
            span,
        });
        body.params.push(value_local);
        let stored = body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty,
            span,
        });
        let store = body.push_expr(Expr {
            kind: ExprKind::GlobalSet {
                item: shadow,
                value: stored,
            },
            ty: shadow_ty,
            span,
        });
        body.push_stmt(Stmt::Expr(store));
        let flag = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        let mark = body.push_expr(Expr {
            kind: ExprKind::GlobalSet {
                item: own,
                value: flag,
            },
            ty: bool_ty,
            span,
        });
        body.push_stmt(Stmt::Expr(mark));
        let answer = body.push_expr(Expr {
            kind: ExprKind::Local(value_local),
            ty,
            span,
        });
        body.push_stmt(Stmt::Return(Some(answer)));
        let body = self.ctx.krate.push_body(body);
        self.ctx
            .krate
            .push_item(Item::Function(smelt_hir::Function {
                name,
                span,
                type_params: Vec::new(),
                params: vec![Param {
                    name: value_symbol,
                    local: value_local,
                    ty,
                    span,
                }],
                rest: None,
                required_params: None,
                return_ty: ty,
                is_async: false,
                is_test: false,
                body: Some(body),
                owner: smelt_hir::FunctionOwner::ClassStaticMethod {
                    class: class_name,
                    method: name,
                },
            }))
    }
}
