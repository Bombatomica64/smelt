//! Class construction: the synthesized default constructor of a derived class,
//! field and parameter-property initializers, and the error marker fields of
//! `Error` subclasses.

use super::classes::ClassFieldInit;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::decls::super_call;
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, Field, Function, FunctionOwner, Item, LocalDecl, Param,
    Span, Stmt, Type,
};
use std::collections::HashMap;

impl ModuleBuilder<'_> {
    /// Synthesize the implicit constructor for a TypeScript class.
    ///
    /// A class with no base behaves like `constructor() {}` and takes no
    /// parameters. A DERIVED class behaves like `constructor(...args) {
    /// super(...args) }`, and `...args` is not open-ended: TypeScript types a
    /// `new Derived(..)` call against the BASE constructor's signature, so the
    /// parameters the implicit constructor forwards are exactly the base
    /// constructor's own.
    ///
    /// So when the base is a source-declared class
    /// ([`Self::class_is_initializable_base`]), the synthesized constructor
    /// declares the base constructor's parameters by name and type and forwards
    /// them through the same `super(...)` lowering an explicit
    /// `constructor(..) { super(..) }` uses. That is what a hand-writing Rust
    /// team would spell, and it is what makes `new Point3(1, 2)` over a
    /// `class Point { constructor(public x: number, public y: number) {} }`
    /// arrive at all: the previous single erased argument both dropped the
    /// second argument (E0061 at the call site) and left the base's field
    /// initializers and parameter properties unrun, so `p.x` read `0`.
    ///
    /// A generic base's parameters are forwarded at the DERIVED class's type
    /// arguments (`class Wrap<U> extends Box<U> {}` takes `(v: U)`), and an
    /// abstract base's constructor runs like any other: neither is constructed
    /// on its own, its initialization runs over the derived instance.
    ///
    /// The single `Option<SmeltUnknown>` parameter survives only where the base
    /// is NOT a source-declared class — a host/prelude constructor such as
    /// `Date` — or declares a variadic constructor, and there it is a real dynamic
    /// boundary: the base's arity and parameter types are not available to this
    /// lowering at all, so there is no concrete type, union, or scoped generic
    /// that could carry the forwarded argument, and a call-compatible erased
    /// slot is the only shape that keeps `new Subclass(x)` from becoming a
    /// blocker. See `blocker-logs/hono-h68-class-expression-binding.md` (H69).
    pub(in crate::lowering) fn synthesize_default_class_constructor(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        has_base: bool,
        field_initializers: &[(
            smelt_hir::Symbol,
            ClassFieldInit<'_>,
            smelt_hir::TypeId,
            Span,
        )],
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let saved_class = self.current_class.replace(class_text.to_owned());
        let ctx = self.nested_fn_ctx(BodyKind::Method, false, Some(class_ty));
        let scope = ScopeGroups::MEMBER_BODY;
        let lowered = self.with_nested_body(ctx, scope, |this, _| {
            let mut body = Body::new(None, span);
            let this_symbol = this.ctx.krate.symbols.intern("this");
            let this_local = body.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: class_ty,
                mutable: true,
                span,
            });
            this.scope.bind("this".to_owned(), this_local);
            // A per-evaluation class's constructor takes its frame first
            // (`decls::evaluation_statics`).
            let frame_param = this.bind_evaluation_frame(
                class_name, class_ty, this_local, true, false, span, &mut body,
            );
            let forwarded = if has_base {
                this.reproducible_base_constructor_signature(class_text)
            } else {
                None
            };
            let (params, required_params) = if let Some(forwarded) = forwarded {
                // `constructor(...args) { super(...args) }`, spelled positionally:
                // one local per base parameter, then the same `super(...)` lowering
                // an explicit derived constructor uses, so the base's parameter
                // defaults, field initializers, parameter properties and its own
                // `super(...)` all run exactly once and in source order.
                let mut params = Vec::new();
                let mut arguments = Vec::new();
                for parameter in &forwarded.parameters {
                    let local = body.push_local(LocalDecl {
                        name: Some(parameter.0),
                        ty: parameter.1,
                        mutable: false,
                        span,
                    });
                    body.params.push(local);
                    params.push(Param {
                        name: parameter.0,
                        local,
                        ty: parameter.1,
                        span,
                    });
                    arguments.push(body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty: parameter.1,
                        span,
                    }));
                }
                // The forwarded `super(...)` runs BEFORE this class's own field
                // initializers, exactly as JavaScript orders them.
                Self::lower_base_constructor_init(
                    forwarded.base,
                    &arguments,
                    this_local,
                    class_ty,
                    span,
                    &mut body,
                );
                (params, forwarded.required_params)
            } else if let Some(base_name) =
                this.error_like_base_name(class_text).filter(|_| has_base)
            {
                // `class E extends Error {}`: the implicit constructor forwards to
                // the builtin `Error(message?: string)`, whose signature IS known,
                // so the forwarded message is a typed optional string and runs the
                // same slot initialization an explicit `super(message)` does.
                // Ignoring it (the erased fallback below) left every such error with
                // an empty `message` and `name`.
                let string_ty = this.ctx.krate.types.intern(Type::String);
                let message_ty = this.ctx.krate.types.intern(Type::Optional(string_ty));
                let message_symbol = this.ctx.krate.symbols.intern("message");
                let message_local = body.push_local(LocalDecl {
                    name: Some(message_symbol),
                    ty: message_ty,
                    mutable: false,
                    span,
                });
                body.params.push(message_local);
                let message = body.push_expr(Expr {
                    kind: ExprKind::Local(message_local),
                    ty: message_ty,
                    span,
                });
                this.lower_error_base_super_call(
                    &base_name,
                    &[message],
                    this_local,
                    class_ty,
                    class_text,
                    span,
                    &mut body,
                );
                (
                    vec![Param {
                        name: message_symbol,
                        local: message_local,
                        ty: message_ty,
                        span,
                    }],
                    Some(0),
                )
            } else if has_base {
                // A base this lowering cannot reproduce: see the doc comment for why
                // the forwarded argument is a genuine dynamic boundary here.
                let unknown_ty = this.erase(ErasureReason::UnmodeledType);
                let arg_ty = this.ctx.krate.types.intern(Type::Optional(unknown_ty));
                let arg_symbol = this
                    .ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new("_smelt_super_arg"));
                let arg_local = body.push_local(LocalDecl {
                    name: Some(arg_symbol),
                    ty: arg_ty,
                    mutable: false,
                    span,
                });
                body.params.push(arg_local);
                (
                    vec![Param {
                        name: arg_symbol,
                        local: arg_local,
                        ty: arg_ty,
                        span,
                    }],
                    Some(0),
                )
            } else {
                (Vec::new(), None)
            };
            this.emit_class_field_initializers(
                this_local,
                class_ty,
                field_initializers,
                &mut body,
            )?;
            let (params, required_params) = match frame_param {
                Some(frame_param) => (
                    std::iter::once(frame_param).chain(params).collect(),
                    required_params.map(|required| required + 1),
                ),
                None => (params, required_params),
            };
            Ok((body, params, required_params))
        });
        self.current_class = saved_class;
        let (body, params, required_params) = lowered?;
        let body_id = self.ctx.krate.push_body(body);
        let name = self.ctx.krate.symbols.intern("new");
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            // Synthetic default constructor takes generics from the class.
            type_params: Vec::new(),
            params,
            rest: None,
            required_params,
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_name },
        })))
    }

    /// The builtin error class a class DIRECTLY extends, when it extends one.
    ///
    /// Only the immediate base is consulted: a class extending a source class
    /// that itself extends `Error` forwards to that source constructor through
    /// [`Self::reproducible_base_constructor_signature`] instead.
    fn error_like_base_name(&self, class_text: &str) -> Option<String> {
        let (base, _) = self.classes.base(class_text).cloned()?;
        let base_name = self.ctx.krate.symbols.get(base)?;
        super_call::is_error_like_base(base_name).then(|| base_name.to_owned())
    }

    /// Resolve the base constructor an implicit derived constructor forwards to.
    ///
    /// Returns `None` — leaving the caller on the erased single-argument shape —
    /// whenever the base is not a source-declared class: an unresolved name or a
    /// host/prelude constructor (`Date`, `Error` and friends). Those are exactly
    /// the cases [`Self::class_is_initializable_base`] and
    /// [`super_call::is_error_like_base`] already separate for an EXPLICIT
    /// `super(...)`, so the implicit constructor and the explicit one agree on
    /// which bases they can run.
    ///
    /// The signature is read from the base class item's own constructor, which
    /// is itself either a source constructor or a previously synthesized one, so
    /// a chain of implicit constructors forwards the original parameters all the
    /// way down without this helper walking the chain. A GENERIC base's
    /// parameter types are rewritten at this class's `extends Base<..>` type
    /// arguments (a missing argument takes the parameter's declared default), so
    /// the forwarded signature names only types in scope for the derived class.
    fn reproducible_base_constructor_signature(
        &mut self,
        class_text: &str,
    ) -> Option<ForwardedBaseConstructor> {
        let (base, base_args) = self.classes.base(class_text).cloned()?;
        let base_name = self.ctx.krate.symbols.get(base).map(ToOwned::to_owned)?;
        if super_call::is_error_like_base(&base_name) || !self.class_is_initializable_base(base) {
            return None;
        }
        // Resolved by symbol for the same reason `class_is_initializable_base` is:
        // the base's source spelling may belong to another class entirely once a
        // cross-module collision has been renamed apart.
        let constructor = self.class_by_symbol(base)?.constructor?;
        let constructor_index = usize::try_from(constructor.0).unwrap_or(usize::MAX);
        let Some(Item::Function(function)) = self.ctx.krate.items.get(constructor_index) else {
            return None;
        };
        // A base whose own constructor is variadic has no fixed parameter list
        // to copy; forwarding a prefix of it would silently drop the rest, which
        // is the defect this whole change is about.
        if function.rest.is_some() {
            return None;
        }
        // Owned before substituting: substitution interns types, which needs
        // the crate mutably while `function` borrows it.
        let parameters = function.params.clone();
        let required_params = function.required_params;
        let substitutions = self
            .class_by_symbol(base)?
            .type_params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                base_args
                    .get(index)
                    .copied()
                    .or(param.default)
                    .map(|ty| (param.name, ty))
            })
            .collect::<HashMap<_, _>>();
        let parameters = parameters
            .into_iter()
            .map(|param| {
                (
                    param.name,
                    self.substitute_type_params(param.ty, &substitutions),
                )
            })
            .collect();
        Some(ForwardedBaseConstructor {
            base,
            parameters,
            required_params,
        })
    }

    /// Emit TypeScript instance field initializers at the start of a constructor.
    ///
    /// JavaScript evaluates public instance field initializers for each new
    /// object before the constructor body runs. Lowering them as assignments to
    /// `this` preserves observable fields for both native field reads and later
    /// erasure through `unknown` object helpers.
    pub(in crate::lowering) fn emit_class_field_initializers(
        &mut self,
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        field_initializers: &[(
            smelt_hir::Symbol,
            ClassFieldInit<'_>,
            smelt_hir::TypeId,
            Span,
        )],
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        for (field, initializer, field_ty, span) in field_initializers {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span: *span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: *field,
                },
                ty: *field_ty,
                span: *span,
            });
            // Lower the initializer against the declared field type so empty
            // collection literals (`new Map()`, `[]`, `{}`) and other
            // contextually-typed expressions adopt the field's element/key
            // types instead of defaulting to `Unknown`. Without the hint an
            // empty `new Map()` for a `Map<T, string>` field infers
            // `Map<Unknown, Unknown>`, forcing a lossy key conversion at the
            // assignment that cannot collect into the generic field type (E0277).
            let value = match initializer {
                ClassFieldInit::Expression(expression) => {
                    self.expression_with_hint(expression, body, Some(*field_ty))?
                }
                // A method reassigned through `this` is carried as a
                // function-typed field, so its own body becomes the field's
                // initializer: the class still behaves as declared until
                // something assigns over it. Lowered as a function VALUE (the
                // same path a `f = function () {..}` field initializer takes)
                // rather than as an inherent method, because an inherent method
                // is what cannot be assigned in the first place.
                ClassFieldInit::Method(function) => {
                    self.function_expression_value(function, Some(*field_ty), function.span, body)?
                }
            };
            body.push_stmt(Stmt::Assign { target, value });
        }
        Ok(())
    }

    /// Emit constructor parameter-property assignments to `this`.
    ///
    /// TypeScript parameter properties such as `constructor(private value: T)`
    /// both declare an instance field and assign the constructor argument into
    /// that field. Class lowering already records the field itself; this helper
    /// emits the constructor-side assignment so later `this.value` reads have a
    /// concrete field in HIR/MIR and generated Rust.
    pub(in crate::lowering) fn emit_parameter_property_initializers(
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        parameter_properties: &[(
            smelt_hir::Symbol,
            smelt_hir::LocalId,
            smelt_hir::TypeId,
            Span,
        )],
        body: &mut Body,
    ) {
        for (field, local, ty, span) in parameter_properties {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span: *span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: *field,
                },
                ty: *ty,
                span: *span,
            });
            let value = body.push_expr(Expr {
                kind: ExprKind::Local(*local),
                ty: *ty,
                span: *span,
            });
            body.push_stmt(Stmt::Assign { target, value });
        }
    }

    /// Inject the inherited `Error`-instance slots into a class that extends the
    /// JavaScript `Error` builtin (or one of its standard subclasses).
    ///
    /// The `Error` constructor is a modeled host builtin, not a source-declared
    /// Smelt class, so `effective_class_fields` finds no base layout to inherit.
    /// A subclass constructor nevertheless assigns and reads the inherited
    /// `name`/`message`/`stack`/`cause` slots (`this.name = ...`, `super(msg)`),
    /// which otherwise reference struct fields that were never declared (E0609).
    ///
    /// The slots are declared with the types TypeScript's lib gives them, NOT as
    /// a uniform erased shape: `name`/`message` are `string`, `stack` is an
    /// optional `string`, and only `cause` is `Unknown`. See
    /// [`super_call::ERROR_MARKER_FIELDS`] for that reasoning — it owns the layout
    /// so the declared types and the types `super(...)` writes cannot drift.
    ///
    /// Fields the subclass redeclares itself are left untouched. Only the direct
    /// Error-extending class needs injection; deeper subclasses inherit these
    /// through the normal flattened class layout.
    pub(super) fn inject_error_marker_fields(
        &mut self,
        base: Option<smelt_hir::Symbol>,
        class_span: Span,
        fields: &mut Vec<Field>,
    ) {
        let Some(base) = base else {
            return;
        };
        let Some(base_name) = self.ctx.krate.symbols.get(base) else {
            return;
        };
        // Error-like host bases carry the same `name`/`message`/`stack`/`cause`
        // slots as `Error`. `DOMException` (and its runtime fallback to `Error`)
        // is not a source-declared class nor a `mir.classes` entry — it is a
        // const alias to a host constructor — so a subclass such as
        // `AbortError extends DOMException` inherits no fields unless we inject the
        // Error marker slots here too.
        //
        // The base-name predicate and the injected slot layout are shared with
        // the `super(...)` lowering in `super_call`, so the fields a class gains
        // and the slots its derived constructor writes cannot drift apart.
        let base_name = base_name.to_owned();
        if !super_call::is_error_like_base(&base_name) {
            return;
        }
        *fields = self.with_error_marker_fields(std::mem::take(fields), class_span);
    }
}

/// The base constructor an implicitly synthesized derived constructor forwards.
///
/// Produced by
/// [`ModuleBuilder::reproducible_base_constructor_signature`](crate::lowering::ModuleBuilder::reproducible_base_constructor_signature)
/// and consumed only by
/// [`ModuleBuilder::synthesize_default_class_constructor`](crate::lowering::ModuleBuilder::synthesize_default_class_constructor).
/// Everything here is owned so the caller can keep mutating the crate while it
/// builds the forwarding body.
struct ForwardedBaseConstructor {
    /// The base class's interned name.
    base: smelt_hir::Symbol,
    /// The base constructor's parameters, in order, as (name, type), with a
    /// generic base's type parameters already substituted at the derived
    /// class's `extends` arguments.
    parameters: Vec<(smelt_hir::Symbol, smelt_hir::TypeId)>,
    /// How many of those parameters are required, as the base declares it.
    required_params: Option<usize>,
}
