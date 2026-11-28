//! A module namespace used as a value (design decision D2,
//! `docs/design/record-identity.md` §5 step 4).
//!
//! # The rule
//!
//! `import * as NS from "./m"` binds the module's namespace object. A member
//! read (`NS.x`, `NS.f(..)`) resolves statically to the export and never
//! needs the object. Every other use takes the object as a value: passing
//! `NS` on, `Object.keys(NS)`, `NS[k]` with a key of a literal-union type,
//! `typeof NS` as a parameter type. Such a value is a plain struct of the
//! module's value exports, one field per export in the namespace's own key
//! order (export names sorted by code unit, ES2015 §9.4.6), built where it is
//! used:
//!
//! ```ts
//! // m.ts
//! export const alpha = 1;
//! export function gamma(x: number): number { return x * 3; }
//! // main.ts
//! import * as NS from "./m";
//! use(NS);                        // use(MNamespace { alpha: 1.0, gamma: <fn> })
//! ```
//!
//! A hand port writes the same thing: a struct of the module's constants and
//! function pointers. The fields are typed by the exports' own types, so no
//! value of the namespace is erased; `Object.keys` of a namespace value is
//! the constant list of its export names (`object_projection_call`), and an
//! index by a literal-union key reads the struct's field by a typed match, as
//! for any record.
//!
//! # Why a copy is exact
//!
//! A namespace's properties are live bindings. A `const`, a function and a
//! `let` nothing reassigns can never change, so a copy taken at the use site
//! reads what the live binding would. A module-level `let`/`var` that is
//! reassigned (a lifted mutable global, `HirCtx::reassignable_globals`)
//! could change after the copy, so a namespace value of a module exporting one
//! is refused with a diagnostic naming the binding; its member reads
//! (`NS.counter`) stay live reads of the global. Writes through `NS` are a type
//! error in TypeScript already.
//!
//! # What keeps the erased fallback
//!
//! A namespace whose value exports include a class, a generic function or a
//! nested namespace (`export * as Q`), none of which a struct field can hold
//! as its own type yet, keeps the previous erased placeholder.

use std::hash::{Hash as _, Hasher as _};

use smelt_hir::{Body, Expr, ExprKind, Field, Item, Literal, Type, Visibility};

use super::ModuleBuilder;
use crate::SmeltError;
use crate::lowering::decls::types_iface::StructuralMembers;

/// One value export of a namespace: its export name and its item.
type NamespaceMember = (String, smelt_hir::ItemId);

