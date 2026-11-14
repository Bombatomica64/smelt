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
use smelt_hir::{
    Body, Expr, ExprKind, Field, Item, LocalDecl, Param, Pattern, Span, Stmt, Type, Visibility,
};

/// Name of the hidden instance field an owner's instances carry their frame in.
pub(in crate::lowering) const STATICS_FRAME_FIELD: &str = "__smelt_statics";

/// The scope name a per-evaluation class's constructor binds its frame
/// parameter under; not a JavaScript identifier, so no source name shadows it.
const CONSTRUCTOR_FRAME_BINDING: &str = "#smelt constructor frame";

/// Name of the hidden frame field linking a subclass frame to its base's.
const BASE_FRAME_FIELD: &str = "__smelt_base";

/// `name` for the per-evaluation class `depth` levels below the first: the
/// first level keeps the bare name, every deeper one is suffixed, so the
/// hidden fields of all levels coexist in one flattened instance or frame.
fn level_name(name: &str, depth: usize) -> String {
    if depth == 0 {
        name.to_owned()
    } else {
        format!("{name}_{depth}")
    }
}

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
    /// The owner's type parameters. The frame itself is never generic
    /// (TypeScript forbids a static member to name a class type parameter),
    /// but `new` through it instantiates the owner.
    pub(in crate::lowering) type_params: Vec<smelt_hir::TypeParamDef>,
    /// The statics of the class this one extends, when that class is
    /// per-evaluation too (`decls::evaluation_statics`, "Subclasses").
    pub(in crate::lowering) base: Option<Box<Self>>,
    /// The hidden frame field linking this frame to its base class's frame
    /// (the class's `[[Prototype]]`), when it has one.
    pub(in crate::lowering) base_link: Option<smelt_hir::Symbol>,
    /// How many per-evaluation classes this one extends, transitively. Every
    /// hidden name a level adds is suffixed with it, because a subclass frame
    /// and a subclass instance flatten the fields of every level.
    pub(in crate::lowering) depth: usize,
    /// The static fields this class inherits from its base frame, each read
    /// through an accessor that delegates to the base until this class
    /// writes its own (filled when the frame's fields are built).
    pub(in crate::lowering) inherited: Vec<InheritedStatic>,
    /// The class declaration's source range, which the checker names as the
    /// declaration of the class's constructor type (`typeof Box`).
    pub(in crate::lowering) declared_at: (u32, u32),
}

