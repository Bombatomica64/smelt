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

use smelt_hir::{Body, Expr, ExprKind, LocalDecl, Pattern, Stmt, Type, TypeId};

use super::ModuleBuilder;
use crate::SmeltError;

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
        params
    }
}