impl ModuleBuilder<'_> {
    /// The value exports of the namespace bound to `namespace`, sorted by
    /// name, or `None` when the namespace holds an export a struct field
    /// cannot hold (see the module docs). Refuses a reassignable binding.
    fn namespace_value_members(
        &self,
        namespace: &str,
        span: smelt_hir::Span,
    ) -> Result<Option<Vec<NamespaceMember>>, SmeltError> {
        let prefix = format!("{namespace}.");
        let mut members = Vec::new();
        #[expect(
            clippy::iter_over_hash_type,
            reason = "the members are sorted by name below"
        )]
        for (key, item) in &self.items {
            let Some(name) = key.strip_prefix(&prefix) else {
                continue;
            };
            // A nested namespace re-exported through this one.
            if name.contains('.') {
                return Ok(None);
            }
            match self.item_ref(*item) {
                Item::Interface(_) | Item::TypeAlias(_) => {}
                Item::Function(function) if function.type_params.is_empty() => {
                    members.push((name.to_owned(), *item));
                }
                Item::Const(_) => members.push((name.to_owned(), *item)),
                Item::MutableGlobal(_) if self.ctx.reassignable_globals.contains(item) => {
                    return Err(SmeltError::unsupported(
                        span,
                        format!(
                            "`{namespace}` is used as a value, but its module exports the \
                             mutable binding `{name}` (`export let`): a namespace value is a \
                             copy of the module's exports and would not see a later \
                             reassignment of `{name}`. Read it as `{namespace}.{name}`, or \
                             declare it `const`."
                        ),
                    ));
                }
                Item::MutableGlobal(_) => members.push((name.to_owned(), *item)),
                Item::Function(_) | Item::Class(_) => return Ok(None),
            }
        }
        if members.is_empty() {
            return Ok(None);
        }
        members.sort_by(|(left, _), (right, _)| left.cmp(right));
        Ok(Some(members))
    }

    /// The struct type of the namespace bound to `namespace` (`typeof NS`, and
    /// the type of its value), registering the struct on first use, or `None`
    /// when the namespace keeps the erased fallback.
    pub(in crate::lowering) fn namespace_value_type(
        &mut self,
        namespace: &str,
        span: smelt_hir::Span,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if !self.imports.is_namespace(namespace) || self.scope.is_bound(namespace) {
            return Ok(None);
        }
        let Some(members) = self.namespace_value_members(namespace, span)? else {
            return Ok(None);
        };
        let mut fields = Vec::with_capacity(members.len());
        for (name, item) in &members {
            let ty = match self.item_ref(*item) {
                Item::Const(constant) => constant.ty,
                Item::MutableGlobal(global) => global.ty,
                _ => self.item_expr_type(*item, span)?,
            };
            if matches!(self.ctx.krate.types.get(ty), Some(Type::Unknown)) {
                return Ok(None);
            }
            fields.push(Field {
                name: self.intern_source_name(name),
                ty,
                visibility: Visibility::Public,
                optional: false,
                span,
            });
        }
        let name = self.namespace_struct_name(namespace, &fields);
        let keys = members.into_iter().map(|(key, _)| key).collect();
        self.ctx.namespace_structs.insert(name, keys);
        Ok(Some(self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        })))
    }

    /// The struct named for the namespace `namespace` with `fields`: the
    /// module's name in Pascal case (`PatternsNamespace` for
    /// `import * as P from "./patterns"`), registered once. A second module of
    /// the same name with other exports takes a name suffixed by a hash of its
    /// fields.
    fn namespace_struct_name(&mut self, namespace: &str, fields: &[Field]) -> smelt_hir::Symbol {
        let module = match self.imports.imported_name(namespace) {
            Some(imported) if imported != "*" => imported.to_owned(),
            _ => self.imports.import_source(namespace).map_or_else(
                || namespace.to_owned(),
                |source| {
                    let file = source.rsplit('/').next().unwrap_or(source);
                    let stem = file.split('.').next().unwrap_or(file);
                    if stem.is_empty() || stem == "index" {
                        source
                            .trim_end_matches(file)
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .filter(|dir| !dir.is_empty() && *dir != ".." && *dir != ".")
                            .unwrap_or(namespace)
                            .to_owned()
                    } else {
                        stem.to_owned()
                    }
                },
            ),
        };
        let mut pascal = String::new();
        let mut upper = true;
        for ch in module.chars() {
            if ch.is_ascii_alphanumeric() {
                if upper {
                    pascal.push(ch.to_ascii_uppercase());
                } else {
                    pascal.push(ch);
                }
                upper = false;
            } else {
                upper = true;
            }
        }
        if pascal.is_empty() || pascal.starts_with(|ch: char| ch.is_ascii_digit()) {
            pascal.insert_str(0, "Module");
        }
        let same_fields = |existing: &[Field]| {
            existing.len() == fields.len()
                && existing
                    .iter()
                    .zip(fields)
                    .all(|(left, right)| left.name == right.name && left.ty == right.ty)
        };
        let mut minted = smelt_hir::SynthesizedName::new(format!("{pascal}Namespace"));
        let mut symbol = self.intern_synthesized_type_name(&minted);
        if let Some(existing) = self.find_interface(symbol) {
            if same_fields(&existing.fields) {
                return symbol;
            }
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for field in fields {
                crate::lowering::support::source_spelling(&self.ctx.krate, field.name)
                    .hash(&mut hasher);
                field.ty.hash(&mut hasher);
            }
            minted = smelt_hir::SynthesizedName::new(format!(
                "{pascal}Namespace{:08x}",
                hasher.finish() & 0xffff_ffff
            ));
            symbol = self.intern_synthesized_type_name(&minted);
            if self.find_interface(symbol).is_some() {
                return symbol;
            }
        }
        let item = self.register_callable_object_interface(
            symbol,
            minted.as_str(),
            Vec::new(),
            StructuralMembers {
                fields: fields.to_vec(),
                ..Default::default()
            },
            self.span(0, 0),
        );
        self.items.insert(minted.as_str().to_owned(), item);
        symbol
    }

    /// The namespace bound to `namespace` as a value: its struct, built here
    /// from the module's exports (see the module docs). `None` keeps the
    /// caller's fallback.
    pub(in crate::lowering) fn namespace_value_expression(
        &mut self,
        namespace: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(start, end);
        let Some(ty) = self.namespace_value_type(namespace, span)? else {
            return Ok(None);
        };
        let Some(members) = self.namespace_value_members(namespace, span)? else {
            return Ok(None);
        };
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let mut entries = Vec::with_capacity(members.len());
        for (name, item) in members {
            let value = match self.item_ref(item).clone() {
                Item::Function(_) => {
                    self.item_function_closure_expression(item, start, end, body)?
                }
                Item::Const(constant) => self.const_item_expression(&constant, start, end, body)?,
                Item::MutableGlobal(global) => body.push_expr(Expr {
                    kind: ExprKind::GlobalGet { item },
                    ty: global.ty,
                    span,
                }),
                _ => return Ok(None),
            };
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(name)),
                ty: string_ty,
                span,
            });
            entries.push((key, value));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span,
        })))
    }

    /// `Object.keys(value)` of a namespace value: the constant list of its
    /// export names, in the namespace's key order. `None` for a value of any
    /// other type.
    pub(in crate::lowering) fn namespace_keys_expression(
        &mut self,
        value_ty: smelt_hir::TypeId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(value_ty) else {
            return None;
        };
        let keys = self.ctx.namespace_structs.get(name)?.clone();
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let items = keys
            .into_iter()
            .map(|key| {
                body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(key)),
                    ty: string_ty,
                    span,
                })
            })
            .collect();
        let ty = self.ctx.krate.types.intern(Type::List(string_ty));
        Some(body.push_expr(Expr {
            kind: ExprKind::ListLit(items),
            ty,
            span,
        }))
    }
}
