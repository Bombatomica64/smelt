//! TypeScript `enum` declaration lowering into const-folded member values.
//!
//! Smelt does not materialize a distinct HIR enum type. Instead it treats an
//! `enum` as a namespace of compile-time constants: each member folds to a
//! numeric or string [`Literal`], exactly as TypeScript itself computes enum
//! member values. The folded values are stored in
//! the module's constant registry (and mirrored into
//! [`crate::context::HirCtx::enum_members`] for later modules), so that
//! `EnumName.Member` reads and `case EnumName.Member:` switch labels can inline
//! the same literal a manual `const` would produce.
//!
//! This keeps enum usage flowing through the general const-folding rules rather
//! than a per-enum special case: a member is registered only when its value can
//! be resolved at compile time (an explicit literal/foldable initializer, an
//! implicit auto-increment from the previous numeric member, or a reference to
//! an already-folded member of the same enum). Members whose value cannot be
//! resolved statically are simply not registered, so a later reference falls
//! through to the honest unsupported-lowering blocker instead of silently
//! producing a wrong value.
//!
//! A TypeScript `enum` also names a *type* (`function f(c: Color)`). Since an
//! enum is lowered to its member literals, its natural underlying type is the
//! primitive those literals share — `Float` for a numeric enum, `String` for a
//! string enum. When every folded member shares one primitive, the enum name is
//! registered as a HIR type alias to that primitive so `c: Color` lowers to the
//! concrete `f64`/`String` and a `switch (c)` over folded numeric/string case
//! labels type-checks. Mixed or unresolved enums register no alias and keep
//! their opaque type.

use crate::lowering::js_text::{has_lone_surrogate, js_str_text};
use oxc::ast::ast::{ComputedMemberExpression, Expression, TSEnumDeclaration, TSEnumMemberName};
use std::borrow::Cow;

use crate::lowering::{ConstLiteral, ModuleBuilder};
use crate::error::SmeltError;
use smelt_hir::{Body, Expr, ExprId, ExprKind, Item, Literal, Type, TypeAlias};


