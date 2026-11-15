//! Identity slots: the one object a module-level object declaration evaluates
//! to, shared by every value read of it.
//!
//! # The rule
//!
//! `const eo = { code: "eo", ... }` declares ONE object. Every reader that takes
//! the binding as a value (`setDefaultOptions({ locale: eo })`, `x === eo`,
//! `const table = formatters`) gets that same object, so `getDefaultOptions()
//! .locale === eo` holds and a write through one alias is seen through the
//! other. The folded-constant paths (object constants, const items, object
//! namespaces of methods) recreate the literal at each use site instead, which
//! is right for a member read they resolve statically (`eo.code`,
//! `formatters.y(...)`) but makes a value read a fresh object with equal
//! contents: identity comparisons answered `false` and date-fns'
//! `setDefaultOptions` specs failed.
//!
//! A value read therefore resolves through a module slot (`GlobalGet` of an
//! [`smelt_hir::Item::MutableGlobal`]) whose lazy initializer is the very
//! expression the use site used to inline. The slot is keyed by the
//! declaration ([`IdentitySlotKey`]) in the crate-wide context, so the
//! declaring module and every importer, under any alias or re-export, read the
//! same object. Only types with reference identity take a slot; a primitive
//! re-read is indistinguishable from the original.
//!
//! The initializer is side-effect free for an object constant or namespace
//! (static literals and function items), and for a const item it is the
//! initializer the source evaluates once, so evaluating it lazily on first read
//! is the source's single evaluation (a correct program cannot read a `const`
//! before its declaration ran).

use super::super::ModuleBuilder;
use crate::SmeltError;
use crate::context::IdentitySlotKey;
use smelt_hir::{Body, Expr, ExprId, ExprKind, Item, ItemId, Stmt, Type, TypeId, Visibility};

impl ModuleBuilder<'_> {
    /// Whether a value of `ty` has JavaScript reference identity that a
    /// re-materialized copy would lose: an object, array, tuple, `Set` or `Map`.
    ///
    /// A class type takes a slot only when it renders as a concrete Rust type
    /// (a declared class or interface, or a modelled host class). One the
    /// program does not declare (`IArguments`) is an erased value either way,
    /// and a slot would only add a second erased binding for it.
    ///
    pub(in crate::lowering) fn has_reference_identity(&mut self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Dict(..) | Type::List(..) | Type::Tuple(..) | Type::Set(..) | Type::JsMap(..),
            ) => true,
            Some(Type::Class { name, args }) => {
                let (name, args) = (*name, args.clone());
                self.class_by_symbol(name).is_some()
                    || self.find_interface(name).is_some()
                    || self
                        .json_class_fields(name, &args)
                        .is_some_and(|fields| !fields.is_empty())
                    || self
                        .ctx
                        .krate
                        .symbols
                        .get(name)
                        .is_some_and(|text| smelt_stdlib::typescript_stdlib_class(text).is_some())
            }
            _ => false,
        }
    }

    /// Known gap, for a const ITEM only: a value whose SOURCE annotation holds
    /// an erased part (`export const falsey: unknown[]`, recorded in
    /// `HirCtx::source_erased_consts`) keeps being rebuilt per read.
    /// Its slot's initializer would be one more erased binding than the inline
    /// reads it replaces, which the typedness ratchet forbids; such a value is
    /// therefore not yet `===` to itself across reads. A folded object
    /// constant keeps its slot whatever its metadata type (date-fns' locales).
    ///
    /// Whether `ty` holds `unknown` anywhere in its element, entry, field-less
    /// tuple or function signature types (`unknown[]`, `Record<string,
    /// unknown>`). Bounded so a recursive type terminates.
    pub(in crate::lowering) fn type_holds_erasure(&self, ty: TypeId, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        let deeper = |inner: TypeId| self.type_holds_erasure(inner, depth + 1);
        match self.ctx.krate.types.get(ty) {
            Some(Type::Unknown) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => deeper(*item),
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                deeper(*key) || deeper(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items.iter().any(|item| deeper(*item)),
            Some(Type::Function(function)) => {
                function.params.iter().any(|param| deeper(*param)) || deeper(function.return_ty)
            }
            _ => false,
        }
    }

    /// Read the object declared at `key` through its shared module slot,
    /// creating the slot (with `build` as its lazy initializer) on first use.
    ///
    /// `build` lowers the declaration's value into the initializer body it is
    /// given; it runs once per crate, not once per read. `ty` is the value's
    /// type when the caller knows it before building; `None` takes the type of
    /// the expression `build` returns.
    pub(in crate::lowering) fn identity_slot_read(
        &mut self,
        key: IdentitySlotKey,
        name: &str,
        ty: Option<TypeId>,
        span: smelt_hir::Span,
        body: &mut Body,
        build: impl FnOnce(&mut Self, &mut Body) -> Result<ExprId, SmeltError>,
    ) -> Result<ExprId, SmeltError> {
        let item = match self.ctx.identity_slots.get(&key).copied() {
            Some(item) => item,
            None => self.create_identity_slot(key, name, ty, span, build)?,
        };
        let slot_ty = match self.item_ref(item) {
            Item::MutableGlobal(global) => global.ty,
            _ => self.erase_with(smelt_hir::ErasureReason::ByName, "identity slot"),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item },
            ty: slot_ty,
            span,
        }))
    }

    /// Register the slot for `key` and install its initializer.
    fn create_identity_slot(
        &mut self,
        key: IdentitySlotKey,
        name: &str,
        ty: Option<TypeId>,
        span: smelt_hir::Span,
        build: impl FnOnce(&mut Self, &mut Body) -> Result<ExprId, SmeltError>,
    ) -> Result<ItemId, SmeltError> {
        let mut init_body = Body::new(None, span);
        let symbol = self.intern_synthesized_name(&format!("smelt_identity__{name}"));
        let items_before = self.ctx.krate.items.len();
        let push_slot = |this: &mut Self, slot_ty: TypeId| {
            let item =
                this.ctx
                    .krate
                    .push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                        name: symbol,
                        ty: slot_ty,
                        init: smelt_hir::MutableGlobalInit::Pending,
                        visibility: Visibility::Public,
                        span,
                    }));
            this.ctx.identity_slots.insert(key, item);
            item
        };
        // A known type registers the slot before the initializer is built, so
        // a declaration that reads itself as a value (through a nested
        // function) resolves to the slot rather than recursing.
        let (item, value, slot_ty) = if let Some(known_ty) = ty {
            let item = push_slot(self, known_ty);
            (item, build(self, &mut init_body)?, known_ty)
        } else {
            let value = build(self, &mut init_body)?;
            let built_ty = Self::expr_ty(&init_body, value);
            (push_slot(self, built_ty), value, built_ty)
        };
        init_body.push_stmt(Stmt::Return(Some(value)));
        self.install_global_initializer(item, name, init_body, slot_ty, span);
        self.ctx.identity_slot_log.push((key, items_before));
        Ok(item)
    }
}
