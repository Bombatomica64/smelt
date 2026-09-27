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
//! * A source-declared base class (plain, generic, or abstract) runs its
//!   constructor as an INITIALIZER over the derived `this`
//!   ([`ModuleBuilder::lower_base_constructor_init`]). JavaScript has one object:
//!   the base constructor's field initializers, parameter properties and body
//!   all run on the instance the derived constructor is building, so closures it
//!   stores capture the derived instance and virtual calls reach the derived
//!   overrides. Codegen re-emits the base constructor body over the derived
//!   `Self` (the way inherited methods are re-emitted), and the base's own
//!   `super(...)` is in turn an initializer call — which is what makes the
//!   lowering compose with multi-level inheritance. The previous lowering
//!   constructed a SEPARATE base value and copied its fields into `this`, which
//!   left base-created closures bound to the discarded base object and could not
//!   run a generic base at all (`blocker-logs/hono-phase3-round5.md`, round 8).
//! * An `Error`-like host base has no Smelt class body to run, so its documented
//!   instance slots are assigned directly
//!   ([`ModuleBuilder::lower_error_base_super_call`]). The slot list is shared
//!   with the field injection in `functions.rs`, so `class X extends Error` and
//!   every standard error subclass go through one rule.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, Expression, Statement};
use smelt_hir::{
    Body, Expr, ExprKind, Field, Literal, Span, Stmt, Type,
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
    ErrorMarkerField {
        name: "name",
        shape: ErrorMarkerShape::Text,
        spec_default: ErrorSpecDefault::BaseName,
    },
    ErrorMarkerField {
        name: "message",
        shape: ErrorMarkerShape::Text,
        spec_default: ErrorSpecDefault::EmptyText,
    },
    // `stack?: string` in lib.d.ts: hosts may omit it, so the slot is an
    // optional string, matching what a `.stack` read already resolves to.
    ErrorMarkerField {
        name: "stack",
        shape: ErrorMarkerShape::OptionalText,
        spec_default: ErrorSpecDefault::EmptyText,
    },
    ErrorMarkerField {
        name: "cause",
        shape: ErrorMarkerShape::Dynamic,
        spec_default: ErrorSpecDefault::None,
    },
];

/// One inherited `Error` instance slot and the shape it carries.
pub(in crate::lowering) struct ErrorMarkerField {
    /// The property name as JavaScript spells it.
    pub(in crate::lowering) name: &'static str,
    /// The HIR shape the slot is declared with.
    pub(in crate::lowering) shape: ErrorMarkerShape,
    /// What the spec writes into the slot when the constructor argument is
    /// absent.
    pub(in crate::lowering) spec_default: ErrorSpecDefault,
}

