//! Rest parameters whose type is a fixed-width tuple.
//!
//! `(...args: [string, number?]) => ..` accepts at most two arguments, each at
//! its own type: TypeScript gives `args` the tuple type, not `T[]`. That is a
//! fixed-arity signature with a convenient name for its parameters, so Smelt
//! lowers it as one: the function type carries the tuple's elements as
//! ordinary parameters (`fn(String, Option<f64>)`, `rest: None`), and the body
//! binds the rest name to the tuple of those parameters (`let args = (a0,
//! a1);`). Calls, overload selection, spreads and the adapters into
//! fixed-arity slots all then take the fixed-arity paths they already have,
//! and every element keeps its own type where a list would have needed the
//! union of them all.
//!
//! The source of the tuple is either the annotation or, for an unannotated
//! rest parameter, the type the TypeScript 7 checker gives the binding, which
//! is the tuple of the contextual signature's parameters. A tuple with a rest
//! element (`[A, ...B[]]`) has no fixed width; it lowers to a list (see
//! `tuple_rest_list_type`), and a list rest parameter keeps the list ABI.
//!
//! # `length` of a tuple with optional elements
//!
//! `(...args: [key?: string, pattern?: P]) => ..` accepts zero, one or two
//! arguments, and `args.length` says how many came: ts-pattern's `select`,
//! `P.map` and `P.set` branch on it. The fixed parameters always exist, so
//! the length is recovered from them: the required elements, plus every
//! optional element up to the last one present (not `undefined`). That is
//! what a hand port writing the optional elements as `Option`s would compute.
//! It differs from JavaScript only for a call passing an explicit trailing
//! `undefined`, which JavaScript counts and this does not.

use oxc::ast::ast::{StaticMemberExpression, TSTupleElement, TSType, TSTypeAnnotation};
use smelt_hir::{
    BinOp, Body, Expr, ExprKind, Literal, LocalDecl, Pattern, Stmt, Type, TypeId, UnknownKind,
};

use super::ModuleBuilder;
use super::body_arena::BodyArena;
use crate::SmeltError;

/// The scope key of the hidden local holding a rest tuple's argument count:
/// not a JavaScript identifier, so no source binding can collide with it. A
/// closure capturing the rest binding captures this key with it
/// (`collect_expression_capture_names`).
pub(in crate::lowering) fn tuple_rest_length_key(name: &str) -> String {
    format!("{name}#length")
}

/// How many leading elements of a rest parameter's tuple annotation are
/// required, when any element after them is optional (`[a: A, b?: B]` is
/// `Some(1)`); `None` when every element is required or the annotation is no
/// tuple.
pub(in crate::lowering) fn tuple_rest_required(
    annotation: Option<&TSTypeAnnotation<'_>>,
) -> Option<usize> {
    let TSType::TSTupleType(tuple) = &annotation?.type_annotation else {
        return None;
    };
    let optional = |element: &TSTupleElement<'_>| match element {
        TSTupleElement::TSOptionalType(_) => true,
        TSTupleElement::TSNamedTupleMember(member) => member.optional,
        _ => false,
    };
    let required = tuple
        .element_types
        .iter()
        .take_while(|element| !optional(element))
        .count();
    (required < tuple.element_types.len()).then_some(required)
}

/// One fixed parameter a tuple rest parameter contributes.
pub(in crate::lowering) struct TupleRestParam {
    /// The parameter's synthetic name (`<rest>__0`, `<rest>__1`, ..).
    pub name: smelt_hir::Symbol,
    /// Its local in the function body.
    pub local: smelt_hir::LocalId,
    /// Its type: the tuple element's type.
    pub ty: TypeId,
}

