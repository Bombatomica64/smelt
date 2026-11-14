//! Per-evaluation static storage: classes declared inside a function whose
//! static initializers read that function's locals.
//!
//! JavaScript evaluates a class declaration every time control reaches it, and
//! every evaluation creates a NEW class with its own static fields:
//!
//! ```ts
//! function make(n: number) {
//!   class Box { static size = n; static grow() { this.size++; return this.size; } }
//!   return Box;
//! }
//! const A = make(1), B = make(10); // two classes, two `size` fields
//! ```
//!
//! A class evaluated once (top level), or whose statics are the same for every
//! evaluation, keeps the single-global fast path: one module global per static
//! field (`StaticField::storage`). A class whose static initializers read the
//! enclosing function's locals cannot share one storage between evaluations, so
//! its statics live in a **statics frame** instead:
//!
//! - The frame is a synthesized class `F` (`Class::statics_of = Some(owner)`)
//!   whose fields are the owner's static fields and whose methods are the
//!   owner's static methods, lowered as frame methods (`this` is the frame).
//!   Codegen emits it as a reference class (one `Rc` cell shared by every
//!   holder), so a write through one holder is seen by all of them.
//! - Each evaluation constructs one frame where the declaration is, runs the
//!   static initializers into it in source order (with `this` bound to the
//!   frame and the function's locals in scope), and binds the class's name to
//!   it for the rest of the scope: `Box.size`, `Box.size = v`, `Box.grow()`
//!   and `this.size` in a static method are ordinary typed field and method
//!   accesses on the frame.
//! - The owner class gets a hidden instance field holding the frame it was
//!   constructed by, and its constructor a hidden leading parameter that
//!   `new` fills with the frame the callee names. Inside the owner's methods
//!   and constructor, the class name is bound to that frame.
//! - The class used as a value is the frame itself: a typed handle wherever it
//!   stays typed, and at an erased boundary the class value object, whose
//!   constructor, static methods and static-field hooks close over that frame
//!   (codegen, `class_proto::statics_frame_class_value`). `instanceof` against
//!   a frame compares frame identity, so instances of two evaluations are
//!   instances of their own evaluation's class only.

use std::collections::HashSet;

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::visibility;
use oxc::ast::ast::{ClassElement, Expression, MethodDefinitionKind, PropertyKey};
use oxc::span::GetSpan;
use smelt_hir::{Body, Expr, ExprKind, Field, Item, LocalDecl, Param, Pattern, Span, Stmt, Type, Visibility};

/// Name of the hidden instance field an owner's instances carry their frame in.
pub(in crate::lowering) const STATICS_FRAME_FIELD: &str = "__smelt_statics";

/// The statics frame of one per-evaluation class.
#[derive(Debug, Clone)]
pub(in crate::lowering) struct EvaluationStatics {
    /// The owner class's symbol.
    pub(in crate::lowering) owner: smelt_hir::Symbol,
    /// The source name the class is bound to inside its own scope (`Box`),
    /// `None` for an anonymous class expression.
    pub(in crate::lowering) binding: Option<String>,
    /// The synthesized frame class's symbol.
    pub(in crate::lowering) frame: smelt_hir::Symbol,
    /// The class-registry key of the frame class.
    pub(in crate::lowering) frame_text: String,
    /// `Type::Class` of the frame.
    pub(in crate::lowering) frame_ty: smelt_hir::TypeId,
    /// The hidden instance field of the owner holding its frame.
    pub(in crate::lowering) frame_field: smelt_hir::Symbol,
}