/// What an `Error` base writes into one slot when its argument is absent.
///
/// `new Error()` leaves `message` the EMPTY STRING, not `undefined`, and the
/// same default answers the case where the argument is *supplied but optional*
/// (`super(options?.message)`): the callee's spec defaults an absent argument,
/// so the coercion into the required slot is a defaulting, not a narrowing
/// assertion. Keeping the default in the slot table means the absent-argument
/// path and the optional-argument path cannot disagree.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum ErrorSpecDefault {
    /// The base constructor's own name (`Error.prototype.name`).
    BaseName,
    /// The empty string (`message`, `stack`).
    EmptyText,
    /// No default: an absent argument leaves the slot unwritten (`cause`).
    None,
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
        let Some((base, _)) = self.classes.base(class_text).cloned() else {
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
        // A base that is neither an `Error`-like host constructor nor a
        // source-declared class (an unmodeled host constructor) keeps the
        // historical drop: there is no body to run.
        let error_base = is_error_like_base(&base_name);
        if !error_base && !self.class_is_initializable_base(base) {
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
            Self::lower_base_constructor_init(base, &arguments, this_local, class_ty, span, body);
        }
        Ok(())
    }

    /// Run a source-declared base constructor over `this`: `this = Base.init(this, args)`.
    ///
    /// Emits [`ExprKind::BaseConstructorInit`] and assigns its result back to the
    /// constructor's `this`, so the receiver passes through the base's
    /// initialization and every later statement of the derived constructor sees
    /// the initialised instance. Nothing about the base's fields is decided here:
    /// the base constructor body itself (parameter defaults, field initializers,
    /// parameter properties, statements, and its own `super(...)`) is what runs,
    /// re-emitted by codegen over the derived receiver. That is also why a
    /// generic base needs no special handling — no base VALUE is ever built, so
    /// its type arguments never have to fit the derived class's flattened layout.
    pub(in crate::lowering) fn lower_base_constructor_init(
        base: smelt_hir::Symbol,
        arguments: &[smelt_hir::ExprId],
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) {
        let receiver = body.push_expr(Expr {
            kind: ExprKind::Local(this_local),
            ty: class_ty,
            span,
        });
        let initialised = body.push_expr(Expr {
            kind: ExprKind::BaseConstructorInit {
                base,
                receiver,
                args: arguments.to_vec(),
            },
            ty: class_ty,
            span,
        });
        let target = body.push_expr(Expr {
            kind: ExprKind::Local(this_local),
            ty: class_ty,
            span,
        });
        body.push_stmt(Stmt::Assign {
            target,
            value: initialised,
        });
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
    pub(in crate::lowering) fn lower_error_base_super_call(
        &mut self,
        base_name: &str,
        arguments: &[smelt_hir::ExprId],
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        class_text: &str,
        span: Span,
        body: &mut Body,
    ) {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
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
        // Positionally paired with `ERROR_MARKER_FIELDS`, so the slot an
        // argument is written to and the type that slot was declared with
        // cannot drift. `None` means the constructor supplied nothing for the
        // slot, and the slot's own spec default (if it has one) answers it.
        let supplied = [
            None,
            arguments.first().copied(),
            None,
            cause_value,
        ];
        for (marker, supplied) in ERROR_MARKER_FIELDS.into_iter().zip(supplied) {
            let field = self.ctx.krate.symbols.intern(marker.name);
            let field_ty = self
                .declared_class_field_ty(class_text, field)
                .unwrap_or_else(|| self.error_marker_shape_ty(marker.shape));
            let value = match supplied {
                Some(value) => {
                    self.error_slot_value(value, field_ty, marker.spec_default, base_name, span, body)
                }
                None => match self.error_spec_default_expr(marker.spec_default, base_name, span, body)
                {
                    Some(default) => default,
                    None => continue,
                },
            };
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

    /// The value written into one `Error` slot for a supplied argument.
    ///
    /// A supplied argument whose own type is still OPTIONAL, written into a
    /// slot that is not, is the `super(options?.message)` shape: the callee's
    /// spec defaults an absent argument (`new Error(undefined).message` is
    /// `""`), so the coercion is a defaulting and not a narrowing assertion.
    /// Without the coalesce the assignment fell through to the emitter's
    /// optional-to-required coercion, which asserts presence and panicked
    /// ("optional value was absent after narrowing") for every `Error`
    /// subclass forwarding an optional message.
    ///
    /// A slot with no spec default (`cause`) keeps the argument as-is: there is
    /// nothing to default to, and its slot is erased anyway.
    fn error_slot_value(
        &mut self,
        value: smelt_hir::ExprId,
        field_ty: smelt_hir::TypeId,
        spec_default: ErrorSpecDefault,
        base_name: &str,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let value_ty = Self::expr_ty(body, value);
        let value_is_optional = matches!(
            self.ctx.krate.types.get(value_ty),
            Some(Type::Optional(_))
        );
        let slot_is_optional = matches!(
            self.ctx.krate.types.get(field_ty),
            Some(Type::Optional(_))
        );
        if !value_is_optional || slot_is_optional {
            return value;
        }
        let Some(fallback) = self.error_spec_default_expr(spec_default, base_name, span, body) else {
            return value;
        };
        body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce {
                optional: value,
                fallback,
            },
            ty: field_ty,
            span,
        })
    }

    /// Build the expression an `Error` slot's spec default writes.
    ///
    /// `None` for a slot the spec leaves unwritten when its argument is absent.
    fn error_spec_default_expr(
        &mut self,
        spec_default: ErrorSpecDefault,
        base_name: &str,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let text = match spec_default {
            ErrorSpecDefault::BaseName => base_name.to_owned(),
            ErrorSpecDefault::EmptyText => String::new(),
            ErrorSpecDefault::None => return None,
        };
        let string_ty = self.ctx.krate.types.intern(Type::String);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(text)),
            ty: string_ty,
            span,
        }))
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

    /// Return whether a base symbol resolves to a source-declared class whose
    /// constructor `super(...)` can run over the derived instance.
    ///
    /// Plain, generic and abstract classes all qualify: the constructor runs as an
    /// initializer over the derived receiver
    /// ([`Self::lower_base_constructor_init`]), so neither "MIR cannot construct
    /// an abstract class" nor "a constructed `Base<string>` does not fit the
    /// derived layout" — the two reasons the old construct-and-copy lowering
    /// excluded them — applies.
    ///
    /// Keyed on the base's resolved SYMBOL rather than its source spelling: a
    /// class renamed for a cross-module collision (`Store_1`) is registered by
    /// name under the source spelling it shares with the class that displaced
    /// it, so a by-name lookup of an import-aliased base answered with the
    /// importing module's own subclass (or with nothing at all, which silently
    /// dropped the `super(...)` call).
    pub(in crate::lowering) fn class_is_initializable_base(&self, base: smelt_hir::Symbol) -> bool {
        self.class_by_symbol(base).is_some()
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