impl ModuleBuilder<'_> {
    /// The element types of a rest parameter's type when it is a fixed-width
    /// tuple, or `None` (a list, an erased value, a type parameter).
    pub(in crate::lowering) fn fixed_tuple_rest_elements(&self, ty: TypeId) -> Option<Vec<TypeId>> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Tuple(items)) => Some(items.clone()),
            _ => None,
        }
    }

    /// Append a rest parameter of type `rest_ty` to a function type's
    /// parameter list, returning the `rest` index the type records.
    ///
    /// A fixed-width tuple contributes its elements as fixed parameters and no
    /// rest index; anything else is the rest list, at the end.
    pub(in crate::lowering) fn push_rest_param_type(
        &mut self,
        params: &mut Vec<TypeId>,
        rest_ty: TypeId,
    ) -> Result<Option<usize>, SmeltError> {
        if let Some(elements) = self.fixed_tuple_rest_elements(rest_ty) {
            params.extend(elements);
            return Ok(None);
        }
        let (list_ty, _) = self.rest_param_array_type(rest_ty)?;
        params.push(list_ty);
        Ok(Some(params.len() - 1))
    }

    /// Declare the fixed parameters of a tuple rest parameter in `body` and
    /// bind `name` to their tuple.
    ///
    /// Pushes one parameter local per element (in order, after any parameters
    /// already declared), then a `let <name> = (p0, p1, ..)` statement, and
    /// binds the source name to that local. The caller records the returned
    /// parameters in its own parameter list.
    pub(in crate::lowering) fn bind_tuple_rest_params(
        &mut self,
        name: &str,
        tuple_ty: TypeId,
        elements: &[TypeId],
        required: Option<usize>,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> Vec<TupleRestParam> {
        let mut params = Vec::with_capacity(elements.len());
        let mut reads = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let param_name = self.intern_source_name(&format!("{name}__{index}"));
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty: *element,
                mutable: false,
                span,
            });
            body.params.push(local);
            reads.push(body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty: *element,
                span,
            }));
            params.push(TupleRestParam {
                name: param_name,
                local,
                ty: *element,
            });
        }
        let tuple = body.push_expr(Expr {
            kind: ExprKind::TupleLit(reads),
            ty: tuple_ty,
            span,
        });
        let rest_name = self.intern_source_name(name);
        let rest_local = body.push_local(LocalDecl {
            name: Some(rest_name),
            ty: tuple_ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(rest_local));
        let root = body.root;
        body.push_stmt_to_block(
            root,
            Stmt::Let {
                pat,
                ty: tuple_ty,
                value: Some(tuple),
            },
        );
        self.scope.bind(name.to_owned(), rest_local);
        if let Some(required) = required {
            let element_locals: Vec<_> = params.iter().map(|param| param.local).collect();
            let length = self.optional_tuple_length_expr(&element_locals, required, span, body);
            let float_ty = self.ctx.krate.types.intern(Type::Float);
            let symbol = self.intern_synthesized_name(&format!("smelt_{name}_length"));
            let length_local = body.push_local(LocalDecl {
                name: Some(symbol),
                ty: float_ty,
                mutable: false,
                span,
            });
            let length_pat = body.push_pattern(Pattern::Binding(length_local));
            body.push_stmt_to_block(
                root,
                Stmt::Let {
                    pat: length_pat,
                    ty: float_ty,
                    value: Some(length),
                },
            );
            self.scope.bind(tuple_rest_length_key(name), length_local);
        }
        params
    }

    /// `args.length` on a tuple rest with optional elements: the argument
    /// count computed where the rest was bound (see the module docs), read
    /// through its hidden local, which a closure captures with the rest
    /// binding. `None` for any other member read.
    pub(in crate::lowering) fn optional_tuple_rest_length(
        &self,
        member: &StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        if member.property.name != "length" || member.optional {
            return None;
        }
        let oxc::ast::ast::Expression::Identifier(identifier) = &member.object else {
            return None;
        };
        let rest = self.scope.lookup(identifier.name.as_str())?;
        if !matches!(
            self.ctx.krate.types.get(body.local_at(rest)?.ty),
            Some(Type::Tuple(_))
        ) {
            return None;
        }
        let length = self
            .scope
            .lookup(&tuple_rest_length_key(identifier.name.as_str()))?;
        let ty = body.local_at(length)?.ty;
        Some(body.push_expr(Expr {
            kind: ExprKind::Local(length),
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// The argument count of a rest tuple whose elements from `required` on
    /// are optional: the required count, raised to each optional element's
    /// position when it is present (not `undefined`, not `None`).
    fn optional_tuple_length_expr(
        &mut self,
        elements: &[smelt_hir::LocalId],
        required: usize,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let float_ty = self.ctx.krate.types.intern(Type::Float);
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let count = |target: &mut Body, value: usize| {
            target.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(f64::from(
                    u32::try_from(value).unwrap_or(u32::MAX),
                ))),
                ty: float_ty,
                span,
            })
        };
        // Innermost first: with no optional element present the length is the
        // required count; each element present raises it to its own position.
        let mut length = count(body, required);
        for (index, element) in elements.iter().enumerate().skip(required) {
            let element_ty = Self::local_ty(body, *element);
            let read = body.push_expr(Expr {
                kind: ExprKind::Local(*element),
                ty: element_ty,
                span,
            });
            let present = match self.ctx.krate.types.get(element_ty) {
                Some(Type::Unknown) => {
                    let undefined = body.push_expr(Expr {
                        kind: ExprKind::UnknownIs {
                            value: read,
                            kind: UnknownKind::Undefined,
                        },
                        ty: bool_ty,
                        span,
                    });
                    body.push_expr(Expr {
                        kind: ExprKind::UnaryOp {
                            op: smelt_hir::UnaryOp::Not,
                            operand: undefined,
                        },
                        ty: bool_ty,
                        span,
                    })
                }
                Some(Type::Optional(_)) => {
                    let none = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty: element_ty,
                        span,
                    });
                    body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::NotEq,
                            lhs: read,
                            rhs: none,
                        },
                        ty: bool_ty,
                        span,
                    })
                }
                // A type with no absent value: the element is always present.
                _ => {
                    length = count(body, index.saturating_add(1));
                    continue;
                }
            };
            let here = count(body, index.saturating_add(1));
            length = body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond: present,
                    then_expr: here,
                    else_expr: length,
                },
                ty: float_ty,
                span,
            });
        }
        length
    }
}