impl ModuleBuilder<'_> {
    /// Const-fold every statically-resolvable member of an `enum` declaration
    /// and register the results under the enum's name.
    ///
    /// Mirrors TypeScript's own member-value computation:
    /// - An explicit initializer is folded through the shared const-expression
    ///   folder ([`Self::literal_const_expression`]); numeric results also
    ///   advance the auto-increment counter to `value + 1`.
    /// - A member without an initializer takes the running auto-increment value
    ///   (starting at `0`), then advances the counter.
    /// - A member initializer may reference an earlier member of the same enum
    ///   (`A = 1, B = A`); those references resolve against members folded so
    ///   far in this call.
    ///
    /// Registration is best-effort: any member whose value cannot be resolved
    /// at compile time is skipped so its later use produces the ordinary
    /// unsupported blocker rather than an incorrect constant.
    pub(in crate::lowering) fn collect_enum_declaration(&mut self, decl: &TSEnumDeclaration<'_>) {
        self.collect_enum_members(decl, true);
    }

    /// [`Self::collect_enum_declaration`], publishing the folded members to
    /// later modules only when `publish` (a module-level declaration they can
    /// import).
    pub(in crate::lowering) fn collect_enum_members(&mut self, decl: &TSEnumDeclaration<'_>, publish: bool) {
        let enum_name = decl.id.name.to_string();
        let mut reverse = Vec::new();
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let mut members: std::collections::HashMap<String, ConstLiteral> =
            std::collections::HashMap::new();
        // Running value for members declared without an initializer. TypeScript
        // starts implicit numbering at 0 and increments after each numeric
        // member (implicit or explicit numeric).
        let mut next_auto = 0.0_f64;

        for member in &decl.body.members {
            let Some(member_name) = Self::enum_member_name(&member.id) else {
                continue;
            };

            let value = if let Some(initializer) = &member.initializer {
                // Unresolvable initializer: skip this member. A string member
                // (or any non-numeric value) also stops implicit auto-increment
                // in TypeScript, but since we could not fold it we leave the
                // counter untouched and let downstream code report the gap.
                let Some(value) = self.enum_member_initializer_literal(initializer, &members) else {
                    continue;
                };
                value
            } else {
                let literal = ConstLiteral {
                    literal: Literal::Float(next_auto),
                    ty: float_ty,
                };
                next_auto += 1.0_f64;
                literal
            };

            // Keep the auto-increment counter aligned after an explicit numeric
            // member so a following implicit member continues from `value + 1`.
            if let Literal::Float(number) = value.literal {
                next_auto = number + 1.0_f64;
            }

            if let Literal::Float(number) = value.literal {
                reverse.push((number, member_name.to_string()));
            }
            members.insert(member_name.into_owned(), value);
        }

        if members.is_empty() {
            return;
        }
        // TypeScript gives a numeric member a reverse entry (`E[0] === "A"`);
        // a string member has none.
        self.enum_reverse_names.insert(enum_name.clone(), reverse);

        // Register the enum name as a type alias to the primitive its members
        // share, so `c: EnumName` annotations lower to `f64`/`String` and the
        // switch scrutinee matches the folded case labels.
        self.register_enum_underlying_type(decl, &enum_name, &members);

        // Publish to the current module for immediate use, and to the shared
        // context so later modules that import this enum fold its members too.
        self.consts.set_enum_members(enum_name.clone(), members.clone());
        if publish {
            self.ctx.enum_members.write().insert(enum_name, members);
        }
    }

    /// Lower `E[key]` on a numeric member of an `enum` declared in this module
    /// to the member's name, the reverse mapping TypeScript emits for every
    /// numeric member (`enum Color { Red }` defines `Color[0] = "Red"`).
    ///
    /// A key that folds to a constant reads its entry statically; any other key
    /// indexes the reverse table `{ "0": "Red", .. }` as a `Record<number, string>`
    /// read would, which is how TypeScript types it (`string`). Two members with
    /// one value keep the later name, as the emitted assignments do. Returns
    /// `None` for every other computed read, including a string key (`E["Red"]`)
    /// and an enum name shadowed by a local binding.
    pub(in crate::lowering) fn enum_reverse_mapping_read(
        &mut self,
        member: &ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<ExprId>, SmeltError> {
        let Expression::Identifier(object) = member.object.without_parentheses() else {
            return Ok(None);
        };
        if self.scope.lookup(object.name.as_str()).is_some() {
            return Ok(None);
        }
        let Some(entries) = self.enum_reverse_names.get(object.name.as_str()).cloned() else {
            return Ok(None);
        };
        let mut table: Vec<(f64, String)> = Vec::new();
        for (value, name) in entries {
            match table.iter_mut().find(|(existing, _)| existing.to_bits() == value.to_bits()) {
                Some(entry) => entry.1 = name,
                None => table.push((value, name)),
            }
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(member.span.start, member.span.end);
        let folded = self.literal_const_expression(&member.expression).ok();
        match folded.map(|key| key.literal) {
            Some(Literal::Float(key)) => {
                let Some((_, name)) = table.iter().find(|(value, _)| value.to_bits() == key.to_bits()) else {
                    return Ok(None);
                };
                return Ok(Some(body.push_expr(Expr { kind: ExprKind::Literal(Literal::String(name.clone())), ty: string_ty, span })));
            }
            Some(_) => return Ok(None),
            None => {}
        }
        let index = self.expression(&member.expression, body)?;
        if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, index)), Some(Type::Float)) {
            return Ok(None);
        }
        let reverse_entries = table
            .into_iter()
            .map(|(value, member_name)| {
                let value_key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(Self::numeric_property_key_name(value))),
                    ty: string_ty,
                    span,
                });
                let name_value =
                    body.push_expr(Expr { kind: ExprKind::Literal(Literal::String(member_name)), ty: string_ty, span });
                (value_key, name_value)
            })
            .collect();
        let table_ty = self.ctx.krate.types.intern(Type::Dict(string_ty, string_ty));
        let reverse_table = body.push_expr(Expr { kind: ExprKind::DictLit(reverse_entries), ty: table_ty, span });
        Ok(Some(body.push_expr(Expr { kind: ExprKind::Index { receiver: reverse_table, index }, ty: string_ty, span })))
    }

    /// Register the enum name as a HIR type alias to its underlying primitive.
    ///
    /// Only registers when every folded member shares one primitive literal
    /// kind (all numeric or all string); a mixed or heterogeneous enum keeps its
    /// opaque type so no incorrect concrete type leaks. The alias mirrors an
    /// ordinary `type EnumName = number`-style declaration and is
    /// pushed into the shared crate so later modules resolve the imported enum's
    /// type identically.
    fn register_enum_underlying_type(
        &mut self,
        decl: &TSEnumDeclaration<'_>,
        enum_name: &str,
        members: &std::collections::HashMap<String, ConstLiteral>,
    ) {
        let all_float = members
            .values()
            .all(|value| matches!(value.literal, Literal::Float(_)));
        let all_string = members
            .values()
            .all(|value| matches!(value.literal, Literal::String(_)));
        let underlying = if all_float {
            Type::Float
        } else if all_string {
            Type::String
        } else {
            return;
        };
        let ty = self.ctx.krate.types.intern(underlying);
        let name_text = self.types.qualify(enum_name);
        let name = self.declared_type_name_symbol(enum_name, &name_text);
        let item = self.ctx.krate.push_item(Item::TypeAlias(TypeAlias {
            name,
            type_params: Vec::new(),
            ty,
            span: self.span(decl.span.start, decl.span.end),
            decode_facts: None,
        }));
        self.items.insert(name_text, item);
    }

    /// Fold an `enum` member initializer, resolving references to earlier
    /// members of the same enum against `members`.
    ///
    /// Returns `None` when the initializer is not a compile-time constant so the
    /// caller can skip registering the member.
    fn enum_member_initializer_literal(
        &mut self,
        initializer: &Expression<'_>,
        members: &std::collections::HashMap<String, ConstLiteral>,
    ) -> Option<ConstLiteral> {
        // `A = 1, B = A`: a bare identifier that names an earlier member folds
        // to that member's value. Checked before the general folder so an
        // enum-local name shadows any module const with the same spelling.
        if let Expression::Identifier(ident) = initializer
            && let Some(value) = members.get(ident.name.as_str())
        {
            return Some(value.clone());
        }
        self.literal_const_expression(initializer).ok()
    }

    /// Return the source spelling of a supported enum member name.
    ///
    /// Only plain identifier and string-literal member names have a stable
    /// runtime key that a `case EnumName.Member:` label or `EnumName.Member`
    /// read can reference; computed names are not lowered.
    fn enum_member_name<'a>(name: &'a TSEnumMemberName<'a>) -> Option<Cow<'a, str>> {
        match name {
            TSEnumMemberName::Identifier(ident) => Some(Cow::Borrowed(ident.name.as_str())),
            TSEnumMemberName::String(string) if has_lone_surrogate(string.value) => None,
            TSEnumMemberName::String(string) => Some(js_str_text(string.value)),
            TSEnumMemberName::ComputedString(_) | TSEnumMemberName::ComputedTemplateString(_) => {
                None
            }
        }
    }

    /// Resolve `EnumName.Member` to its const-folded member value, if known.
    ///
    /// Shared by the switch-label folder and the ordinary static-member reader
    /// so both inline the identical literal for an enum member reference.
    pub(in crate::lowering) fn enum_member_literal(
        &self,
        enum_name: &str,
        member_name: &str,
    ) -> Option<ConstLiteral> {
        self.consts.enum_member(enum_name, member_name).cloned()
    }
}