impl ModuleBuilder<'_> {
    /// Decide whether the class being declared needs per-evaluation statics,
    /// and if so mint its frame class.
    ///
    /// It does when the declaration sits in a function or closure body and a
    /// static field initializer reads a binding of that body (a parameter or
    /// local in scope where the class is declared). Such an initializer gives
    /// each evaluation a different value, so one program-wide storage would be
    /// wrong for every evaluation but the first.
    pub(in crate::lowering) fn begin_evaluation_statics(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
        class_text: &str,
        class_name: smelt_hir::Symbol,
    ) -> Option<EvaluationStatics> {
        if !self.classes.in_body() || !self.static_initializers_read_enclosing_locals(class) {
            return None;
        }
        let frame_name = smelt_hir::SynthesizedName::new(format!("{class_text}__smelt_statics"));
        let frame_text = frame_name.as_str().to_owned();
        let frame = self.intern_synthesized_type_name(&frame_name);
        let frame_ty = self.ctx.krate.types.intern(Type::Class { name: frame, args: Vec::new() });
        let frame_field = self.intern_synthesized_name(STATICS_FRAME_FIELD);
        let evaluation = EvaluationStatics {
            owner: class_name,
            binding: class.id.as_ref().map(|id| id.name.to_string()),
            frame,
            frame_text,
            frame_ty,
            frame_field,
        };
        self.evaluation_statics.insert(class_name, evaluation.clone());
        self.evaluation_frames.insert(frame, class_name);
        Some(evaluation)
    }

    /// Whether a static field initializer of `class` reads a binding of the
    /// body the class is declared in.
    ///
    /// Every identifier reference in a static initializer is checked against
    /// the bindings in scope at the declaration. One that a nested function
    /// of the initializer itself binds only makes the answer conservative
    /// (per-evaluation storage is correct for any class; only the fast path
    /// needs the proof).
    fn static_initializers_read_enclosing_locals(&self, class: &oxc::ast::ast::Class<'_>) -> bool {
        let mut names = HashSet::new();
        let mut collector = IdentifierCollector { names: &mut names };
        for element in &class.body.body {
            let ClassElement::PropertyDefinition(property) = element else {
                continue;
            };
            if !property.r#static {
                continue;
            }
            if let Some(value) = &property.value {
                oxc::ast_visit::Visit::visit_expression(&mut collector, value);
            }
        }
        names.iter().any(|name| self.scope.is_bound(name))
    }

    /// The frame field a static property of a per-evaluation class declares:
    /// its source name and type, with the property's visibility.
    ///
    /// The type is the annotation, else the type TypeScript gave the
    /// property, else the type of a literal initializer; a property without
    /// an initializer is `T | undefined` and starts at `undefined`
    /// ([`Self::class_uninitialized_static_field`]). A property the checker
    /// did not type and whose initializer is not a literal needs an
    /// annotation, which the error says.
    pub(in crate::lowering) fn evaluation_static_field(
        &mut self,
        property: &oxc::ast::ast::PropertyDefinition<'_>,
    ) -> Result<Field, SmeltError> {
        let span = self.span(property.span.start, property.span.end);
        let static_field = match &property.value {
            None => self.class_uninitialized_static_field(property)?,
            Some(value) => {
                if let Some(literal) = self.class_static_field(property)? {
                    literal
                } else {
                    let name = self.property_key_symbol(&property.key)?;
                    let key_span = property.key.span();
                    let ty = if let Some(annotation) = &property.type_annotation {
                        self.ts_type_to_hir(&annotation.type_annotation)?
                    } else if let Some(ty) = self.checker_binding_type(key_span.start, key_span.end) {
                        ty
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(value.span().start, value.span().end),
                            "a static field of a class declared inside a function needs a type annotation \
                             when its initializer is not a literal",
                        ));
                    };
                    let visibility = if matches!(&property.key, PropertyKey::PrivateIdentifier(_)) {
                        Visibility::Hidden
                    } else {
                        visibility(property.accessibility)
                    };
                    smelt_hir::StaticField {
                        name,
                        ty,
                        visibility,
                        value: None,
                        mutable: !property.readonly,
                        storage: None,
                        span,
                    }
                }
            }
        };
        Ok(Field {
            name: static_field.name,
            ty: static_field.ty,
            // A frame field is reached only through the frame (typed field
            // access, or the class value's static hooks); it is never an own
            // property of an erased frame, so it stays out of every projection
            // except the one the class value builds itself.
            visibility: static_field.visibility,
            optional: false,
            span,
        })
    }

    /// The signatures of `class`'s static methods, as the frame class's
    /// instance methods.
    pub(in crate::lowering) fn evaluation_static_method_signatures(
        &mut self,
        elements: &[ClassElement<'_>],
    ) -> Result<Vec<smelt_hir::MethodSig>, SmeltError> {
        let mut methods = Vec::new();
        for element in elements {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            if method.kind != MethodDefinitionKind::Method || !method.r#static || method.value.body.is_none() {
                continue;
            }
            methods.push(self.abstract_class_method_sig(method)?);
        }
        Ok(methods)
    }

    /// Bind the class name and the frame inside one function of a
    /// per-evaluation class, right after its `this` is bound.
    ///
    /// - The owner's constructor takes the frame as a hidden leading
    ///   parameter, stores it in the hidden field before anything else runs
    ///   (field initializers included), and binds the class name to it.
    /// - Any other owner method binds the class name to its instance's frame
    ///   (`this.__smelt_statics`), when its body names the class.
    /// - A frame method (a source static method) binds the class name to
    ///   `this`, which is the frame.
    ///
    /// Returns the hidden parameter the constructor gained, which the caller
    /// adds to its signature.
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame binding threads the method's receiver, kind and body being built"
    )]
    pub(in crate::lowering) fn bind_evaluation_frame(
        &mut self,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        this_local: smelt_hir::LocalId,
        is_constructor: bool,
        names_class: bool,
        span: Span,
        body: &mut Body,
    ) -> Option<Param> {
        let evaluation = self.lowering_evaluation_statics.clone()?;
        if class_name == evaluation.frame {
            if let Some(binding) = &evaluation.binding {
                self.scope.bind(binding.clone(), this_local);
            }
            return None;
        }
        if class_name != evaluation.owner {
            return None;
        }
        if is_constructor {
            let local = body.push_local(LocalDecl {
                name: Some(evaluation.frame_field),
                ty: evaluation.frame_ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            if let Some(binding) = &evaluation.binding {
                self.scope.bind(binding.clone(), local);
            }
            Self::emit_parameter_property_initializers(
                this_local,
                class_ty,
                &[(evaluation.frame_field, local, evaluation.frame_ty, span)],
                body,
            );
            return Some(Param { name: evaluation.frame_field, local, ty: evaluation.frame_ty, span });
        }
        if let Some(binding) = evaluation.binding.as_ref().filter(|_| names_class) {
            let receiver = body.push_expr(Expr { kind: ExprKind::Local(this_local), ty: class_ty, span });
            let frame = body.push_expr(Expr {
                kind: ExprKind::Field { receiver, field: evaluation.frame_field },
                ty: evaluation.frame_ty,
                span,
            });
            let symbol = self.intern_source_name(binding);
            let local = body.push_local(LocalDecl { name: Some(symbol), ty: evaluation.frame_ty, mutable: false, span });
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt(Stmt::Let { pat, ty: evaluation.frame_ty, value: Some(frame) });
            self.scope.bind(binding.clone(), local);
        }
        None
    }

    /// Bind the name of the per-evaluation class being lowered to a frame
    /// local of `body`, for an expression lowered only to learn its type (an
    /// unannotated instance field's initializer). Returns what to restore.
    pub(in crate::lowering) fn bind_scratch_evaluation_frame(
        &mut self,
        body: &mut Body,
        span: Span,
    ) -> Option<(String, Option<smelt_hir::LocalId>)> {
        let evaluation = self.lowering_evaluation_statics.clone()?;
        let binding = evaluation.binding?;
        let local = body.push_local(LocalDecl { name: Some(evaluation.frame_field), ty: evaluation.frame_ty, mutable: false, span });
        let previous = self.scope.bind(binding.clone(), local);
        Some((binding, previous))
    }

    /// Undo [`Self::bind_scratch_evaluation_frame`].
    pub(in crate::lowering) fn restore_scratch_evaluation_frame(&mut self, saved: Option<(String, Option<smelt_hir::LocalId>)>) {
        let Some((binding, previous)) = saved else {
            return;
        };
        match previous {
            Some(previous) => {
                self.scope.bind(binding, previous);
            }
            None => self.scope.unbind(&binding),
        }
    }

    /// Whether a method body names `class_name` (so the frame binding is worth
    /// a local).
    pub(in crate::lowering) fn function_body_names(
        function_body: &oxc::ast::ast::FunctionBody<'_>,
        class_name: &str,
    ) -> bool {
        let mut names = HashSet::new();
        let mut collector = IdentifierCollector { names: &mut names };
        oxc::ast_visit::Visit::visit_function_body(&mut collector, function_body);
        names.contains(class_name)
    }

    /// Push the frame class of `evaluation`, once the owner class item exists.
    pub(in crate::lowering) fn finish_evaluation_statics(
        &mut self,
        evaluation: &EvaluationStatics,
        fields: Vec<Field>,
        methods: Vec<smelt_hir::ItemId>,
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let constructor = self.synthesize_default_class_constructor(
            &evaluation.frame_text,
            evaluation.frame,
            evaluation.frame_ty,
            false,
            &[],
            span,
        )?;
        let item = self.ctx.krate.push_item(Item::Class(smelt_hir::Class {
            name: evaluation.frame,
            span,
            kind: smelt_hir::ClassKind::Plain,
            type_params: Vec::new(),
            base: None,
            base_args: Vec::new(),
            fields,
            static_fields: Vec::new(),
            descriptors: Vec::new(),
            constructor: Some(constructor),
            methods,
            static_methods: Vec::new(),
            abstract_methods: Vec::new(),
            implements: Vec::new(),
            protocols: Vec::new(),
            statics_of: Some(evaluation.owner),
        }));
        self.classes.register(evaluation.frame_text.clone(), item);
        Ok(item)
    }

    /// Evaluate the statics of a per-evaluation class where its declaration
    /// is: construct this evaluation's frame, run the static initializers
    /// into it in source order, and bind the class name to it.
    ///
    /// Each initializer is lowered in the enclosing body (so it reads the
    /// function's locals) with `this` bound to the frame, exactly as
    /// JavaScript evaluates it with `this` bound to the class being defined;
    /// an earlier static and a static method are reachable through the frame.
    /// A static without an initializer keeps the frame's default, `undefined`.
    ///
    /// Returns the frame local, or `None` for a class with module-global
    /// statics.
    pub(in crate::lowering) fn evaluate_class_statics(
        &mut self,
        item: smelt_hir::ItemId,
        class: &oxc::ast::ast::Class<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::LocalId>, SmeltError> {
        let Item::Class(owner) = self.item_ref(item) else {
            return Ok(None);
        };
        let Some(evaluation) = self.evaluation_statics.get(&owner.name).cloned() else {
            return Ok(None);
        };
        let span = self.span(class.span.start, class.span.end);
        let local_name = match &evaluation.binding {
            Some(binding) => self.intern_source_name(binding),
            None => self.intern_synthesized_name(STATICS_FRAME_FIELD),
        };
        let frame_local =
            body.push_local(LocalDecl { name: Some(local_name), ty: evaluation.frame_ty, mutable: false, span });
        let construct =
            body.push_expr(Expr { kind: ExprKind::New { class: evaluation.frame, args: Vec::new() }, ty: evaluation.frame_ty, span });
        let pat = body.push_pattern(Pattern::Binding(frame_local));
        self.push_stmt_to_sink(body, Stmt::Let { pat, ty: evaluation.frame_ty, value: Some(construct) });
        if let Some(binding) = &evaluation.binding {
            self.scope.bind(binding.clone(), frame_local);
        }
        let fields = self.classes.fields(&evaluation.frame_text).cloned().unwrap_or_default();
        let previous_this = self.scope.bind("this".to_owned(), frame_local);
        let lowered = (|| {
            for element in &class.body.body {
                let ClassElement::PropertyDefinition(property) = element else {
                    continue;
                };
                let Some(value) = property.value.as_ref().filter(|_| property.r#static) else {
                    continue;
                };
                if matches!(value, Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)) {
                    continue;
                }
                let name = self.property_key_symbol(&property.key)?;
                let Some(field) = fields.iter().find(|field| field.name == name) else {
                    continue;
                };
                let value_span = self.span(value.span().start, value.span().end);
                let value = self.expression_with_hint(value, body, Some(field.ty))?;
                let receiver =
                    body.push_expr(Expr { kind: ExprKind::Local(frame_local), ty: evaluation.frame_ty, span: value_span });
                let target = body.push_expr(Expr { kind: ExprKind::Field { receiver, field: name }, ty: field.ty, span: value_span });
                self.push_stmt_to_sink(body, Stmt::Assign { target, value });
            }
            Ok::<_, SmeltError>(())
        })();
        match previous_this {
            Some(previous) => {
                self.scope.bind("this".to_owned(), previous);
            }
            None => self.scope.unbind("this"),
        }
        lowered?;
        Ok(Some(frame_local))
    }

    /// The owner class whose statics frame `ty` is, if it is one.
    pub(in crate::lowering) fn statics_frame_owner(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::Symbol> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { name, .. }) => self.evaluation_frames.get(name).copied(),
            _ => None,
        }
    }

    /// The value a `new` callee or `instanceof` target names when it is a
    /// statics frame, with the frame's owner: the class name of a
    /// per-evaluation class in its scope, `this` in one of its static
    /// methods, or any other binding holding a frame (`const A = make(1)`).
    ///
    /// Only a plain name or `this` is asked; a module-scope name is lowered as
    /// a read to learn its type, which a name that turns out not to hold a
    /// frame leaves as an unused expression.
    pub(in crate::lowering) fn statics_frame_value(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<Option<(smelt_hir::ExprId, smelt_hir::Symbol)>, SmeltError> {
        let name = match expression.without_parentheses() {
            Expression::Identifier(identifier) => identifier.name.as_str(),
            Expression::ThisExpression(_) => "this",
            _ => return Ok(None),
        };
        let span = self.expression_span(expression);
        if let Some(local) = self.scope.lookup(name) {
            let ty = Self::local_ty(body, local);
            let Some(owner) = self.statics_frame_owner(ty) else {
                return Ok(None);
            };
            return Ok(Some((body.push_expr(Expr { kind: ExprKind::Local(local), ty, span }), owner)));
        }
        if self.evaluation_frames.is_empty()
            || !matches!(self.resolve_value(name), crate::lowering::resolve::ValueBinding::Module)
        {
            return Ok(None);
        }
        let value = self.expression(expression, body)?;
        Ok(self.statics_frame_owner(Self::expr_ty(body, value)).map(|owner| (value, owner)))
    }

    /// Lower `new C(args)` where `C` is a statics frame: construct the owner
    /// class with the frame as the hidden leading constructor argument.
    ///
    /// Each source argument is lowered against its constructor parameter's
    /// type, as for any class construction.
    pub(in crate::lowering) fn new_through_statics_frame(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((frame, owner)) = self.statics_frame_value(&new_expr.callee, body)? else {
            return Ok(None);
        };
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let constructor_params = self
            .class_by_symbol(owner)
            .and_then(|class| class.constructor)
            .and_then(|constructor| match self.item_ref(constructor) {
                Item::Function(function) => Some(function.params.iter().skip(1).map(|param| param.ty).collect::<Vec<_>>()),
                _ => None,
            })
            .unwrap_or_default();
        let mut args = vec![frame];
        for (index, arg) in new_expr.arguments.iter().enumerate() {
            args.push(self.argument_with_hint(arg, body, constructor_params.get(index).copied())?);
        }
        let ty = self.ctx.krate.types.intern(Type::Class { name: owner, args: Vec::new() });
        Ok(Some(body.push_expr(Expr { kind: ExprKind::New { class: owner, args }, ty, span })))
    }
}

/// Collects every identifier reference under the visited node.
struct IdentifierCollector<'names> {
    /// The referenced names.
    names: &'names mut HashSet<String>,
}

impl<'a> oxc::ast_visit::Visit<'a> for IdentifierCollector<'_> {
    /// Records one identifier reference.
    fn visit_identifier_reference(&mut self, identifier: &oxc::ast::ast::IdentifierReference<'a>) {
        self.names.insert(identifier.name.to_string());
    }
}