/// One static field a per-evaluation subclass inherits from its base.
///
/// JavaScript resolves `Sub.x` through `Sub`'s prototype chain: `Base.x`
/// until something writes `Sub.x`, which creates an own property of `Sub`
/// and leaves `Base.x` alone. The subclass frame models exactly that with an
/// accessor pair over two hidden fields: `own` says whether `Sub` has its own
/// `x`, and `shadow` holds it.
#[derive(Debug, Clone)]
pub(in crate::lowering) struct InheritedStatic {
    /// The static field's name.
    pub(in crate::lowering) name: smelt_hir::Symbol,
    /// Its type, as the base declares it.
    pub(in crate::lowering) ty: smelt_hir::TypeId,
    /// The hidden field holding this class's own value once written.
    pub(in crate::lowering) shadow: smelt_hir::Symbol,
    /// The hidden field saying whether this class has written its own value.
    pub(in crate::lowering) own: smelt_hir::Symbol,
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
        type_params: &[smelt_hir::TypeParamDef],
        base: Option<EvaluationStatics>,
    ) -> Option<EvaluationStatics> {
        if base.is_none()
            && (!self.classes.in_body() || !self.static_initializers_read_enclosing_locals(class))
        {
            return None;
        }
        // Named after the owner's own (scope-qualified) symbol, not its source
        // spelling: two functions may each declare a per-evaluation `Box`.
        let owner_text = self
            .ctx
            .krate
            .symbols
            .get(class_name)
            .unwrap_or(class_text)
            .to_owned();
        let frame_name = smelt_hir::SynthesizedName::new(format!("{owner_text}__smelt_statics"));
        let frame_text = frame_name.as_str().to_owned();
        let frame = self.intern_synthesized_type_name(&frame_name);
        let frame_ty = self.ctx.krate.types.intern(Type::Class {
            name: frame,
            args: Vec::new(),
        });
        let depth = base.as_ref().map_or(0, |base| base.depth + 1);
        let frame_field = self.intern_synthesized_name(&level_name(STATICS_FRAME_FIELD, depth));
        let base_link = base
            .as_ref()
            .map(|_| self.intern_synthesized_name(&level_name(BASE_FRAME_FIELD, depth)));
        if let Some(base) = &base {
            // The frame inherits the base frame's members, so a static method
            // of the base is the subclass frame's method too, with `this`
            // being the subclass frame (codegen re-emits it for this frame).
            self.classes
                .set_base(frame_text.clone(), frame, base.frame, Vec::new());
        }
        let evaluation = EvaluationStatics {
            owner: class_name,
            binding: class.id.as_ref().map(|id| id.name.to_string()),
            frame,
            frame_text,
            frame_ty,
            frame_field,
            type_params: type_params.to_vec(),
            base: base.map(Box::new),
            base_link,
            depth,
            inherited: Vec::new(),
            declared_at: (class.span.start, class.span.end),
        };
        self.evaluation_statics
            .insert(class_name, evaluation.clone());
        self.evaluation_frames.insert(frame, class_name);
        Some(evaluation)
    }

    /// The per-evaluation class an `extends` clause that is not a class name
    /// evaluates to: `class Ext extends B1` with `const B1 = make(1)`,
    /// `extends make(2)` or `extends one.Box`, where `make` returns a class
    /// declared in its body.
    ///
    /// TypeScript types such a clause `typeof Box`, an object type whose
    /// symbol is `Box`'s declaration, which names the class; its frame is
    /// the clause's VALUE, evaluated where the subclass is declared
    /// (`Self::evaluate_class_statics`).
    pub(in crate::lowering) fn heritage_statics_owner(
        &self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Option<smelt_hir::Symbol> {
        let heritage = class.heritage.as_ref()?;
        if let Expression::Identifier(identifier) = heritage.expression.without_parentheses()
            && self.classes.contains(identifier.name.as_str())
        {
            return None;
        }
        if self.evaluation_statics.is_empty() {
            return None;
        }
        let facts = self.ts_facts?;
        let span = heritage.expression.span();
        let id = facts.type_id_at(&self.path, span.start, span.end)?;
        let symbol = match facts.types.get(id).map(|ty| &ty.kind) {
            Some(smelt_ts_model::TsTypeKind::Object(object)) => object.symbol.as_ref()?,
            _ => return None,
        };
        let declaration = symbol.declaration.as_ref()?;
        self.evaluation_statics.values().find_map(|evaluation| {
            let (start, end) = evaluation.declared_at;
            (start <= declaration.start && declaration.end <= end).then_some(evaluation.owner)
        })
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
            None => {
                let (static_field, hides_undefined) =
                    self.class_uninitialized_static_field(property)?;
                if hides_undefined
                    && let Some(evaluation) = self.lowering_evaluation_statics.as_ref()
                {
                    self.ctx
                        .absent_frame_fields
                        .insert((evaluation.frame, static_field.name));
                }
                static_field
            }
            Some(value) => {
                if let Some(literal) = self.class_static_field(property)? {
                    literal
                } else {
                    let name = self.property_key_symbol(&property.key)?;
                    let key_span = property.key.span();
                    let ty = if let Some(annotation) = &property.type_annotation {
                        self.ts_type_to_hir(&annotation.type_annotation)?
                    } else if let Some(ty) = self.checker_binding_type(key_span.start, key_span.end)
                    {
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
                        inherited: None,
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
            if method.kind != MethodDefinitionKind::Method
                || !method.r#static
                || method.value.body.is_none()
            {
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
        named: &HashSet<String>,
        span: Span,
        body: &mut Body,
    ) -> Option<Param> {
        let evaluation = self.lowering_evaluation_statics.clone()?;
        if class_name == evaluation.frame {
            // A static method runs with `this` = the class it was called on,
            // which for an inherited one is a SUBCLASS frame, but its source
            // names its own class lexically: `Box.n` in `Box`'s static method
            // is `Box`'s `n` even when `Sub.grow()` runs it. So the name binds
            // to this method's own level of the receiver's frame chain
            // (`Self::frame_level_accessors`), which is `this` itself unless
            // the method runs as an inherited copy on a subclass frame.
            self.bind_ancestor_frames(
                &evaluation,
                named,
                body,
                span,
                &|this, frame_body, ancestor| {
                    let receiver = frame_body.push_expr(Expr {
                        kind: ExprKind::Local(this_local),
                        ty: evaluation.frame_ty,
                        span,
                    });
                    let method = this.frame_level_method(ancestor.depth);
                    frame_body.push_expr(Expr {
                        kind: ExprKind::Method {
                            receiver,
                            method,
                            args: Vec::new(),
                        },
                        ty: ancestor.frame_ty,
                        span,
                    })
                },
            );
            if let Some(binding) = evaluation
                .binding
                .as_ref()
                .filter(|binding| named.contains(binding.as_str()))
            {
                let receiver = body.push_expr(Expr {
                    kind: ExprKind::Local(this_local),
                    ty: evaluation.frame_ty,
                    span,
                });
                let level = body.push_expr(Expr {
                    kind: ExprKind::Method {
                        receiver,
                        method: self.frame_level_method(evaluation.depth),
                        args: Vec::new(),
                    },
                    ty: evaluation.frame_ty,
                    span,
                });
                let symbol = self.intern_source_name(binding);
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: evaluation.frame_ty,
                    mutable: false,
                    span,
                });
                let pat = body.push_pattern(Pattern::Binding(local));
                body.push_stmt(Stmt::Let {
                    pat,
                    ty: evaluation.frame_ty,
                    value: Some(level),
                });
                self.scope.bind(binding.clone(), local);
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
            // `super(...)` passes the base class's frame, which this frame
            // links to (`Self::base_frame_argument`).
            self.scope.bind(CONSTRUCTOR_FRAME_BINDING.to_owned(), local);
            self.bind_ancestor_frames(
                &evaluation,
                named,
                body,
                span,
                &|this, frame_body, ancestor| {
                    let receiver = frame_body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty: evaluation.frame_ty,
                        span,
                    });
                    let method = this.frame_level_method(ancestor.depth);
                    frame_body.push_expr(Expr {
                        kind: ExprKind::Method {
                            receiver,
                            method,
                            args: Vec::new(),
                        },
                        ty: ancestor.frame_ty,
                        span,
                    })
                },
            );
            Self::emit_parameter_property_initializers(
                this_local,
                class_ty,
                &[(evaluation.frame_field, local, evaluation.frame_ty, span)],
                body,
            );
            return Some(Param {
                name: evaluation.frame_field,
                local,
                ty: evaluation.frame_ty,
                span,
            });
        }
        self.bind_ancestor_frames(
            &evaluation,
            named,
            body,
            span,
            &|_, frame_body, ancestor| {
                let receiver = frame_body.push_expr(Expr {
                    kind: ExprKind::Local(this_local),
                    ty: class_ty,
                    span,
                });
                frame_body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver,
                        field: ancestor.frame_field,
                    },
                    ty: ancestor.frame_ty,
                    span,
                })
            },
        );
        if let Some(binding) = evaluation
            .binding
            .as_ref()
            .filter(|binding| named.contains(binding.as_str()))
        {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span,
            });
            let frame = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: evaluation.frame_field,
                },
                ty: evaluation.frame_ty,
                span,
            });
            let symbol = self.intern_source_name(binding);
            let local = body.push_local(LocalDecl {
                name: Some(symbol),
                ty: evaluation.frame_ty,
                mutable: false,
                span,
            });
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt(Stmt::Let {
                pat,
                ty: evaluation.frame_ty,
                value: Some(frame),
            });
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
        let local = body.push_local(LocalDecl {
            name: Some(evaluation.frame_field),
            ty: evaluation.frame_ty,
            mutable: false,
            span,
        });
        let previous = self.scope.bind(binding.clone(), local);
        Some((binding, previous))
    }

    /// Undo [`Self::bind_scratch_evaluation_frame`].
    pub(in crate::lowering) fn restore_scratch_evaluation_frame(
        &mut self,
        saved: Option<(String, Option<smelt_hir::LocalId>)>,
    ) {
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

    /// Every identifier a method body references, to decide which class
    /// names of a per-evaluation chain are worth a frame binding.
    pub(in crate::lowering) fn function_body_identifiers(
        function_body: &oxc::ast::ast::FunctionBody<'_>,
    ) -> HashSet<String> {
        let mut names = HashSet::new();
        let mut collector = IdentifierCollector { names: &mut names };
        oxc::ast_visit::Visit::visit_function_body(&mut collector, function_body);
        names
    }

    /// Bind each per-evaluation ANCESTOR of the class being lowered that
    /// `named` references to its frame in this evaluation: inside `Sub`,
    /// `Base` is the base class `Sub` extends, whose frame is reached
    /// through `level(k)` (the frame of chain level `k`, from the method's
    /// own receiver or frame).
    fn bind_ancestor_frames(
        &mut self,
        evaluation: &EvaluationStatics,
        named: &HashSet<String>,
        body: &mut Body,
        span: Span,
        level: &dyn Fn(&mut Self, &mut Body, &EvaluationStatics) -> smelt_hir::ExprId,
    ) {
        let mut ancestor = evaluation.base.as_deref();
        while let Some(base) = ancestor {
            if let Some(binding) = base
                .binding
                .as_ref()
                .filter(|binding| named.contains(binding.as_str()))
                .filter(|binding| evaluation.binding.as_ref() != Some(*binding))
            {
                let value = level(self, body, base);
                let symbol = self.intern_source_name(binding);
                let local = body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: base.frame_ty,
                    mutable: false,
                    span,
                });
                let pat = body.push_pattern(Pattern::Binding(local));
                body.push_stmt(Stmt::Let {
                    pat,
                    ty: base.frame_ty,
                    value: Some(value),
                });
                self.scope.bind(binding.clone(), local);
            }
            ancestor = base.base.as_deref();
        }
    }

    /// The hidden frame fields a per-evaluation subclass adds for its base:
    /// the link to the base frame, and for each static field it inherits
    /// (one its base frame declares or itself inherits, and that this class
    /// does not redeclare) the own-flag and shadow pair of
    /// [`InheritedStatic`]. Records the inherited fields on `evaluation`.
    ///
    /// A private static (`static #x`) is not inherited: JavaScript's private
    /// names are per class, and reading one through a subclass throws.
    pub(in crate::lowering) fn evaluation_inherited_fields(
        &mut self,
        evaluation: &mut EvaluationStatics,
        own: &[Field],
        span: Span,
    ) -> Vec<Field> {
        let (Some(base), Some(base_link)) = (evaluation.base.as_deref(), evaluation.base_link)
        else {
            return Vec::new();
        };
        let mut fields = vec![Field {
            name: base_link,
            ty: base.frame_ty,
            visibility: Visibility::Hidden,
            optional: false,
            span,
        }];
        let mut visible = self
            .classes
            .fields(&base.frame_text)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|field| field.visibility != Visibility::Hidden)
            .map(|field| (field.name, field.ty))
            .collect::<Vec<_>>();
        visible.extend(
            base.inherited
                .iter()
                .map(|inherited| (inherited.name, inherited.ty)),
        );
        let depth = evaluation.depth;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let mut inherited = Vec::new();
        for (name, ty) in visible {
            if own.iter().any(|field| field.name == name)
                || inherited
                    .iter()
                    .any(|known: &InheritedStatic| known.name == name)
            {
                continue;
            }
            let spelling = self
                .ctx
                .krate
                .symbols
                .get(name)
                .unwrap_or("field")
                .to_owned();
            let shadow = self
                .intern_synthesized_name(&level_name(&format!("__smelt_shadow_{spelling}"), depth));
            let own_flag = self
                .intern_synthesized_name(&level_name(&format!("__smelt_own_{spelling}"), depth));
            let shadow_ty = if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
                ty
            } else {
                self.ctx.krate.types.intern(Type::Optional(ty))
            };
            fields.push(Field {
                name: shadow,
                ty: shadow_ty,
                visibility: Visibility::Hidden,
                optional: false,
                span,
            });
            fields.push(Field {
                name: own_flag,
                ty: bool_ty,
                visibility: Visibility::Hidden,
                optional: false,
                span,
            });
            inherited.push(InheritedStatic {
                name,
                ty,
                shadow,
                own: own_flag,
            });
        }
        evaluation.inherited = inherited;
        fields
    }

    /// The accessor pair of every static field `evaluation`'s class
    /// inherits, as methods of its frame and the descriptors routing
    /// `Sub.x` and `Sub.x = v` (and `this.x` in a static method running on
    /// `Sub`) to them:
    ///
    /// ```ts
    /// get x(): T { if (this.own_x) { return this.shadow_x; } return this.base.x; }
    /// set x(value: T) { this.shadow_x = value; this.own_x = true; }
    /// ```
    ///
    /// `this.base.x` is itself the base frame's field or, two levels down,
    /// the base frame's own accessor, so a chain resolves level by level.
    fn inherited_static_accessors(
        &mut self,
        evaluation: &EvaluationStatics,
        span: Span,
    ) -> (Vec<smelt_hir::ItemId>, Vec<smelt_hir::Descriptor>) {
        let (Some(base), Some(base_link)) = (evaluation.base.as_deref(), evaluation.base_link)
        else {
            return (Vec::new(), Vec::new());
        };
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let value_symbol = self.ctx.krate.symbols.intern("value");
        let mut methods = Vec::new();
        let mut descriptors = Vec::new();
        for inherited in &evaluation.inherited {
            let spelling = self
                .ctx
                .krate
                .symbols
                .get(inherited.name)
                .unwrap_or("field")
                .to_owned();
            let shadow_ty = self
                .classes
                .fields(&evaluation.frame_text)
                .and_then(|fields| fields.iter().find(|field| field.name == inherited.shadow))
                .map_or(inherited.ty, |field| field.ty);
            let field = |body: &mut Body, receiver: smelt_hir::ExprId, field, ty| {
                body.push_expr(Expr {
                    kind: ExprKind::Field { receiver, field },
                    ty,
                    span,
                })
            };
            let local = |body: &mut Body, local, ty| {
                body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty,
                    span,
                })
            };
            // get x(): T
            let getter_name =
                self.ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new(format!(
                        "__smelt_get_{spelling}"
                    )));
            let mut getter = Body::new(None, span);
            let this_local = getter.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: evaluation.frame_ty,
                mutable: false,
                span,
            });
            getter.params.push(this_local);
            let own_receiver = local(&mut getter, this_local, evaluation.frame_ty);
            let own = field(&mut getter, own_receiver, inherited.own, bool_ty);
            let then_block = getter.push_block(span);
            let shadow_receiver = local(&mut getter, this_local, evaluation.frame_ty);
            let shadow = field(&mut getter, shadow_receiver, inherited.shadow, shadow_ty);
            getter.push_stmt_to_block(then_block, Stmt::Return(Some(shadow)));
            getter.push_stmt(Stmt::If {
                cond: own,
                then_block,
                else_block: None,
            });
            let base_receiver = local(&mut getter, this_local, evaluation.frame_ty);
            let base_frame = field(&mut getter, base_receiver, base_link, base.frame_ty);
            let delegated = field(&mut getter, base_frame, inherited.name, inherited.ty);
            getter.push_stmt(Stmt::Return(Some(delegated)));
            let getter_body = self.ctx.krate.push_body(getter);
            let getter_item = self
                .ctx
                .krate
                .push_item(Item::Function(smelt_hir::Function {
                    name: getter_name,
                    span,
                    type_params: Vec::new(),
                    params: vec![Param {
                        name: this_symbol,
                        local: this_local,
                        ty: evaluation.frame_ty,
                        span,
                    }],
                    rest: None,
                    required_params: None,
                    return_ty: inherited.ty,
                    is_async: false,
                    is_test: false,
                    body: Some(getter_body),
                    owner: smelt_hir::FunctionOwner::ClassMethod {
                        class: evaluation.frame,
                        method: getter_name,
                    },
                }));
            // set x(value: T)
            let setter_name =
                self.ctx
                    .krate
                    .symbols
                    .intern_synthesized(&smelt_hir::SynthesizedName::new(format!(
                        "__smelt_set_{spelling}"
                    )));
            let mut setter = Body::new(None, span);
            let setter_this = setter.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: evaluation.frame_ty,
                mutable: false,
                span,
            });
            let value_local = setter.push_local(LocalDecl {
                name: Some(value_symbol),
                ty: inherited.ty,
                mutable: false,
                span,
            });
            setter.params.push(setter_this);
            setter.params.push(value_local);
            // A container static mutated in place through the subclass
            // (`this.history.push(v)` in a static method run on `Sub`) is
            // read through the getter, mutated, and replayed through this
            // setter (MIR's projected-receiver writeback). JavaScript never
            // assigns there, so the subclass must not gain its own value:
            // writing back the very handle the base holds, while the subclass
            // has none of its own, changes nothing. (An explicit
            // `Sub.history = Sub.history` is the one source spelling this
            // also leaves inherited; it is otherwise indistinguishable.)
            if matches!(
                self.ctx.krate.types.get(inherited.ty),
                Some(Type::List(_) | Type::JsMap(_, _) | Type::Set(_) | Type::Dict(_, _))
            ) {
                let flag_receiver = local(&mut setter, setter_this, evaluation.frame_ty);
                let has_own = field(&mut setter, flag_receiver, inherited.own, bool_ty);
                let inherited_block = setter.push_block(span);
                let own_block = setter.push_block(span);
                let current_receiver = local(&mut setter, setter_this, evaluation.frame_ty);
                let current_base = field(&mut setter, current_receiver, base_link, base.frame_ty);
                let current = field(&mut setter, current_base, inherited.name, inherited.ty);
                let written = local(&mut setter, value_local, inherited.ty);
                let same = setter.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: smelt_hir::BinOp::JsStrictEq,
                        lhs: written,
                        rhs: current,
                    },
                    ty: bool_ty,
                    span,
                });
                let unchanged = setter.push_block(span);
                setter.push_stmt_to_block(unchanged, Stmt::Return(None));
                setter.push_stmt_to_block(
                    inherited_block,
                    Stmt::If {
                        cond: same,
                        then_block: unchanged,
                        else_block: None,
                    },
                );
                setter.push_stmt(Stmt::If {
                    cond: has_own,
                    then_block: own_block,
                    else_block: Some(inherited_block),
                });
            }
            let shadow_target_receiver = local(&mut setter, setter_this, evaluation.frame_ty);
            let shadow_target = field(
                &mut setter,
                shadow_target_receiver,
                inherited.shadow,
                shadow_ty,
            );
            let stored = local(&mut setter, value_local, inherited.ty);
            setter.push_stmt(Stmt::Assign {
                target: shadow_target,
                value: stored,
            });
            let own_target_receiver = local(&mut setter, setter_this, evaluation.frame_ty);
            let target = field(&mut setter, own_target_receiver, inherited.own, bool_ty);
            let value = setter.push_expr(Expr {
                kind: ExprKind::Literal(smelt_hir::Literal::Bool(true)),
                ty: bool_ty,
                span,
            });
            setter.push_stmt(Stmt::Assign { target, value });
            let setter_body = self.ctx.krate.push_body(setter);
            let setter_item = self
                .ctx
                .krate
                .push_item(Item::Function(smelt_hir::Function {
                    name: setter_name,
                    span,
                    type_params: Vec::new(),
                    params: vec![
                        Param {
                            name: this_symbol,
                            local: setter_this,
                            ty: evaluation.frame_ty,
                            span,
                        },
                        Param {
                            name: value_symbol,
                            local: value_local,
                            ty: inherited.ty,
                            span,
                        },
                    ],
                    rest: None,
                    required_params: None,
                    return_ty: none_ty,
                    is_async: false,
                    is_test: false,
                    body: Some(setter_body),
                    owner: smelt_hir::FunctionOwner::ClassMethod {
                        class: evaluation.frame,
                        method: setter_name,
                    },
                }));
            methods.push(getter_item);
            methods.push(setter_item);
            descriptors.push(smelt_hir::Descriptor {
                name: inherited.name,
                read_ty: inherited.ty,
                write_ty: Some(inherited.ty),
                getter: Some(getter_item),
                setter: Some(setter_item),
                data_descriptor: false,
                is_static: false,
                value_fields: Vec::new(),
            });
        }
        (methods, descriptors)
    }

    /// The name of the frame method answering the frame of chain level
    /// `depth` (`Self::frame_level_accessors`).
    fn frame_level_method(&mut self, depth: usize) -> smelt_hir::Symbol {
        self.intern_synthesized_name(&format!("__smelt_level_{depth}"))
    }

    /// One method per level of `evaluation`'s frame chain, `__smelt_level_k`,
    /// answering the frame of the class `k` levels below the first:
    ///
    /// ```ts
    /// __smelt_level_<depth>() { return this; }
    /// __smelt_level_<k>() { return this.__smelt_base.__smelt_level_<k>(); } // k < depth
    /// ```
    ///
    /// A subclass frame redefines every level its base frame defines, so a
    /// static method of the base, re-emitted for the subclass frame, still
    /// finds the base's own frame through the subclass's link.
    fn frame_level_accessors(
        &mut self,
        evaluation: &EvaluationStatics,
        span: Span,
    ) -> Vec<smelt_hir::ItemId> {
        let mut chain = vec![(evaluation.depth, evaluation.frame_ty)];
        let mut level = evaluation.base.as_deref();
        while let Some(base) = level {
            chain.push((base.depth, base.frame_ty));
            level = base.base.as_deref();
        }
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let mut methods = Vec::new();
        for (depth, frame_ty) in chain {
            let name = self.frame_level_method(depth);
            let mut body = Body::new(None, span);
            let this_local = body.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: evaluation.frame_ty,
                mutable: false,
                span,
            });
            body.params.push(this_local);
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: evaluation.frame_ty,
                span,
            });
            let value = match (
                depth == evaluation.depth,
                evaluation.base.as_deref(),
                evaluation.base_link,
            ) {
                (false, Some(base), Some(base_link)) => {
                    let base_frame = body.push_expr(Expr {
                        kind: ExprKind::Field {
                            receiver,
                            field: base_link,
                        },
                        ty: base.frame_ty,
                        span,
                    });
                    body.push_expr(Expr {
                        kind: ExprKind::Method {
                            receiver: base_frame,
                            method: name,
                            args: Vec::new(),
                        },
                        ty: frame_ty,
                        span,
                    })
                }
                _ => receiver,
            };
            body.push_stmt(Stmt::Return(Some(value)));
            let body_id = self.ctx.krate.push_body(body);
            methods.push(
                self.ctx
                    .krate
                    .push_item(Item::Function(smelt_hir::Function {
                        name,
                        span,
                        type_params: Vec::new(),
                        params: vec![Param {
                            name: this_symbol,
                            local: this_local,
                            ty: evaluation.frame_ty,
                            span,
                        }],
                        rest: None,
                        required_params: None,
                        return_ty: frame_ty,
                        is_async: false,
                        is_test: false,
                        body: Some(body_id),
                        owner: smelt_hir::FunctionOwner::ClassMethod {
                            class: evaluation.frame,
                            method: name,
                        },
                    })),
            );
        }
        methods
    }

    /// The name of the frame method constructing the receiver frame's class
    /// for a `new this(..)` written at chain level `depth`
    /// (`Self::frame_constructor_hooks`).
    fn frame_constructor_method(&mut self, depth: usize) -> smelt_hir::Symbol {
        self.intern_synthesized_name(&format!("__smelt_new_{depth}"))
    }

    /// One method per level of `evaluation`'s frame chain, `__smelt_new_k`,
    /// constructing THIS frame's class for a `new this(..)` written in a
    /// static method of the level-`k` class:
    ///
    /// ```ts
    /// __smelt_new_<k>(a1: P1, ..): Owner_k { return new Owner(this, a1, ..); }
    /// ```
    ///
    /// JavaScript constructs the class `this` is, so `Base.create()` run as
    /// `Sub.create()` builds a `Sub`. The parameters are the level-`k`
    /// constructor's, which is what TypeScript checked the arguments
    /// against; the result is a level-`k` instance (a subclass instance is
    /// one). A generic owner has no hook: its instantiation is not a fixed
    /// signature, and `new this(..)` there constructs it directly.
    fn frame_constructor_hooks(
        &mut self,
        evaluation: &EvaluationStatics,
        span: Span,
    ) -> Vec<smelt_hir::ItemId> {
        let constructor_params = |this: &Self, owner: smelt_hir::Symbol| {
            let class = this.class_by_symbol(owner)?;
            if !class.type_params.is_empty() {
                return None;
            }
            let constructor = class.constructor?;
            match this.item_ref(constructor) {
                Item::Function(function) if function.rest.is_none() => Some(
                    function
                        .params
                        .iter()
                        .skip(1)
                        .map(|param| (param.name, param.ty))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            }
        };
        let Some(own_params) = constructor_params(self, evaluation.owner) else {
            return Vec::new();
        };
        let owner_ty = self.ctx.krate.types.intern(Type::Class {
            name: evaluation.owner,
            args: Vec::new(),
        });
        let mut chain = vec![(evaluation.depth, evaluation.owner)];
        let mut level = evaluation.base.as_deref();
        while let Some(base) = level {
            chain.push((base.depth, base.owner));
            level = base.base.as_deref();
        }
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let mut methods = Vec::new();
        for (depth, level_owner) in chain {
            let Some(level_params) = constructor_params(self, level_owner) else {
                continue;
            };
            let level_ty = self.ctx.krate.types.intern(Type::Class {
                name: level_owner,
                args: Vec::new(),
            });
            let name = self.frame_constructor_method(depth);
            let mut body = Body::new(None, span);
            let this_local = body.push_local(LocalDecl {
                name: Some(this_symbol),
                ty: evaluation.frame_ty,
                mutable: false,
                span,
            });
            body.params.push(this_local);
            let mut params = vec![Param {
                name: this_symbol,
                local: this_local,
                ty: evaluation.frame_ty,
                span,
            }];
            let mut level_locals = Vec::new();
            for (param_name, param_ty) in &level_params {
                let local = body.push_local(LocalDecl {
                    name: Some(*param_name),
                    ty: *param_ty,
                    mutable: false,
                    span,
                });
                body.params.push(local);
                params.push(Param {
                    name: *param_name,
                    local,
                    ty: *param_ty,
                    span,
                });
                level_locals.push((local, *param_ty));
            }
            let frame = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: evaluation.frame_ty,
                span,
            });
            let mut args = vec![frame];
            // JavaScript hands the arguments over positionally: a parameter
            // the level-`k` constructor has no argument for is `undefined`.
            for (index, (_, own_ty)) in own_params.iter().enumerate() {
                let arg = if let Some((local, ty)) = level_locals.get(index) {
                    body.push_expr(Expr {
                        kind: ExprKind::Local(*local),
                        ty: *ty,
                        span,
                    })
                } else {
                    let absent =
                        if matches!(self.ctx.krate.types.get(*own_ty), Some(Type::Optional(_))) {
                            *own_ty
                        } else {
                            self.ctx.krate.types.intern(Type::Optional(*own_ty))
                        };
                    body.push_expr(Expr {
                        kind: ExprKind::Literal(smelt_hir::Literal::Undefined),
                        ty: absent,
                        span,
                    })
                };
                args.push(arg);
            }
            let built = body.push_expr(Expr {
                kind: ExprKind::New {
                    class: evaluation.owner,
                    args,
                },
                ty: owner_ty,
                span,
            });
            body.push_stmt(Stmt::Return(Some(built)));
            let body_id = self.ctx.krate.push_body(body);
            methods.push(
                self.ctx
                    .krate
                    .push_item(Item::Function(smelt_hir::Function {
                        name,
                        span,
                        type_params: Vec::new(),
                        required_params: Some(params.len()),
                        params,
                        rest: None,
                        return_ty: level_ty,
                        is_async: false,
                        is_test: false,
                        body: Some(body_id),
                        owner: smelt_hir::FunctionOwner::ClassMethod {
                            class: evaluation.frame,
                            method: name,
                        },
                    })),
            );
        }
        methods
    }

    /// The base-frame argument a per-evaluation subclass's `super(...)` (or
    /// its implicit constructor) passes to its per-evaluation base's
    /// constructor: the base frame its own frame links to. `None` when the
    /// base is not per-evaluation, so it takes no frame.
    pub(in crate::lowering) fn base_frame_argument(
        &self,
        base: smelt_hir::Symbol,
        body: &mut Body,
        span: Span,
    ) -> Option<smelt_hir::ExprId> {
        let evaluation = self.lowering_evaluation_statics.clone()?;
        let base_evaluation = evaluation
            .base
            .as_deref()
            .filter(|base_evaluation| base_evaluation.owner == base)?;
        let base_link = evaluation.base_link?;
        let frame = self.scope.lookup(CONSTRUCTOR_FRAME_BINDING)?;
        let receiver = body.push_expr(Expr {
            kind: ExprKind::Local(frame),
            ty: evaluation.frame_ty,
            span,
        });
        Some(body.push_expr(Expr {
            kind: ExprKind::Field {
                receiver,
                field: base_link,
            },
            ty: base_evaluation.frame_ty,
            span,
        }))
    }

    /// Push the frame class of `evaluation`, once the owner class item exists.
    pub(in crate::lowering) fn finish_evaluation_statics(
        &mut self,
        evaluation: &EvaluationStatics,
        fields: Vec<Field>,
        mut methods: Vec<smelt_hir::ItemId>,
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let (accessors, descriptors) = self.inherited_static_accessors(evaluation, span);
        methods.extend(accessors);
        methods.extend(self.frame_level_accessors(evaluation, span));
        methods.extend(self.frame_constructor_hooks(evaluation, span));
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
            base: evaluation.base.as_ref().map(|base| base.frame),
            base_args: Vec::new(),
            fields,
            static_fields: Vec::new(),
            descriptors,
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
        let frame_local = body.push_local(LocalDecl {
            name: Some(local_name),
            ty: evaluation.frame_ty,
            mutable: false,
            span,
        });
        let construct = body.push_expr(Expr {
            kind: ExprKind::New {
                class: evaluation.frame,
                args: Vec::new(),
            },
            ty: evaluation.frame_ty,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(frame_local));
        self.push_stmt_to_sink(
            body,
            Stmt::Let {
                pat,
                ty: evaluation.frame_ty,
                value: Some(construct),
            },
        );
        // A subclass's frame links to the frame of the base its `extends`
        // clause names in THIS evaluation: `class Sub extends Base` inside
        // `make(n)` extends the `Base` of the same call, and
        // `class Ext extends make(1)` the class that call returned. The
        // clause is evaluated before the static initializers, as in
        // JavaScript.
        if let (Some(base_link), Some(base)) = (evaluation.base_link, evaluation.base.as_deref())
            && let Some(heritage) = &class.heritage
        {
            let base_frame = match self.statics_frame_value(&heritage.expression, body)? {
                Some((value, _)) => value,
                None => self.expression(&heritage.expression, body)?,
            };
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(frame_local),
                ty: evaluation.frame_ty,
                span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: base_link,
                },
                ty: base.frame_ty,
                span,
            });
            self.push_stmt_to_sink(
                body,
                Stmt::Assign {
                    target,
                    value: base_frame,
                },
            );
        }
        if let Some(binding) = &evaluation.binding {
            self.scope.bind(binding.clone(), frame_local);
        }
        let fields = self
            .classes
            .fields(&evaluation.frame_text)
            .cloned()
            .unwrap_or_default();
        let previous_this = self.scope.bind("this".to_owned(), frame_local);
        let lowered = (|| {
            for element in &class.body.body {
                let ClassElement::PropertyDefinition(property) = element else {
                    continue;
                };
                let Some(value) = property.value.as_ref().filter(|_| property.r#static) else {
                    continue;
                };
                if matches!(
                    value,
                    Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
                ) {
                    continue;
                }
                let name = self.property_key_symbol(&property.key)?;
                let Some(field) = fields.iter().find(|field| field.name == name) else {
                    continue;
                };
                let value_span = self.span(value.span().start, value.span().end);
                let value = self.expression_with_hint(value, body, Some(field.ty))?;
                let receiver = body.push_expr(Expr {
                    kind: ExprKind::Local(frame_local),
                    ty: evaluation.frame_ty,
                    span: value_span,
                });
                let target = body.push_expr(Expr {
                    kind: ExprKind::Field {
                        receiver,
                        field: name,
                    },
                    ty: field.ty,
                    span: value_span,
                });
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

    /// Give a module-level per-evaluation class its statics frame: a module
    /// global whose initializer builds the frame exactly as a declaration
    /// in a function body does (`Self::evaluate_class_statics`: the base
    /// link from the `extends` clause, then the static initializers). Its
    /// name, used as a value anywhere in the module, reads the global
    /// (`class_value_expression`), and the module body touches the global
    /// where the declaration is, so it evaluates in source order
    /// (`Self::touch_module_class_frame`).
    pub(in crate::lowering) fn module_class_frame(
        &mut self,
        item: smelt_hir::ItemId,
        class: &oxc::ast::ast::Class<'_>,
        class_text: &str,
    ) -> Result<(), SmeltError> {
        let Item::Class(owner) = self.item_ref(item) else {
            return Ok(());
        };
        let owner = owner.name;
        let Some(evaluation) = self.evaluation_statics.get(&owner).cloned() else {
            return Ok(());
        };
        let span = self.span(class.span.start, class.span.end);
        let name = self.intern_synthesized_name(&format!("{class_text}__smelt_statics_value"));
        let global = self
            .ctx
            .krate
            .push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                name,
                ty: evaluation.frame_ty,
                init: smelt_hir::MutableGlobalInit::Pending,
                visibility: Visibility::Private,
                span,
            }));
        self.module_class_frames
            .insert(owner, (global, evaluation.frame_ty));
        let ctx = self.nested_fn_ctx(
            crate::lowering::state::fn_ctx::BodyKind::Method,
            false,
            Some(evaluation.frame_ty),
        );
        let lowered = self.with_nested_body(
            ctx,
            crate::lowering::state::local_scope::ScopeGroups::MEMBER_BODY,
            |this, _| {
                let mut init_body = Body::new(None, span);
                if let Some(frame) = this.evaluate_class_statics(item, class, &mut init_body)? {
                    let value = init_body.push_expr(Expr {
                        kind: ExprKind::Local(frame),
                        ty: evaluation.frame_ty,
                        span,
                    });
                    init_body.push_stmt(Stmt::Return(Some(value)));
                }
                Ok::<_, SmeltError>(init_body)
            },
        )?;
        self.install_global_initializer(
            global,
            &format!("{class_text}__smelt_statics_value"),
            lowered,
            evaluation.frame_ty,
            span,
        );
        Ok(())
    }

    /// Evaluate a module-level per-evaluation class's frame where its
    /// declaration is (`Self::module_class_frame`): a unit-typed read of its
    /// global, as `touch_class_static_initializers` does for statics.
    pub(in crate::lowering) fn touch_module_class_frame(
        &mut self,
        item: smelt_hir::ItemId,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) {
        let Item::Class(owner) = self.item_ref(item) else {
            return;
        };
        let Some(&(global, _)) = self.module_class_frames.get(&owner.name) else {
            return;
        };
        let span = owner.span;
        let unit = self.ctx.krate.types.intern(Type::None);
        let read = body.push_expr(Expr {
            kind: ExprKind::GlobalGet { item: global },
            ty: unit,
            span,
        });
        body.push_stmt_to_block(block, Stmt::Expr(read));
    }

    /// The owner class whose statics frame `ty` is, if it is one.
    pub(in crate::lowering) fn statics_frame_owner(
        &self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::Symbol> {
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
            return Ok(Some((
                body.push_expr(Expr {
                    kind: ExprKind::Local(local),
                    ty,
                    span,
                }),
                owner,
            )));
        }
        if self.evaluation_frames.is_empty()
            || !matches!(
                self.resolve_value(name),
                crate::lowering::resolve::ValueBinding::Module
                    | crate::lowering::resolve::ValueBinding::Class
            )
        {
            return Ok(None);
        }
        let value = self.expression(expression, body)?;
        Ok(self
            .statics_frame_owner(Self::expr_ty(body, value))
            .map(|owner| (value, owner)))
    }

    /// Lower `new C(args)` where `C` is a statics frame: construct the owner
    /// class with the frame as the hidden leading constructor argument.
    ///
    /// Each source argument is lowered against its constructor parameter's
    /// type, as for any class construction, and a generic owner is
    /// instantiated as any `new` instantiates its class
    /// ([`Self::class_instantiation_args`]): `new A<string>("x")` and
    /// `new A(1)` are an owner of `string` and of `number`. While the owner's
    /// own body is still being lowered its constructor's parameters come from
    /// the prepass, and the instantiation from the explicit arguments, the
    /// contextual type and the checker.
    pub(in crate::lowering) fn new_through_statics_frame(
        &mut self,
        new_expr: &oxc::ast::ast::NewExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((frame, owner)) = self.statics_frame_value(&new_expr.callee, body)? else {
            return Ok(None);
        };
        let span = self.span(new_expr.span.start, new_expr.span.end);
        let owner_class = self.class_by_symbol(owner).cloned();
        let constructor_params = owner_class
            .as_ref()
            .and_then(|class| class.constructor)
            .and_then(|constructor| match self.item_ref(constructor) {
                Item::Function(function) => Some(
                    function
                        .params
                        .iter()
                        .skip(1)
                        .map(|param| param.ty)
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            });
        let hints = match &constructor_params {
            Some(params) => params.iter().copied().map(Some).collect::<Vec<_>>(),
            None => self.pending_constructor_hints(owner, new_expr)?.0,
        };
        let mut source_args = Vec::new();
        for (index, arg) in new_expr.arguments.iter().enumerate() {
            source_args.push(self.argument_with_hint(
                arg,
                body,
                hints.get(index).copied().flatten(),
            )?);
        }
        let type_params = match &owner_class {
            Some(class) => class.type_params.clone(),
            None => self
                .evaluation_statics
                .get(&owner)
                .map(|evaluation| evaluation.type_params.clone())
                .unwrap_or_default(),
        };
        let class_args = if type_params.is_empty() {
            Vec::new()
        } else {
            self.class_instantiation_args(
                owner,
                &type_params,
                constructor_params.as_deref().unwrap_or_default(),
                &mut source_args,
                new_expr,
                type_hint,
                body,
            )?
        };
        let ty = self.ctx.krate.types.intern(Type::Class {
            name: owner,
            args: class_args,
        });
        // `new this(..)` in a static method constructs the class `this` is,
        // which an inherited copy runs on a subclass frame: it goes through
        // the frame's constructor hook (`Self::frame_constructor_hooks`).
        if matches!(
            new_expr.callee.without_parentheses(),
            Expression::ThisExpression(_)
        ) && type_params.is_empty()
            && let Some(depth) = self
                .evaluation_statics
                .get(&owner)
                .map(|evaluation| evaluation.depth)
        {
            let method = self.frame_constructor_method(depth);
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver: frame,
                    method,
                    args: source_args,
                },
                ty,
                span,
            })));
        }
        let mut args = vec![frame];
        args.extend(source_args);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::New { class: owner, args },
            ty,
            span,
        })))
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
