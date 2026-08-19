//! Derived-constructor `super(...)` lowering.
//!
//! Rust has no class inheritance, so Smelt flattens a derived class's struct to
//! carry its base's fields ahead of its own (see `effective_class_fields` in
//! `smelt-codegen-rust`). A derived constructor's `super(...)` call therefore has
//! no callee to defer to: nothing runs the base's initialization unless this
//! module emits it against the derived `this`.
//!
//! Two base kinds are handled, keyed on the *resolved base type* rather than on
//! anything about the derived class:
//!
//! * A source-declared base class constructs normally and its fields move into
//!   `this` ([`ModuleBuilder::lower_declared_base_super_call`]). Because the base
//!   is built through its own constructor, the base's parameter defaults, field
//!   initializers, side effects, and its own `super(...)` all run — which is what
//!   makes the lowering compose with multi-level inheritance for free.
//! * An `Error`-like host base has no Smelt class body to run, so its documented
//!   instance slots are assigned directly
//!   ([`ModuleBuilder::lower_error_base_super_call`]). The slot list is shared
//!   with the field injection in `functions.rs`, so `class X extends Error` and
//!   every standard error subclass go through one rule.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, Expression, Statement};
use smelt_hir::{
    Body, Expr, ExprKind, Field, Item, Literal, LocalDecl, Pattern, Span, Stmt, Type,
};

/// The instance slots every JavaScript `Error` subclass inherits from its base,
/// paired with the HIR type each one carries.
///
/// `Error.prototype` supplies `name`, the constructor supplies `message` and
/// (ES2022) `cause`, and hosts attach `stack`. They are declared in the order a
/// real base-class layout would contribute them.
///
/// **The asymmetry is deliberate — do not "make it consistent" by erasing the
/// first three.** TypeScript's own lib types are `name: string`,
/// `message: string`, `stack?: string`, `cause?: unknown`, and `tsc` rejects
/// source that violates them before Smelt runs (see the "Frontend validation
/// boundaries" rule in `AGENTS.md`). So three of these four slots are ordinary
/// strings, not dynamic boundaries, and Smelt's own `Error.name`/`.message`/
/// `.stack` read-path resolver already types them exactly this way
/// (`error_builtin_field_type` in `lowering/ty/annotations.rs`).
///
/// `cause` is the one real boundary: ES2022 types it `unknown` because it
/// carries an arbitrary thrown value, so it — and only it — stays
/// [`Type::Unknown`].
pub(in crate::lowering) const ERROR_MARKER_FIELDS: [ErrorMarkerField; 4] = [
    ErrorMarkerField { name: "name", shape: ErrorMarkerShape::Text },
    ErrorMarkerField { name: "message", shape: ErrorMarkerShape::Text },
    // `stack?: string` in lib.d.ts: hosts may omit it, so the slot is an
    // optional string, matching what a `.stack` read already resolves to.
    ErrorMarkerField { name: "stack", shape: ErrorMarkerShape::OptionalText },
    ErrorMarkerField { name: "cause", shape: ErrorMarkerShape::Dynamic },
];

/// One inherited `Error` instance slot and the shape it carries.
pub(in crate::lowering) struct ErrorMarkerField {
    /// The property name as JavaScript spells it.
    pub(in crate::lowering) name: &'static str,
    /// The HIR shape the slot is declared with.
    pub(in crate::lowering) shape: ErrorMarkerShape,
}

/// The HIR shape an inherited `Error` slot carries.
///
/// Only [`ErrorMarkerShape::Dynamic`] erases; see [`ERROR_MARKER_FIELDS`] for why
/// the other two must not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum ErrorMarkerShape {
    /// A plain `string` slot (`name`, `message`).
    Text,
    /// A `string | undefined` slot (`stack`).
    OptionalText,
    /// A genuinely dynamic slot carrying any thrown value (`cause`).
    Dynamic,
}

/// Return whether a resolved base-class name is an `Error`-like host constructor.
///
/// These constructors are modeled host builtins rather than source-declared
/// Smelt classes: they contribute the [`ERROR_MARKER_FIELDS`] layout and the
/// `message`/`name` constructor behaviour, but have no lowerable class body.
/// `DOMException` is included because runtimes without it alias it to `Error`
/// (es-toolkit's `_internal/DOMException.ts`), and Smelt models it as a host
/// object with the same error slots.
///
/// The predicate is deliberately a function of the *base* name only, so it
/// covers every `class Foo extends Error`/`extends TypeError`/… in any project
/// instead of recognizing particular subclass names.
pub(in crate::lowering) fn is_error_like_base(name: &str) -> bool {
    matches!(
        name,
        "Error"
            | "EvalError"
            | "RangeError"
            | "ReferenceError"
            | "SyntaxError"
            | "TypeError"
            | "URIError"
            | "AggregateError"
            | "DOMException"
    )
}

impl ModuleBuilder<'_> {
    /// Lower a constructor-body `super(...)` statement against the derived `this`.
    ///
    /// Returns `Ok(())` without emitting anything when the base cannot be
    /// resolved to either supported base kind (an unmodeled host constructor, a
    /// spread argument list, or a `super` shape that is not a plain call), which
    /// preserves the pre-existing "drop the call" behaviour for those cases
    /// rather than failing a whole build over them.
    pub(in crate::lowering) fn lower_super_call_statement(
        &mut self,
        statement: &Statement<'_>,
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        class_text: &str,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let Statement::ExpressionStatement(statement) = statement else {
            return Ok(());
        };
        let Expression::CallExpression(call) = &statement.expression else {
            return Ok(());
        };
        if !matches!(call.callee, Expression::Super(_)) {
            return Ok(());
        }
        // A spread argument list needs the base constructor's arity resolved
        // against a runtime-length list; leave it to the existing drop until the
        // general spread-into-call lowering covers constructors too.
        if call
            .arguments
            .iter()
            .any(|argument| matches!(argument, Argument::SpreadElement(_)))
        {
            return Ok(());
        }
        let Some((base, base_args)) = self.classes.base(class_text).cloned() else {
            return Ok(());
        };
        let Some(base_name) = self
            .ctx
            .krate
            .symbols
            .get(base)
            .map(ToOwned::to_owned)
        else {
            return Ok(());
        };
        // The base kind is decided before any argument is lowered: an unsupported
        // argument expression must not turn a previously-dropped `super(...)` into
        // a build failure.
        //
        // Bases this lowering cannot reproduce (abstract, generic) keep the
        // historical drop; see `class_is_reproducible_base` for why each is
        // excluded and what running their initialization would need instead.
        let error_base = is_error_like_base(&base_name);
        if !error_base && !self.class_is_reproducible_base(&base_name) {
            return Ok(());
        }
        let span = self.span(call.span.start, call.span.end);
        let mut arguments = Vec::new();
        for argument in &call.arguments {
            let Some(expression) = argument.as_expression() else {
                return Ok(());
            };
            arguments.push(self.expression(expression, body)?);
        }
        if error_base {
            self.lower_error_base_super_call(
                &base_name,
                &arguments,
                this_local,
                class_ty,
                class_text,
                span,
                body,
            );
        } else {
            self.lower_declared_base_super_call(
                base,
                &base_name,
                base_args,
                &arguments,
                this_local,
                class_ty,
                span,
                body,
            );
        }
        Ok(())
    }

    /// Run a source-declared base constructor and move its fields into `this`.
    ///
    /// The base is constructed through its own `ExprKind::New`, so every part of
    /// its initialization runs exactly once and in source order: parameter
    /// defaults, field initializers, parameter properties, statement side
    /// effects, and — for a base that itself extends something — its own
    /// `super(...)`. Multi-level inheritance therefore needs no special handling
    /// here: each level only ever reproduces its *immediate* base, and the
    /// flattened layouts agree because a base struct's fields are a prefix of the
    /// derived struct's.
    ///
    /// Callable slots synthesized for overridden base methods are skipped: those
    /// hold the *derived* class's method value, so copying the base's would
    /// re-bind virtual dispatch back to the base.
    #[expect(
        clippy::too_many_arguments,
        reason = "one emit site threading the resolved base, the receiver, and the call span"
    )]
    fn lower_declared_base_super_call(
        &mut self,
        base: smelt_hir::Symbol,
        base_name: &str,
        base_args: Vec<smelt_hir::TypeId>,
        arguments: &[smelt_hir::ExprId],
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) {
        let base_ty = self.ctx.krate.types.intern(Type::Class {
            name: base,
            args: base_args,
        });
        let constructed = body.push_expr(Expr {
            kind: ExprKind::New {
                class: base,
                args: arguments.to_vec(),
            },
            ty: base_ty,
            span,
        });
        let base_local_name = self.intern_source_name("__smelt_super");
        let base_local = body.push_local(LocalDecl {
            name: Some(base_local_name),
            ty: base_ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(base_local));
        body.push_stmt(Stmt::Let {
            pat,
            ty: base_ty,
            value: Some(constructed),
        });
        for field in self.inherited_base_fields(base_name) {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: field.name,
                },
                ty: field.ty,
                span,
            });
            let source = body.push_expr(Expr {
                kind: ExprKind::Local(base_local),
                ty: base_ty,
                span,
            });
            let value = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver: source,
                    field: field.name,
                },
                ty: field.ty,
                span,
            });
            body.push_stmt(Stmt::Assign { target, value });
        }
    }

    /// Assign the `Error` base constructor's instance slots on `this`.
    ///
    /// An `Error`-like base has no Smelt class body to run, so its observable
    /// constructor behaviour is reproduced directly: `message` comes from the
    /// first argument (JavaScript's `new Error()` leaves the empty string),
    /// `cause` from the ES2022 options argument's `cause` property, `name` is the
    /// base constructor's own name — exactly what `Error.prototype.name` /
    /// `DOMException.prototype.name` yield for a subclass that does not override
    /// it — and `stack` is the host-attached string.
    ///
    /// A slot the subclass redeclares keeps its declared type; otherwise the slot
    /// keeps the shape [`ERROR_MARKER_FIELDS`] declares for it. `name`, `message`,
    /// and `stack` are concretely typed strings, so nothing here erases — only
    /// `cause` is `Unknown`, because ES2022 types it `unknown` and it genuinely
    /// carries an arbitrary thrown value.
    #[expect(
        clippy::too_many_arguments,
        reason = "one emit site threading the resolved base, the receiver, and the call span"
    )]
    fn lower_error_base_super_call(
        &mut self,
        base_name: &str,
        arguments: &[smelt_hir::ExprId],
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        class_text: &str,
        span: Span,
        body: &mut Body,
    ) {
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let name_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(base_name.to_owned())),
            ty: string_ty,
            span,
        });
        let message_value = arguments.first().copied().unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: string_ty,
                span,
            })
        });
        let stack_value = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(String::new())),
            ty: string_ty,
            span,
        });
        let cause_value = arguments.get(1).copied().map(|options| {
            let field = self.ctx.krate.symbols.intern("cause");
            body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver: options,
                    field,
                },
                ty: unknown_ty,
                span,
            })
        });
        // Positionally paired with `ERROR_MARKER_FIELDS`, so the slot a value is
        // written to and the type that slot was declared with cannot drift.
        let values = [
            Some(name_value),
            Some(message_value),
            Some(stack_value),
            cause_value,
        ];
        for (marker, value) in ERROR_MARKER_FIELDS.into_iter().zip(values) {
            let Some(value) = value else {
                continue;
            };
            let field = self.ctx.krate.symbols.intern(marker.name);
            let field_ty = self
                .declared_class_field_ty(class_text, field)
                .unwrap_or_else(|| self.error_marker_shape_ty(marker.shape));
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field,
                },
                ty: field_ty,
                span,
            });
            body.push_stmt(Stmt::Assign { target, value });
        }
    }

    /// Intern the HIR type for an inherited `Error` slot's shape.
    ///
    /// The single place an [`ErrorMarkerShape`] becomes a `TypeId`, shared by the
    /// field injection and the `super(...)` assignment so a slot's declared type
    /// and its written type always agree.
    fn error_marker_shape_ty(&mut self, shape: ErrorMarkerShape) -> smelt_hir::TypeId {
        match shape {
            ErrorMarkerShape::Text => self.ctx.krate.types.intern(Type::String),
            ErrorMarkerShape::OptionalText => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                self.ctx.krate.types.intern(Type::Optional(string_ty))
            }
            ErrorMarkerShape::Dynamic => self.ctx.krate.types.intern(Type::Unknown),
        }
    }

    /// Return the type a class declares for `field`, if it declares it at all.
    ///
    /// A subclass may redeclare an inherited slot with a concrete type
    /// (`message: string`); the constructor-side assignment must then use that
    /// type rather than the erased host one.
    fn declared_class_field_ty(
        &self,
        class_text: &str,
        field: smelt_hir::Symbol,
    ) -> Option<smelt_hir::TypeId> {
        self.classes.fields(class_text)?
            .iter()
            .find(|declared| declared.name == field)
            .map(|declared| declared.ty)
    }

    /// Collect the flattened field layout a base class contributes to `this`.
    ///
    /// The walk mirrors codegen's `effective_class_fields`: base-most fields
    /// first, then each level's own fields, and the `Error` marker slots for a
    /// level whose own base is an `Error`-like host constructor (that injection
    /// happens after the per-class field map is published, so it is reproduced
    /// here rather than read back).
    ///
    /// Fields that name a method somewhere in the chain are omitted: those are
    /// the callable slots synthesized for virtual dispatch, and the derived
    /// class's own initialization owns them.
    fn inherited_base_fields(&mut self, base_name: &str) -> Vec<Field> {
        let mut chain: Vec<(String, Vec<Field>, Vec<smelt_hir::Symbol>)> = Vec::new();
        let mut cursor = Some(base_name.to_owned());
        while let Some(name) = cursor {
            if chain.iter().any(|(visited, _, _)| visited == &name) {
                break;
            }
            let Some(item) = self.classes.item(&name) else {
                break;
            };
            let Some((fields, method_items, abstract_methods, base, class_span)) =
                self.class_layout_parts(item)
            else {
                break;
            };
            let next = base
                .and_then(|base| self.ctx.krate.symbols.get(base))
                .map(ToOwned::to_owned);
            let mut method_names = method_items
                .iter()
                .filter_map(|method| {
                    let index = usize::try_from(method.0).unwrap_or(usize::MAX);
                    match self.ctx.krate.items.get(index) {
                        Some(Item::Function(function)) => Some(function.name),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            method_names.extend(abstract_methods);
            let fields = if next.as_deref().is_some_and(is_error_like_base) {
                self.with_error_marker_fields(fields, class_span)
            } else {
                fields
            };
            chain.push((name, fields, method_names));
            cursor = next;
        }
        let methods = chain
            .iter()
            .flat_map(|(_, _, methods)| methods.iter().copied())
            .collect::<Vec<_>>();
        chain
            .into_iter()
            .rev()
            .flat_map(|(_, fields, _)| fields)
            .filter(|field| !methods.contains(&field.name))
            .collect()
    }

    /// Return whether a name resolves to a class this lowering can reproduce.
    ///
    /// Two kinds are excluded:
    ///
    /// * Abstract classes, which MIR refuses to construct — matching
    ///   TypeScript's own `new AbstractClass()` error.
    /// * Generic classes, because a derived class's flattened layout erases the
    ///   base's type parameters: a constructed `Box<string>` carries a `String`
    ///   slot where the derived struct declares the erased one, so the field
    ///   moves would not type-check.
    fn class_is_reproducible_base(&self, class_text: &str) -> bool {
        let Some(item) = self.classes.item(class_text) else {
            return false;
        };
        let index = usize::try_from(item.0).unwrap_or(usize::MAX);
        matches!(
            self.ctx.krate.items.get(index),
            Some(Item::Class(class))
                if class.kind != smelt_hir::ClassKind::Abstract && class.type_params.is_empty()
        )
    }

    /// Read the layout-relevant parts of a lowered class item.
    ///
    /// Returns the class's own fields, its method items, its abstract method
    /// names, its declared base, and its span, all owned so the caller can keep
    /// mutating the crate while walking the base chain.
    fn class_layout_parts(
        &self,
        item: smelt_hir::ItemId,
    ) -> Option<(
        Vec<Field>,
        Vec<smelt_hir::ItemId>,
        Vec<smelt_hir::Symbol>,
        Option<smelt_hir::Symbol>,
        Span,
    )> {
        let index = usize::try_from(item.0).unwrap_or(usize::MAX);
        let Some(Item::Class(class)) = self.ctx.krate.items.get(index) else {
            return None;
        };
        Some((
            class.fields.clone(),
            class.methods.clone(),
            class
                .abstract_methods
                .iter()
                .map(|method| method.name)
                .collect(),
            class.base,
            class.span,
        ))
    }

    /// Prepend the `Error` marker slots a class is missing to its field layout.
    ///
    /// Shared by the class-declaration injection and the inherited-layout walk so
    /// both agree on the slot set, its order, and its erased host type.
    pub(in crate::lowering) fn with_error_marker_fields(
        &mut self,
        fields: Vec<Field>,
        class_span: Span,
    ) -> Vec<Field> {
        let mut injected = Vec::new();
        for marker in ERROR_MARKER_FIELDS {
            let symbol = self.ctx.krate.symbols.intern(marker.name);
            if fields.iter().any(|field| field.name == symbol) {
                continue;
            }
            injected.push(Field {
                name: symbol,
                ty: self.error_marker_shape_ty(marker.shape),
                visibility: smelt_hir::Visibility::Public,
                optional: false,
                span: class_span,
            });
        }
        // Inherited slots come first so the flattened layout mirrors a real base
        // class (`effective_class_fields` orders base fields before own fields).
        injected.extend(fields);
        injected
    }
}
